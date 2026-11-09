// The peers of decision 0013, P0, tested end to end: one loopback peer runs a
// `run` neuron the driver's engine would otherwise have run itself, and the run
// is the same run — the same answers on stdout and the same events, wall-clock
// stamps aside. The wire's refusals are tested where they happen: the placement
// at the plant, the token at the handshake, an unreachable peer at the call.
use rust_engine::json::{self, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

const NET: &str = r#"{
  "version": 0,
  "name": "peer-test",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "worker", "description": "echo", "run": ["/bin/cat"]}
  ],
  "links": [{"from": "src", "to": "worker"}]
}
"#;

const NET_ENV: &str = r#"{
  "version": 0,
  "name": "peer-env",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "worker", "description": "where and with what", "run": ["/bin/sh", "-c", "printf '{\"x\":\"%s\",\"d\":\"%s\"}' \"$PEER_X\" \"$PWD\""], "env": ["PEER_X"]}
  ],
  "links": [{"from": "src", "to": "worker"}]
}
"#;

const INPUTS: &str = "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n";

/// A graph a peer hosts as one neuron (decision 0013): composed-shaped — an
/// `entry` to inject at, an `exit` that answers, no source (2.6).
const INNER: &str = r#"{
  "version": 0,
  "name": "inner",
  "neurons": [
    {"id": "entry", "description": "in", "run": ["/bin/cat"]},
    {"id": "exit", "description": "tag", "run": ["/bin/sh", "-c", "read line; printf '{\"tagged\":%s}' \"$line\""]}
  ],
  "links": [{"from": "entry", "to": "exit"}],
  "entry": "entry",
  "exit": "exit"
}
"#;

const INNER_FAIL: &str = r#"{
  "version": 0,
  "name": "inner-fail",
  "neurons": [
    {"id": "entry", "description": "in", "run": ["/bin/cat"]},
    {"id": "exit", "description": "fails", "run": ["/bin/sh", "-c", "exit 3"]}
  ],
  "links": [{"from": "entry", "to": "exit"}],
  "entry": "entry",
  "exit": "exit"
}
"#;

/// The graph that is itself spread over peers: `worker` lives elsewhere, and
/// from outside the whole thing is one neuron.
const INNER_P2P: &str = r#"{
  "version": 0,
  "name": "inner-p2p",
  "neurons": [
    {"id": "worker", "description": "tag", "run": ["/bin/sh", "-c", "read line; printf '{\"a\":%s}' \"$line\""]},
    {"id": "finish", "description": "wrap", "run": ["/bin/sh", "-c", "read line; printf '{\"wrapped\":%s}' \"$line\""]}
  ],
  "links": [{"from": "worker", "to": "finish"}],
  "entry": "worker",
  "exit": "finish"
}
"#;

/// The outer network: one node, a whole network.
const OUTER: &str = r#"{
  "version": 0,
  "name": "outer",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "graph", "description": "a whole network", "network": "inner.network.json", "timeout": 10000}
  ],
  "links": [{"from": "src", "to": "graph"}]
}
"#;

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-peer-{}-{}-{}",
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

fn engine() -> &'static str {
    env!("CARGO_BIN_EXE_neuron-engine")
}

/// A port nothing listens on: bound once, then given up (the tests run one at a
/// time: `RUST_TEST_THREADS = 1`).
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("a port").port();
    drop(listener);
    port
}

