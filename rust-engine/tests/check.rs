//! The `check` command (SPEC 2.7). The refusal fixtures' networks are the
//! contract for the error sets; the spec examples are the contract for none.
//! The fixtures are read as they are, never edited (SPEC 8, M0's rules).

use rust_engine::check;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repo's spec directory, from the crate.
fn spec() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("spec")
}

fn fixture_network(path: &str) -> PathBuf {
    spec().join("fixtures").join(path).join("network.json")
}

fn err_lines(diags: &[rust_engine::net::Diag]) -> Vec<String> {
    diags.iter().filter(|d| d.error).map(|d| d.message.clone()).collect()
}

fn warn_lines(diags: &[rust_engine::net::Diag]) -> Vec<String> {
    diags.iter().filter(|d| !d.error).map(|d| d.message.clone()).collect()
}

/// A budget demands a cost on every non-source: this fixture's network has
/// exactly one such error (the uncosted `b`), and the stand-in executables
/// warn, exactly twice.
#[test]
fn check_refusal_budget_demands_a_cost_on_every_non_source() {
    let path = fixture_network("budget/refuses-a-budget-with-an-uncosted-neuron");
    let diags = check::tree(path.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert_eq!(errs.len(), 1, "{}", errs.join("\n"));
    assert!(
        errs[0].contains("a budget requires a cost on every non-source neuron ('b' has none, 10.1)"),
        "{}",
        errs[0]
    );
    // The position is the uncosted neuron's own line: its id key.
    let b_line = diags.iter().find(|d| d.error).and_then(|d| d.pos).unwrap();
    assert_eq!(b_line.line, 20, "the error sits at the neuron 'b' of the file");
    // The warnings: the two stand-in executables, nothing else.
    let warns = warn_lines(&diags);
    assert_eq!(warns.len(), 2, "{}", warns.join("\n"));
    assert!(warns[0].contains("the executable 'neuron-a' of 'a' is not findable on PATH"));
    assert!(warns[1].contains("the executable 'neuron-b' of 'b' is not findable on PATH"));
}

/// A composed neuron is a neuron: under a budget it needs a cost too. The one
/// error is the composed neuron's; the composed file itself checks otherwise
/// (its own two neurons carry costs) and only warns on its executables.
#[test]
fn check_refusal_an_uncosted_composed_neuron() {
    let path = fixture_network("budget/refuses-a-budget-with-an-uncosted-composed-neuron");
    let diags = check::tree(path.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert_eq!(errs.len(), 1, "{}", errs.join("\n"));
    assert!(
        errs[0].contains("a budget requires a cost on every non-source neuron ('diag' has none, 10.1)"),
        "{}",
        errs[0]
    );
    let warn_count = warn_lines(&diags).len();
    assert_eq!(warn_count, 2, "the inner file's two stand-in executables warn: {}", warn_count);
    // The composed file is walked: every error of it is reported under its
    // own file name (there are none here; the assertion pins the walk).
    assert!(diags
        .iter()
        .all(|d| d.file.as_deref().map(|f| f.ends_with(".json")).unwrap_or(false)));
}

/// A composed file may not contain an effect neuron (11.5): the error is the
/// inner file's, at its own position; the outer file itself checks clean.
#[test]
fn check_refusal_an_effect_inside_a_composed_file() {
    let path = fixture_network("effect/refuses-an-effect-inside-a-composed-file");
    let diags = check::tree(path.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert_eq!(errs.len(), 1, "{}", errs.join("\n"));
    assert!(
        errs[0].contains("a composed file contains a neuron with effect (11.5)"),
        "{}",
        errs[0]
    );
    let error = diags.iter().find(|d| d.error).unwrap();
    assert!(
        error.file.as_deref().unwrap().ends_with("inner.network.json"),
        "the finding is the inner file's"
    );
    assert_eq!(error.pos.map(|p| p.line), Some(10), "the effect neuron's own line");
}

/// The spec examples check clean: no errors (the whole run set of the demos).
#[test]
fn check_examples_have_no_errors() {
    for example in [
        "alerts.network.json",
        "alerts-python.network.json",
        "alerts-python-serve.network.json",
        "alerts-budget.network.json",
        "diagnosis.network.json",
        "notify.network.json",
    ] {
        let path = spec().join("examples").join(example);
        let diags = check::tree(path.to_string_lossy().as_ref());
        assert_eq!(err_lines(&diags), Vec::<String>::new(), "{}: {:?}", example, err_lines(&diags));
        // and the positions, where present, name real lines of the file
        let line_count = std::fs::read_to_string(&path).unwrap().lines().count();
        for d in &diags {
            if let Some(p) = d.pos {
                assert!(p.line <= line_count, "{}: {:?}", example, d);
            }
        }
    }
}

// -- The rest of 2.7, on networks written here (the wordings are the
//    engine's own; the fixtures pin the refusal, not the words). ------------

/// A closed schema: two duplicate ids, two duplicated links, and links to
/// unknown ids (the nearest-id suggestion included) and into the source.
#[test]
fn check_reports_the_graph_errors() {
    let dir = tempfile();
    let file = dir.path().join("net.json");
    std::fs::write(
        &file,
        r#"{
  "version": 0,
  "name": "graph",
  "neurons": [
    {"id": "src", "source": true},
    {"id": "a", "run": ["true"]},
    {"id": "ab", "run": ["true"]},
    {"id": "i", "run": ["true"]},
    {"id": "z", "run": ["true"]}
  ],
  "links": [
    {"from": "src", "to": "aa"},
    {"from": "src", "to": "a"},
    {"from": "src", "to": "a"},
    {"from": "i", "to": "src"},
    {"from": "a", "to": "b"}
  ]
}
"#,
    )
    .unwrap();
    let diags = check::tree(file.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert_eq!(errs.len(), 4, "{}", errs.join("\n"));
    assert!(
        errs[0].contains("links[0]: 'aa' names a neuron the file does not have (the nearest id is 'a')"),
        "{}",
        errs[0]
    );
    assert!(errs[1].contains("links[2]: the link src -> a is duplicated"), "{}", errs[1]);
    assert!(errs[2].contains("links[3]: a link into the source 'src'"), "{}", errs[2]);
    assert!(
        errs[3].contains("links[4]: 'b' names a neuron the file does not have"),
        "{}",
        errs[3]
    );
}

/// A graph errors: a cycle in which no neuron has maxHops (2.7).
#[test]
fn check_reports_a_cycle_without_maxhops() {
    let dir = tempfile();
    let file = dir.path().join("net.json");
    std::fs::write(
        &file,
        r#"{
  "version": 0,
  "name": "loop",
  "neurons": [
    {"id": "src", "source": true},
    {"id": "a", "run": ["true"]},
    {"id": "b", "run": ["true"]},
    {"id": "c", "run": ["true"], "maxHops": 3}
  ],
  "links": [
    {"from": "src", "to": "a"},
    {"from": "a", "to": "b"},
    {"from": "b", "to": "a"},
    {"from": "b", "to": "c"}
  ]
}
"#,
    )
    .unwrap();
    let diags = check::tree(file.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert_eq!(errs.len(), 1, "{}", errs.join("\n"));
    assert!(errs[0].contains("a cycle in which no neuron has maxHops"), "{}", errs[0]);
}

/// The budget and cost amounts: not a whole number, over 2^53 - 1, an empty
/// budget, an empty cost, and a cost unit the budget lacks.
#[test]
fn check_reports_the_amount_rules() {
    let dir = tempfile();
    let file = dir.path().join("net.json");
    std::fs::write(
        &file,
        r#"{
  "version": 0,
  "name": "amounts",
  "budget": {"credits": 5},
  "neurons": [
    {"id": "src", "source": true},
    {"id": "a", "run": ["true"], "cost": {"credits": -2, "gold": 3, "silver": 0}}
  ],
  "links": [{"from": "src", "to": "a"}]
}
"#,
    )
    .unwrap();
    let diags = check::tree(file.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    // The negative amount, the units the budget lacks (gold and silver).
    assert_eq!(errs.len(), 3, "{}", errs.join("\n"));
    assert!(
        errs.iter()
            .any(|m| m.contains("'credits' is not a whole number from 0 to 2^53 - 1")),
        "{}",
        errs.join("\n")
    );
    assert!(
        errs.iter()
            .any(|m| m.contains("the cost of 'a' names the unit 'gold' which the file's budget lacks")),
        "{}",
        errs.join("\n")
    );
    assert!(
        errs.iter()
            .any(|m| m.contains("the cost of 'a' names the unit 'silver' which the file's budget lacks")),
        "{}",
        errs.join("\n")
    );

    // The empty forms.
    let file2 = dir.path().join("net2.json");
    std::fs::write(
        &file2,
        r#"{
  "version": 0,
  "name": "amounts",
  "budget": {},
  "neurons": [
    {"id": "src", "source": true},
    {"id": "a", "run": ["true"], "cost": {}}
  ],
  "links": [{"from": "src", "to": "a"}]
}
"#,
    )
    .unwrap();
    let diags = check::tree(file2.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert_eq!(errs.len(), 2, "{}", errs.join("\n"));
    assert!(errs[0].contains("budget is empty"), "{}", errs[0]);
    assert!(errs[1].contains("cost is empty"), "{}", errs[1]);
}

/// The warnings: a cost above the file's ceiling, and a composed file whose
/// neurons cost a unit the declaring neuron's cost does not name (10.5).
#[test]
fn check_warns_about_costs_that_can_never_run() {
    let dir = tempfile();
    let inner = dir.path().join("inner.network.json");
    std::fs::write(
        &inner,
        r#"{
  "version": 0,
  "entry": "e",
  "exit": "x",
  "neurons": [
    {"id": "e", "run": ["true"], "cost": {"credits": 1}},
    {"id": "x", "run": ["true"], "cost": {"credits": 1}},
    {"id": "w", "run": ["true"], "cost": {"credits": 1, "gold": 1}}
  ],
  "links": [{"from": "e", "to": "x"}]
}
"#,
    )
    .unwrap();
    let file = dir.path().join("net.json");
    std::fs::write(
        &file,
        r#"{
  "version": 0,
  "name": "ceilings",
  "budget": {"credits": 10},
  "neurons": [
    {"id": "src", "source": true},
    {"id": "a", "run": ["true"], "cost": {"credits": 20}},
    {"id": "box", "network": "inner.network.json", "cost": {"credits": 5}}
  ],
  "links": [
    {"from": "src", "to": "a"},
    {"from": "src", "to": "box"}
  ]
}
"#,
    )
    .unwrap();
    let diags = check::tree(file.to_string_lossy().as_ref());
    assert_eq!(err_lines(&diags), Vec::<String>::new(), "{:?}", diags);
    let warns = warn_lines(&diags);
    assert_eq!(warns.len(), 2, "{}", warns.join("\n"));
    assert!(
        warns[0].contains("the cost of 'a' is above the file's ceiling in 'credits' (it can never run)"),
        "{}",
        warns[0]
    );
    assert!(
        warns[1].contains("the file 'inner.network.json' has neurons costing the unit 'gold' which the cost of 'box' does not name"),
        "{}",
        warns[1]
    );
}

/// An `http` neuron is a warning for check and a refusal for run; a missing
/// composed file is an error at the key that names it.
#[test]
fn check_warns_http_and_errors_a_missing_composed_file() {
    let dir = tempfile();
    let file = dir.path().join("net.json");
    std::fs::write(
        &file,
        r#"{
  "version": 0,
  "name": "missing",
  "neurons": [
    {"id": "src", "source": true},
    {"id": "h", "http": "http://127.0.0.1:9/x"},
    {"id": "box", "network": "not-here.network.json"}
  ],
  "links": [
    {"from": "src", "to": "h"},
    {"from": "h", "to": "box"}
  ]
}
"#,
    )
    .unwrap();
    let diags = check::tree(file.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert!(
        errs.len() == 1 && errs[0].contains("the network file 'not-here.network.json' is missing"),
        "{}",
        errs.join("\n")
    );
    let warns = warn_lines(&diags);
    assert!(
        warns.len() == 1 && warns[0].contains("an http neuron does not run yet"),
        "{}",
        warns.join("\n")
    );
}

/// The composed file rules (2.6): a source inside, an exit with outgoing
/// links, entry/exit existence and pairing. The rules hold for the file USED
/// as composed (the wrapper names it), so the errors are the inner file's.
#[test]
fn check_reports_the_composed_file_rules() {
    let dir = tempfile();
    let inner = dir.path().join("inner.json");
    std::fs::write(
        &inner,
        r#"{
  "version": 0,
  "name": "box",
  "entry": "e",
  "exit": "x",
  "neurons": [
    {"id": "s", "source": true},
    {"id": "e", "run": ["true"]},
    {"id": "x", "run": ["true"]},
    {"id": "w", "run": ["true"]}
  ],
  "links": [{"from": "x", "to": "w"}, {"from": "e", "to": "x"}]
}
"#,
    )
    .unwrap();
    let outer = dir.path().join("outer.json");
    std::fs::write(
        &outer,
        r#"{
  "version": 0,
  "name": "outer",
  "neurons": [
    {"id": "src", "source": true},
    {"id": "box", "network": "inner.json"}
  ],
  "links": [{"from": "src", "to": "box"}]
}
"#,
    )
    .unwrap();
    let diags = check::tree(outer.to_string_lossy().as_ref());
    let errs = err_lines(&diags);
    assert_eq!(errs.len(), 2, "{}", errs.join("\n"));
    assert!(errs[0].contains("a composed file contains a source"), "{}", errs.join("\n"));
    assert!(errs[1].contains("exit has outgoing links"), "{}", errs.join("\n"));
    let diags_inner: Vec<&rust_engine::net::Diag> = diags
        .iter()
        .filter(|d| d.error)
        .filter(|d| d.file.as_deref().unwrap_or("").ends_with("inner.json"))
        .collect();
    assert_eq!(diags_inner.len(), 2, "the findings are the inner file's: {:?}", diags);
}

