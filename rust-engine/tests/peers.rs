// The peers of decision 0013, P0, tested end to end: one loopback peer runs a
// `run` neuron the driver's engine would otherwise have run itself, and the run
// is the same run — the same answers on stdout and the same events, wall-clock
// stamps aside. The wire's refusals are tested where they happen: the placement
// at the plant, the token at the handshake, an unreachable peer at the call.
use rust_engine::json::{self, Obj, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
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

/// P2: a placed graph places a graph of its own. `review` runs on one engine,
/// and inside it `audit` — a whole network — runs on another.
const NEST_AUDIT: &str = r#"{
  "version": 0,
  "name": "audit",
  "neurons": [
    {"id": "stamp", "description": "in", "run": ["/bin/sh", "-c", "read line; printf '{\"audited\":%s}' \"$line\""]},
    {"id": "sign", "description": "out", "run": ["/bin/sh", "-c", "read line; printf '{\"signed\":%s}' \"$line\""]}
  ],
  "links": [{"from": "stamp", "to": "sign"}],
  "entry": "stamp",
  "exit": "sign"
}
"#;

const NEST_REVIEW: &str = r#"{
  "version": 0,
  "name": "review",
  "neurons": [
    {"id": "wrap", "description": "tags", "run": ["/bin/sh", "-c", "read line; printf '{\"wrapped\":%s}' \"$line\""]},
    {"id": "audit", "description": "a whole network, run elsewhere", "network": "audit.network.json"}
  ],
  "links": [{"from": "wrap", "to": "audit"}],
  "entry": "wrap",
  "exit": "audit"
}
"#;

const NEST_OUTER: &str = r#"{
  "version": 0,
  "name": "nest-outer",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "review", "description": "a graph on one engine", "network": "review.network.json", "timeout": 20000}
  ],
  "links": [{"from": "src", "to": "review"}]
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

impl Worker {
    /// End the peer now (a machine that died mid-call).
    fn kill(&mut self) {
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

/// A peers file with as many peers as a test needs (the same token for all).
fn peers_file(local: &str, peers: &[(&str, u16)], placement: &str) -> String {
    let list: Vec<String> = peers
        .iter()
        .map(|(name, port)| {
            format!(
                "{{\"name\": \"{}\", \"address\": \"127.0.0.1:{}\", \"token\": \"{}\"}}",
                name, port, TOKEN
            )
        })
        .collect();
    format!(
        "{{\"version\": 0, \"self\": \"{}\", \"peers\": [{}], \"placement\": {}}}",
        local,
        list.join(", "),
        placement
    )
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
        stderr(&out).contains("places 'nope', which this network does not declare"),
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

/// A `network` node and a program worker do not compose: the handshake says so
/// before a call is made, and the outer trace carries the boundary's message.
#[test]
fn a_graph_node_needs_a_graph_peer() {
    let temp = Temp::new("mode");
    let inner = temp.write("inner.network.json", INNER);
    let _ = inner;
    let outer = temp.write("outer.network.json", OUTER);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let port = free_port();
    // A worker that runs programs, and an outer node that is a whole network.
    let _peer = start_worker(port, TOKEN, "b", &[], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let out = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(out.status.success(), "the run itself completes: {}", stderr(&out));
    assert_eq!(stdout(&out), "", "a mismatched peer answers nothing");
    let trace = std::fs::read_to_string(temp.path("trace.json")).expect("the trace");
    assert!(
        trace.contains("speaks as a program peer, and this call needs a graph peer"),
        "the trace reads: {trace}"
    );
    assert!(trace.contains("\"code\":\"inner\""), "the trace reads: {trace}");
}

// -- A cut reaches the peer (P1: the cancel message) ------------------------

/// The network's `replaces` cuts a *running* call on the peer: the driver sends
/// a cancel, the peer stops the program and answers, and the run ends with the
/// same drop/cancel/term notes a local cut writes — not held for the 2 s grace.
const CUT: &str = r#"{
  "version": 0,
  "name": "cut",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "worker", "description": "slow, or quick", "run": ["/bin/sh", "-c", "read line; case \"$line\" in *late*) sleep 5; printf '{\"late\":1}';; *) printf '{\"ok\":1}';; esac"], "timeout": 60000}
  ],
  "links": [{"from": "src", "to": "worker"}]
}
"#;

#[test]
fn a_cut_reaches_the_peer_and_the_run_is_not_held() {
    let temp = Temp::new("cut");
    let net = temp.write("net.json", CUT);
    // Two inputs on one key: the second cuts the first, whose program sleeps.
    let inputs = temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"mode\": \"late\"}, \"replaces\": \"k\"}\n\
         {\"at\": 200, \"source\": \"src\", \"payload\": {\"mode\": \"now\"}, \"replaces\": \"k\"}\n",
    );

