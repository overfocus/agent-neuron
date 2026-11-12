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
    /// The next line the peer wrote on its stderr.
    fn stderr_line(&mut self) -> String {
        let mut line = String::new();
        self._stderr.read_line(&mut line).expect("the peer's stderr");
        line
    }

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

/// The occurrences of a needle in the outputs the run gave, the way the real
/// suite counts a line of stdout.
fn count(hay: &str, needle: &str) -> usize {
    hay.matches(needle).count()
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

// -- The composed boundary of one hosted instance (finding 3, 0014) ----------

/// A program of `tests/progs/`, as the suites use it (the peers tests need
/// one again: 0014's finding 3 probes a stateful serve across calls).
fn progs(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/progs")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// The graph the audit probed state with: a counter whose count is the serve
/// process's own. One shared inner network per instance (2.6) means the count
/// says what the instance kept: two calls see 1 then 2. An engine planted per
/// call restarts the process and restates 1.
const STATEFUL_OUTER: &str = r#"{
  "version": 0,
  "name": "stateful-outer",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "graph", "description": "a stateful network", "network": "stateful-inner.network.json", "timeout": 20000}
  ],
  "links": [{"from": "src", "to": "graph"}]
}
"#;

const STATEFUL_INPUTS: &str = "{\"at\": 0, \"source\": \"src\", \"payload\": {}}\n{\"at\": 500, \"source\": \"src\", \"payload\": {}}\n";

#[test]
fn a_hosted_graph_keeps_its_inner_serve_state_across_calls() {
    let temp = Temp::new("graph-state");
    let inner = temp.write(
        "stateful-inner.network.json",
        &format!(
            r#"
{{
  "version": 0,
  "name": "stateful-inner",
  "entry": "entry",
  "exit": "counter",
  "neurons": [
    {{"id": "entry", "description": "input", "run": ["/bin/cat"]}},
    {{"id": "counter", "description": "remembers calls", "serve": ["python3", "{}"], "timeout": 20000}}
  ],
  "links": [{{"from": "entry", "to": "counter"}}]
}}
"#,
            progs("counter_serve.py"),
        ),
    );
    let outer = temp.write("outer.network.json", STATEFUL_OUTER);
    let inputs = temp.write("inputs.jsonl", STATEFUL_INPUTS);

    // The same two calls, one local run: the local composed boundary is the
    // reference answer (the instance is one, so the count climbs).
    let local = run(&temp, &outer, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));
    assert!(stdout(&local).contains("\"count\":1"), "the first call: {}", stdout(&local));
    assert!(
        stdout(&local).contains("\"count\":2"),
        "the same state seen twice: {}",
        stdout(&local)
    );

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

    // The hosted instance is one engine (2.6): the second call enters the
    // serve process the first call started, so it reads 2 — a fresh engine
    // per call restates 1.
    assert_eq!(stdout(&local), stdout(&remote), "the answers differ");
    assert!(stdout(&remote).contains("\"count\":1"), "the first call: {}", stdout(&remote));
    assert!(
        stdout(&remote).contains("\"count\":2"),
        "the second call saw the first's state: {}",
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

/// The graph whose `exit` answers while a sibling still runs: the first reply
/// of `exit` is the call's answer and the remaining inner work is cancelled
/// (2.6 step 2) — the sibling's program is stopped before it can write its
/// late marker, the way a local composed cut stops it (the composed-cut test
/// of the real suite, finding 2's style, watched from here through the wire).
const CUT_OUTER: &str = r#"{
  "version": 0,
  "name": "cut-outer",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "graph", "description": "a whole network", "network": "cut-inner.network.json", "timeout": 20000}
  ],
  "links": [{"from": "src", "to": "graph"}]
}
"#;

#[test]
fn a_hosted_graphs_first_exit_reply_cuts_the_work_running_inside() {
    let temp = Temp::new("graph-cut");
    let marker = "late-ran.txt";
    let _ = std::fs::remove_file(temp.path(marker));
    let inner = temp.write(
        "cut-inner.network.json",
        &format!(
            r#"
{{
  "version": 0,
  "name": "inner-cut",
  "neurons": [
    {{"id": "entry", "description": "in", "run": ["/bin/cat"]}},
    {{"id": "slow", "description": "a slow sibling", "run": ["/bin/sh", "-c", "read line; sleep 1; printf ran > {marker}"]}},
    {{"id": "exit", "description": "answers first", "run": ["/bin/sh", "-c", "read line; sleep 0.3; printf '{{\"fast\":1}}'"]}}
  ],
  "links": [{{"from": "entry", "to": "slow"}}, {{"from": "entry", "to": "exit"}}],
  "entry": "entry",
  "exit": "exit"
}}
"#,
            marker = temp.path(marker).to_string_lossy(),
        ),
    );
    let outer = temp.write("outer.network.json", CUT_OUTER);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--network", inner.to_str().expect("a path")]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let out = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(out.status.success(), "the run failed: {}", stderr(&out));
    // The first reply of `exit` is the whole answer of the call: nothing else
    // of the inside crosses (2.6 step 2, sealing).
    assert_eq!(stdout(&out).trim(), "{\"fast\":1}", "the composed answer: {}", stdout(&out));
    // The sibling was cut with the call: it never reached its late marker.
    // A worker that waits the whole inside out is the old engine-per-call
    // shape the finding forbids.
    for _ in 0..20 {
        if temp.path(marker).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        !temp.path(marker).exists(),
        "the sibling's program wrote its late marker: the exit's first reply did not cut the work running inside (2.6 step 2)"
    );
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

// -- Budgets across the wire (0014, handoff priority 2) ---------------------

/// Both calls of one trace are placed on a peer, and the root budget is
/// smaller than two calls' worth: the first reserves and commits in the trace
/// exactly as a local call does, and the second — the pool cannot cover — is
/// refused with the budget error (10.3, 10.4, 6.1), never started.
const BUDGETED: &str = r#"{
  "version": 0,
  "name": "budgeted",
  "budget": {"credits": 3},
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "a", "description": "first call", "run": ["/bin/cat"], "cost": {"credits": 2}},
    {"id": "b", "description": "second call", "run": ["/bin/cat"], "cost": {"credits": 2}}
  ],
  "links": [
    {"from": "src", "to": "a"},
    {"from": "a", "to": "b"}
  ]
}
"#;

