// A neuron's own settings (decision 0022, point 6), through the engine binary
// on real processes: `.env` beside the file that declares the neuron fills the
// names in its `env`, wins over the engine's environment, and is never widened.
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-rs-settings-{}-{}-{}",
            tag,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        Temp { dir }
    }
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.dir.join(name);
        std::fs::create_dir_all(p.parent().expect("a parent")).expect("dirs");
        std::fs::write(&p, contents).expect("write file");
        p
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A program that answers what it sees of three names, as JSON.
const SHOW: &str = r#"["/bin/sh", "-c", "printf '{\"color\":\"%s\",\"shape\":\"%s\",\"extra\":\"%s\"}' \"$COLOR\" \"$SHAPE\" \"$EXTRA\""]"#;

fn run(temp: &Temp, command: &str, args: &[&str], vars: &[(&str, &str)]) -> Output {
    temp.write("in.jsonl", "{\"at\":0,\"source\":\"s\",\"payload\":1}\n");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_neuron-engine"));
    cmd.args([command, "n.json"]).args(args).current_dir(&temp.dir);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    cmd.output().expect("the engine runs")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

const RUN: [&str; 2] = ["--inputs", "in.jsonl"];

fn inline(env: &str) -> String {
    format!(
        r#"{{"version":0,"name":"settings","neurons":[{{"id":"s","description":"in","source":true}},
        {{"id":"a","description":"shows","run":{SHOW},"env":{env}}}],"links":[{{"from":"s","to":"a"}}]}}"#
    )
}

#[test]
fn a_listed_name_gets_the_files_value_and_an_unlisted_one_does_not() {
    let temp = Temp::new("listed");
    temp.write("n.json", &inline(r#"["COLOR","SHAPE"]"#));
    temp.write(".env", "# a comment\n\nCOLOR=teal\nSHAPE=\"a round one\"\nEXTRA=never\n");
    let o = run(&temp, "run", &RUN, &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), r#"{"color":"teal","shape":"a round one","extra":""}"#);
}

#[test]
fn the_file_wins_over_the_engines_environment_and_a_name_it_lacks_falls_back() {
    let temp = Temp::new("wins");
    temp.write("n.json", &inline(r#"["COLOR","SHAPE"]"#));
    temp.write(".env", "COLOR=teal\n");
    let o = run(&temp, "run", &RUN, &[("COLOR", "red"), ("SHAPE", "square")]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), r#"{"color":"teal","shape":"square","extra":""}"#);
}

#[test]
fn no_file_is_as_before() {
    let temp = Temp::new("nofile");
    temp.write("n.json", &inline(r#"["COLOR"]"#));
    let o = run(&temp, "run", &RUN, &[("COLOR", "red")]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), r#"{"color":"red","shape":"","extra":""}"#);
}

#[test]
fn two_neurons_with_their_own_files_see_their_own_values() {
    let temp = Temp::new("own");
    for (dir, color) in [("one", "teal"), ("two", "amber")] {
        temp.write(
            &format!("{dir}/show.neuron.json"),
            &format!(r#"{{"version":0,"run":{SHOW},"env":["COLOR"]}}"#),
        );
        temp.write(&format!("{dir}/.env"), &format!("COLOR={color}\n"));
    }
    // The network's own directory has a file too: a neuron named by its file never reads it.
    temp.write(".env", "COLOR=wrong\n");
    temp.write(
        "n.json",
        r#"{"version":0,"name":"settings","neurons":[{"id":"s","description":"in","source":true},
        {"id":"one","neuron":"one/show.neuron.json"},{"id":"two","neuron":"two/show.neuron.json"}],
        "links":[{"from":"s","to":"one"},{"from":"s","to":"two"}]}"#,
    );
    let o = run(&temp, "run", &RUN, &[]);
    assert!(o.status.success(), "{}", err(&o));
    let mut lines: Vec<String> = out(&o).lines().map(str::to_string).collect();
    lines.sort();
    assert_eq!(
        lines,
        [
            r#"{"color":"amber","shape":"","extra":""}"#,
            r#"{"color":"teal","shape":"","extra":""}"#
        ]
    );
}

#[test]
fn a_serve_program_sees_its_own_settings() {
    let temp = Temp::new("serve");
    temp.write(
        "serve.py",
        r#"import json, os, sys
def send(m):
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()
json.loads(sys.stdin.readline())
send({"v": 1, "type": "ready"})
for line in sys.stdin:
    m = json.loads(line)
    if m["type"] == "signal":
        send({"type": "answer", "re": m["re"], "payload": {"color": os.environ.get("COLOR", "")}})
    elif m["type"] == "ping":
        send({"type": "pong", "n": m["n"]})
"#,
    );
    temp.write(
        "n.json",
        r#"{"version":0,"name":"settings","neurons":[{"id":"s","description":"in","source":true},
        {"id":"a","description":"serves","serve":["python3","serve.py"],"env":["COLOR"]}],"links":[{"from":"s","to":"a"}]}"#,
    );
    temp.write(".env", "COLOR='teal'\n");
    let o = run(&temp, "run", &RUN, &[("COLOR", "red")]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), r#"{"color":"teal"}"#);
}

#[test]
fn a_malformed_line_refuses_the_start_and_names_the_line_without_its_text() {
    let temp = Temp::new("bad");
    temp.write("n.json", &inline(r#"["COLOR"]"#));
    temp.write(".env", "COLOR=teal\nthis is a secret sentence\n");
    for (command, args) in [("check", &[][..]), ("run", &RUN[..])] {
        let o = run(&temp, command, args, &[]);
        assert!(!o.status.success(), "{command} should refuse");
        let said = format!("{}{}", out(&o), err(&o));
        assert!(said.contains(".env line 2: not NAME=value"), "{command}: {said}");
        assert!(!said.contains("secret"), "{command} must not print the line: {said}");
        assert!(!said.contains("teal"), "{command} must not print a value: {said}");
    }
}

#[test]
fn a_file_is_not_read_for_a_neuron_that_lists_no_names() {
    let temp = Temp::new("unread");
    temp.write("n.json", &inline("[]").replace(r#","env":[]"#, ""));
    temp.write(".env", "not a setting at all\n");
    let o = run(&temp, "run", &RUN, &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), r#"{"color":"","shape":"","extra":""}"#);
}