/// A peer process, readied: the worker's listening line is the ready signal.
/// The stderr pipe is kept open for the peer's life: its diagnostics go there,
/// and a closed reader would make the next log line fail.
struct Worker {
    child: Child,
    _stderr: BufReader<std::process::ChildStderr>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_worker(port: u16, token: &str, name: &str, env: &[(&str, &str)], extra: &[&str]) -> Worker {
    let mut cmd = Command::new(engine());
    cmd.arg("peer")
        .arg("--listen")
        .arg(format!("127.0.0.1:{}", port))
        .arg("--token")
        .arg(token)
        .arg("--name")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for a in extra {
        cmd.arg(a);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("the peer starts");
    let stderr = child.stderr.take().expect("piped stderr");
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    reader.read_line(&mut line).expect("the peer's readiness line");
    assert!(line.contains("listening"), "the peer said: {line}");
    Worker { child, _stderr: reader }
}

fn peers_text(port: u16, token: &str, placement: &str) -> String {
    peers_text_named("a", "b", port, token, placement)
}

fn peers_text_named(local: &str, name: &str, port: u16, token: &str, placement: &str) -> String {
    format!(
        "{{\"version\": 0, \"self\": \"{}\", \"peers\": [{{\"name\": \"{}\", \"address\": \"127.0.0.1:{}\", \"token\": \"{}\"}}], \"placement\": {}}}",
        local, name, port, token, placement
    )
}

fn run(temp: &Temp, net: &Path, inputs: &Path, trace: &Path, extra: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(engine());
    cmd.arg("run").arg(net).arg("--inputs").arg(inputs).arg("--trace").arg(trace);
    for a in extra {
        cmd.arg(a);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.current_dir(temp.dir.clone());
    cmd.output().expect("the engine runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// The trace's events with every `t` zeroed (two real runs stamp wall-clock
/// milliseconds) and its ids renormalised to `#1`, `#2`, ... by first
/// appearance (CONFORMANCE 8.3's rule): ids are opaque and per engine, and a
/// graph hosted elsewhere does not draw on this engine's counter.
fn events(path: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(path).expect("the trace was written");
    let v = json::parse(&text).expect("the trace is JSON");
    let Some(Value::Arr(items)) = v.get("events") else {
        panic!("the trace has no events");
    };
    let mut names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for e in items {
        let Value::Obj(entries) = e else {
            out.push(e.clone());
            continue;
        };
        let mut renamed = Vec::new();
        for (k, v) in entries {
            if k == "t" {
                renamed.push((k.clone(), Value::Int(0)));
            } else if matches!(k.as_str(), "id" | "trace" | "replyTo" | "cause" | "trigger") {
                if let Value::Str(s) = v {
                    let cur = s.clone();
                    let n = match names.iter().position(|x| x == &cur) {
                        Some(i) => i,
                        None => {
                            names.push(cur);
                            names.len() - 1
                        }
                    };
                    renamed.push((k.clone(), Value::Str(format!("#{}", n + 1))));
                } else {
                    renamed.push((k.clone(), v.clone()));
                }
            } else {
                renamed.push((k.clone(), v.clone()));
            }
        }
        out.push(Value::Obj(renamed));
    }
    out
}

#[test]
fn a_peer_call_is_the_local_call() {
    let temp = Temp::new("same");
    let net = temp.write("net.json", NET);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let local = run(&temp, &net, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));

    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"worker\": \"b\"}"));
    let remote = run(
        &temp,
        &net,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(remote.status.success(), "the peer run failed: {}", stderr(&remote));

    assert_eq!(stdout(&local), stdout(&remote), "the answers differ");
    assert!(stdout(&remote).contains("\"n\":1"), "the answer: {}", stdout(&remote));
    let a = events(&temp.path("local.json"));
    let b = events(&temp.path("remote.json"));
    assert_eq!(a.len(), b.len(), "the event counts differ: {:?} vs {:?}", a, b);
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            x.deep_eq(y),
            "event {} differs:\n  local: {}\n  peer:  {}",
            i,
            json::write(x),
            json::write(y)
        );
    }
}

#[test]
fn a_peer_call_carries_the_working_directory_and_the_environment() {
    let temp = Temp::new("env");
    let net = temp.write("net.json", NET_ENV);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[("PEER_X", "hello-from-the-peer")], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"worker\": \"b\"}"));
    let out = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[("PEER_X", "hello-from-the-peer")],
    );
    assert!(out.status.success(), "the run failed: {}", stderr(&out));
    let said = stdout(&out);
    assert!(said.contains("hello-from-the-peer"), "the env did not cross: {said}");
    assert!(
        said.contains(&temp.dir.to_string_lossy().to_string()),
        "the cwd did not cross: {said}"
    );
}