/// The events with the commit's real `ms` zeroed too: a placed call spends its
/// milliseconds on the wire and on the peer, and the budget notes travel in
/// the trace of the run that metered them (`t` and the ids are already
/// renormalised by `events`).
fn without_ms(events: Vec<Value>) -> Vec<Value> {
    events
        .into_iter()
        .map(|e| match e {
            Value::Obj(entries) => Value::Obj(
                entries
                    .into_iter()
                    .map(|(k, v)| if k == "ms" { (k, Value::Int(0)) } else { (k, v) })
                    .collect(),
            ),
            other => other,
        })
        .collect()
}

#[test]
fn a_remote_call_reserves_commits_and_refuses_the_second_under_a_budget() {
    let temp = Temp::new("budget");
    let net = temp.write("net.json", BUDGETED);
    let inputs = temp.write("inputs.jsonl", INPUTS);
    let local = run(&temp, &net, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));

    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, r#"{"a": "b", "b": "b"}"#));
    let remote = run(
        &temp,
        &net,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(remote.status.success(), "the peer run failed: {}", stderr(&remote));

    // The meter of the run is the driver's, whatever ran where: the reserve
    // and the commit of the first call are in the trace, and the second call
    // — for which 3 credits do not cover two calls of 2 — fails with the
    // budget error without a reserve of its own (nothing was touched).
    let trace = std::fs::read_to_string(temp.path("remote.json")).expect("the trace");
    assert!(trace.contains("\"what\":\"reserve\""), "the reserve is in the trace: {trace}");
    assert!(trace.contains("\"what\":\"commit\""), "the commit is in the trace: {trace}");
    assert!(trace.contains("\"code\":\"budget\""), "the budget error is in the trace: {trace}");
    assert!(
        trace.contains("budget: needs 2 credits, 1 left"),
        "the refusal says what was short: {trace}"
    );

    // A placed call is metered like a local one: the whole event list agrees.
    let a = without_ms(events(&temp.path("local.json")));
    let b = without_ms(events(&temp.path("remote.json")));
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

// -- An effect that never started (finding 8, 0014) -------------------------

/// An effect call whose program exists only in the file: on a peer the call
/// cannot even reach (`peer b: cannot reach ...`), or the worker takes it and
/// finds no such program to start. Nothing ever ran, and 6.1/11.3's rule for
/// a call that never started is an `error` — the `unknown` of a started effect
/// stays pinned by `an_effect_call_on_a_dead_peer_is_unknown`, which is the
/// genuinely ambiguous case (the wire carried the call and died mid-flight).
const CHARGE_NOEXE: &str = r#"{
  "version": 0,
  "name": "charge",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "charge", "description": "an irreversible call", "run": ["/no/such/charge"], "effect": true, "timeout": 20000}
  ],
  "links": [{"from": "src", "to": "charge"}]
}
"#;

