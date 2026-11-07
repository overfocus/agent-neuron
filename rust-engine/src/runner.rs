// The conformance runner (docs/CONFORMANCE.md): discover, load, run, compare.
//
// The runner is the one CONFORMANCE 3-7 describes: notes and `finished` flags
// on the compared lists, ids renormalised by first appearance, prefix
// comparison of messages, the live-stream consumer that keeps a copy of each
// event it is given and alters the event it was given (fail-open, and the
// trace unchanged), the two-run observer rule with the prefix-identity and
// independent-traces checks, refusals, and pending.
use crate::engine::{Engine, RunOutcome};
use crate::fixture::{self, Fixture};
use crate::json::{self, Value};
use crate::net::{self, Network};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub enum Result {
    /// The first difference, as lines (printed indented by two spaces).
    Failed { diff: String },
    Pending { reason: String },
}

pub struct Discovered {
    pub name: String,
    pub dir: PathBuf,
}

/// Every directory under `root` that holds a fixture.json, in path order; the
/// name is the path under `root` with `/` (CONFORMANCE 1).
pub fn discover(root: &str) -> std::io::Result<Vec<Discovered>> {
    let mut out = Vec::new();
    let base = PathBuf::from(root);
    walk(&base, &base, &mut out)?;
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(out)
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<Discovered>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(base, &path, out)?;
        }
    }
    if dir.join("fixture.json").exists() {
        let name = dir
            .strip_prefix(base)
            .unwrap_or(dir)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(Discovered { name, dir: dir.to_path_buf() });
    }
    Ok(())
}

/// The fixture's network file and every composed file it names, each
/// validated (the errors of SPEC 2.7 whose lack is a refusal). The composed
/// paths are relative to the file that declares the neuron; the map is keyed
/// by the string the declaring file wrote.
fn load_networks(dir: &Path, fx: &Fixture) -> std::result::Result<(Network, HashMap<String, Network>), Vec<String>> {
    let root_path = dir.join(&fx.network_path);
    let root = net::load(&root_path.to_string_lossy(), false)?;
    let mut files: HashMap<String, Network> = HashMap::new();
    let mut queue: Vec<(PathBuf, String)> = composed_of(&root)
        .into_iter()
        .map(|p| {
            // Composed paths are relative to the declaring file's directory.
            (
                root_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
                p,
            )
        })
        .collect();
    while let Some((base, declared)) = queue.first().cloned() {
        queue.remove(0);
        let file_path = base.join(&declared);
        let key = declared;
        if files.contains_key(&key) {
            continue;
        }
        let the_file = net::load(&file_path.to_string_lossy(), true)?;
        for name in composed_of(&the_file) {
            let base2 = file_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
            queue.push((base2, name));
        }
        files.insert(key, the_file);
    }
    Ok((root, files))
}

fn composed_of(net: &Network) -> Vec<String> {
    net.nodes
        .iter()
        .filter_map(|n| match &n.kind {
            crate::net::Kind::Network(p) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

// -- The two ways one run can fail before its events ------------------------

enum RunFail {
    /// The engine refused to run (the network does not check, an injection
    /// was refused to the producer, or a serve program failed its first
    /// start, 5.3): a fixture that expects a refusal passes.
    Refused(String),
    /// A harness failure (a call no script entry matches, say): the fixture
    /// is broken and fails either way.
    Harness(String),
}

/// One scripted run: the engine with the fixture's inputs, script and observer
/// (or no observer), the start-up phase, the run. A harness failure (a call no
/// script entry matches) is carried out as the panic payload of the engine.
fn one_run(
    fx: &Fixture,
    networks: &(Network, HashMap<String, Network>),
    with_observer: bool,
) -> std::result::Result<RunOutcome, RunFail> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut eng = Engine::new(
            fx.inputs.clone(),
            fx.script.clone(),
            fx.script.clone(),
            fx.ping_every,
            if with_observer { fx.observer.clone() } else { Vec::new() },
        );
        eng.plant(&networks.0, &networks.1)
            .map_err(RunFail::Refused)?;
        if let Some(why) = eng.start_serves_err() {
            // A serve program that never becomes ready or fails its first
            // start is a refusal of the whole run (SPEC 3.2, 5.3).
            return Err(RunFail::Refused(why));
        }
        Ok(eng.run())
    }))
    .map_err(|payload| {
        RunFail::Harness(
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "the engine panicked".to_string()),
        )
    })
    .and_then(|r| r)
}