    let local = run(&temp, &net, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));

    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"worker\": \"b\"}"));
    let started = std::time::Instant::now();
    let remote = run(
        &temp,
        &net,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    let took = started.elapsed();
    assert!(remote.status.success(), "the peer run failed: {}", stderr(&remote));

    assert_eq!(stdout(&local), stdout(&remote), "the answers differ");
    assert_eq!(stdout(&remote).trim(), "{\"ok\":1}", "the quick answer: {}", stdout(&remote));
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
    let trace = std::fs::read_to_string(temp.path("remote.json")).expect("the trace");
    assert!(trace.contains("\"what\":\"term\""), "the cut signalled the peer: {trace}");
    assert!(
        !trace.contains("\"what\":\"kill\""),
        "the peer stopped on TERM, so no kill is owed: {trace}"
    );
    // The 5 s program was stopped when it was cut: a run held for the grace
    // (2 s) or for the program (5 s) is the failure this test exists for.
    assert!(
        took < std::time::Duration::from_millis(2000),
        "the run took {:?}, so the cancel did not reach the peer",
        took
    );
}

// -- A dead peer, an effect, an `unknown` (P1) ------------------------------

/// An effect call on a peer that dies after it started is `unknown` (11.3), not
/// an error: nobody can tell whether the effect happened, and it is never
/// retried.
const CHARGE: &str = r#"{
  "version": 0,
  "name": "charge",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "charge", "description": "an irreversible call", "run": ["/bin/sh", "-c", "read line; sleep 5; printf '{\"charged\":1}'"], "effect": true, "timeout": 60000}
  ],
  "links": [{"from": "src", "to": "charge"}]
}
"#;

#[test]
fn an_effect_call_on_a_dead_peer_is_unknown() {
    let temp = Temp::new("dead-peer");
    let net = temp.write("net.json", CHARGE);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let port = free_port();
    let mut peer = start_worker(port, TOKEN, "b", &[], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"charge\": \"b\"}"));

    // The run has to be up while the peer dies: spawn it, kill the peer, then
    // read the run's outcome.
    let child = Command::new(engine())
        .arg("run")
        .arg(&net)
        .arg("--inputs")
        .arg(&inputs)
        .arg("--trace")
        .arg(temp.path("trace.json"))
        .arg("--peers")
        .arg(&peers)
        .current_dir(temp.dir.clone())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the engine runs");
    std::thread::sleep(std::time::Duration::from_millis(400));
    peer.kill();
    let out = child.wait_with_output().expect("the run ends");
    assert!(out.status.success(), "the run completes: {}", String::from_utf8_lossy(&out.stderr));

    let trace = std::fs::read_to_string(temp.path("trace.json")).expect("the trace");
    assert!(trace.contains("\"type\":\"unknown\""), "the trace reads: {trace}");
    assert!(trace.contains("\"code\":\"exit\""), "the trace reads: {trace}");
    assert!(!trace.contains("\"type\":\"error\""), "an effect is never an error: {trace}");
    assert_eq!(stdout(&out), "", "an unknown outcome answers nothing");
}

/// P2: placement is a path into the tree, so a composed node inside a hosted
/// graph can be placed too — a graph that places a graph, three engines deep.
/// From outside it is one neuron; inside, it is two hops across two machines.
#[test]
fn a_placed_graph_can_place_a_graph_of_its_own() {
    let temp = Temp::new("nested-graph");
    let audit = temp.write("audit.network.json", NEST_AUDIT);
    let review = temp.write("review.network.json", NEST_REVIEW);
    let outer = temp.write("outer.network.json", NEST_OUTER);
    let inputs = temp.write("inputs.jsonl", INPUTS);

    let local = run(&temp, &outer, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));

    // helper runs review's `wrap`; north hosts the whole `audit` network; east
    // hosts `review` and places both of its nodes.
    let port_h = free_port();
    let _helper = start_worker(port_h, TOKEN, "helper", &[], &[]);
    let port_n = free_port();
    let _north = start_worker(port_n, TOKEN, "north", &[], &["--network", audit.to_str().expect("a path")]);
    let east_peers = temp.write(
        "east.peers.json",
        &peers_file(
            "east",
            &[("helper", port_h), ("north", port_n)],
            "{\"wrap\": \"helper\", \"audit\": \"north\"}",
        ),
    );
    let port_e = free_port();
    let _east = start_worker(
        port_e,
        TOKEN,
        "east",
        &[],
        &[
            "--network",
            review.to_str().expect("a path"),
            "--peers",
            east_peers.to_str().expect("a path"),
        ],
    );
    let peers = temp.write("peers.json", &peers_file("driver", &[("east", port_e)], "{\"review\": \"east\"}"));
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
        stdout(&remote).contains("\"signed\":{\"audited\":{\"wrapped\":{\"n\":1}}}"),
        "the graph did not nest: {}",
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

// -- A late remote outcome (finding 4, 0014) --------------------------------

/// The inner graph whose `exit` answers late: past any short outer timeout.
const LATE_INNER: &str = r#"{
  "version": 0,
  "name": "inner-late",
  "neurons": [
    {"id": "entry", "description": "in", "run": ["/bin/cat"]},
    {"id": "exit", "description": "late", "run": ["/bin/sh", "-c", "sleep 1.2; printf '{\"late\":1}'"]}
  ],
  "links": [{"from": "entry", "to": "exit"}],
  "entry": "entry",
  "exit": "exit"
}
"#;