#[test]
fn an_effect_that_never_started_on_a_peer_is_an_error() {
    let temp = Temp::new("effect-unstarted");
    let net = temp.write("net.json", CHARGE_NOEXE);
    let inputs = temp.write("inputs.jsonl", INPUTS);

    // A peer that is not there: the call is never delivered to anything.
    let down = temp.write("peers.json", &peers_text(free_port(), TOKEN, "{\"charge\": \"b\"}"));
    let _ = run(
        &temp,
        &net,
        &inputs,
        &temp.path("down.json"),
        &["--peers", down.to_str().expect("a path")],
        &[],
    );
    let trace = std::fs::read_to_string(temp.path("down.json")).expect("the trace");
    assert!(trace.contains("\"type\":\"error\""), "the unreachable peer's failure: {trace}");
    assert!(trace.contains("\"code\":\"exit\""), "the exit code stands: {trace}");
    assert!(trace.contains("exit 127: peer b: cannot reach"), "the message reads: {trace}");
    assert!(!trace.contains("\"type\":\"unknown\""), "the effect never started: {trace}");

    // A peer that takes the call and finds no such program on its machine.
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &[]);
    let peers = temp.write("peers2.json", &peers_text(port, TOKEN, "{\"charge\": \"b\"}"));
    let _ = run(
        &temp,
        &net,
        &inputs,
        &temp.path("noexe.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    let trace = std::fs::read_to_string(temp.path("noexe.json")).expect("the trace");
    assert!(trace.contains("\"type\":\"error\""), "the spawn refusal: {trace}");
    assert!(trace.contains("\"code\":\"exit\""), "the exit code stands: {trace}");
    assert!(!trace.contains("\"type\":\"unknown\""), "the effect never started: {trace}");
}

// -- A cut against a backpressured peer (finding 9, 0014) --------------------

/// A peer that completes the handshake and then never reads: the call line it
/// is sent cannot drain, so the driver's dispatch write blocks in its own
/// thread, and the cut that follows cannot deliver its cancel either. The
/// decision thread must not wait on that socket at all: the cancel write
/// leaves it, and the socket is aborted when the line cannot be delivered
/// promptly, with the kill grace owning the end (6.3). A decision thread held
/// for the socket's write timeout (~5 s here) is the failure this pins; the
/// second connection the replacing call opens is refused at once, so the
/// run's other shape is quick either way.
#[test]
fn a_cut_of_a_backpressured_peer_does_not_stall_the_run() {
    let temp = Temp::new("backpressure");
    let net = temp.write("net.json", NET);
    let fill = "x".repeat(4 * 1024 * 1024);
    let inputs = temp.write(
        "inputs.jsonl",
        &format!(
            "{{\"at\": 0, \"source\": \"src\", \"payload\": {{\"n\": 1, \"fill\": \"{fill}\"}}, \"replaces\": \"k\"}}\n\
             {{\"at\": 200, \"source\": \"src\", \"payload\": {{\"n\": 2}}, \"replaces\": \"k\"}}\n"
        ),
    );

    // The fake peer: one connection is taken (handshake only — the call line
    // sits unread for good), later ones are refused at the handshake.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("the fake peer listens");
    let port = listener.local_addr().expect("a port").port();
    std::thread::spawn(move || {
        let mut first = true;
        for stream in listener.incoming() {
            let Ok(s) = stream else { continue };
            let take_calls = first;
            first = false;
            std::thread::spawn(move || {
                let mut line = String::new();
                let Ok(mut reader) = s.try_clone().map(std::io::BufReader::new) else {
                    return;
                };
                let _ = std::io::BufRead::read_line(&mut reader, &mut line);
                let mut s = reader.into_inner();
                let _ = s.set_write_timeout(Some(std::time::Duration::from_secs(5)));
                let reply = if take_calls {
                    "{\"type\":\"hello\",\"v\":1,\"peer\":\"b\",\"mode\":\"program\",\"ok\":true}\n"
                } else {
                    "{\"type\":\"hello\",\"v\":1,\"peer\":\"b\",\"mode\":\"program\",\"ok\":false,\"error\":\"one at a time\"}\n"
                };
                let _ = s.write_all(reply.as_bytes());
                let _ = s.flush();
                if take_calls {
                    // The call line that arrives next is never read: the
                    // peer's buffers fill and stay full.
                    std::thread::sleep(std::time::Duration::from_secs(30));
                }
            });
        }
    });

    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"worker\": \"b\"}"));
    let started = std::time::Instant::now();
    let out = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    let took = started.elapsed();

    assert!(out.status.success(), "the run failed: {}", stderr(&out));
    // The decision thread never owns a stuck socket: a cut moves the run
    // along, and the cancel is delivered by its own writer or the socket is
    // aborted. A run held for the socket's write timeout (5 s) plus its
    // grace is the blocking the finding describes.
    assert!(
        took < std::time::Duration::from_millis(4000),
        "the run took {took:?}, so a decision-thread write blocked the run"
    );
    let trace = std::fs::read_to_string(temp.path("trace.json")).expect("the trace");
    assert!(trace.contains("\"what\":\"term\""), "the cut asked the peer to stop: {trace}");
    assert!(
        trace.contains("\"what\":\"kill\""),
        "the abort ended the call at the grace: {trace}"
    );
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

// -- A serve hosted on a peer (P1: the last call kind, decision 0013) --------

/// One serve neuron on `tests/progs/serve.py`: `delay` ms answers, and the
/// ready's `maxInflight` of 2 is the program's word (3.2), not a file field.
fn serve_net(delay: u64, timeout: u64) -> String {
    format!(
        r#"{{
  "version": 0,
  "name": "peer-serve",
  "neurons": [
    {{"id": "src", "description": "in", "source": true}},
    {{"id": "s", "description": "a served neuron", "serve": ["python3", "{}", "{}"], "timeout": {}}}
  ],
  "links": [{{"from": "src", "to": "s"}}]
}}
"#,
        progs("serve.py"),
        delay,
        timeout
    )
}

/// A serve placed on a peer starts there: the driver sends its serve start
/// over the wire, the peer spawns the process and answers ready, and the
/// served call's answer is the one the local run gives — same stdout, the
/// same events (ids renormalised by first appearance, CONFORMANCE 8.3; the
/// wall-clock `t` zeroed).
///
/// The wire's relay loop never sleeps holding the serve host's mutex: a first
/// cut polled with a 50 ms tick inside the lock, the relay re-acquired within
/// microseconds of letting go, and the wire reader's own l1_send starved
/// behind it for up to 33 s (observed) — the call died on its timeout while
/// the relay kept its turn. A millisecond tick and a lock-free sleep let the
/// reader through; the answer now crosses within its own milliseconds.
#[test]
fn a_serve_on_a_peer_answers_like_a_local_one() {
    let temp = Temp::new("peer-serve");
    let net = temp.write("net.json", &serve_net(10, 5000));
    let inputs = temp.write("inputs.jsonl", "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 2}}\n");
    let local = run(&temp, &net, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));
    assert_eq!(stdout(&local).trim(), "{\"n\":4}");

    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--serve"]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"s\": \"b\"}"));
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

