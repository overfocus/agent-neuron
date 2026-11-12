// The `http` neurons, real (M2): a python3 server as the call's other side,
// one mode per test, and the behaviors of SPEC 3.3 driven end to end through
// the engine binary — the 2xx answer, the 204 quiet, a non-2xx, a non-JSON
// body, a refused connection, the timeout's error (unknown for an effect
// neuron), a cancel by staleness, the threshold input's shape, a no-link
// neuron's answer on the run's stdout, a composed instance with an http
// neuron inside, and the https refusal (that one in tests/real.rs).
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-rs-http-{}-{}-{}",
            tag,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        Temp { dir }
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.dir.join(name);
        std::fs::write(&p, text).expect("write file");
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

/// The server of `tests/progs/httpsrv.py`, its mode and its bound port; the
/// process is killed at the end of the test.
struct Server {
    port: u16,
    child: std::process::Child,
}

impl Server {
    fn new(mode: &str) -> Server {
        use std::io::BufRead as _;
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/progs/httpsrv.py");
        let mut child = Command::new("python3")
            .arg(&script)
            .arg(mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the python3 of the test server");
        let stdout = child.stdout.take().expect("the server's stdout");
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("the server's port");
        let port: u16 = line.trim().parse().unwrap_or_else(|_| panic!("the server's port line: {line}"));
        // The reader is dropped: the server keeps its pipe, the test reads
        // nothing more of it.
        Server { port, child }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn network(entries: &[(&str, &str, &str, &str)]) -> String {
    // Per entry: (id, kind, the kind's value, extra options) — for http, the
    // URL, quoted; the network is a source feeding each.
    let mut neurons = String::from("{\"id\": \"src\", \"description\": \"in\", \"source\": true}");
    let mut links = Vec::new();
    for (id, kind, value, opts) in entries {
        neurons.push_str(&format!(
            ",\n    {{\"id\": \"{id}\", \"description\": \"{id}\", \"{kind}\": {value}{opts}}}"
        ));
        links.push(format!("{{\"from\": \"src\", \"to\": \"{id}\"}}"));
    }
    format!(
        "{{\n  \"version\": 0,\n  \"name\": \"test\",\n  \"description\": \"a test network\",\n  \"neurons\": [\n{neurons}\n  ],\n  \"links\": [\n{}\n  ]\n}}\n",
        links.join(",\n    ")
    )
}

fn inputs_file(temp: &Temp, entries: &[(&str, &str, &str)]) -> PathBuf {
    let p = temp.path("inputs.jsonl");
    let mut f = std::fs::File::create(&p).expect("inputs");
    for (at, source, payload) in entries {
        writeln!(f, "{{\"at\": {}, \"source\": \"{}\", \"payload\": {}}}", at, source, payload).expect("inputs line");
    }
    p
}

fn engine() -> &'static str {
    env!("CARGO_BIN_EXE_neuron-engine")
}

fn run_cli(temp: &Temp, net: &Path, inputs: &Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(engine());
    cmd.arg("run").arg(net).arg("--inputs").arg(inputs);
    for a in extra_args {
        cmd.arg(a);
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

fn has(hay: &str, needle: &str) -> bool {
    hay.contains(needle)
}

fn trace_of(temp: &Temp, out_path: &str) -> String {
    std::fs::read_to_string(temp.path(out_path)).unwrap()
}

#[test]
fn http_answers_a_2xx_json_body() {
    let server = Server::new("json");
    let temp = Temp::new("answer");
    let net = temp.write(
        "network.json",
        &network(&[("h", "http", &format!("\"{}\"", server.url("/answer")), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "21")]);
    let out = run_cli(&temp, &net, &inputs, &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "{\"seen\":21,\"ok\":true}");
}

#[test]
fn http_answers_204_with_quiet() {
    let server = Server::new("no204");
    let temp = Temp::new("quiet204");
    let net = temp.write("network.json", &network(&[("q", "http", &format!("\"{}\"", server.url("/")), "")]));
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), ""); // a quiet prints nothing
    let trace = trace_of(&temp, "t.json");
    assert!(has(&trace, "\"type\":\"quiet\""), "{}", trace);
}

#[test]
fn http_answers_an_empty_2xx_body_with_quiet() {
    let server = Server::new("empty");
    let temp = Temp::new("quietempty");
    let net = temp.write("network.json", &network(&[("q", "http", &format!("\"{}\"", server.url("/")), "")]));
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "");
    let trace = trace_of(&temp, "t.json");
    assert!(has(&trace, "\"type\":\"quiet\""), "{}", trace);
}

#[test]
fn http_error_on_a_non_2xx() {
    let server = Server::new("bad");
    let temp = Temp::new("non2xx");
    let net = temp.write("network.json", &network(&[("e", "http", &format!("\"{}\"", server.url("/")), "")]));
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(
        out.status.success(),
        "a failing call is an event, not a failed run: {}",
        stderr(&out)
    );
    let trace = trace_of(&temp, "t.json");
    assert!(
        has(&trace, "\"code\":\"http\"") && has(&trace, "\"message\":\"http: the server answered 500"),
        "{}",
        trace
    );
}

#[test]
fn http_error_on_a_non_json_body() {
    let server = Server::new("text");
    let temp = Temp::new("nonjson");
    let net = temp.write("network.json", &network(&[("n", "http", &format!("\"{}\"", server.url("/")), "")]));
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = trace_of(&temp, "t.json");
    assert!(
        has(&trace, "\"code\":\"http\"") && has(&trace, "not one JSON value: not a json value at all"),
        "{}",
        trace
    );
}

#[test]
fn http_error_on_a_connection_refused() {
    // A port nothing listens on (the OS refuses the connect).
    let port = free_port();
    let temp = Temp::new("refused");
    let net = temp.write(
        "network.json",
        &network(&[("c", "http", &format!("\"http://127.0.0.1:{port}/x\"",), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = trace_of(&temp, "t.json");
    // What 6.1 fixes: a connection failure is an `http` error. Whether the
    // OS words a loopback refusal as ECONNREFUSED or resets it is timing.
    assert!(has(&trace, "\"code\":\"http\"") && has(&trace, "\"message\":\"http:"), "{}", trace);
}

#[test]
fn http_timeout_is_an_error_and_for_a_started_effect_unknown() {
    // A server that answers after the call's timeout: the timeout event (the
    // error, or the unknown of an effect call, 11.3), the cancel, and one
    // outcome only: the late reply is discarded and frees nothing.
    for effect in ["", "\", \"effect\": true"] {
        // A wide margin between the timeout and the server's reply: under a
        // parallel test run the engine's deadline processing can be delayed,
        // and the reply must stay the later of the two by an order of
        // magnitude.
        let server = Server::new("slow 2000");
        let temp = Temp::new(if effect.is_empty() { "tmout" } else { "tmoutfx" });
        let opts = match effect {
            "" => ", \"timeout\": 200".to_string(),
            _ => ", \"timeout\": 200, \"effect\": true".to_string(),
        };
        let net = temp.write(
            "network.json",
            &network(&[("t", "http", &format!("\"{}\"", server.url("/")), &opts)]),
        );
        let inputs = inputs_file(&temp, &[("0", "src", "{\"n\": 1}")]);
        let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let trace = trace_of(&temp, "t.json");
        let outcome = if effect.is_empty() {
            has(&trace, "\"type\":\"error\"") && has(&trace, "\"code\":\"timeout\"")
        } else {
            has(&trace, "\"type\":\"unknown\"") && has(&trace, "\"code\":\"timeout\"")
        };
        assert!(outcome, "{}", trace);
        assert!(has(&trace, "timed out after 200 ms"), "{}", trace);
        assert!(has(&trace, "\"what\":\"cancel\""), "{}", trace);
        let _ = server;
    }
}

#[test]
fn http_cancel_by_staleness() {
    // A newer input with the same key cuts the in-flight request: the drop
    // (phase running), the cancel note, and the second call answers; the
    // first request's later reply is discarded.
    let server = Server::new("slow 500");
    let temp = Temp::new("stale");
    let net = temp.write(
        "network.json",
        &network(&[("s", "http", &format!("\"{}\"", server.url("/")), ", \"timeout\": 4000")]),
    );
    let inputs = temp.path("inputs.jsonl");
    std::fs::write(
        &inputs,
        "{\"at\":0,\"source\":\"src\",\"payload\":{\"n\":1},\"replaces\":\"k\"}\n{\"at\":150,\"source\":\"src\",\"payload\":{\"n\":2},\"replaces\":\"k\"}\n",
    )
    .unwrap();
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let trace = trace_of(&temp, "t.json");
    assert!(has(&trace, "\"reason\":\"replaced\",\"phase\":\"running\""), "{}", trace);
    assert!(has(&trace, "\"what\":\"cancel\""), "{}", trace);
    // One answer only (the second call's), and it is the second payload's.
    let text = stdout(&out);
    let answers: Vec<&str> = text.lines().collect();
    assert_eq!(answers.len(), 1, "{}", text);
    assert!(has(answers[0], "\"seen\":{\"n\":2}"), "{}", answers[0]);
    let _ = server;
}

#[test]
fn http_threshold_input_is_the_payload_inputs_pair() {
    // A threshold above 1, formed the way 5.2 forms one: one trace fan out
    // through two relays (s → d1/d2 → h), so two distinct senders hold an
    // input of the same trace; the POST body is
    // {"payload": P, "inputs": [...]} and the echo server answers it back.
    let server = Server::new("echo");
    let temp = Temp::new("threshold");
    let echo = progs_echo();
    let neurons = format!(
        "{{\"id\": \"src\", \"description\": \"in\", \"source\": true}},\n    {{\"id\": \"d1\", \"description\": \"d1\", \"run\": [\"python3\", \"{echo}\"]}},\n    {{\"id\": \"d2\", \"description\": \"d2\", \"run\": [\"python3\", \"{echo}\"]}},\n    {{\"id\": \"h\", \"description\": \"h\", \"http\": \"{}\", \"threshold\": 2}}",
        server.url("/")
    );
    let links = "{\"from\": \"src\", \"to\": \"d1\"},\n    {\"from\": \"src\", \"to\": \"d2\"},\n    {\"from\": \"d1\", \"to\": \"h\"},\n    {\"from\": \"d2\", \"to\": \"h\"}";
    let net = temp.write(
        "network.json",
        &format!(
            "{{\n  \"version\": 0,\n  \"name\": \"test\",\n  \"description\": \"a test network\",\n  \"neurons\": [\n{neurons}\n  ],\n  \"links\": [\n{links}\n  ]\n}}\n"
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "\"a\"")]);
    let out = run_cli(&temp, &net, &inputs, &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out).trim(),
        "{\"payload\":{\"seen\":\"a\"},\"inputs\":[{\"seen\":\"a\"},{\"seen\":\"a\"}]}"
    );
}

#[test]
fn http_answers_a_no_link_neuron_on_the_run_stdout() {
    // A no-link http neuron is a results neuron: its answer is the run's
    // printed output (4.4), the trace's payload kept whole.
    let server = Server::new("echo");
    let temp = Temp::new("results");
    let long = format!("\"{}\"", "x".repeat(2400));
    let net = temp.write("network.json", &network(&[("r", "http", &format!("\"{}\"", server.url("/")), "")]));
    let inputs = temp.path("inputs.jsonl");
    std::fs::write(&inputs, format!("{{\"at\":0,\"source\":\"src\",\"payload\":{long}}}\n")).unwrap();
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let line = stdout(&out);
    // The run's accounting keeps the reply whole (9.1): the printed answer is
    // the full text.
    assert!(line.trim().len() >= 2400, "the reply is whole ({} bytes)", line.len());
    // The trace's event cuts the payload at 2000 characters and marks it
    // (TRACE.md).
    let trace = trace_of(&temp, "t.json");
    assert!(has(&trace, "\"truncated\":true"), "{}", &trace[..trace.len().min(400)]);
}

#[test]
fn http_inside_a_composed_instance() {
    // A composed neuron whose inside is an http neuron: the call answers on
    // the inner file's surface, the inside's events are sealed, and the
    // composed neuron's answer reaches the run's results.
    let server = Server::new("json");
    let temp = Temp::new("composed");
    let inner = temp.path("inner.network.json");
    // A real composed file: the entry is the echo program (its answer is what
    // the http neuron POSTs), the exit the http neuron itself.
    std::fs::write(
        &inner,
        format!(
            "{{\n  \"version\": 0,\n  \"name\": \"inner\",\n  \"description\": \"inner\",\n  \"entry\": \"e\",\n  \"exit\": \"h\",\n  \"neurons\": [\n    {{\"id\": \"e\", \"description\": \"e\", \"run\": [\"python3\", \"{}\"]}},\n    {{\"id\": \"h\", \"description\": \"h\", \"http\": \"{}\"}}\n  ],\n  \"links\": [\n    {{\"from\": \"e\", \"to\": \"h\"}}\n  ]\n}}\n",
            progs_echo(),
            server.url("/answer")
        ),
    )
    .unwrap();
    let net = temp.write("network.json", &network(&[("box", "network", "\"inner.network.json\"", "")]));
    let inputs = inputs_file(&temp, &[("0", "src", "7")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "{\"seen\":{\"seen\":7},\"ok\":true}");
    let trace = trace_of(&temp, "t.json");
    assert!(has(&trace, "\"kind\":\"source\""), "{}", trace);
}

fn progs_echo() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/progs/echo.py")
        .to_string_lossy()
        .to_string()
}

/// A port nothing listens on: the OS's refusal is the test's other side.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("a port").port();
    drop(listener);
    port
}

#[test]
fn http_max_inflight_one_request_at_a_time() {
    // `maxInflight` 1 on an http neuron (2.4, decision 0021): three inputs at
    // once make one request at a time, in arrival order. Each takes 300 ms,
    // so the answers come about 300 ms apart; a timeout of 500 ms counts from
    // each request's start, so none of the three times out though the last
    // waited 600 ms for its place.
    let server = Server::new("slow 300");
    let temp = Temp::new("inflight");
    let net = temp.write(
        "network.json",
        &network(&[(
            "s",
            "http",
            &format!("\"{}\"", server.url("/")),
            ", \"timeout\": 500, \"maxInflight\": 1",
        )]),
    );
    let inputs = inputs_file(
        &temp,
        &[("0", "src", "{\"n\":1}"), ("0", "src", "{\"n\":2}"), ("0", "src", "{\"n\":3}")],
    );
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    let answers: Vec<&str> = text.lines().collect();
    assert_eq!(answers.len(), 3, "{}", text);
    for (i, line) in answers.iter().enumerate() {
        assert!(has(line, &format!("\"seen\":{{\"n\":{}}}", i + 1)), "arrival order: {}", text);
    }
    let trace = trace_of(&temp, "t.json");
    assert!(!has(&trace, "\"type\":\"error\""), "{}", trace);
    let times: Vec<u64> = trace
        .split("\"type\":\"answer\"")
        .skip(1)
        .map(|rest| {
            let at = rest.find("\"t\":").expect("an answer's t") + 4;
            let digits: String = rest[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().expect("a time")
        })
        .collect();
    assert_eq!(times.len(), 3, "{}", trace);
    assert!(
        times[1] >= times[0] + 250 && times[2] >= times[1] + 250,
        "one request at a time: {times:?}"
    );
    let _ = server;
}

#[test]
fn http_each_forwards_one_signal_per_item() {
    // `each` on an http neuron (2.4, 4.4, decision 0022): the endpoint echoes
    // the array it was sent, and each item is forwarded as a signal of its
    // own. A body that is not an array is a failure of the call, `output`.
    let server = Server::new("echo");
    let temp = Temp::new("each");
    let net = temp.write(
        "network.json",
        &format!(
            r#"{{
  "version": 0,
  "name": "test",
  "neurons": [
    {{"id": "src", "source": true}},
    {{"id": "list", "http": "{}", "each": true}},
    {{"id": "one", "run": ["/bin/cat"]}}
  ],
  "links": [{{"from": "src", "to": "list"}}, {{"from": "list", "to": "one"}}]
}}
"#,
            server.url("/")
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "[1, \"two\", {\"n\": 3}]"), ("400", "src", "{\"n\": 4}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    let mut answers: Vec<&str> = text.lines().collect();
    answers.sort();
    assert_eq!(answers, vec!["1", "two", "{\"n\":3}"], "{}", text);
    let trace = trace_of(&temp, "t.json");
    assert_eq!(trace.matches("\"from\":\"list\",\"to\":[\"one\"]").count(), 3, "{}", trace);
    assert_eq!(trace.matches("\"code\":\"output\"").count(), 1, "{}", trace);
    assert!(has(&trace, "\"message\":\"not an array"), "{}", trace);
    let _ = server;
}