/// The command line: exit 0 with no error, 1 with one, 2 for usage and for a
/// file that cannot be read; an http neuron does not fail a check.
#[test]
fn check_command_exits() {
    let dir = tempfile();
    let clean = dir.path().join("clean.json");
    std::fs::write(
        &clean,
        r#"{"version": 0, "name": "clean", "neurons": [{"id": "src", "source": true}], "links": []}"#,
    )
    .unwrap();
    let http = dir.path().join("http.json");
    std::fs::write(
        &http,
        r#"{
  "version": 0,
  "name": "http",
  "neurons": [
    {"id": "src", "source": true},
    {"id": "h", "http": "http://127.0.0.1:9/x"}
  ],
  "links": [{"from": "src", "to": "h"}]
}
"#,
    )
    .unwrap();
    let bad = dir.path().join("bad.json");
    std::fs::write(&bad, r#"{"version": 0, "name": "bad", "neurons": [], "links": []}"#).unwrap();
    let engine = env!("CARGO_BIN_EXE_neuron-engine");
    let run = |args: &[&str]| {
        let out = Command::new(engine).arg("check").args(args).output().unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stdout).to_string())
    };
    let (code, _) = run(&[clean.to_string_lossy().as_ref()]);
    assert_eq!(code, Some(0));
    let (code, out) = run(&[http.to_string_lossy().as_ref()]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, _) = run(&[bad.to_string_lossy().as_ref()]);
    assert_eq!(code, Some(1));
    let (code, _) = run(&["/no/such/file.json"]);
    assert_eq!(code, Some(2));
    let (code, _) = run(&[]);
    assert_eq!(code, Some(2));
    let (code, _) = run(&["a", "b"]);
    assert_eq!(code, Some(2));
}

// -- A scratch directory the files above are written to -------------------

struct Temp {
    dir: PathBuf,
}

fn tempfile() -> Temp {
    let dir = std::env::temp_dir().join(format!(
        "neuron-check-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    Temp { dir }
}

impl Temp {
    /// The scratch directory itself, for files written by name.
    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