/// Slots cross the wire the same way they hold locally: the ready's
/// `maxInflight` of 2 rides the relayed ready line, three calls of 200 ms
/// arrive 50 ms apart — two run, the third queues — and all three answer in
/// the local run's order, with the local run's events (3.2).
#[test]
fn a_peer_serve_enforces_its_ready_across_the_wire() {
    let temp = Temp::new("peer-serve-queue");
    let net = temp.write("net.json", &serve_net(200, 8000));
    let inputs = temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n\
         {\"at\": 50, \"source\": \"src\", \"payload\": {\"n\": 2}}\n\
         {\"at\": 100, \"source\": \"src\", \"payload\": {\"n\": 3}}\n",
    );
    let local = run(&temp, &net, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));
    assert_eq!(stdout(&local).lines().count(), 3, "{}", stdout(&local));

    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--serve"]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"s\": \"b\"}"));
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

/// The mode-mismatch trap the hello already names for program and graph: a
/// serve placement on a worker that cannot host one refuses at the handshake
/// with its own wording, before any neuron of the run could have answered.
#[test]
fn a_serve_node_needs_a_serve_peer() {
    let temp = Temp::new("peer-serve-mode");
    let net = temp.write("net.json", &serve_net(10, 5000));
    let inputs = temp.write("inputs.jsonl", "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n");
    let port = free_port();
    // A worker that runs programs; the placement asks it to host a serve.
    let _peer = start_worker(port, TOKEN, "b", &[], &[]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"s\": \"b\"}"));
    let out = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(!out.status.success(), "a serve on a program worker must refuse the run");
    let err = stderr(&out);
    assert!(err.contains("failed its first start"), "{}", err);
    assert!(
        err.contains("speaks as a program peer, and this serve needs a serve peer"),
        "{}",
        err
    );
}

/// `start_serves` waits for a ready; a peer that takes the handshake and
/// never answers its serve start is a start failure by the neuron's deadline
/// (finding 5, 0014): the run refuses there — it never hangs the run.
#[test]
fn a_serve_peer_that_never_answers_its_start_fails_by_the_deadline() {
    let temp = Temp::new("peer-serve-silent");
    let net = temp.write("net.json", &serve_net(10, 400));
    let inputs = temp.write("inputs.jsonl", "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n");
    // The fake peer: the handshake is answered (as a serve host), the serve
    // start that follows is taken and never answered.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("the fake peer listens");
    let port = listener.local_addr().expect("a port").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut line = String::new();
            let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(&mut s), &mut line);
            let _ = s.write_all(b"{\"type\":\"hello\",\"v\":1,\"peer\":\"b\",\"mode\":\"serve\",\"ok\":true}\n");
            let _ = s.flush();
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    });
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"s\": \"b\"}"));
    let started = std::time::Instant::now();
    let out = run(
        &temp,
        &net,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    let took = started.elapsed();
    assert!(!out.status.success(), "a serve that never starts must refuse the run");
    let err = stderr(&out);
    assert!(err.contains("failed its first start"), "{}", err);
    assert!(err.contains("no ready within 400 ms"), "{}", err);
    assert!(
        took < std::time::Duration::from_millis(5000),
        "the run took {:?}, so the start waited for patience instead of the deadline",
        took
    );
}

/// Unused today, kept for the next test that needs a live write: the same
/// helper the desk and http suites use.
#[allow(dead_code)]
fn line(stream: &mut impl Write, text: &str) {
    let _ = writeln!(stream, "{}", text);
}

// -- A restart, a cancel, a crash on a peer (P3: the serve lifecycle) --------

/// One serve neuron on `tests/progs/serve_crash.py`: its first start says
/// ready, then dies with a word on stderr; the restarted instance answers
/// like a working serve (a marker in the working directory marks the crashed
/// start, 3.2).
fn serve_crash_net(timeout: u64) -> String {
    format!(
        r#"{{
  "version": 0,
  "name": "peer-serve-crash",
  "neurons": [
    {{"id": "src", "description": "in", "source": true}},
    {{"id": "s", "description": "a served neuron", "serve": ["python3", "{}"], "timeout": {}}}
  ],
  "links": [{{"from": "src", "to": "s"}}]
}}
"#,
        progs("serve_crash.py"),
        timeout
    )
}

