// `args` on an entry that names a neuron's file (SPEC 2.10, decision 0022).
// The conformance fixtures script every program, so they cannot see a
// command: these tests start real processes through the engine binary and
// read back the arguments each program received. Then `check` on the
// fixtures: the one that is clean, and each refusal's own message.
use rust_engine::check;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-rs-args-{}-{}-{}",
            tag,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(dir.join("kit")).expect("temp dir");
        Temp { dir }
    }
    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.dir.join(name), contents).expect("write file");
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn prog(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/progs")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// A network of a source and one neuron `a`, written as `a` (the members
/// after its id).
fn network(a: &str) -> String {
    format!(
        "{{\"version\": 0, \"neurons\": [{{\"id\": \"s\", \"source\": true}}, {{\"id\": \"a\", {}}}], \"links\": [{{\"from\": \"s\", \"to\": \"a\"}}]}}",
        a
    )
}

/// The engine's standard output, exit status and standard error for one
/// injection into `net`.
fn run(temp: &Temp, net: &str) -> (String, Option<i32>, String) {
    temp.write("n.json", net);
    temp.write("in.jsonl", "{\"at\":0,\"source\":\"s\",\"payload\":1}\n");
    let o = Command::new(env!("CARGO_BIN_EXE_neuron-engine"))
        .args(["run", "n.json", "--inputs", "in.jsonl"])
        .current_dir(&temp.dir)
        .output()
        .expect("the engine runs");
    (
        String::from_utf8_lossy(&o.stdout).to_string(),
        o.status.code(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

/// The entry's `args` reach the program after the file's own arguments, each
/// as written (a space stays inside its argument: no shell), and the run
/// prints what the neuron written inline with the longer command prints.
fn appended(tag: &str, kind: &str, command: &str) {
    let temp = Temp::new(tag);
    temp.write(
        "kit/a.neuron.json",
        &format!("{{\"version\": 0, \"{}\": [{}, \"own\"]}}", kind, command),
    );
    let (named, code, stderr) = run(
        &temp,
        &network("\"neuron\": \"kit/a.neuron.json\", \"args\": [\"--apply\", \"two words\", \"--apply\"]"),
    );
    assert_eq!(code, Some(0), "{}", stderr);
    assert!(
        named.contains(r#"["own","--apply","two words","--apply"]"#),
        "the program's arguments, in the run's output: {}\n{}",
        named,
        stderr
    );
    let (inline, code, stderr) = run(
        &temp,
        &network(&format!(
            "\"{}\": [{}, \"own\", \"--apply\", \"two words\", \"--apply\"], \"cwd\": \"kit\"",
            kind, command
        )),
    );
    assert_eq!(code, Some(0), "{}", stderr);
    assert_eq!(named, inline, "named by its file with args, and inline with the longer command");
    // Without `args` the file's command is the whole command, as before.
    let (bare, _, stderr) = run(&temp, &network("\"neuron\": \"kit/a.neuron.json\""));
    assert!(bare.contains(r#"["own"]"#), "{}\n{}", bare, stderr);
}

#[test]
fn a_run_program_receives_the_appended_arguments() {
    appended("run", "run", &format!("\"/bin/sh\", \"{}\"", prog("argv.sh")));
}

#[test]
fn a_serve_program_receives_the_appended_arguments() {
    appended("serve", "serve", &format!("\"python3\", \"{}\"", prog("argv_serve.py")));
}

/// A refused entry starts nothing: the engine exits 1 and prints no answer.
#[test]
fn the_engine_refuses_to_start_on_args_beside_a_file_with_no_command() {
    let temp = Temp::new("refused");
    temp.write("kit/a.neuron.json", "{\"version\": 0, \"http\": \"http://localhost:8099/a\"}");
    let (stdout, code, stderr) = run(&temp, &network("\"neuron\": \"kit/a.neuron.json\", \"args\": [\"--apply\"]"));
    assert_eq!(code, Some(1), "{}", stderr);
    assert_eq!(stdout, "");
    assert!(stderr.contains("'args' adds to a command"), "{}", stderr);
}

fn errors(fixture: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("spec/fixtures/neuronfile")
        .join(fixture)
        .join("network.json");
    check::tree(path.to_string_lossy().as_ref())
        .iter()
        .filter(|d| d.error)
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn check_takes_args_on_a_program_file_and_refuses_them_elsewhere() {
    let clean = errors("args-are-appended-to-the-neurons-command");
    assert!(clean.is_empty(), "{}", clean.join("\n"));
    for (fixture, want) in [
        ("refuses-args-on-an-inline-neuron", "option 'args' is not allowed here"),
        (
            "refuses-args-on-an-http-neuron-file",
            "'args' adds to a command, and 'a.neuron.json' is not a run or serve neuron",
        ),
        (
            "refuses-args-on-a-human-neuron-file",
            "'args' adds to a command, and 'a.neuron.json' is not a run or serve neuron",
        ),
        (
            "refuses-args-that-are-not-literals",
            "args must be a non-empty array of literal strings",
        ),
    ] {
        let errs = errors(fixture);
        assert_eq!(errs.len(), 1, "{}: {}", fixture, errs.join("\n"));
        assert!(errs[0].contains(want), "{}: {}", fixture, errs[0]);
    }
}