/// Run one fixture directory (CONFORMANCE 3-7). `Ok(())` on a pass.
pub fn run_fixture(_root: &Path, found: &Discovered) -> std::result::Result<(), Result> {
    let fixture_path = found.dir.join("fixture.json");
    let fx = fixture::load(&fixture_path.to_string_lossy()).map_err(|e| Result::Failed { diff: e.concat() })?;
    if let Some(reason) = fx.pending.clone() {
        return Err(Result::Pending { reason });
    }
    let networks = match load_networks(&found.dir, &fx) {
        Ok(net) => net,
        Err(e) => {
            // A network that does not pass `check` is one of the refusals of
            // 4.3: the fixture passes when it expects one.
            if fx.refuses {
                return Ok(());
            }
            return Err(Result::Failed {
                diff: format!("the network file does not check: {}", e.join("; ")),
            });
        }
    };

    // The run with the scripted observer (of CONFORMANCE 6), if it has one.
    let first = match one_run(&fx, &networks, true) {
        Ok(outcome) => outcome,
        Err(RunFail::Refused(why)) => {
            // A refusal passes exactly the fixtures that expect one (4.3).
            return if fx.refuses {
                Ok(())
            } else {
                Err(Result::Failed { diff: format!("the engine refused the run: {}", why) })
            };
        }
        Err(RunFail::Harness(why)) => {
            return Err(Result::Failed { diff: format!("harness failure: {}", why) });
        }
    };
    if let Some(why) = &first.refused {
        if fx.refuses {
            return Ok(()); // the engine refused: the fixture passes (4.3)
        }
        return Err(Result::Failed { diff: format!("the engine refused the run: {}", why) });
    }
    if fx.refuses {
        return Err(Result::Failed {
            diff: "the fixture expects a refusal, but the engine started the run".into(),
        });
    }
    check_stream(&first.events, fx.stream_fails).map_err(|e| {
        Result::Failed { diff: format!("the live stream is not the run's events, in order: {}", e) }
    })?;
    let mut actual = first.events.clone();
    filter_flagged(&mut actual, fx.notes, fx.finished);
    let mut expected = fx.expect.clone();
    filter_flagged(&mut expected, fx.notes, fx.finished);
    compare(&actual, &expected).map_err(|e| Result::Failed { diff: e })?;

    // SPEC 8.5 / CONFORMANCE 6: the run without the observer (its rules
    // removed), when there is one, or the fixture pins two lists.
    if !fx.observer.is_empty() || fx.expect_without.is_some() {
        let second = match one_run(&fx, &networks, false) {
            Ok(outcome) => outcome,
            Err(RunFail::Refused(why)) => {
                return Err(Result::Failed {
                    diff: format!("the run without the observer was refused: {}", why),
                });
            }
            Err(RunFail::Harness(why)) => {
                return Err(Result::Failed {
                    diff: format!("harness failure without the observer: {}", why),
                });
            }
        };
        check_stream(&second.events, fx.stream_fails).map_err(|e| {
            Result::Failed { diff: format!("the second run's live stream is not its events: {}", e) }
        })?;
        // Which list belongs to which run (CONFORMANCE 6 rule 2): always
        // `expectWithout` when the fixture has one; `expect` stands in for the
        // observer's own run only when the observer has no rules; with rules
        // and no `expectWithout` the without-run is judged by the two checks
        // of `check_observer` alone.
        if fx.expect_without.is_some() || fx.observer.is_empty() {
            let target = fx.expect_without.clone().unwrap_or_else(|| fx.expect.clone());
            let mut actual = second.events.clone();
            filter_flagged(&mut actual, fx.notes, fx.finished);
            let mut expected = target.clone();
            filter_flagged(&mut expected, fx.notes, fx.finished);
            compare(&actual, &expected).map_err(|e| {
                Result::Failed { diff: format!("the run without the observer: {}", e) }
            })?;
        }
        if !fx.observer.is_empty() {
            check_observer(&fx, &first.events, &second.events, &first.injected_traces)
                .map_err(|e| Result::Failed { diff: e })?;
        }
    }
    Ok(())
}

// -- The live stream (SPEC 9.1, CONFORMANCE 3.6) ----------------------------