/// A crash mid-life reports honestly over the wire: the worker's `down`
/// carries the process's stderr head, the call fails with `crash` and that
/// word, the neuron restarts with the backoff (the restart note, 6.3), and
/// the restarted process answers the next call — the local run's trace, ids
/// renormalised (CONFORMANCE 8.3).
#[test]
fn a_peer_serve_crash_restarts_and_the_next_call_answers() {
    // Two working directories on purpose: `serve_crash.py` marks its crashed
    // start in the neuron's working directory, and the comparison runs the
    // same scenario twice — a crashed flag left by the local run's own
    // served process would make the peer run never crash at all.
    let local_temp = Temp::new("peer-serve-crash-local");
    let temp = Temp::new("peer-serve-crash");
    let net = serve_crash_net(10000);
    // The second input sits at 3000 ms — an order past the crash's ~100 ms
    // settle and the 200 ms restart backoff (the real suite's own wide
    // margins between scripted delays and call timeouts): at 300 ms the two
    // engines' orderings race the tie and a loaded machine can flip it.
    let inputs = temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n\
         {\"at\": 3000, \"source\": \"src\", \"payload\": {\"n\": 2}}\n",
    );
    let local_net = local_temp.write("net.json", &net);
    let local_inputs = local_temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n\
         {\"at\": 3000, \"source\": \"src\", \"payload\": {\"n\": 2}}\n",
    );

    let local = run(&local_temp, &local_net, &local_inputs, &local_temp.path("local.json"), &[], &[]);
    assert!(local.status.success(), "the local run failed: {}", stderr(&local));
    let local_trace = std::fs::read_to_string(local_temp.path("local.json")).expect("the trace was written");
    assert!(
        local_trace.contains("\"code\":\"crash\"") && local_trace.contains("the model went away"),
        "the crashed call's word: {}",
        local_trace
    );
    assert!(
        local_trace.contains("\"what\":\"restart\""),
        "the restart is counted: {}",
        local_trace
    );
    assert_eq!(
        count(&stdout(&local), "\"seen\":"),
        1,
        "the restarted serve answers once: {}",
        stdout(&local)
    );

    let net = temp.write("net.json", &net);
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--serve"]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"s\": \"b\"}"));
    let remote = run(
        &temp,
        &net,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(remote.status.success(), "the peer run failed: {}", stderr(&remote));
    let trace = std::fs::read_to_string(temp.path("remote.json")).expect("the trace was written");
    assert!(
        trace.contains("\"code\":\"crash\"") && trace.contains("the model went away"),
        "the crash's word came over the wire: {}",
        trace
    );
    assert!(trace.contains("\"what\":\"restart\""), "the restart is counted: {}", trace);
    assert_eq!(
        count(&stdout(&remote), "\"seen\":"),
        1,
        "the restarted serve answers once: {}",
        stdout(&remote)
    );

    let a = events(&local_temp.path("local.json"));
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

/// One serve neuron on `tests/progs/composed_serve.py` (the same program the
/// real suite's composed-cut test reads): `maxInflight` in its first
/// argument, `delay` in its second, and each `start`/`cancel`/`answer` it
/// sees recorded at `log` — the program's own word on what reached it.
fn serve_log_net(delay: u64, timeout: u64, log: &Path) -> String {
    format!(
        r#"{{
  "version": 0,
  "name": "peer-serve-cut",
  "neurons": [
    {{"id": "src", "description": "in", "source": true}},
    {{"id": "s", "description": "a served neuron", "serve": ["python3", "{}", "2", "{}", "{}"], "timeout": {}}}
  ],
  "links": [{{"from": "src", "to": "s"}}]
}}
"#,
        progs("composed_serve.py"),
        delay,
        log.to_string_lossy(),
        timeout
    )
}

/// The cancel line crosses the wire (3.2): a newer input on one key cuts the
/// running served call — the drop is running, the cancel note, and the peer's
/// program records `cancel` in its own log and answers quiet for the cut
/// reply, which frees the slot; the replacing call answers. A lost cancel
/// would show only as the program's own `answer` where `cancel` should be.
#[test]
fn a_cancel_line_reaches_a_served_call_on_a_peer() {
    let temp = Temp::new("peer-serve-cancel");
    let local = temp.path("local.cancels.log");
    let remote_log = temp.path("remote.cancels.log");
    let _ = std::fs::remove_file(&local);
    let _ = std::fs::remove_file(&remote_log);
    let local_net = temp.write("net-local.json", &serve_log_net(1500, 60000, &local));
    let remote_net = temp.write("net-remote.json", &serve_log_net(1500, 60000, &remote_log));
    let inputs = temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}, \"replaces\": \"k\"}\n\
         {\"at\": 150, \"source\": \"src\", \"payload\": {\"n\": 2}, \"replaces\": \"k\"}\n",
    );

    let first = run(&temp, &local_net, &inputs, &temp.path("local.json"), &[], &[]);
    assert!(first.status.success(), "the local run failed: {}", stderr(&first));
    let said = std::fs::read_to_string(&local).unwrap_or_default();
    assert!(said.contains("\"what\":\"start\""), "the first call ran: {said}");
    assert!(said.contains("\"what\":\"cancel\""), "the local cut cancel: {said}");
    assert_eq!(
        count(&stdout(&first), "\"slow\":"),
        1,
        "the replacing call answers: {}",
        stdout(&first)
    );
    let trace = std::fs::read_to_string(temp.path("local.json")).expect("the trace was written");
    assert!(trace.contains("\"reason\":\"replaced\",\"phase\":\"running\""), "{}", trace);
    assert!(trace.contains("\"what\":\"cancel\""), "{}", trace);

    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--serve"]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"s\": \"b\"}"));
    let second = run(
        &temp,
        &remote_net,
        &inputs,
        &temp.path("remote.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(second.status.success(), "the peer run failed: {}", stderr(&second));
    let said = std::fs::read_to_string(&remote_log).unwrap_or_default();
    assert!(said.contains("\"what\":\"start\""), "the first call ran on the peer: {said}");
    assert!(
        said.contains("\"what\":\"cancel\""),
        "the cancel line crossed the wire — the program's own word: {said}"
    );
    let trace = std::fs::read_to_string(temp.path("remote.json")).expect("the trace was written");
    assert!(trace.contains("\"reason\":\"replaced\",\"phase\":\"running\""), "{}", trace);

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

/// The outer network of the idle test: a short timeout on the hosted graph.
const OUTER_SHORT: &str = r#"{
  "version": 0,
  "name": "outer",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "graph", "description": "a whole network", "network": "inner.network.json", "timeout": 1000}
  ],
  "links": [{"from": "src", "to": "graph"}]
}
"#;

