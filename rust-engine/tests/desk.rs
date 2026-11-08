// The desk (decision 0008, Appendix D) driven without a browser: the tests
// read the desk's address from the run's stderr line and work the HTTP API
// directly — approve, edit and no; an empty edit refused; a withdrawn item;
// a timed-out one; the token, Host and Origin refusals; the page's nonce CSP
// and its values-as-text; and a human inside a composed instance. The page's
// own script is the surface a browser drives, behind exactly this API.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-rs-desk-{}-{}-{}",
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

fn engine() -> &'static str {
    env!("CARGO_BIN_EXE_neuron-engine")
}

fn progs(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/progs")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// One started run: its stdout and stderr in files that can be read while
/// the run blocks on the person.
struct Run {
    out_path: PathBuf,
    err_path: PathBuf,
    child: std::process::Child,
}

impl Run {
    /// Start a run in the temp dir, its streams in `stdout` and `stderr`
    /// there.
    fn start(temp: &Temp, network: &Path, inputs: &Path, extra_args: &[&str]) -> Run {
        let out_path = temp.path("stdout");
        let err_path = temp.path("stderr");
        let child = Command::new(engine())
            .arg("run")
            .arg(network)
            .arg("--inputs")
            .arg(inputs)
            .args(extra_args)
            .stdout(std::fs::File::create(&out_path).expect("stdout file"))
            .stderr(std::fs::File::create(&err_path).expect("stderr file"))
            .current_dir(temp.dir.clone())
            .spawn()
            .expect("the engine runs");
        Run { out_path, err_path, child }
    }

    fn out(&self) -> String {
        std::fs::read_to_string(&self.out_path).unwrap_or_default()
    }

    fn err(&self) -> String {
        std::fs::read_to_string(&self.err_path).unwrap_or_default()
    }

    /// The desk's port and token, from the `desk: <url>` line on stderr (one
    /// page per run, its token in the query, Appendix D).
    fn desk(&mut self) -> (u16, String) {
        for _ in 0..500 {
            for line in self.err().lines() {
                if let Some(rest) = line.strip_prefix("desk: http://127.0.0.1:") {
                    let (port, query) = rest.split_once('/').expect("the page's path after its port");
                    let token = query.strip_prefix("?token=").expect("the token in the query");
                    return (port.parse().expect("the desk's port"), token.to_string());
                }
            }
            if !self.running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the run never announced its desk: {}", self.err());
    }

    fn wait(&mut self) -> ExitStatus {
        self.child.wait().expect("the run ends")
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if self.child.try_wait().map(|r| r.is_none()).unwrap_or(false) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Run {
    /// Is the run still going?
    fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

struct Response {
    code: u16,
    head: String,
    body: String,
}

/// One request, `Connection: close`; the response's status, head and body.
fn request(port: u16, head: &str) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the desk's port");
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    stream.write_all(head.as_bytes()).expect("the request's write");
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Response {
    let text = String::from_utf8_lossy(raw).to_string();
    let head_end = text.find("\r\n\r\n").unwrap_or(text.len());
    let head = text[..head_end].to_string();
    let code = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    let body = text[head_end + 4.min(text.len())..].to_string();
    Response { code, head, body }
}

fn get_state(port: u16, token: &str) -> Response {
    request(
        port,
        &format!(
            "GET /state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nX-Desk-Token: {token}\r\nConnection: close\r\n\r\n"
        ),
    )
}

/// POST /answer with the token header; more headers ride with it.
fn post(port: u16, token: &str, body: &str) -> Response {
    let head = format!(
        "POST /answer HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\nX-Desk-Token: {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    request(port, &head)
}

/// The page, the token in the query (a browser's way in).
fn get_page(port: u16, token: &str) -> Response {
    request(
        port,
        &format!("GET /?token={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    )
}

/// The desk's state, polled until one item is at least waiting.
fn wait_items(run: &mut Run, port: u16, token: &str, at_least: usize) -> String {
    poll_state(run, port, token, |s, over| {
        over || s.matches("\"status\":\"waiting\"").count() >= at_least
    })
}

/// The desk's state, polled until `pred(state, over)`.
fn poll_state(run: &mut Run, port: u16, token: &str, pred: impl Fn(&str, bool) -> bool) -> String {
    for _ in 0..1000 {
        let r = get_state(port, token);
        let over = r.body.contains("\"over\":true");
        if pred(&r.body, over) {
            return r.body;
        }
        if !run.running() {
            return r.body;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the desk's state never showed itself: {:?}", get_state(port, token).body);
}

/// The item's id from the state's JSON (ids run `i-1`, `i-2`, ...).
fn extract_id(state: &str, which: usize) -> Option<String> {
    let mut found = Vec::new();
    let bytes = state.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i] == b'"' && bytes[i + 1] == b'i' && bytes[i + 2] == b'-' && bytes[i + 3].is_ascii_digit() {
            let start = i + 1;
            // The id's rest, from the digit that follows the hyphen.
            let mut end = i + 3;
            while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
                end += 1;
            }
            found.push(state[start..end].to_string());
            i = end;
        } else {
            i += 1;
        }
    }
    found.get(which).cloned()
}

fn inputs_file(temp: &Temp, entries: &[(&str, &str, &str)]) -> PathBuf {
    let p = temp.path("inputs.jsonl");
    let mut text = String::new();
    for (at, source, payload) in entries {
        text.push_str(&format!("{{\"at\": {at}, \"source\": \"{source}\", \"payload\": {payload}}}\n"));
    }
    std::fs::write(&p, text).expect("inputs");
    p
}

/// The network of a source feeding one `human` neuron, whose extra options
/// are `extra` (already the `, "name": value` form), and optionally the run
/// `echo` after it.
fn human_network(extra: &str, echo: bool) -> String {
    let echo_pair = if echo {
        format!(
            ",\n    {{\"id\": \"after\", \"description\": \"after\", \"run\": [\"python3\", \"{}\"]}}",
            progs("echo.py")
        )
    } else {
        String::new()
    };
    let after_link = if echo {
        ",\n    {\"from\": \"who\", \"to\": \"after\"}"
    } else {
        ""
    };
    format!(
        "  {{
  \"version\": 0,
  \"name\": \"test\",
  \"description\": \"desk test\",
  \"neurons\": [
    {{\"id\": \"src\", \"description\": \"in\", \"source\": true}},
    {{\"id\": \"who\", \"description\": \"who\", \"human\": true{extra}}}{}
  ],
  \"links\": [
    {{\"from\": \"src\", \"to\": \"who\"}}{}
  ]
}}
",
        echo_pair, after_link
    )
}

fn answer_ok(run: &mut Run, port: u16, token: &str, item: &str, kind: &str, text: Option<&str>) {
    let body = match text {
        Some(t) => format!("{{\"item\":\"{item}\",\"kind\":\"{kind}\",\"text\":\"{t}\"}}"),
        None => format!("{{\"item\":\"{item}\",\"kind\":\"{kind}\"}}"),
    };
    let r = post(port, token, &body);
    assert_eq!(r.code, 200, "{}", r.body);
    let status = run.wait();
    assert!(status.success(), "stderr: {}", run.err());
}

#[test]
fn desk_approve_sends_the_proposal() {
    let temp = Temp::new("approve");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(
        &temp,
        &[(
            "0",
            "src",
            "{\"draft\":\"the old text\",\"proposal\":\"the new text\",\"title\":\"passage one\",\"notes\":[\"checks passed\"]}",
        )],
    );
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    // The item's shape: the pair, with title and notes; the other fields of
    // the payload are shown, never sent (Appendix D).
    assert!(
        state.contains("\"draft\":\"the old text\"") && state.contains("\"proposal\":\"the new text\""),
        "{}",
        state
    );
    assert!(
        state.contains("\"title\":\"passage one\"") && state.contains("\"notes\":[\"checks passed\"]"),
        "{}",
        state
    );
    let item = extract_id(&state, 0).expect("the item's id");
    // The status goes answered at the settle.
    answer_ok(&mut run, port, &token, &item, "approve", None);
    // The proposal, unchanged, is the printed answer (Appendix D).
    assert_eq!(run.out().trim(), "the new text");
}

#[test]
fn desk_state_carries_the_ask_and_the_stderr_carries_the_answer() {
    // A host recording a run reads `/state` for what was asked (the item's
    // `payload`, the input value whole, and its `t` in run milliseconds) and
    // the run's stderr for what was answered: the desk closes with the run,
    // so a last answer can only be read live — the desk's POST lands it as
    // one `desk answer:` line, the answer the run will carry (an approve's
    // text is the proposal, not the POST's). The page fields stay as
    // Appendix D has them; these ride beside them.
    let temp = Temp::new("ask-answer");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(
        &temp,
        &[(
            "0",
            "src",
            "{\"draft\":\"the old text\",\"proposal\":\"the new text\",\"title\":\"passage one\",\"notes\":[\"checks passed\"],\"id\":\"a#1\",\"round\":2,\"status\":\"ready\",\"extra\":{\"n\":1}}",
        )],
    );
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    // The ask: the whole input under `payload`, and the ask's run time under
    // `t` (a whole millisecond figure, never negative). No answer rides in
    // the state: the desk closes with the run, and answers go to stderr.
    assert!(state.contains("\"payload\":{\"draft\":\"the old text\""), "{}", state);
    assert!(
        state.contains("\"id\":\"a#1\"") && state.contains("\"round\":2") && state.contains("\"status\":\"ready\""),
        "{}",
        state
    );
    assert!(
        state.contains("\"extra\":{\"n\":1}"),
        "the payload's own nesting rides verbatim: {}",
        state
    );
    assert!(state.contains("\"t\":"), "the ask's run time: {}", state);
    assert!(!state.contains("\"t\":-"), "t is a run millisecond, never negative: {}", state);
    // The item is answered on the page (a person's approve); the stderr lands
    // the answer the run will carry, and the printed answer matches it.
    let item = extract_id(&state, 0).expect("the item's id");
    let r = post(port, &token, &format!("{{\"item\":\"{item}\",\"kind\":\"approve\"}}"));
    assert_eq!(r.code, 200, "{}", r.body);
    let status = run.wait();
    assert!(status.success(), "stderr: {}", run.err());
    let want = format!("desk answer: {{\"item\":\"{item}\",\"kind\":\"approve\",\"text\":\"the new text\"}}");
    assert!(run.err().contains(want.as_str()), "the answer as the run carries it: {}", run.err());
    assert_eq!(run.out().trim(), "the new text");
}

#[test]
fn desk_approve_of_a_string_input_sends_the_input() {
    let temp = Temp::new("approve-str");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(&temp, &[("0", "src", "\"go on\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    // A string input is no diff: the page shows its text.
    assert!(state.contains("\"text\":\"go on\""), "{}", state);
    let item = extract_id(&state, 0).expect("the item's id");
    answer_ok(&mut run, port, &token, &item, "approve", None);
    assert_eq!(run.out().trim(), "go on");
}

#[test]
fn desk_edit_sends_the_persons_text() {
    let temp = Temp::new("edit");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(&temp, &[("0", "src", "\"proposed\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    let item = extract_id(&state, 0).expect("the item's id");
    answer_ok(&mut run, port, &token, &item, "edit", Some("my own words"));
    assert_eq!(run.out().trim(), "my own words");
}

#[test]
fn desk_no_answers_quiet() {
    let temp = Temp::new("no");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(&temp, &[("0", "src", "\"anything\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    let item = extract_id(&state, 0).expect("the item's id");
    let r = post(port, &token, &format!("{{\"item\":\"{item}\",\"kind\":\"no\"}}"));
    assert_eq!(r.code, 200, "{}", r.body);
    let status = run.wait();
    assert!(status.success(), "stderr: {}", run.err());
    // The quiet reads back as its own answer line (the desk closes with the
    // run, so a host records answers from the stderr, not a final poll).
    let want = format!("desk answer: {{\"item\":\"{item}\",\"kind\":\"no\",\"text\":\"\"}}");
    assert!(run.err().contains(want.as_str()), "{}", run.err());
    assert_eq!(run.out(), ""); // a quiet prints nothing
}

#[test]
fn desk_empty_edit_is_an_error_reply_not_a_quiet() {
    // The person's edit text, empty: an error reply, and the item keeps
    // waiting (Appendix D); a second, proper answer settles it.
    let temp = Temp::new("edit-empty");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(&temp, &[("0", "src", "\"proposed\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    let item = extract_id(&state, 0).expect("the item's id");
    let r = post(port, &token, &format!("{{\"item\":\"{item}\",\"kind\":\"edit\",\"text\":\"\"}}"));
    assert_eq!(r.code, 400, "{}", r.body);
    assert!(r.body.contains("error"), "{}", r.body);
    // The item still waits: an empty POST was not an answer.
    assert!(get_state(port, &token).body.contains("\"status\":\"waiting\""));
    answer_ok(&mut run, port, &token, &item, "edit", Some("the real edit"));
    assert_eq!(run.out().trim(), "the real edit");
}

#[test]
fn desk_newer_input_withdraws_a_question() {
    // A newer input with the key (5.4) cuts the running human call: its item
    // is withdrawn, an answer to it is refused, and the newer question runs
    // on; the trace shows the running drop.
    let temp = Temp::new("withdraw");
    let net = temp.write("network.json", &human_network("", false));
    // The injections carry the key in their own entries.
    let inputs_p = temp.path("inputs.jsonl");
    std::fs::write(
        &inputs_p,
        "{\"at\":0,\"source\":\"src\",\"payload\":\"the older text\",\"replaces\":\"k\"}\n{\"at\":150,\"source\":\"src\",\"payload\":\"the newer text\",\"replaces\":\"k\"}\n",
    )
    .unwrap();
    let mut run = Run::start(&temp, &net, &inputs_p, &["--trace", &temp.path("t.json").to_string_lossy()]);
    let (port, token) = run.desk();
    let _ = wait_items(&mut run, port, &token, 1);
    let state = poll_state(&mut run, port, &token, |s, over| over || s.matches("\"id\":\"i-").count() >= 2);
    // The first question was withdrawn; its answer is refused.
    let first = extract_id(&state, 0).expect("the first item's id");
    let second = extract_id(&state, 1).expect("the second item's id");
    let r = post(port, &token, &format!("{{\"item\":\"{first}\",\"kind\":\"approve\"}}"));
    assert_eq!(r.code, 410, "an answer to a withdrawn item is refused: {}", r.body);
    assert!(r.body.contains("withdrawn"), "{}", r.body);
    // The newer question answers.
    answer_ok(&mut run, port, &token, &second, "approve", None);
    assert_eq!(run.out().trim(), "the newer text");
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(trace.contains("\"reason\":\"replaced\",\"phase\":\"running\""), "{}", trace);
    assert!(trace.contains("\"what\":\"cancel\""), "{}", trace);
}

#[test]
fn desk_timeout_marks_the_item_and_refuses_its_answer() {
    // One human's timeout is short; the run stays open on the other's: the
    // timed-out item shows timed-out, its answer is refused, and the engine's
    // outcome is the timeout's error (or the unknown of an effect call).
    let temp = Temp::new("timeout");
    let neurons = "{\"id\": \"src\", \"description\": \"in\", \"source\": true},
    {\"id\": \"who\", \"description\": \"who\", \"human\": true, \"timeout\": 400},
    {\"id\": \"other\", \"description\": \"other\", \"human\": true, \"timeout\": 8000}"
        .to_string();
    let links = "{\"from\": \"src\", \"to\": \"who\"},\n    {\"from\": \"src\", \"to\": \"other\"}";
    let net = temp.write(
        "network.json",
        &format!(
            "{{\n  \"version\": 0,\n  \"name\": \"test\",\n  \"description\": \"desk test\",\n  \"neurons\": [\n{neurons}\n  ],\n  \"links\": [\n{links}\n  ]\n}}\n"
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "\"x\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    let (port, token) = run.desk();
    // The two questions are on the page; the first of them takes its
    // timeout: the state shows timed-out while the other still waits.
    let _ = wait_items(&mut run, port, &token, 2);
    let state = poll_state(&mut run, port, &token, |s, over| over || s.contains("\"status\":\"timed-out\""));
    let timed_out = extract_id(&state, 0).expect("the first item's id");
    let other = extract_id(&state, 1).expect("the second item's id");
    let r = post(port, &token, &format!("{{\"item\":\"{timed_out}\",\"kind\":\"approve\"}}"));
    assert_eq!(r.code, 410, "an answer to a timed-out item is refused: {}", r.body);
    assert!(r.body.contains("timed-out"), "{}", r.body);
    answer_ok(&mut run, port, &token, &other, "no", None);
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        trace.contains("\"type\":\"error\"") && trace.contains("\"code\":\"timeout\""),
        "{}",
        trace
    );
    assert!(trace.contains("timed out after 400 ms"), "{}", trace);
}

#[test]
fn desk_a_human_effect_questions_timeout_is_unknown() {
    // A `human` with `effect`: its timeout is an `unknown` (11.3), as the M0
    // fixtures prove for the scripted person.
    let temp = Temp::new("timeout-fx");
    let net = temp.write("network.json", &human_network(", \"timeout\": 400, \"effect\": true", false));
    let inputs = inputs_file(&temp, &[("0", "src", "\"x\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    let _desk = run.desk();
    let status = run.wait();
    assert!(status.success(), "stderr: {}", run.err());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        trace.contains("\"type\":\"unknown\"") && trace.contains("\"code\":\"timeout\""),
        "{}",
        trace
    );
    assert!(trace.contains("timed out after 400 ms"), "{}", trace);
}

#[test]
fn desk_refuses_the_wrong_credentials() {
    let temp = Temp::new("creds");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(&temp, &[("0", "src", "\"x\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    // GET / without the token in the query: no page.
    let r = request(
        port,
        &format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(r.code, 403, "{}", r.head);
    // GET /?token= with a wrong token: no page.
    let r = request(
        port,
        &format!("GET /?token=00000000000000000000000000000000 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(r.code, 403, "{}", r.head);
    // /state with a wrong token, and with none at all.
    let r = request(
        port,
        &format!(
            "GET /state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Desk-Token: 00000000000000000000000000000000\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(r.code, 401, "{}", r.head);
    let r = request(
        port,
        &format!("GET /state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(r.code, 401, "{}", r.head);
    // POST with a wrong token header, and without the header at all.
    let r = post(port, "00000000000000000000000000000000", "{\"item\":\"i-1\",\"kind\":\"no\"}");
    assert_eq!(r.code, 401, "{}", r.body);
    let r = request(
        port,
        &format!(
            "POST /answer HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: 25\r\nConnection: close\r\n\r\n{{\"item\":\"i-1\",\"kind\":\"no\"}}"
        ),
    );
    assert_eq!(r.code, 401, "{}", r.body);
    // A different Host (a rebinding's word) is no host of this desk.
    let r = request(
        port,
        &format!("GET /state HTTP/1.1\r\nHost: evil.example.com:{port}\r\nX-Desk-Token: {token}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(r.code, 403, "{}", r.head);
    // A POST with another Origin, and one without an Origin at all (a
    // browser's fetch always sends one).
    let r = request(
        port,
        &format!(
            "POST /answer HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.2:5\r\nX-Desk-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: 25\r\nConnection: close\r\n\r\n{{\"item\":\"i-1\",\"kind\":\"no\"}}"
        ),
    );
    assert_eq!(r.code, 403, "{}", r.body);
    let r = request(
        port,
        &format!(
            "POST /answer HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Desk-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: 25\r\nConnection: close\r\n\r\n{{\"item\":\"i-1\",\"kind\":\"no\"}}"
        ),
    );
    assert_eq!(r.code, 403, "{}", r.body);
    // A wrong path: nothing here.
    let r = request(
        port,
        &format!("GET /nowhere HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(r.code, 404, "{}", r.head);
    // The desk itself is fine: with the credentials right, the run settles.
    let state = get_state(port, &token);
    assert_eq!(state.code, 200, "{}", state.head);
    let item = extract_id(&state.body, 0).expect("the item's id");
    answer_ok(&mut run, port, &token, &item, "no", None);
    assert_eq!(run.out(), "");
}

#[test]
fn desk_page_has_a_nonce_csp_and_shows_values_as_text() {
    let temp = Temp::new("page");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(
        &temp,
        &[(
            "0",
            "src",
            "{\"draft\":\"old words\",\"proposal\":\"new words\",\"title\":\"a title\",\"notes\":[\"a note\"]}",
        )],
    );
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let page = get_page(port, &token);
    assert_eq!(page.code, 200, "{}", page.head);
    let csp = page
        .head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-security-policy:"))
        .map(|l| l.to_ascii_lowercase())
        .expect("the page's CSP");
    assert!(csp.contains("default-src 'none'"), "{}", csp);
    assert!(csp.contains("script-src 'nonce-") && csp.contains("style-src 'nonce-"), "{}", csp);
    assert!(csp.contains("connect-src 'self'"), "{}", csp);
    // One inline script and one inline style, both under the nonce.
    let nonce = csp
        .split("'nonce-")
        .nth(1)
        .and_then(|n| n.split('\'').next())
        .expect("the nonce")
        .to_string();
    assert!(page.body.contains(&format!("<script nonce=\"{nonce}\">")), "{}", nonce);
    assert!(page.body.contains(&format!("<style nonce=\"{nonce}\">")), "the style is nonced");
    // Values are text, never HTML; nothing is fetched from anywhere outside.
    assert!(!page.body.contains("innerHTML"), "values are text, never HTML");
    assert!(!page.body.contains("document.write"), "values are text, never HTML");
    assert!(!page.body.contains("src=\"http"), "nothing is fetched from elsewhere");
    assert!(!page.body.contains("href=\"http"), "nothing is fetched from elsewhere");
    // The input's values ride in /state as strings, verbatim.
    let state = get_state(port, &token).body;
    assert!(state.contains("old words") && state.contains("new words"), "{}", state);
    assert!(state.contains("a title") && state.contains("a note"), "{}", state);
    // The page's person says no; the run ends.
    let item = extract_id(&state, 0).expect("the item's id");
    answer_ok(&mut run, port, &token, &item, "no", None);
    assert_eq!(run.out(), "");
}

#[test]
fn desk_item_of_a_human_inside_a_composed_instance() {
    // The question of a human inside a composed file: its item is keyed by
    // the instance path, and the composed neuron's answer reaches the run.
    let inner = format!(
        "  {{
  \"version\": 0,
  \"name\": \"inner\",
  \"description\": \"inner\",
  \"entry\": \"e\",
  \"exit\": \"who\",
  \"neurons\": [
    {{\"id\": \"e\", \"description\": \"e\", \"run\": [\"python3\", \"{}\"]}},
    {{\"id\": \"who\", \"description\": \"who\", \"human\": true}}
  ],
  \"links\": [
    {{\"from\": \"e\", \"to\": \"who\"}}
  ]
}}
",
        progs("echo.py")
    );
    let temp = Temp::new("composed");
    temp.write("inner.json", &inner);
    temp.write(
        "network.json",
        "  {
  \"version\": 0,
  \"name\": \"test\",
  \"description\": \"composed desk\",
  \"neurons\": [
    {\"id\": \"src\", \"description\": \"in\", \"source\": true},
    {\"id\": \"box\", \"description\": \"box\", \"network\": \"inner.json\"}
  ],
  \"links\": [
    {\"from\": \"src\", \"to\": \"box\"}
  ]
}
",
    );
    let inputs = inputs_file(&temp, &[("0", "src", "\"say\"")]);
    let mut run = Run::start(&temp, &temp.path("network.json"), &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    assert!(state.contains("\"neuron\":\"box/who\""), "{}", state);
    let item = extract_id(&state, 0).expect("the item's id");
    answer_ok(&mut run, port, &token, &item, "approve", None);
    // The relay's answer is the JSON `{"seen":"a"}`-shaped value, whose
    // Approve text is the input as JSON text.
    assert_eq!(run.out().trim(), "{\"seen\":\"say\"}");
}

// -- Grants (decision 0010) ---------------------------------------------------

/// Two people's reviews and a lead's gate: `r1` and `r2` are reviewers, `gate`
/// is the lead's; each is a human neuron fed by the source.
fn table_network() -> String {
    "{
  \"version\": 0,
  \"name\": \"table\",
  \"description\": \"desk grants test\",
  \"neurons\": [
    {\"id\": \"src\", \"description\": \"in\", \"source\": true},
    {\"id\": \"r1\", \"description\": \"first review\", \"human\": true},
    {\"id\": \"gate\", \"description\": \"the gate\", \"human\": true}
  ],
  \"links\": [
    {\"from\": \"src\", \"to\": \"r1\"},
    {\"from\": \"src\", \"to\": \"gate\"}
  ]
}
"
    .to_string()
}

const TABLE_GRANTS: &str = r#"{
  "version": 0,
  "people": [
    { "id": "kit", "name": "Kit", "roles": ["reviewer"], "answers": ["r1"] },
    { "id": "rue", "name": "Rue", "roles": ["lead"], "answers": ["gate", "r1"] }
  ]
}"#;

impl Run {
    /// Each person's token, from the `desk person:` lines.
    fn people(&mut self) -> Vec<(String, String)> {
        for _ in 0..500 {
            let found: Vec<(String, String)> = self
                .err()
                .lines()
                .filter_map(|l| l.strip_prefix("desk person: "))
                .map(|j| {
                    let id = j
                        .split("\"id\":\"")
                        .nth(1)
                        .and_then(|r| r.split('"').next())
                        .unwrap_or("")
                        .to_string();
                    let token = j
                        .split("?token=")
                        .nth(1)
                        .and_then(|r| r.split('"').next())
                        .unwrap_or("")
                        .to_string();
                    (id, token)
                })
                .collect();
            if !found.is_empty() {
                return found;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no desk person lines: {}", self.err());
    }
}

fn token_of(people: &[(String, String)], id: &str) -> String {
    people
        .iter()
        .find(|(p, _)| p == id)
        .map(|(_, t)| t.clone())
        .expect("the person's token")
}

#[test]
fn desk_grants_give_each_person_a_token_and_refuse_what_is_not_theirs() {
    let temp = Temp::new("grants");
    let net = temp.write("network.json", &table_network());
    let grants = temp.write("grants.json", TABLE_GRANTS);
    let inputs = inputs_file(
        &temp,
        &[("0", "src", "{\"title\":\"which way\",\"answerable_by\":[\"reviewer\",\"lead\"]}")],
    );
    let mut run = Run::start(&temp, &net, &inputs, &["--grants", grants.to_str().unwrap()]);
    let (port, owner) = run.desk();
    let people = run.people();
    assert_eq!(people.len(), 2, "{}", run.err());
    let kit = token_of(&people, "kit");
    let rue = token_of(&people, "rue");
    assert_ne!(kit, owner);
    // Kit's view: who Kit is, the table, and which items are Kit's.
    let state = wait_items(&mut run, port, &kit, 2);
    assert!(
        state.contains("\"me\":{\"id\":\"kit\",\"name\":\"Kit\",\"roles\":[\"reviewer\"]}"),
        "{}",
        state
    );
    assert!(state.contains("\"people\":["), "{}", state);
    let r1 = extract_id(&state, 0).expect("r1's item");
    let gate = extract_id(&state, 1).expect("gate's item");
    // The gate is not Kit's: refused with 403, logged, and still waiting.
    let r = post(port, &kit, &format!("{{\"item\":\"{gate}\",\"kind\":\"approve\"}}"));
    assert_eq!(r.code, 403, "{}", r.body);
    assert!(
        run.err().contains(&format!(
            "desk refused: {{\"item\":\"{gate}\",\"neuron\":\"gate\",\"by\":\"kit\",\"why\":\"not granted\"}}"
        )),
        "{}",
        run.err()
    );
    // Kit answers r1 with a why; Rue gates.
    let r = post(
        port,
        &kit,
        &format!("{{\"item\":\"{r1}\",\"kind\":\"edit\",\"text\":\"left\",\"why\":\"shorter\"}}"),
    );
    assert_eq!(r.code, 200, "{}", r.body);
    let r = post(port, &rue, &format!("{{\"item\":\"{gate}\",\"kind\":\"edit\",\"text\":\"go\"}}"));
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(run.wait().success(), "{}", run.err());
    let err = run.err();
    assert!(
        err.contains(&format!(
            "desk answer: {{\"item\":\"{r1}\",\"kind\":\"edit\",\"text\":\"left\",\"by\":\"kit\",\"why\":\"shorter\"}}"
        )),
        "{}",
        err
    );
    assert!(
        err.contains(&format!(
            "desk answer: {{\"item\":\"{gate}\",\"kind\":\"edit\",\"text\":\"go\",\"by\":\"rue\"}}"
        )),
        "{}",
        err
    );
    // The network got the people's text only: the why and the names stay on the desk.
    let out = run.out();
    assert!(
        out.contains("left") && out.contains("go") && !out.contains("shorter") && !out.contains("kit"),
        "{}",
        out
    );
}

#[test]
fn desk_answerable_by_must_name_a_role_of_the_person() {
    let temp = Temp::new("roles");
    let net = temp.write("network.json", &table_network());
    let grants = temp.write("grants.json", TABLE_GRANTS);
    // r1 is granted to Kit, but this ask is for a lead only.
    let inputs = inputs_file(&temp, &[("0", "src", "{\"title\":\"leads only\",\"answerable_by\":\"lead\"}")]);
    let mut run = Run::start(&temp, &net, &inputs, &["--grants", grants.to_str().unwrap()]);
    let (port, owner) = run.desk();
    let people = run.people();
    let kit = token_of(&people, "kit");
    let state = wait_items(&mut run, port, &kit, 2);
    // Kit sees r1's item as not Kit's; only Rue may answer it.
    assert!(state.contains("\"yours\":false,\"answerable_by\":[\"rue\"]"), "{}", state);
    let r1 = extract_id(&state, 0).expect("r1's item");
    let r = post(port, &kit, &format!("{{\"item\":\"{r1}\",\"kind\":\"no\"}}"));
    assert_eq!(r.code, 403, "{}", r.body);
    // The owner's token answers anything, recorded as the owner.
    let gate = extract_id(&state, 1).expect("gate's item");
    for item in [&r1, &gate] {
        let r = post(port, &owner, &format!("{{\"item\":\"{item}\",\"kind\":\"no\"}}"));
        assert_eq!(r.code, 200, "{}", r.body);
    }
    assert!(run.wait().success(), "{}", run.err());
    assert!(
        run.err().contains(&format!(
            "desk answer: {{\"item\":\"{r1}\",\"kind\":\"no\",\"text\":\"\",\"by\":\"owner\"}}"
        )),
        "{}",
        run.err()
    );
}

#[test]
fn desk_choose_sends_the_options_text_and_records_the_choice() {
    let temp = Temp::new("choose");
    let net = temp.write("network.json", &human_network("", false));
    let ask = "{\"title\":\"pick one\",\"options\":[{\"id\":\"a\",\"label\":\"Plan A\",\"text\":\"plan-a\"},{\"id\":\"b\",\"label\":\"Plan B\"}]}";
    let inputs = inputs_file(&temp, &[("0", "src", ask)]);
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    let state = wait_items(&mut run, port, &token, 1);
    let item = extract_id(&state, 0).expect("the item");
    // An option the item does not have is refused; the item keeps waiting.
    let r = post(
        port,
        &token,
        &format!("{{\"item\":\"{item}\",\"kind\":\"choose\",\"option\":\"z\"}}"),
    );
    assert_eq!(r.code, 400, "{}", r.body);
    let r = post(port, &token, &format!("{{\"item\":\"{item}\",\"kind\":\"choose\"}}"));
    assert_eq!(r.code, 400, "{}", r.body);
    let r = post(
        port,
        &token,
        &format!("{{\"item\":\"{item}\",\"kind\":\"choose\",\"option\":\"a\",\"why\":\"cheaper\"}}"),
    );
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(run.wait().success(), "{}", run.err());
    assert_eq!(run.out().trim(), "plan-a");
    // Without grants there is no `by`; the option and the why are on the line.
    assert!(
        run.err().contains(&format!(
            "desk answer: {{\"item\":\"{item}\",\"kind\":\"choose\",\"text\":\"plan-a\",\"option\":\"a\",\"why\":\"cheaper\"}}"
        )),
        "{}",
        run.err()
    );
}

#[test]
fn desk_grants_that_name_no_human_neuron_refuse_the_run() {
    let temp = Temp::new("badgrants");
    let net = temp.write("network.json", &table_network());
    let inputs = inputs_file(&temp, &[("0", "src", "\"x\"")]);
    for (grants, says) in [
        (r#"{"version":0,"people":[{"id":"kit","answers":["src"]}]}"#, "not a human neuron"),
        (
            r#"{"version":0,"people":[{"id":"kit","answers":["r1"],"email":"x"}]}"#,
            "do not know",
        ),
        (r#"{"version":0,"people":[{"id":"owner","answers":["r1"]}]}"#, "'owner'"),
        (
            r#"{"version":0,"people":[{"id":"kit","answers":["r1"]},{"id":"kit","answers":["gate"]}]}"#,
            "twice",
        ),
    ] {
        let g = temp.write("grants.json", grants);
        let mut run = Run::start(&temp, &net, &inputs, &["--grants", g.to_str().unwrap()]);
        let status = run.wait();
        assert!(!status.success());
        assert!(run.err().contains(says), "{} -> {}", grants, run.err());
        assert!(!run.err().contains("desk: "), "{}", run.err());
    }
}

#[test]
fn desk_answers_a_client_that_connects_before_it_writes() {
    // A connection accepted before its request has arrived must still be
    // read: on a platform where an accepted socket inherits the listener's
    // non-blocking mode (macOS), a read at accept time would fail at once and
    // the desk would drop the connection unanswered.
    let temp = Temp::new("slowclient");
    let net = temp.write("network.json", &human_network("", false));
    let inputs = inputs_file(&temp, &[("0", "src", "\"hello\"")]);
    let mut run = Run::start(&temp, &net, &inputs, &[]);
    let (port, token) = run.desk();
    wait_items(&mut run, port, &token, 1);
    for _ in 0..3 {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the desk's port");
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        // Let the desk accept the connection before a byte of it arrives.
        std::thread::sleep(Duration::from_millis(300));
        let head = format!("GET /state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Desk-Token: {token}\r\nConnection: close\r\n\r\n");
        stream.write_all(head.as_bytes()).expect("the request's write");
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        let r = parse_response(&raw);
        assert_eq!(r.code, 200, "a slow client got {:?}", String::from_utf8_lossy(&raw));
    }
}