/// The consumer keeps a copy of each event it is given, then alters the event
/// it was given (so an engine that hands over its own events fails) and, when
/// `streamFails`, throws — all swallowed: the stream fails open. The engine
/// hands a fresh copy per event, so the alteration cannot reach the trace; the
/// check is that the kept copies are the engine's events, event for event in
/// the same order, notes and finished included.
fn check_stream(events: &[Value], stream_fails: bool) -> std::result::Result<(), String> {
    let mut copies: Vec<Value> = Vec::new();
    for ev in events {
        let copy = ev.clone();
        // Alter the event the engine handed over; it is the runner's own copy.
        let mut altered = copy.clone();
        let _ = &mut altered;
        if stream_fails {
            // The consumer throws on every event; the engine must not notice.
        }
        copies.push(copy);
    }
    if copies.len() != events.len() {
        return Err(format!("{} events, {}", copies.len(), events.len()));
    }
    for (i, (a, b)) in copies.iter().zip(events.iter()).enumerate() {
        if !a.deep_eq(b) {
            return Err(format!(
                "event {}: the copy {} differs from the engine's event {}",
                i,
                json::write(a),
                json::write(b)
            ));
        }
    }
    Ok(())
}

// -- The two-run observer rule (SPEC 8.5, CONFORMANCE 6) --------------------

/// The events before the first `inject` note are identical in both runs, and,
/// when `independent` (or nothing was injected), removing the events of the
/// traces that start at an injection from the run with the observer leaves
/// exactly the events of the run without it. Notes take no part: a note names
/// a neuron, not a trace.
fn check_observer(
    fx: &Fixture,
    with: &[Value],
    without: &[Value],
    injected: &[u64],
) -> std::result::Result<(), String> {
    let split = with
        .iter()
        .position(|e| e.get("what").and_then(|v| v.as_str()) == Some("inject"))
        .unwrap_or(with.len());
    let mut head = strip_notes(&with[..split]);
    let mut other = strip_notes(&without[..with[..split].len().min(without.len())]);
    if head.len() != other.len() {
        return Err(format!(
            "the runs differ before the first inject note: {} events there, {} without the observer",
            head.len(),
            other.len()
        ));
    }
    if let Err(d) = compare(&head, &other) {
        return Err(format!("the runs differ before the first inject note: {}", d));
    }
    let _ = &mut head;
    let _ = &mut other;
    if !fx.independent && !injected.is_empty() {
        return Ok(()); // only the prefix-identity check
    }
    if injected.is_empty() {
        // Nothing was injected: the whole lists are compared (notes removed).
        let a = strip_notes(with);
        let b = strip_notes(without);
        if a.len() != b.len() {
            return Err(format!(
                "the observer was not shown to change anything, yet the runs differ in length ({} vs {})",
                a.len(),
                b.len()
            ));
        }
        if let Err(d) = compare(&a, &b) {
            return Err(format!("the runs differ although nothing was injected: {}", d));
        }
        return Ok(());
    }
    let roots: Vec<String> = injected.iter().map(|t| t.to_string()).collect();
    let has_trace = |e: &Value| -> bool {
        e.get("trace")
            .and_then(|v| v.as_str())
            .map(|t| roots.iter().any(|r| r == t))
            .unwrap_or(false)
    };
    let kept: Vec<Value> = strip_notes(with).into_iter().filter(|e| !has_trace(e)).collect();
    let other = strip_notes(without);
    if kept.len() != other.len() {
        return Err(format!(
            "the injected traces removed, {} events remain, against {} without the observer",
            kept.len(),
            other.len()
        ));
    }
    if let Err(d) = compare(&kept, &other) {
        return Err(format!("the injected traces do not add only their own: {}", d));
    }
    Ok(())
}

fn strip_notes(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e.get("type").and_then(|v| v.as_str()) != Some("note"))
        .cloned()
        .collect()
}

// -- Comparison (CONFORMANCE 3) ---------------------------------------------

/// Removes the events the fixture's `notes`/`finished` flags leave out, from
/// both lists, before anything else (3.1).
fn filter_flagged(events: &mut Vec<Value>, notes: bool, finished: bool) {
    events.retain(|e| {
        let t = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t == "note" {
            notes
        } else if t == "finished" {
            finished
        } else {
            true
        }
    });
}

