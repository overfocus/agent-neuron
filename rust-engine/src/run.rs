// The `run` command: one file, one inputs file, real processes (SPEC 9.1's
// command-line form; the answers of the neurons with no outgoing links).
use crate::engine::Engine;
use crate::fixture;
use crate::json::{self, Value};
use crate::net::{NetFile, Network};
use crate::trace;
use std::collections::HashMap;
use std::path::Path;

/// The live-stream sink of a run (9.1): each event, in order; never holds the
/// run up. `Send`: an MCP job runs the engine in its own thread, so the sink
/// it gives the engine crosses that thread's boundary.
pub type StreamSink = Box<dyn FnMut(&Value) + Send>;

/// One finished run: the answers of the neurons with no outgoing links in
/// their printed form (4.4), the events, and the network's tree (the declared
/// file first, then every composed file it names).
pub struct RunResult {
    pub answers: Vec<String>,
    pub events: Vec<Value>,
    pub network: Network,
    pub files: HashMap<String, NetFile>,
}

pub struct RunOpts {
    pub inputs: String,
    pub ping_every: u64,
    /// A grants file for the desk (decision 0010): who may answer what.
    pub grants: Option<String>,
    /// A peers file (decision 0013): the machines that execute some of this
    /// run's neurons, and who runs which. Absent: every call is local.
    pub peers: Option<String>,
}

/// The inputs file: JSONL entries `{ at, source, payload, replaces?, budget? }`
/// (SPEC 4.1), the shape a fixture's `inputs` entries take.
pub fn read_inputs(path: &str) -> Result<Vec<fixture::Input>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: cannot read ({})", path, e))?;
    let mut inputs = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v = crate::json::parse(line).map_err(|e| format!("{} line {}: not JSON ({})", path, i + 1, e))?;
        let mut errs = Vec::new();
        let input = fixture::inputs_of(&v, i, &mut errs);
        if !errs.is_empty() {
            return Err(format!("{} line {}: {}", path, i + 1, errs.join("; ")));
        }
        inputs.push(input);
    }
    Ok(inputs)
}

/// One run, on real processes. Time zero is the instant the last serve
/// program said ready (CONFORMANCE 4.2's real analogue); the inputs are then
/// injected `at` ms from it, at real times; the engine returns after the
/// processes are gone (Appendix C), and never before its last event was
/// offered to the stream (9.1). On an engine that refuses (a serve program
/// that never said ready), the start's processes are stopped, silently.
///
/// Returns the run's printed answers — the whole payloads of the neurons with
/// no outgoing links, in the order they happened (4.4), a text payload as its
/// text and the rest as its JSON — the events, and the network's tree.
pub fn run_with_stream(file: &str, opts: &RunOpts, sink: Option<StreamSink>) -> Result<RunResult, String> {
    let inputs = read_inputs(&opts.inputs)?;
    let path = Path::new(file);
    // The declaring file's directory is the default working directory of its
    // neurons (SPEC 2.4); a file named bare is resolved against the caller's
    // own working directory before its parent becomes one, because what the
    // spawns chdir to has to exist.
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot run: the working directory is unreadable ({})", e))?
            .join(path)
    };
    let (outer, files) = crate::runner::load_network_tree(&absolute).map_err(|e| e.join("; "))?;
    // The declaring file's directory is the default working directory of its
    // neurons (SPEC 2.4); the engine plants with it.
    let base = absolute
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut eng = Engine::new_real(inputs, opts.ping_every);
    if let Some(g) = &opts.grants {
        let grants = crate::desk::Grants::load(g).map_err(|e| format!("cannot run: the grants {e}"))?;
        let humans: Vec<String> = outer
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, crate::net::Kind::Human))
            .map(|n| n.id.clone())
            .collect();
        grants.check_against(&humans).map_err(|e| format!("cannot run: {e}"))?;
        eng.set_grants(grants);
    }
    if let Some(p) = &opts.peers {
        let peers = crate::peers::Peers::load(p).map_err(|e| format!("cannot run: {e}"))?;
        eng.set_peers(peers);
    }
    if let Err(e) = eng.plant(&outer, &files, Some(&base)) {
        return Err(format!("cannot run: {}", e));
    }
    // The desk's address (decision 0008, Appendix D): one page per run, its
    // token in the query, on the stderr lines (stdout carries the answers).
    if let Some(line) = eng.desk_line() {
        eprintln!("{}", line);
    }
    for line in eng.desk_people_lines() {
        eprintln!("{}", line);
    }
    if let Some(sink) = sink {
        eng.set_stream(sink);
    }
    if let Some(why) = eng.start_serves_err() {
        eng.stop_after_refusal();
        return Err(format!("the engine refused the run: {}", why));
    }
    let outcome = eng.run();
    if let Some(why) = &outcome.refused {
        return Err(format!("the engine refused the run: {}", why));
    }
    Ok(RunResult {
        answers: outcome.printed_answers(),
        events: outcome.events,
        network: outer,
        files,
    })
}

/// The answers on stdout, one per line (already the printed form).
pub fn print_answers(answers: &[String]) {
    for line in answers {
        println!("{}", line);
    }
}

/// The trace file, and the self-check: its events (read back from the disk)
/// are the engine's events, and the stream's lines — when a stream fed the
/// run — are the same events in the same order (the P2 gate; 9.1 says the
/// stream is the trace). The `run` command and the MCP route share it.
pub fn write_trace_checked(
    out: &str,
    events: &[json::Value],
    outer: &Network,
    written: Option<&std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
) -> Result<(), String> {
    let fallback = Path::new(out)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let demo = if outer.name.is_empty() { fallback } else { outer.name.clone() };
    trace::write_trace(out, outer, &demo, events)?;
    // The header's `ids` is written with the trace; the read-back carries it
    // whole (the ids the file declares are the ids the run assigned).
    let ids = trace::read_ids(out)?;
    if !ids.deep_eq(&trace::ids_value()) {
        return Err(format!(
            "the trace self-check failed: the header's ids is {}, the writer writes {}",
            json::write(&ids),
            json::write(&trace::ids_value())
        ));
    }
    let from_trace = trace::read_events(out)?;
    if from_trace.len() != events.len() {
        return Err(format!(
            "the trace self-check failed: the file holds {} events, the run reported {}",
            from_trace.len(),
            events.len()
        ));
    }
    for (i, (a, b)) in from_trace.iter().zip(events.iter()).enumerate() {
        if !a.deep_eq(b) {
            return Err(format!(
                "the trace self-check failed at event {}: {} != {}",
                i,
                json::write(a),
                json::write(b)
            ));
        }
    }
    if let Some(written) = written {
        let lines = written.lock().map(|v| v.clone()).unwrap_or_default();
        if lines.len() != events.len() {
            return Err(format!(
                "the stream self-check failed: the stream holds {} events, the run reported {}",
                lines.len(),
                events.len()
            ));
        }
        for (i, (line, ev)) in lines.iter().zip(events.iter()).enumerate() {
            if line != &json::write(ev) {
                return Err(format!("the stream self-check failed at event {}", i));
            }
        }
    }
    Ok(())
}
