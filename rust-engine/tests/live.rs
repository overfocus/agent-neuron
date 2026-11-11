// A live run (decision 0020, S4b): `run <file> --inputs -` takes injections
// from standard input as they arrive and stays up until it closes. Tested
// through the engine binary, with the test as the producer: it writes a line,
// reads the answer while standard input is still open, and only then goes on.
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-live-{}-{}-{}",
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
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn progs(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/progs")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// `src -> a`, where `a` is the neuron given (its kind and command, as JSON
/// members).
fn network(a: &str) -> String {
    format!(
        "{{\"version\": 0, \"name\": \"live\", \"neurons\": [{{\"id\": \"src\", \"source\": true}}, {{\"id\": \"a\", {}}}], \"links\": [{{\"from\": \"src\", \"to\": \"a\"}}]}}",
        a
    )
}

/// An echo in sh: the payload back, unchanged.
const ECHO: &str = "\"run\": [\"/bin/cat\"]";

struct Live {
    child: Child,
    stdin: Option<ChildStdin>,
    answers: Receiver<String>,
    refusals: Receiver<String>,
}

impl Live {
    fn start(temp: &Temp, net: &str, extra: &[&str]) -> Live {
        let net = temp.write("net.json", net);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_neuron-engine"));
        cmd.arg("run").arg(&net).arg("--inputs").arg("-");
        for a in extra {
            cmd.arg(a);
        }
        let mut child = cmd
            .current_dir(&temp.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the engine starts");
        let lines = |r: Box<dyn std::io::Read + Send>| {
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                for line in BufReader::new(r).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
            rx
        };
        let answers = lines(Box::new(child.stdout.take().expect("stdout")));
        let refusals = lines(Box::new(child.stderr.take().expect("stderr")));
        let stdin = child.stdin.take();
        Live {
            child,
            stdin,
            answers,
            refusals,
        }
    }
    fn say(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        writeln!(stdin, "{}", line).expect("the engine reads");
        stdin.flush().expect("flush");
    }
    fn answer(&self) -> String {
        self.answers
            .recv_timeout(Duration::from_secs(10))
            .expect("an answer while the run is still up")
    }
    fn refusal(&self) -> String {
        loop {
            let line = self.refusals.recv_timeout(Duration::from_secs(10)).expect("a line on stderr");
            if line.starts_with("refused an injection") {
                return line;
            }
        }
    }
    fn running(&mut self) -> bool {
        self.child.try_wait().expect("try_wait").is_none()
    }
    /// Close standard input and wait for the run to end.
    fn finish(mut self) -> (i32, Vec<String>) {
        drop(self.stdin.take());
        let status = self.child.wait().expect("the run ends");
        let mut rest = Vec::new();
        while let Ok(line) = self.refusals.recv_timeout(Duration::from_millis(500)) {
            rest.push(line);
        }
        (status.code().unwrap_or(-1), rest)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn an_injection_is_answered_while_the_run_is_still_up() {
    let temp = Temp::new("answers");
    let stream = temp.path("stream.jsonl");
    let mut live = Live::start(&temp, &network(ECHO), &["--stream", &stream.to_string_lossy()]);
    live.say("{\"source\": \"src\", \"payload\": {\"n\": 1}}");
    assert_eq!(live.answer(), "{\"n\":1}");
    // Quiet now, and standard input is still open: the run waits.
    std::thread::sleep(Duration::from_millis(400));
    assert!(live.running(), "a quiet live run stays up");
    live.say("{\"source\": \"src\", \"payload\": \"second\"}");
    assert_eq!(live.answer(), "second");
    // The stream has the first trace's events before the run is over.
    let so_far = std::fs::read_to_string(&stream).expect("the stream");
    assert!(so_far.matches("\"type\":\"finished\"").count() >= 1, "{}", so_far);
    let (code, stderr) = live.finish();
    assert_eq!(code, 0, "{}", stderr.join("\n"));
    assert!(stderr.iter().any(|l| l.starts_with("ran live: ")), "{}", stderr.join("\n"));
    let all = std::fs::read_to_string(&stream).expect("the stream");
    assert_eq!(all.matches("\"type\":\"finished\"").count(), 2, "{}", all);
    assert_eq!(all.matches("\"type\":\"answer\"").count(), 2, "{}", all);
}

#[test]
fn a_serve_neuron_keeps_its_state_across_live_injections() {
    let temp = Temp::new("state");
    let stream = temp.path("stream.jsonl");
    let net = network(&format!("\"serve\": [\"python3\", \"{}\"]", progs("counter_serve.py")));
    let mut live = Live::start(&temp, &net, &["--stream", &stream.to_string_lossy()]);
    for want in 1..=3 {
        live.say("{\"source\": \"src\", \"payload\": 1}");
        assert_eq!(live.answer(), format!("{{\"count\":{}}}", want));
        std::thread::sleep(Duration::from_millis(100));
    }
    let (code, stderr) = live.finish();
    assert_eq!(code, 0, "{}", stderr.join("\n"));
}

#[test]
fn a_line_the_engine_refuses_does_not_end_the_run() {
    let temp = Temp::new("refusals");
    let stream = temp.path("stream.jsonl");
    let mut live = Live::start(&temp, &network(ECHO), &["--stream", &stream.to_string_lossy()]);
    for (line, why) in [
        ("{ not json", "not JSON"),
        (
            "{\"at\": 5, \"source\": \"src\", \"payload\": 1}",
            "`at` counts from the start of a run",
        ),
        (
            "{\"source\": \"nobody\", \"payload\": 1}",
            "it names 'nobody', which is not a source",
        ),
        ("{\"source\": \"a\", \"payload\": 1}", "it names 'a', which is not a source"),
        (
            "{\"source\": \"src\", \"payload\": 1, \"budget\": {\"credits\": 1}}",
            "the injection carries a budget but the file has none",
        ),
    ] {
        live.say(line);
        let refusal = live.refusal();
        assert!(refusal.contains(why), "{} -> {}", line, refusal);
    }
    live.say("{\"source\": \"src\", \"payload\": \"still here\"}");
    assert_eq!(live.answer(), "still here");
    let (code, _) = live.finish();
    assert_eq!(code, 0);
    let all = std::fs::read_to_string(&stream).expect("the stream");
    assert_eq!(
        all.matches("\"type\":\"signal\"").count(),
        1,
        "only the good line was injected: {}",
        all
    );
}

#[test]
fn a_live_key_replaces_as_a_scheduled_one_does() {
    let temp = Temp::new("replaces");
    let stream = temp.path("stream.jsonl");
    // `a` takes a second to answer; a newer input with the same key cuts it.
    let slow = "\"run\": [\"/bin/sh\", \"-c\", \"/bin/sleep 1; /bin/cat\"]";
    let mut live = Live::start(&temp, &network(slow), &["--stream", &stream.to_string_lossy()]);
    live.say("{\"source\": \"src\", \"payload\": \"old\", \"replaces\": \"k\"}");
    std::thread::sleep(Duration::from_millis(200));
    live.say("{\"source\": \"src\", \"payload\": \"new\", \"replaces\": \"k\"}");
    assert_eq!(live.answer(), "new");
    let (code, _) = live.finish();
    assert_eq!(code, 0);
    let all = std::fs::read_to_string(&stream).expect("the stream");
    assert!(all.contains("\"reason\":\"replaced\",\"phase\":\"running\""), "{}", all);
    assert_eq!(all.matches("\"type\":\"answer\"").count(), 1, "{}", all);
}

#[test]
fn a_live_run_takes_a_stream_and_not_a_trace() {
    let temp = Temp::new("flags");
    let net = temp.write("net.json", &network(ECHO));
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_neuron-engine"))
            .arg("run")
            .arg(&net)
            .arg("--inputs")
            .arg("-")
            .args(extra)
            .current_dir(&temp.dir)
            .stdin(Stdio::null())
            .output()
            .expect("the engine runs")
    };
    for extra in [
        &[][..],
        &["--trace", "t.json"][..],
        &["--stream", "s.jsonl", "--trace", "t.json"][..],
    ] {
        let out = run(extra);
        assert_eq!(out.status.code(), Some(1), "{:?}", extra);
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("takes --stream <path> and not --trace"),
            "{:?}",
            extra
        );
    }
    // With a stream and standard input already closed: an empty run, over at once.
    let out = run(&["--stream", "s.jsonl"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
}