/// A hosted graph that sat idle for longer than a call's timeout still gives
/// the next call its whole timeout: the call's clock starts when it arrives,
/// not when the graph last had something to do.
#[test]
fn a_hosted_graph_that_sat_idle_gives_the_next_call_its_whole_timeout() {
    let temp = Temp::new("graph-idle");
    let inner = temp.write("inner.network.json", INNER);
    let outer = temp.write("outer.network.json", OUTER_SHORT);
    let inputs = temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n{\"at\": 2500, \"source\": \"src\", \"payload\": {\"n\": 2}}\n",
    );
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--network", inner.to_str().expect("a path")]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let out = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(out.status.success(), "the run failed: {}", stderr(&out));
    let answers = stdout(&out);
    assert!(answers.contains("\"tagged\":{\"n\":1}"), "the first call: {answers}");
    assert!(answers.contains("\"tagged\":{\"n\":2}"), "the call after the idle gap: {answers}");
}

// -- A hosted graph's calls overlap (decision 0020, S4c) ---------------------

/// The outer network of the overlap tests.
const OUTER_MANY: &str = r#"{
  "version": 0,
  "name": "outer",
  "neurons": [
    {"id": "src", "description": "in", "source": true},
    {"id": "graph", "description": "a whole network", "network": "inner.network.json", "timeout": 20000}
  ],
  "links": [{"from": "src", "to": "graph"}]
}
"#;

/// A hosted graph whose one program logs when it began and when it ended, a
/// second apart: the log's order says whether two calls ran at once.
fn logging_inner(log: &Path) -> String {
    format!(
        r#"{{
  "version": 0,
  "name": "inner",
  "neurons": [
    {{"id": "entry", "description": "in", "run": ["/bin/cat"]}},
    {{"id": "exit", "description": "slow", "run": ["/bin/sh", "-c", "read line; echo begin >> {log}; sleep 1; echo end >> {log}; printf '{{\"did\":%s}}' \"$line\""]}}
  ],
  "links": [{{"from": "entry", "to": "exit"}}],
  "entry": "entry",
  "exit": "exit"
}}
"#,
        log = log.to_string_lossy(),
    )
}

/// Calls to a hosted graph overlap as traces do in a run: the second begins
/// before the first has ended, and each is answered with its own payload. A
/// graph that takes one call at a time is the lock this step removed.
#[test]
fn calls_to_a_hosted_graph_overlap() {
    let temp = Temp::new("graph-overlap");
    let log = temp.path("log.txt");
    let inner = temp.write("inner.network.json", &logging_inner(&log));
    let outer = temp.write("outer.network.json", OUTER_MANY);
    let inputs = temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}}\n\
         {\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 2}}\n\
         {\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 3}}\n",
    );
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--network", inner.to_str().expect("a path")]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let out = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(out.status.success(), "the run failed: {}", stderr(&out));
    let answers = stdout(&out);
    for n in 1..=3 {
        assert!(
            answers.contains(&format!("{{\"did\":{{\"n\":{n}}}}}")),
            "call {n} was not answered with its own payload: {answers}"
        );
    }
    let lines: Vec<String> = std::fs::read_to_string(&log)
        .expect("the programs' log")
        .lines()
        .map(|l| l.to_string())
        .collect();
    assert_eq!(count(&lines.join("\n"), "begin"), 3, "three calls began: {lines:?}");
    assert_eq!(count(&lines.join("\n"), "end"), 3, "three calls ended: {lines:?}");
    assert_eq!(
        &lines[..3],
        ["begin", "begin", "begin"],
        "every call began before any ended; one at a time is begin, end, begin, end: {lines:?}"
    );
}

/// A cut of one call to a hosted graph (a newer signal on its key, 5.4) stops
/// that call's program and leaves the call beside it running to its answer.
#[test]
fn a_cut_of_one_hosted_call_leaves_the_other_running() {
    let temp = Temp::new("graph-cut-one");
    let log = temp.path("log.txt");
    let inner = temp.write("inner.network.json", &logging_inner(&log));
    let outer = temp.write("outer.network.json", OUTER_MANY);
    // Two calls on one key, so the second cuts the first while it sleeps, and
    // a third on no key that began beside the first and is never cut.
    let inputs = temp.write(
        "inputs.jsonl",
        "{\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 1}, \"replaces\": \"k\"}\n\
         {\"at\": 0, \"source\": \"src\", \"payload\": {\"n\": 2}}\n\
         {\"at\": 300, \"source\": \"src\", \"payload\": {\"n\": 3}, \"replaces\": \"k\"}\n",
    );
    let port = free_port();
    let _peer = start_worker(port, TOKEN, "b", &[], &["--network", inner.to_str().expect("a path")]);
    let peers = temp.write("peers.json", &peers_text(port, TOKEN, "{\"graph\": \"b\"}"));
    let out = run(
        &temp,
        &outer,
        &inputs,
        &temp.path("trace.json"),
        &["--peers", peers.to_str().expect("a path")],
        &[],
    );
    assert!(out.status.success(), "the run failed: {}", stderr(&out));
    let answers = stdout(&out);
    assert!(!answers.contains("{\"n\":1}"), "the cut call answered: {answers}");
    assert!(answers.contains("{\"did\":{\"n\":2}}"), "the call beside the cut one: {answers}");
    assert!(answers.contains("{\"did\":{\"n\":3}}"), "the call that cut it: {answers}");
    // The cut call's program was stopped in its sleep: three began, two ended.
    let text = std::fs::read_to_string(&log).expect("the programs' log");
    assert_eq!(count(&text, "begin"), 3, "three calls began: {text}");
    assert_eq!(count(&text, "end"), 2, "the cut call's program was stopped before its end: {text}");
}