#[test]
fn a_placement_of_an_unknown_neuron_refuses_the_run() {
    let temp = Temp::new("unknown");
    let net = temp.write("net.json", NET);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let peers = temp.write("peers.json", &peers_text(free_port(), TOKEN, "{\"nope\": \"b\"}"));
    let out = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(!out.status.success(), "a bad placement must refuse the run");
    assert!(
        stderr(&out).contains("places 'nope', which the network does not declare"),
        "the refusal reads: {}",
        stderr(&out)
    );
}

#[test]
fn a_wrong_token_fails_the_call() {
    let temp = Temp::new("token");
    let net = temp.write("net.json", NET);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let port = free_port();
    let _peer = start_worker(port, "ffffffffffffffffffffffffffffffff", "b", &[], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"worker\": \"b\"}"));
    let out = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    // The call fails inside the run (an `error` event, not a refused run), and
    // the run's answers are empty.
    let trace = std::fs::read_to_string(temp.path("trace.json")).expect("the trace was written");
    assert!(trace.contains("the token does not match"), "the trace reads: {trace}");
    assert_eq!(stdout(&out), "", "nothing answers behind a refused handshake");
}

#[test]
fn a_peer_that_is_not_there_fails_the_call() {
    let temp = Temp::new("down");
    let net = temp.write("net.json", NET);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let peers = temp.write("peers.json", &peers_text(free_port(), TOKEN, "{\"worker\": \"b\"}"));
    let _ = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    let trace = std::fs::read_to_string(temp.path("trace.json")).expect("the trace was written");
    assert!(trace.contains("exit 127: peer b: cannot reach"), "the trace reads: {trace}");
}

// -- A whole network on a peer (the composed boundary) ----------------------

/// One node of the outer network is an entire graph, run by a peer: from
/// outside it is one neuron, and the run is the run a local composed neuron
/// gives — same answer, same events.
#[test]
fn a_peer_hosts_a_whole_network_as_one_neuron() {
    let temp = Temp::new("graph");
    let inner = temp.write("inner.network.json", INNER);
    let outer = temp.write("outer.network.json", OUTER);
    let inputs = temp.write("inputs.jsonl", INPUTS);

    let local = run(&temp, &outer, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));

    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--network", inner.to_str().expect("a path")]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let remote = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(remote.status.success(), "the peer run failed: {}", stderr(&remote));

    assert_eq!(stdout(&local), stdout(&remote), "the answers differ");
    assert!(
        stdout(&remote).contains("\"tagged\":{\"n\":1}"),
        "the graph did not run: {}",
        stdout(&remote)
    );
    let a = events(&temp.path("local.json"));
    let b = events(&temp.path("remote.json"));
    assert_eq!(a.len(), b.len(), "the event counts differ: {:?} vs {:?}", a, b);
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            x.deep_eq(y),
            "event {} differs:\n  local: {}\n  peer:  {}",
            i,
            json::write(x),
            json::write(y)
        );
    }
}

/// A graph that fails inside answers the composed neuron's own error, exactly
/// as a local one does: the inner cause does not cross the boundary (sealing).
#[test]
fn a_peer_graph_that_fails_is_the_inner_error() {
    let temp = Temp::new("graph-fail");
    let inner = temp.write("inner.network.json", INNER_FAIL);
    let outer = temp.write("outer.network.json", OUTER);
    let inputs = temp.write("inputs.jsonl", INPUTS);

    let local = run(&temp, &outer, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--network", inner.to_str().expect("a path")]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let remote = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(remote.status.success(), "the peer run failed: {}", stderr(&remote));

    let a = events(&temp.path("local.json"));
    let b = events(&temp.path("remote.json"));
    assert_eq!(a.len(), b.len(), "the event counts differ: {:?} vs {:?}", a, b);
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            x.deep_eq(y),
            "event {} differs:\n  local: {}\n  peer:  {}",
            i,
            json::write(x),
            json::write(y)
        );
    }
    let text = std::fs::read_to_string(temp.path("remote.json")).expect("the trace");
    assert!(text.contains("\"code\":\"inner\""), "the trace reads: {text}");
    assert!(text.contains("no answer: an inner neuron failed"), "the trace reads: {text}");
    assert_eq!(stdout(&remote), "", "a failing graph answers nothing");
}