/// A remote composed call whose inner answer arrives after its timeout has
/// reported: the trace carries the timeout and nothing else, and stdout
/// prints nothing — one outcome per call; the late outcome is cleanup only
/// (finding 4, 0014).
#[test]
fn a_remote_composed_call_never_answers_after_its_timeout() {
    let temp = Temp::new("late-graph");
    let inner = temp.write("inner.network.json", LATE_INNER);
    let net = temp.write(
        "net.json",
        r#"{
  "version": 0,
  "name": "outer-late",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "graph", "description": "a whole network", "network": "inner.network.json", "timeout": 500}
  ],
  "links": [{"from": "src", "to": "graph"}]
}
"#,
    );
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--network", inner.to_str().expect("a path")]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let out = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(out.status.success(), "the run failed: {}", stderr(&out));
    let trace = std::fs::read_to_string(temp.path("trace.json")).expect("the trace");
    assert!(trace.contains("\"code\":\"timeout\""), "the timeout is reported: {trace}");
    assert!(!trace.contains("\"type\":\"answer\""), "the answer came after the timeout: {trace}");
    assert!(!trace.contains("\"late\""), "the late answer is in the trace: {trace}");
    assert_eq!(stdout(&out), "", "the late answer printed: {}", stdout(&out));
}

// -- A cancelled call never runs (finding 1, 0014) ---------------------------

/// When a cancel reaches the worker, in which order it met the call.
#[derive(Debug, Clone, Copy)]
enum CancelWhen {
    /// The cancel is on the wire before the call: the worker must not clear it
    /// when it takes the call.
    Before,
    /// The call and the cancel went out together, the call first: the program
    /// is stopped while it runs, before it can write anything.
    With,
}

/// One raw call on the wire (the audit's probe shape): a handshake, then a
/// cancel and a call for id 1 in the named order. The called program sleeps
/// two seconds and writes its marker, so a run that happened is seen both in
/// the file and in the reply's kind. Returns the reply's outcome kind.
fn raw_cancelled_call(temp: &Temp, port: u16, marker: &str, when: CancelWhen) -> String {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).expect("the peer accepts");
    sock.set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .expect("a read timeout");
    let mut reader = BufReader::new(sock.try_clone().expect("a second handle"));
    let hello = Obj::new()
        .str("type", "hello")
        .int("v", 1)
        .str("peer", "t")
        .str("token", TOKEN)
        .build();
    sock.write_all(format!("{}\n", json::write(&hello)).as_bytes())
        .expect("the hello goes out");
    let mut reply = String::new();
    reader.read_line(&mut reply).expect("the handshake replies");
    let call = Obj::new()
        .str("type", "call")
        .int("id", 1)
        .str("node", "probe")
        .set(
            "argv",
            Value::Arr(vec![
                Value::Str("/bin/sh".into()),
                Value::Str("-c".into()),
                Value::Str(format!("sleep 2; printf ran > {}", marker)),
            ]),
        )
        .set("cwd", Value::Str(temp.dir.to_string_lossy().to_string()))
        .set("env", Value::Arr(Vec::new()))
        .str("input", "{}")
        .int("timeout", 20000)
        .build();
    let cancel = Obj::new().str("type", "cancel").int("id", 1).build();
    let cancel_line = format!("{}\n", json::write(&cancel));
    let call_line = format!("{}\n", json::write(&call));
    match when {
        CancelWhen::Before => {
            sock.write_all(cancel_line.as_bytes()).expect("the cancel goes out");
            sock.write_all(call_line.as_bytes()).expect("the call goes out");
        }
        CancelWhen::With => {
            let mut both = call_line;
            both.push_str(&cancel_line);
            sock.write_all(both.as_bytes()).expect("the lines go out");
        }
    }
    let mut done = String::new();
    reader.read_line(&mut done).expect("the call replies");
    let done = json::parse(done.trim()).expect("the reply is JSON");
    done.get("out")
        .and_then(|o| o.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("no kind")
        .to_string()
}

/// A cancel is a decision about a call id, never cleared (0014 finding 1): a
/// cancel that reaches the worker before (or with) its call stops that call
/// from running, whatever order the wire gave the two.
#[test]
fn a_cancelled_peer_call_never_runs() {
    let temp = Temp::new("cancel-race");
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &[]);
    for (marker, when) in [("ran-before.txt", CancelWhen::Before), ("ran-with.txt", CancelWhen::With)] {
        let kind = raw_cancelled_call(&temp, port, marker, when);
        assert_ne!(kind, "answer", "{:?}: the cancelled call answered: {kind}", when);
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert!(!temp.path(marker).exists(), "{:?}: the cancelled call wrote its marker", when);
    }
}

/// Unused today, kept for the next test that needs a live write: the same
/// helper the desk and http suites use.
#[allow(dead_code)]
fn line(stream: &mut impl Write, text: &str) {
    let _ = writeln!(stream, "{}", text);
}