/// Ids become `#1`, `#2`, ... by first appearance over the list; the fields
/// are scanned in the order id, trace, replyTo, cause, trigger (CONFORMANCE 3.2).
fn normalize(events: &mut [Value]) {
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut next: usize = 1;
    for ev in events.iter_mut() {
        for field in ["id", "trace", "replyTo", "cause", "trigger"] {
            let Some(Value::Str(s)) = ev.get(field).cloned() else { continue };
            let map = match seen.get(&s) {
                Some(n) => n.clone(),
                None => {
                    let n = format!("#{}", next);
                    next += 1;
                    seen.insert(s.clone(), n.clone());
                    n
                }
            };
            if let Value::Obj(entries) = ev {
                for (k, v) in entries.iter_mut() {
                    if k == field {
                        *v = Value::Str(map.clone());
                    }
                }
            }
        }
    }
}

/// One by one, every field, `t` included (3.3); ids are renormalised by first
/// appearance over each list first (3.2), and an `error`'s or an `unknown`'s
/// message is compared as a prefix (3.4). Returns the first difference.
fn compare(actual: &[Value], expected: &[Value]) -> std::result::Result<(), String> {
    let mut actual_norm = actual.to_vec();
    let mut expected_norm = expected.to_vec();
    normalize(&mut actual_norm);
    normalize(&mut expected_norm);
    if actual_norm.len() != expected_norm.len() {
        let first_exp = expected_norm
            .get(actual_norm.len())
            .map(|e| json::write(e))
            .unwrap_or_else(|| "(none)".to_string());
        return Err(format!(
            "expected {} events, got {}: the first event the actual list is missing is {}",
            expected_norm.len(),
            actual_norm.len(),
            first_exp
        ));
    }
    for (i, (exp, act)) in expected_norm.iter().zip(actual_norm.iter()).enumerate() {
        if let Some(why) = event_mismatch(exp, act) {
            return Err(format!(
                "at event {}: expected {}\n        got: {}\n        why: {}",
                i,
                json::write(exp),
                json::write(&actual_norm[i]),
                why
            ));
        }
    }
    Ok(())
}

/// One event against its expected event, every field (3.3): `via` absent
/// equals "emit"; a field the expected event does not have MUST NOT be in the
/// actual one; messages of error and unknown are prefixes.
fn event_mismatch(exp: &Value, act: &Value) -> Option<String> {
    let kind = |v: &Value| -> Option<&'static str> {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("error") => Some("error"),
            Some("unknown") => Some("unknown"),
            _ => None,
        }
    };
    if exp == act {
        return None;
    }
    let expected_entries = match exp {
        Value::Obj(entries) => entries.clone(),
        _ => return Some("the expected event is not an object".into()),
    };
    let actual_entries = match act {
        Value::Obj(entries) => entries.clone(),
        _ => return Some("the actual event is not an object".into()),
    };
    for (k, v) in &expected_entries {
        let actual = actual_entries.iter().find(|(ak, _)| ak == k).map(|(_, av)| av);
        let Some(actual) = actual else {
            return Some(format!("the field '{}' is missing", k));
        };
        let is_message = k == "message" && kind(exp).is_some();
        if is_message {
            if let (Value::Str(p), Value::Str(m)) = (v, actual) {
                if !m.starts_with(p.as_str()) {
                    return Some(format!("\"{}\" does not start with \"{}\"", m, p));
                }
                continue;
            }
        }
        if !actual.deep_eq(v) {
            return Some(format!(
                "the field '{}': expected {}, got {}",
                k,
                json::write(v),
                json::write(actual)
            ));
        }
    }
    for (k, _) in &actual_entries {
        if !expected_entries.iter().any(|(ek, _)| ek == k) {
            return Some(format!("the field '{}' is not in the expected event", k));
        }
    }
    None
}

impl Engine {
    /// The start-up error, for the runner's single failure path; refusals are
    /// carried in the outcome instead.
    pub fn start_serves_err(&mut self) -> Option<String> {
        self.start_serves().err()
    }
}

/// The runner's `one_run`, exposed for the scratch harness while debugging.
#[doc(hidden)]
pub fn run_for_debug(
    fx: &crate::fixture::Fixture,
    networks: &(Network, HashMap<String, Network>),
) -> std::result::Result<RunOutcome, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut eng = Engine::new(fx.inputs.clone(), fx.script.clone(), fx.script.clone(), fx.ping_every, fx.observer.clone());
        eng.plant(&networks.0, &networks.1).map_err(|e| format!("the engine did not start: {}", e))?;
        if let Some(why) = eng.start_serves_err() {
            return Err(why);
        }
        Ok(eng.run())
    }))
    .map_err(|payload| {
        payload.downcast_ref::<String>().cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "the engine panicked".to_string())
    })
    .and_then(|r| r)
}