/// The composed neuron is itself spread over peers: the outer run places one
/// neuron, that neuron's graph places another on a third engine, and the
/// answer is the one a wholly local run gives.
#[test]
fn a_graph_hosted_by_a_peer_can_itself_span_peers() {
    let temp = Temp::new("nested");
    let inner = temp.write("inner.network.json", INNER_P2P);
    let outer = temp.write("outer.network.json", OUTER);
    let inputs = temp.write("inputs.jsonl", INPUTS);

    let local = run(&temp, &outer, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));

    // Peer c runs the graph's `worker` program; peer b hosts the graph and
    // places that node on c.
    let port_c = free_port();
    let _worker_c = start_worker(port_c, TOKEN, "c", &[], &[]);
    let inner_peers = temp.write(
        "inner.peers.json",
        &peers_text_named("b", "c", port_c, TOKEN, "{\"worker\": \"c\"}"),
    );
    let port_b = free_port();
    let _peer_b = start_worker(
        port_b,
        TOKEN,
        "b",
        &[],
        &[
            "--network",
            inner.to_str().expect("a path"),
            "--peers",
            inner_peers.to_str().expect("a path"),
        ],
    );
    let peers = temp.write("peers.json", &peers_text(port_b, TOKEN, "{\"graph\": \"b\"}"));
    let remote = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(remote.status.success(), "the nested run failed: {}", stderr(&remote));
    assert_eq!(stdout(&local), stdout(&remote), "the answers differ");
    assert!(
        stdout(&remote).contains("\"wrapped\":{\"a\":{\"n\":1}}"),
        "the graph did not run across peers: {}",
        stdout(&remote)
    );
}

/// The peers file itself: closed, and its refusals name the field.
#[test]
fn the_peers_file_is_closed_and_checked() {
    let temp = Temp::new("file");
    let net = temp.write("net.json", NET);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    for (name, text, want) in [
        ("bad-field.json", "{\"version\": 0, \"peers\": [], \"placement\": {}, \"who\": 1}".to_string(), "unknown field 'who'"),
        ("no-peers.json", "{\"version\": 0, \"placement\": {}}".to_string(), "has no `peers` array"),
        ("bad-token.json", "{\"version\": 0, \"peers\": [{\"name\": \"b\", \"address\": \"127.0.0.1:1\", \"token\": \"zz\"}], \"placement\": {}}".to_string(), "not 16 to 128 hex"),
        ("bad-peer.json", "{\"version\": 0, \"peers\": [{\"name\": \"b\", \"address\": \"127.0.0.1:1\", \"token\": \"0123456789abcdef\"}], \"placement\": {\"worker\": \"c\"}}".to_string(), "which is not a peer"),
    ] {
        let peers = temp.write(name, &text);
        let out = run(
            &temp,
            &net,
            &inputs,
            &temp.path("trace.json"),
            &["--peers", peers.to_str().expect("a path")],
            &[],
        );
        assert!(!out.status.success(), "{name} was accepted");
        assert!(stderr(&out).contains(want), "{name} said: {}", stderr(&out));
    }
}

/// Unused today, kept for the next test that needs a live write: the same
/// helper the desk and http suites use.
#[allow(dead_code)]
fn line(stream: &mut impl Write, text: &str) {
    let _ = writeln!(stream, "{}", text);
}