/// A hosted graph whose inside settles with no reply from `exit` is quiet
/// (2.6 step 3), event for event what the same graph gives run locally.
#[test]
fn a_hosted_graph_that_is_quiet_is_quiet() {
    let temp = Temp::new("graph-quiet");
    let inner = temp.write(
        "inner.network.json",
        r#"{
  "version": 0,
  "name": "inner",
  "neurons": [
    {"id": "entry", "description": "in", "run": ["/bin/cat"]},
    {"id": "exit", "description": "says nothing", "run": ["/bin/sh", "-c", "read line; true"]}
  ],
  "links": [{"from": "entry", "to": "exit"}],
  "entry": "entry",
  "exit": "exit"
}
"#,
    );
    let outer = temp.write("outer.network.json", OUTER_MANY);
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
    assert!(
        a.iter().any(|e| e.get("type").and_then(Value::as_str) == Some("quiet")),
        "the local run was not quiet: {a:?}"
    );
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

// -- A hosted file is fed, called, or both (decision 0020, S4c) ---------------

/// A connection to a peer after its handshake: the socket to write lines on
/// and a reader for the lines that come back.
fn wire(port: u16) -> (TcpStream, BufReader<TcpStream>) {
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
    line(&mut sock, &json::write(&hello));
    let mut reply = String::new();
    reader.read_line(&mut reply).expect("the handshake replies");
    assert!(reply.contains("\"ok\":true"), "the handshake: {reply}");
    (sock, reader)
}

/// The events a worker has written to its stream, once `count` of them are
/// of `kind` (the worker writes as things happen, so the test waits).
fn stream_until(path: &Path, kind: &str, count: usize) -> Vec<Value> {
    let mut seen = Vec::new();
    for _ in 0..100 {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        seen = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| json::parse(l).expect("a stream line is JSON"))
            .collect();
        let have = seen.iter().filter(|e| e.get("type").and_then(Value::as_str) == Some(kind)).count();
        if have >= count {
            return seen;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("the stream never had {count} `{kind}` events: {seen:?}");
}

fn field<'a>(ev: &'a Value, name: &str) -> &'a str {
    ev.get(name).and_then(Value::as_str).unwrap_or("")
}

/// A file with a source and nothing to be called through.
const FED: &str = r#"{
  "version": 0,
  "name": "fed",
  "neurons": [
    {"id": "inbox", "description": "what arrives", "source": true},
    {"id": "tag", "description": "tags it", "run": ["/bin/sh", "-c", "read line; printf '{\"tagged\":%s}' \"$line\""]}
  ],
  "links": [{"from": "inbox", "to": "tag"}]
}
"#;

/// A file with a source and an `entry` and `exit`: fed and called.
const FED_AND_CALLED: &str = r#"{
  "version": 0,
  "name": "both",
  "neurons": [
    {"id": "inbox", "description": "what arrives", "source": true},
    {"id": "tag", "description": "tags it", "run": ["/bin/sh", "-c", "read line; printf '{\"tagged\":%s}' \"$line\""]}
  ],
  "links": [{"from": "inbox", "to": "tag"}],
  "entry": "tag",
  "exit": "tag"
}
"#;

