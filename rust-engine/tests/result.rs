// The run-level result (decision 0022, `run --result`), through the engine
// binary on real processes: what the object holds, where it is written, and
// the exit status that goes with it.
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-rs-result-{}-{}-{}",
            tag,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        Temp { dir }
    }
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.dir.join(name);
        std::fs::write(&p, contents).expect("write file");
        p
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `a` fails on the payload 2 and answers anything else; `b` passes it on and
/// has no outgoing links, so its answers are the run's.
const NET: &str = r#"{
  "version": 0,
  "name": "result-test",
  "neurons": [
    {"id": "s", "description": "in", "source": true},
    {"id": "a", "description": "fails on 2", "run": ["/bin/sh", "-c", "read x; if [ \"$x\" = 2 ]; then exit 7; fi; echo \"{\\\"got\\\":$x}\""]},
    {"id": "b", "description": "the result", "run": ["/bin/cat"]}
  ],
  "links": [{"from": "s", "to": "a"}, {"from": "a", "to": "b"}]
}
"#;

fn run(temp: &Temp, inputs: &str, extra: &[&str]) -> Output {
    temp.write("n.json", NET);
    temp.write("in.jsonl", inputs);
    Command::new(env!("CARGO_BIN_EXE_neuron-engine"))
        .args(["run", "n.json", "--inputs", "in.jsonl"])
        .args(extra)
        .current_dir(&temp.dir)
        .output()
        .expect("the engine runs")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

const MIXED: &str = "{\"at\":0,\"source\":\"s\",\"payload\":1,\"replaces\":\"k1\"}\n{\"at\":0,\"source\":\"s\",\"payload\":2}\n";

#[test]
fn a_failed_call_exits_3_with_the_flag_and_0_without() {
    let temp = Temp::new("exit");
    let with = run(&temp, MIXED, &["--result", "-"]);
    assert_eq!(
        with.status.code(),
        Some(3),
        "with --result: {}",
        String::from_utf8_lossy(&with.stderr)
    );
    let without = run(&temp, MIXED, &[]);
    assert_eq!(without.status.code(), Some(0));
    assert_eq!(out(&without).trim(), "{\"got\":1}", "without the flag nothing is added");
}

#[test]
fn the_object_names_each_traces_answers_and_failures_last_on_stdout() {
    let temp = Temp::new("object");
    let o = run(&temp, MIXED, &["--result", "-"]);
    let text = out(&o);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "{\"got\":1}", "the answers come first: {text}");
    assert_eq!(
        lines[lines.len() - 1],
        concat!(
            "{\"network\":\"result-test\",\"ok\":false,",
            "\"totals\":{\"traces\":2,\"answers\":1,\"errors\":1,\"unknowns\":0,\"drops\":0,\"retried\":0},",
            "\"traces\":[",
            "{\"trace\":\"1\",\"source\":\"s\",\"replaces\":\"k1\",\"status\":\"done\",",
            "\"answers\":[{\"neuron\":\"b\",\"payload\":{\"got\":1}}],\"failures\":[],\"drops\":[]},",
            "{\"trace\":\"2\",\"source\":\"s\",\"status\":\"done\",\"answers\":[],",
            "\"failures\":[{\"type\":\"error\",\"neuron\":\"a\",\"code\":\"exit\",\"message\":\"exit 7: \"}],\"drops\":[]}",
            "]}"
        )
    );
}

#[test]
fn a_run_with_no_failure_exits_0_and_writes_the_file() {
    let temp = Temp::new("file");
    let o = run(&temp, "{\"at\":0,\"source\":\"s\",\"payload\":1}\n", &["--result", "res.json"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o).trim(), "{\"got\":1}", "a file keeps the result off stdout");
    let res = std::fs::read_to_string(temp.dir.join("res.json")).expect("the result file");
    assert!(res.starts_with("{\"network\":\"result-test\",\"ok\":true,"), "{res}");
    assert!(res.contains("\"answers\":[{\"neuron\":\"b\",\"payload\":{\"got\":1}}]"), "{res}");
}

#[test]
fn an_answer_longer_than_the_trace_keeps_is_whole_in_the_result() {
    let temp = Temp::new("whole");
    let long = "x".repeat(3000);
    let o = run(
        &temp,
        &format!("{{\"at\":0,\"source\":\"s\",\"payload\":\"{long}\"}}\n"),
        &["--result", "res.json"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let res = std::fs::read_to_string(temp.dir.join("res.json")).expect("the result file");
    assert!(
        res.contains(&format!("{{\"got\":\"{long}\"}}")),
        "the whole payload, not the trace's 2000 characters"
    );
}

#[test]
fn a_live_run_gives_the_totals_only() {
    let temp = Temp::new("live");
    temp.write("n.json", NET);
    let mut child = Command::new(env!("CARGO_BIN_EXE_neuron-engine"))
        .args(["run", "n.json", "--inputs", "-", "--stream", "stream.jsonl", "--result", "-"])
        .current_dir(&temp.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the engine starts");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        writeln!(stdin, "{{\"source\":\"s\",\"payload\":2}}").unwrap();
        writeln!(stdin, "{{\"source\":\"s\",\"payload\":5}}").unwrap();
    }
    let o = child.wait_with_output().expect("the run ends");
    assert_eq!(o.status.code(), Some(3));
    let text = out(&o);
    assert_eq!(
        text.lines().last().unwrap_or(""),
        "{\"network\":\"result-test\",\"ok\":false,\"totals\":{\"traces\":2,\"answers\":1,\"errors\":1,\"unknowns\":0,\"drops\":0,\"retried\":0}}",
        "{text}"
    );
}