/// A hosted file with a source is fed over the wire: an `inject` line is a
/// root signal at the source, with no reply, and the worker's own stream has
/// the trace it started, as a live run's stream would. A file with no `entry`
/// cannot be called, and says so.
#[test]
fn a_hosted_file_is_fed_at_its_source() {
    let temp = Temp::new("graph-fed");
    let file = temp.write("fed.network.json", FED);
    let stream = temp.path("stream.jsonl");
    let port = free_port();
    let _peer = start_worker(
        port,
        TOKEN,
        "b",
        &[],
        &[
            "--network",
            file.to_str().expect("a path"),
            "--stream",
            stream.to_str().expect("a path"),
        ],
    );
    let (mut sock, mut reader) = wire(port);
    line(&mut sock, r#"{"type":"inject","source":"inbox","payload":{"n":1}}"#);
    line(&mut sock, r#"{"type":"inject","source":"inbox","payload":{"n":2}}"#);
    let events = stream_until(&stream, "finished", 2);
    let kinds: Vec<&str> = events.iter().map(|e| field(e, "type")).collect();
    assert_eq!(count(&kinds.join(" "), "signal"), 2, "each injection is one signal: {kinds:?}");
    assert_eq!(count(&kinds.join(" "), "answer"), 2, "each was answered: {kinds:?}");
    assert!(
        events.iter().any(|e| field(e, "type") == "signal" && field(e, "from") == "inbox"),
        "the injection is a signal from the source: {events:?}"
    );
    let text = std::fs::read_to_string(&stream).expect("the stream");
    assert!(text.contains("{\"tagged\":{\"n\":1}}"), "the first answer: {text}");
    assert!(text.contains("{\"tagged\":{\"n\":2}}"), "the second answer: {text}");

    // Nothing came back for the injections; a call is answered that the file
    // has nothing to be called through.
    line(
        &mut sock,
        r#"{"type":"call","id":7,"node":"x","argv":[],"env":[],"input":"{}","timeout":5000}"#,
    );
    let mut done = String::new();
    reader.read_line(&mut done).expect("the call replies");
    let done = json::parse(done.trim()).expect("the reply is JSON");
    assert_eq!(
        done.get("id").and_then(Value::as_i64),
        Some(7),
        "the first line back is the call's: {done:?}"
    );
    assert_eq!(
        done.get("out").and_then(|o| o.get("kind")).and_then(Value::as_str),
        Some("inner"),
        "a file with no entry cannot be called: {done:?}"
    );
}

/// One hosted file is both called and fed: the call is answered on the wire
/// with the first reply of `exit`, the injection is answered to nobody, and
/// the worker's stream has both traces, the call's from the caller's door.
#[test]
fn a_hosted_file_is_called_and_fed() {
    let temp = Temp::new("graph-both");
    let file = temp.write("both.network.json", FED_AND_CALLED);
    let stream = temp.path("stream.jsonl");
    let port = free_port();
    let _peer = start_worker(
        port,
        TOKEN,
        "b",
        &[],
        &[
            "--network",
            file.to_str().expect("a path"),
            "--stream",
            stream.to_str().expect("a path"),
        ],
    );
    let (mut sock, mut reader) = wire(port);
    line(&mut sock, r#"{"type":"inject","source":"inbox","payload":{"fed":1}}"#);
    line(
        &mut sock,
        r#"{"type":"call","id":3,"node":"x","argv":[],"env":[],"input":"{\"called\":1}","timeout":5000}"#,
    );
    let mut done = String::new();
    reader.read_line(&mut done).expect("the call replies");
    let done = json::parse(done.trim()).expect("the reply is JSON");
    let out = done.get("out").expect("an outcome");
    assert_eq!(out.get("kind").and_then(Value::as_str), Some("answer"), "the call: {done:?}");
    assert_eq!(
        json::write(out.get("payload").expect("a payload")),
        "{\"tagged\":{\"called\":1}}",
        "the call is answered with its own payload, not the injection's"
    );

    let events = stream_until(&stream, "finished", 2);
    let roots: Vec<&str> = events
        .iter()
        .filter(|e| field(e, "type") == "signal" && e.get("hops").and_then(Value::as_i64) == Some(0))
        .map(|e| field(e, "from"))
        .collect();
    assert!(roots.contains(&"inbox"), "a trace from the source: {events:?}");
    assert!(roots.contains(&"__call"), "a trace from the caller's door: {events:?}");
    let answers = events.iter().filter(|e| field(e, "type") == "answer").count();
    assert_eq!(answers, 2, "each trace has its answer in the worker's stream: {events:?}");
    let traces: std::collections::HashSet<&str> = events
        .iter()
        .filter(|e| field(e, "type") != "note")
        .map(|e| field(e, "trace"))
        .collect();
    assert_eq!(traces.len(), 2, "two traces, each its own: {events:?}");
    // The work that answered the call was not cut: nothing in the worker's
    // stream says a neuron was dropped or cancelled.
    assert!(
        !events.iter().any(|e| field(e, "type") == "drop" || field(e, "type") == "note"),
        "the answering work is over, not cut: {events:?}"
    );
}

/// An injection the engine cannot take is refused on the worker's stderr and
/// nothing else changes: the connection stays, and the next one is taken.
#[test]
fn a_refused_injection_is_a_line_on_the_workers_stderr() {
    let temp = Temp::new("graph-refused");
    let file = temp.write("fed.network.json", FED);
    let stream = temp.path("stream.jsonl");
    let port = free_port();
    let mut peer = start_worker(
        port,
        TOKEN,
        "b",
        &[],
        &[
            "--network",
            file.to_str().expect("a path"),
            "--stream",
            stream.to_str().expect("a path"),
        ],
    );
    let (mut sock, _reader) = wire(port);
    line(&mut sock, r#"{"type":"inject","source":"inbox","payload":{},"at":5}"#);
    let said = peer.stderr_line();
    assert!(said.contains("refused an injection") && said.contains("`at`"), "an `at`: {said}");
    line(&mut sock, r#"{"type":"inject","source":"nowhere","payload":{}}"#);
    let said = peer.stderr_line();
    assert!(
        said.contains("refused an injection") && said.contains("nowhere"),
        "an unknown source: {said}"
    );
    line(&mut sock, r#"{"type":"inject","source":"inbox","payload":{"n":9}}"#);
    let events = stream_until(&stream, "finished", 1);
    assert_eq!(
        events.iter().filter(|e| field(e, "type") == "finished").count(),
        1,
        "only the injection that was taken ran: {events:?}"
    );
}

/// A file nothing can reach is not hosted, and a stream belongs to a hosted
/// file: the worker refuses its own invocation for both.
#[test]
fn a_worker_refuses_a_file_nothing_can_reach() {
    let temp = Temp::new("graph-unreachable");
    let file = temp.write(
        "alone.network.json",
        r#"{
  "version": 0,
  "name": "alone",
  "neurons": [{"id": "tag", "description": "tags it", "run": ["/bin/cat"]}],
  "links": []
}
"#,
    );
    let out = Command::new(engine())
        .args(["peer", "--listen", "127.0.0.1:0", "--token", TOKEN, "--network"])
        .arg(&file)
        .output()
        .expect("the engine runs");
    assert_eq!(out.status.code(), Some(1), "refused: {}", stderr(&out));
    assert!(stderr(&out).contains("nothing can reach it"), "the reason: {}", stderr(&out));

    let out = Command::new(engine())
        .args(["peer", "--listen", "127.0.0.1:0", "--token", TOKEN, "--stream", "s.jsonl"])
        .output()
        .expect("the engine runs");
    assert_eq!(
        out.status.code(),
        Some(2),
        "a stream with no hosted file is a usage error: {}",
        stderr(&out)
    );
}
