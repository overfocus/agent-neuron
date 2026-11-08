// The MCP server of M3, driven as a subprocess: `neuron-engine mcp` spoken
// to as a client speaks JSON lines and reads replies through a buffered
// reader — no MCP client dependency. Every line the server prints on stdout
// must be a JSON-RPC 2.0 frame; the logs are on stderr and never read here.
// The suites of this crate run one test at a time (.cargo/config.toml: real
// processes and real deadlines are timing-sensitive), and so do these.
use rust_engine::json::{self, Value};
use std::io::{BufRead, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

// -- The subprocess client ------------------------------------------------------

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    notes: Vec<Value>,
}

impl Client {
    fn start() -> Client {
        let mut child = Command::new(engine())
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the server starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (tx, lines) = mpsc::channel::<String>();
        // The reader thread: every line must be a protocol frame, whatever
        // the test reads next (a log on stdout fails the next frame read).
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });
        Client {
            child,
            stdin: Some(stdin),
            lines,
            notes: Vec::new(),
        }
    }

    /// One line out, flushed.
    fn send(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("the client's stdin");
        stdin.write_all(line.as_bytes()).expect("write");
        stdin.write_all(b"\n").expect("write");
        stdin.flush().expect("flush");
    }

    /// The next frame from stdout, parsed, within `secs`. A stray frame is
    /// still asserted to be a JSON-RPC 2.0 object (the protocol's purity).
    fn next_frame(&mut self, secs: u64) -> Value {
        let line = match self.lines.recv_timeout(Duration::from_secs(secs)) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("the server said nothing within {secs}s"),
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the server's stdout closed"),
        };
        self.frame_of(&line)
    }

    fn frame_of(&self, line: &str) -> Value {
        let v = json::parse(line).unwrap_or_else(|e| panic!("the server's line is not JSON ({e}): {line}"));
        assert!(
            v.get("jsonrpc").and_then(|x| x.as_str()) == Some("2.0"),
            "the server's line is not a JSON-RPC 2.0 frame: {line}"
        );
        v
    }

    /// One request; its response (notifications that pass by are skipped,
    /// still asserted as frames, and kept in `notes` for the tests that
    /// count them).
    fn rpc(&mut self, id: u64, body: &str) -> Value {
        self.send(&body.replace("\"ID\"", &format!("{id}")));
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let secs = deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_secs(1))
                .as_secs();
            let v = self.next_frame(secs);
            if v.get("id").is_some_and(|i| i.as_i64() == Some(id as i64)) {
                return v;
            }
            assert!(v.get("method").is_some(), "a response to another id than {id}: {}", json::write(&v));
            self.notes.push(v);
        }
    }

    /// The notifications a request's replies skipped past.
    fn notes_of(&self) -> &[Value] {
        &self.notes
    }

    fn result_of(&self, reply: &Value) -> Value {
        reply.get("result").cloned().expect("the reply carries a result")
    }

    /// The body of a tools/call reply: its structuredContent, and the text
    /// block that must be its byte-identical compact JSON.
    fn tool_body(&self, reply: &Value) -> (Value, bool) {
        let result = self.result_of(reply);
        let content = result.get("content").and_then(|c| c.as_arr()).expect("content").to_vec();
        assert_eq!(content.len(), 1, "one text block: {}", json::write(&result));
        let text = content[0].get("text").and_then(|t| t.as_str()).expect("the text");
        let body = json::parse(text).expect("the text block is JSON");
        let structured = result.get("structuredContent").expect("structuredContent").clone();
        assert!(
            body.deep_eq(&structured),
            "the text block and structuredContent disagree:\n{text}\n{}",
            json::write(&structured)
        );
        (body, result.get("isError").and_then(|b| b.as_bool()).unwrap_or(false))
    }

    /// The `job_id` a start_run came back with (inside the tool result's
    /// structuredContent).
    fn job_of(&self, reply: &Value) -> String {
        let (body, _) = self.tool_body(reply);
        body.get("job_id").and_then(|j| j.as_str()).expect("job_id").to_string()
    }
}

fn engine() -> &'static str {
    env!("CARGO_BIN_EXE_neuron-engine")
}

fn handshake(client: &mut Client) {
    let reply = client.rpc(
        1,
        r#"{"jsonrpc":"2.0","id":"ID","method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"t","version":"1"}}}"#,
    );
    let result = client.result_of(&reply);
    assert!(
        result.get("protocolVersion").and_then(|v| v.as_str()) == Some("2024-11-05"),
        "{}",
        json::write(&result)
    );
    let caps = result.get("capabilities").expect("capabilities");
    let tools = caps.get("tools").expect("tools");
    assert_eq!(tools.get("listChanged").and_then(|b| b.as_bool()), Some(false));
    let resources = caps.get("resources").expect("resources");
    assert_eq!(resources.get("subscribe").and_then(|b| b.as_bool()), Some(true));
    assert_eq!(resources.get("listChanged").and_then(|b| b.as_bool()), Some(false));
    let info = result.get("serverInfo").expect("serverInfo");
    assert_eq!(info.get("name").and_then(|n| n.as_str()), Some("agent-neuron"));
    assert_eq!(info.get("version").and_then(|v| v.as_str()), Some("0.0.0"));
    // The handshake's closing notification is served without an answer, and
    // the very next frame is this ping's.
    client.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    let reply = client.rpc(2, r#"{"jsonrpc":"2.0","id":"ID","method":"ping"}"#);
    assert!(client.result_of(&reply).keys().is_empty());
}

// -- The files of the tests ------------------------------------------------------

struct Temp {
    dir: std::path::PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-mcp-{}-{}-{}",
            tag,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        Temp { dir }
    }
    fn write(&self, name: &str, contents: &str) -> String {
        let p = self.dir.join(name);
        std::fs::write(&p, contents).expect("write file");
        p.to_string_lossy().to_string()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn spec_examples() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("spec")
        .join("examples")
        .to_string_lossy()
        .to_string()
}

/// A network of a source and, per entry, one neuron (the shape tests/real.rs
/// builds): (id, kind field, command JSON, extra options JSON).
fn network(name: &str, entries: &[(&str, &str, &str, &str)]) -> String {
    let mut neurons = String::from("{\"id\": \"src\", \"description\": \"in\", \"source\": true}");
    let mut links = Vec::new();
    for (id, kind, cmd, opts) in entries {
        neurons.push_str(&format!(
            ",\n    {{\"id\": \"{}\", \"description\": \"{}\", {}: {}{}}}",
            id, id, kind, cmd, opts
        ));
        links.push(format!("{{\"from\": \"src\", \"to\": \"{}\"}}", id));
    }
    format!(
        "{{\n  \"version\": 0,\n  \"name\": \"{name}\",\n  \"description\": \"a test network\",\n  \"neurons\": [\n{neurons}\n  ],\n  \"links\": [\n{links}\n  ]\n}}\n",
        neurons = neurons,
        links = links.join(",\n    ")
    )
}

/// A network whose run is honest but takes a moment: one slow sh program
/// that answers (for the job's live resource, the polls, and the
/// subscription).
fn slow_network() -> String {
    network(
        "slowish",
        &[(
            "e",
            "\"run\"",
            r#"["sh", "-c", "cat > /dev/null; sleep 0.5; echo '{\"ok\": true}'"]"#,
            "",
        )],
    )
}

/// A network whose neuron never answers within any sensible deadline (its
/// own `timeout` is 60 s; the MCP tool's `timeout_ms` arrives first).
fn never_answers_network() -> String {
    network(
        "hung",
        &[(
            "s",
            "\"run\"",
            r#"["sh", "-c", "cat > /dev/null; sleep 60"]"#,
            r#", "timeout": 60000"#,
        )],
    )
}

fn run_call(file: &str, inputs_json: &str, extra: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{{"name":"run_network","arguments":{{"file":"{file}","inputs":{inputs_json}{extra}}}}}}}"#
    )
}

fn start_call(file: &str, inputs_json: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{{"name":"start_run","arguments":{{"file":"{file}","inputs":{inputs_json},"keep_trace":true}}}}}}"#
    )
}

fn poll_call(job: &str, wait_ms: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{{"name":"poll_run","arguments":{{"job_id":"{job}","wait_ms":{wait_ms}}}}}}}"#
    )
}

fn check_call(file: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{{"name":"check_network","arguments":{{"file":"{file}"}}}}}}"#)
}

fn describe_call(file: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{{"name":"describe_network","arguments":{{"file":"{file}"}}}}}}"#)
}

fn read_trace_call(path: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{{"name":"read_trace","arguments":{{"path":"{path}"}}}}}}"#)
}

fn plain_inputs() -> &'static str {
    r#"[{"at":0,"source":"src","payload":{"k":1}}]"#
}

fn bad_network(name: &str) -> String {
    format!(
        r#"{{
  "version": 0,
  "name": "{name}",
  "description": "the same id twice",
  "neurons": [
    {{"id": "a", "description": "in", "source": true}},
    {{"id": "a", "description": "dup", "source": true}}
  ],
  "links": []
}}
"#
    )
}

// -- The tests --------------------------------------------------------------------

#[test]
fn the_handshake_pings_and_the_wire_errors() {
    let mut c = Client::start();
    handshake(&mut c);
    // A version the client names that the server speaks is echoed; a second
    // `initialize` on the same server is refused: negotiation happens once.
    let reply = c.rpc(
        3,
        r#"{"jsonrpc":"2.0","id":"ID","method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    );
    assert_eq!(
        reply.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()),
        Some("the server is already initialized"),
        "{}",
        json::write(&reply)
    );
    // An unknown method is a protocol error...
    let reply = c.rpc(4, r#"{"jsonrpc":"2.0","id":"ID","method":"some/method"}"#);
    let error = reply.get("error").cloned().expect("an error reply");
    assert_eq!(error.get("code").and_then(|x| x.as_i64()), Some(-32601), "{}", json::write(&error));
    // ...and so are malformed params, without taking the server down.
    let reply = c.rpc(5, r#"{"jsonrpc":"2.0","id":"ID","method":"ping","params":"not-an-object"}"#);
    assert_eq!(
        reply.get("error").and_then(|e| e.get("code")).and_then(|x| x.as_i64()),
        Some(-32600)
    );
    let reply = c.rpc(6, r#"{"jsonrpc":"2.0","id":"ID","method":"ping"}"#);
    assert!(c.result_of(&reply).keys().is_empty());
}

#[test]
fn a_malformed_line_is_a_parse_error_and_the_server_stays_up() {
    let mut c = Client::start();
    handshake(&mut c);
    c.send("this is not JSON");
    let frame = c.next_frame(30);
    // A parse error carries the null id, whatever else the frame was.
    assert!(
        frame.get("id").map(|i| matches!(i, Value::Null)).unwrap_or(false),
        "{}",
        json::write(&frame)
    );
    let error = frame.get("error").cloned().expect("an error reply");
    assert_eq!(error.get("code").and_then(|x| x.as_i64()), Some(-32700), "{}", json::write(&error));
    let reply = c.rpc(9, r#"{"jsonrpc":"2.0","id":"ID","method":"ping"}"#);
    assert!(c.result_of(&reply).keys().is_empty());
    let mut c2 = Client::start();
    // A version the client names that the server speaks is echoed...
    let reply = c2.rpc(
        1,
        r#"{"jsonrpc":"2.0","id":"ID","method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    );
    assert_eq!(
        c2.result_of(&reply).get("protocolVersion").and_then(|v| v.as_str()),
        Some("2025-06-18")
    );
    // ...and one the server does not speak falls back to the first stable
    // protocol, on the reply, not on a new request.
    let mut c3 = Client::start();
    let reply = c3.rpc(
        1,
        r#"{"jsonrpc":"2.0","id":"ID","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
    );
    assert_eq!(
        c3.result_of(&reply).get("protocolVersion").and_then(|v| v.as_str()),
        Some("2024-11-05")
    );
}

#[test]
fn tools_list_the_six_tools() {
    let mut c = Client::start();
    handshake(&mut c);
    let reply = c.rpc(2, r#"{"jsonrpc":"2.0","id":"ID","method":"tools/list"}"#);
    let result = c.result_of(&reply);
    let tools = result.get("tools").and_then(|t| t.as_arr()).expect("tools").to_vec();
    let names: Vec<&str> = tools.iter().filter_map(|t| t.get("name").and_then(|n| n.as_str())).collect();
    assert_eq!(
        names,
        vec![
            "check_network",
            "describe_network",
            "run_network",
            "start_run",
            "poll_run",
            "read_trace"
        ],
        "{}",
        json::write(&result)
    );
    for tool in &tools {
        assert!(tool.get("description").and_then(|d| d.as_str()).is_some_and(|d| !d.is_empty()));
        let schema = tool.get("inputSchema").expect("inputSchema");
        assert_eq!(schema.get("type").and_then(|t| t.as_str()), Some("object"));
        assert_eq!(schema.get("additionalProperties").and_then(|a| a.as_bool()), Some(false));
        assert!(!schema.get("required").and_then(|r| r.as_arr()).expect("required").is_empty());
    }
    // The run tools' inputs are closed entries: `at`, `source` and `payload`
    // required, and no other fields take part.
    for name in ["run_network", "start_run"] {
        let tool = tools
            .iter()
            .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(name))
            .expect("the tool");
        let schema = tool.get("inputSchema").expect("schema");
        let inputs = schema.get("properties").and_then(|p| p.get("inputs")).expect("inputs");
        assert_eq!(
            inputs.get("type").and_then(|t| t.as_str()),
            Some("array"),
            "{}",
            json::write(inputs)
        );
        let entry = inputs.get("items").expect("items");
        let required: Vec<String> = entry
            .get("required")
            .and_then(|r| r.as_arr())
            .expect("required")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        assert_eq!(required, vec!["at", "source", "payload"], "{}", json::write(entry));
        assert_eq!(entry.get("additionalProperties").and_then(|a| a.as_bool()), Some(false));
        assert!(schema.get("properties").and_then(|p| p.get("timeout_ms")).is_some());
        assert!(schema.get("properties").and_then(|p| p.get("keep_trace")).is_some());
    }
}

#[test]
fn resources_list_the_spec_resources_and_the_live_run() {
    let mut c = Client::start();
    handshake(&mut c);
    let reply = c.rpc(2, r#"{"jsonrpc":"2.0","id":"ID","method":"resources/list"}"#);
    let items = c
        .result_of(&reply)
        .get("resources")
        .and_then(|r| r.as_arr())
        .expect("resources")
        .to_vec();
    let uris: Vec<&str> = items.iter().filter_map(|r| r.get("uri").and_then(|u| u.as_str())).collect();
    assert_eq!(
        uris,
        vec![
            "spec://SPEC.md",
            "spec://CONFORMANCE.md",
            "spec://TRACE.md",
            "spec://network.schema.json"
        ],
        "{}",
        json::write(&Value::Arr(items.clone()))
    );
    for item in &items {
        let uri = item.get("uri").and_then(|u| u.as_str()).unwrap_or("");
        let mime = item.get("mimeType").and_then(|m| m.as_str()).unwrap_or("");
        if uri.ends_with(".json") {
            assert_eq!(mime, "application/json");
        } else {
            assert_eq!(mime, "text/markdown");
        }
        assert!(item.get("name").and_then(|n| n.as_str()).is_some_and(|n| !n.is_empty()));
        assert!(item.get("title").and_then(|t| t.as_str()).is_some_and(|t| !t.is_empty()));
    }
    // A run joins the list while it lives, with the run://{id} uri and the
    // mime of its state.
    let temp = Temp::new("list-run");
    let net = temp.write("net.json", &slow_network());
    let reply = c.rpc(3, &start_call(&net, plain_inputs()));
    let job = c.job_of(&reply);
    let run_uri = format!("run://{job}");
    let reply = c.rpc(4, &poll_call(&job, 10_000));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    assert_eq!(body.get("status").and_then(|s| s.as_str()), Some("done"));
    // Jobs live for the server's lifetime: the resource is listed after the
    // run's result was collected, too.
    let reply = c.rpc(5, r#"{"jsonrpc":"2.0","id":"ID","method":"resources/list"}"#);
    let items = c
        .result_of(&reply)
        .get("resources")
        .and_then(|r| r.as_arr())
        .expect("resources")
        .to_vec();
    let run = items
        .iter()
        .find(|r| r.get("uri").and_then(|u| u.as_str()) == Some(run_uri.as_str()))
        .cloned()
        .expect("the run's resource");
    assert_eq!(run.get("mimeType").and_then(|m| m.as_str()), Some("application/json"));
    assert!(run.get("name").and_then(|n| n.as_str()).is_some_and(|n| !n.is_empty()));
    assert!(run.get("title").and_then(|t| t.as_str()).is_some_and(|t| !t.is_empty()));
}

#[test]
fn resources_read_the_embedded_text_byte_for_byte() {
    let mut c = Client::start();
    handshake(&mut c);
    let reads = [
        ("spec://SPEC.md", "text/markdown", include_str!("../../docs/SPEC.md")),
        ("spec://CONFORMANCE.md", "text/markdown", include_str!("../../docs/CONFORMANCE.md")),
        ("spec://TRACE.md", "text/markdown", include_str!("../../docs/TRACE.md")),
        (
            "spec://network.schema.json",
            "application/json",
            include_str!("../../spec/network.schema.json"),
        ),
    ];
    for (id, (uri, mime, embedded)) in reads.into_iter().enumerate() {
        let id = (2 + id) as u64;
        let reply = c.rpc(
            id,
            &format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"resources/read","params":{{"uri":"{uri}"}}}}"#),
        );
        let contents = c
            .result_of(&reply)
            .get("contents")
            .and_then(|x| x.as_arr())
            .expect("contents")
            .to_vec();
        assert_eq!(contents.len(), 1, "{uri}");
        let content = &contents[0];
        assert_eq!(content.get("uri").and_then(|u| u.as_str()), Some(uri));
        assert_eq!(content.get("mimeType").and_then(|m| m.as_str()), Some(mime));
        assert_eq!(
            content.get("text").and_then(|t| t.as_str()),
            Some(embedded),
            "{uri} is not the text of the repo's file"
        );
    }
    // A run the server does not have is a refused read.
    let reply = c.rpc(
        9,
        r#"{"jsonrpc":"2.0","id":"ID","method":"resources/read","params":{"uri":"run://9999"}}"#,
    );
    assert_eq!(
        reply.get("error").and_then(|e| e.get("code")).and_then(|x| x.as_i64()),
        Some(-32602)
    );
    let reply = c.rpc(
        10,
        r#"{"jsonrpc":"2.0","id":"ID","method":"resources/read","params":{"uri":"spec://none.md"}}"#,
    );
    assert_eq!(
        reply.get("error").and_then(|e| e.get("code")).and_then(|x| x.as_i64()),
        Some(-32602)
    );
}

#[test]
fn check_network_reports_the_check_surfaces_findings() {
    let mut c = Client::start();
    handshake(&mut c);
    // The example network of the spec: no errors, no warnings (the executables
    // of the file are on the server's PATH, as check would find them).
    let good = format!("{}/notify.network.json", spec_examples());
    let reply = c.rpc(2, &check_call(&good));
    let (body, is_error) = c.tool_body(&reply);
    assert_eq!(body.get("ok").and_then(|b| b.as_bool()), Some(true), "{}", json::write(&body));
    assert!(!is_error, "{}", json::write(&body));
    assert!(body.get("errors").and_then(|e| e.as_arr()).expect("errors").is_empty());
    assert!(body.get("warnings").and_then(|w| w.as_arr()).expect("warnings").is_empty());
    // A duplicate id: an error with the file's position.
    let temp = Temp::new("check");
    let bad = temp.write("bad.json", &bad_network("bad"));
    let reply = c.rpc(3, &check_call(&bad));
    let (body, is_error) = c.tool_body(&reply);
    assert_eq!(body.get("ok").and_then(|b| b.as_bool()), Some(false));
    assert!(is_error, "a check that found errors is the tool's error");
    let errors = body.get("errors").and_then(|e| e.as_arr()).expect("errors").to_vec();
    assert!(!errors.is_empty(), "{}", json::write(&body));
    for e in &errors {
        assert!(e.get("file").and_then(|f| f.as_str()).is_some_and(|f| f == bad));
        let line = e.get("line").and_then(|l| l.as_i64()).expect("line");
        let col = e.get("col").and_then(|x| x.as_i64()).expect("col");
        assert!(line >= 1 && col >= 1);
        assert!(!e.get("message").and_then(|m| m.as_str()).is_some_and(|m| m.is_empty()));
    }
    // A file that cannot be read is the tool's error, with the check's own
    // reading; and so is a call that names no file at all.
    let reply = c.rpc(4, &check_call("no/such/file.json"));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error);
    assert_eq!(body.get("ok").and_then(|b| b.as_bool()), Some(false));
    let reply = c.rpc(
        5,
        r#"{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{"name":"check_network","arguments":{}}}"#,
    );
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error);
    assert!(body.get("error").and_then(|e| e.as_str()).is_some_and(|e| e.contains("'file'")));
}

#[test]
fn describe_network_gives_the_shape_with_composed_networks_inside() {
    let mut c = Client::start();
    handshake(&mut c);
    let file = format!("{}/alerts.network.json", spec_examples());
    let reply = c.rpc(2, &describe_call(&file));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    assert_eq!(body.get("ok").and_then(|b| b.as_bool()), Some(true));
    let net = body.get("network").expect("network");
    assert_eq!(net.get("file").and_then(|f| f.as_str()), Some(file.as_str()));
    assert_eq!(net.get("name").and_then(|n| n.as_str()), Some("alerts"));
    let neurons = net.get("neurons").and_then(|n| n.as_arr()).expect("neurons").to_vec();
    let ids: Vec<&str> = neurons.iter().filter_map(|n| n.get("id").and_then(|i| i.as_str())).collect();
    assert_eq!(ids, vec!["alerts", "logs", "deploys", "metrics", "hypothesis", "diagnosis"]);
    // The trace's own neuron shape: kind words of SPEC 7.1, options when set.
    let kind = |id: &str| {
        neurons
            .iter()
            .find(|n| n.get("id").and_then(|i| i.as_str()) == Some(id))
            .expect("the neuron")
            .clone()
    };
    assert_eq!(kind("alerts").get("kind").and_then(|k| k.as_str()), Some("source"));
    let hyp = kind("hypothesis");
    assert_eq!(
        hyp.get("options").and_then(|o| o.get("threshold")).and_then(|t| t.as_i64()),
        Some(2)
    );
    assert_eq!(net.get("links").and_then(|l| l.as_arr()).map(|l| l.len()), Some(7));
    // The composed neuron carries its inner network, named as the file names it.
    let diag = kind("diagnosis");
    assert_eq!(diag.get("kind").and_then(|k| k.as_str()), Some("composed"));
    let inner = diag.get("network").expect("the inner network");
    assert_eq!(inner.get("file").and_then(|f| f.as_str()), Some("diagnosis.network.json"));
    assert_eq!(inner.get("entry").and_then(|e| e.as_str()), Some("topology"));
    assert_eq!(inner.get("exit").and_then(|e| e.as_str()), Some("check"));
    assert_eq!(inner.get("neurons").and_then(|n| n.as_arr()).map(|n| n.len()), Some(2));
}

#[test]
fn describe_network_marks_effect_neurons() {
    let mut c = Client::start();
    handshake(&mut c);
    let file = format!("{}/notify.network.json", spec_examples());
    let reply = c.rpc(2, &describe_call(&file));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    let neurons = body
        .get("network")
        .and_then(|n| n.get("neurons"))
        .and_then(|n| n.as_arr())
        .expect("neurons")
        .to_vec();
    let effect = |id: &str| {
        neurons
            .iter()
            .find(|n| n.get("id").and_then(|i| i.as_str()) == Some(id))
            .expect("the neuron")
            .get("effect")
            .and_then(|e| e.as_bool())
    };
    // `notify` is declared effect; the others carry no mark at all.
    assert_eq!(effect("notify"), Some(true));
    let unmarked = neurons.iter().filter(|n| n.get("effect").is_none()).count();
    assert_eq!(unmarked, neurons.len() - 1);
}

#[test]
fn describe_network_refuses_what_does_not_check_and_ends_a_loop() {
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("describe");
    // A file with errors: the check's diagnostics, and the tool's error.
    let bad = temp.write("bad.json", &bad_network("bad"));
    let reply = c.rpc(2, &describe_call(&bad));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error);
    assert_eq!(body.get("ok").and_then(|b| b.as_bool()), Some(false));
    assert!(!body.get("errors").and_then(|e| e.as_arr()).expect("errors").is_empty());
    assert!(body.get("network").is_none());
    // A composed file that composes itself checks clean; the description
    // ends at the loop and marks it rather than recursing.
    let top = temp.write(
        "top.json",
        r#"{"version":0,"name":"top","description":"t","neurons":[{"id":"src","description":"in","source":true},{"id":"c","description":"c","network":"loop.json"}],"links":[{"from":"src","to":"c"}]}"#,
    );
    temp.write(
        "loop.json",
        r#"{"version":0,"name":"loop","description":"l","entry":"x","exit":"y","neurons":[{"id":"x","description":"x","network":"loop.json"},{"id":"y","description":"y","run":["cat"]}],"links":[{"from":"x","to":"y"}]}"#,
    );
    let reply = c.rpc(3, &describe_call(&top));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    let inner = body
        .get("network")
        .and_then(|n| n.get("neurons"))
        .and_then(|n| n.as_arr())
        .and_then(|n| n.get(1))
        .and_then(|n| n.get("network"))
        .expect("the composed file")
        .clone();
    assert_eq!(inner.get("name").and_then(|n| n.as_str()), Some("loop"));
    let again = inner
        .get("neurons")
        .and_then(|n| n.as_arr())
        .and_then(|n| n.first())
        .and_then(|n| n.get("network"))
        .expect("the loop's mark")
        .clone();
    assert_eq!(again.get("cycle").and_then(|b| b.as_bool()), Some(true));
    assert_eq!(again.get("file").and_then(|f| f.as_str()), Some("loop.json"));
    assert!(again.get("neurons").is_none());
    // A field the schema does not hold is refused, with the tool's name.
    let reply = c.rpc(
        4,
        r#"{"jsonrpc":"2.0","id":"ID","method":"tools/call","params":{"name":"describe_network","arguments":{"file":"x","deep":true}}}"#,
    );
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error);
    assert!(body
        .get("error")
        .and_then(|e| e.as_str())
        .is_some_and(|e| e.contains("describe_network")));
}

#[test]
fn run_network_answers_sync() {
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("answers");
    let net = temp.write(
        "net.json",
        &network(
            "tiny",
            &[("e", "\"run\"", r#"["sh", "-c", "cat > /dev/null; echo '{\"x\": 7}'"]"#, "")],
        ),
    );
    let reply = c.rpc(2, &run_call(&net, plain_inputs(), ""));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    assert_eq!(body.get("status").and_then(|s| s.as_str()), Some("done"));
    let answers = body.get("answers").and_then(|a| a.as_arr()).expect("answers").to_vec();
    assert_eq!(answers.len(), 1, "{}", json::write(&body));
    assert_eq!(answers[0].as_str(), Some("{\"x\":7}"), "{}", json::write(&body));
    let events = body.get("events").and_then(|e| e.as_arr()).expect("events").to_vec();
    assert!(!events.is_empty());
    let summary = body.get("summary").expect("summary");
    assert_eq!(summary.get("demo").and_then(|d| d.as_str()), Some("tiny"));
    assert_eq!(summary.get("events").and_then(|e| e.as_i64()), Some(events.len() as i64));
    assert_eq!(summary.get("answers").and_then(|a| a.as_i64()), Some(1));
    // The answers' order and shape are the engine's (a text payload is not
    // wrapped); the events carry the run's signal and finished.
    assert!(events.iter().any(|e| e.get("type").and_then(|t| t.as_str()) == Some("signal")));
    assert!(events.iter().any(|e| e.get("type").and_then(|t| t.as_str()) == Some("answer")));
    assert!(events.iter().any(|e| e.get("type").and_then(|t| t.as_str()) == Some("finished")));
}

#[test]
fn run_network_refuses_the_subjects_files() {
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("refusals");
    // A file that does not check: the diagnostics of the load are the reason.
    let bad = temp.write("bad.json", &bad_network("bad"));
    let reply = c.rpc(2, &run_call(&bad, plain_inputs(), ""));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error, "{}", json::write(&body));
    assert_eq!(body.get("status").and_then(|s| s.as_str()), Some("refused"));
    let reason = body.get("reason").and_then(|r| r.as_str()).expect("reason").to_string();
    assert!(reason.starts_with("cannot run: "), "{reason}");
    // An `https://` neuron: the honest refusal of a std-only engine.
    let tls = temp.write(
        "tls.json",
        &network("tls", &[("h", "\"http\"", "\"https://example.com/hook\"", "")]),
    );
    let reply = c.rpc(3, &run_call(&tls, plain_inputs(), ""));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error, "{}", json::write(&body));
    let reason = body.get("reason").and_then(|r| r.as_str()).expect("reason").to_string();
    assert!(reason.contains("no TLS"), "{reason}");
    // And a network with a `human` neuron: no desk on this surface, with the
    // desk refusal's extra clause about the MCP route.
    let person = temp.write("person.json", &network("person", &[("ask", "\"human\"", "true", "")]));
    let reply = c.rpc(4, &run_call(&person, plain_inputs(), ""));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error, "{}", json::write(&body));
    assert_eq!(body.get("status").and_then(|s| s.as_str()), Some("refused"));
    let reason = body.get("reason").and_then(|r| r.as_str()).expect("reason").to_string();
    assert!(reason.contains("no desk for a person"), "{reason}");
    assert!(reason.contains("the desk is the person's surface, not an MCP surface"), "{reason}");
}

#[test]
fn run_network_times_out_at_the_deadline() {
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("timeout");
    let net = temp.write("net.json", &never_answers_network());
    let reply = c.rpc(2, &run_call(&net, plain_inputs(), r#","timeout_ms":2000"#));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error, "{}", json::write(&body));
    assert_eq!(
        body.get("status").and_then(|s| s.as_str()),
        Some("failed"),
        "{}",
        json::write(&body)
    );
    let reason = body.get("reason").and_then(|r| r.as_str()).expect("reason").to_string();
    assert_eq!(reason, "timed out at 2000 ms", "{reason}");
    // The events of the run up to the deadline are the body's events, and
    // the summary says their whole count.
    let events = body.get("events").and_then(|e| e.as_arr()).expect("events").to_vec();
    assert!(!events.is_empty(), "{}", json::write(&body));
    assert_eq!(
        body.get("summary").and_then(|s| s.get("events")).and_then(|e| e.as_i64()),
        Some(events.len() as i64)
    );
}

#[test]
fn poll_run_of_an_unknown_job_is_an_error() {
    let mut c = Client::start();
    handshake(&mut c);
    let reply = c.rpc(2, &poll_call("9999", 0));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error);
    assert!(body.get("error").and_then(|e| e.as_str()).is_some_and(|e| e.contains("9999")));
}

#[test]
fn start_run_polls_and_the_resource_lives() {
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("job");
    let net = temp.write("net.json", &slow_network());
    let reply = c.rpc(2, &start_call(&net, plain_inputs()));
    let job = c.job_of(&reply);
    // The live resource while the job runs: its running state, named by the
    // job id, with the events so far and the answers' place.
    let reply = c.rpc(
        3,
        &format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"resources/read","params":{{"uri":"run://{job}"}}}}"#),
    );
    let contents = c
        .result_of(&reply)
        .get("contents")
        .and_then(|x| x.as_arr())
        .expect("contents")
        .to_vec();
    assert_eq!(contents.len(), 1);
    let state = json::parse(contents[0].get("text").and_then(|t| t.as_str()).expect("text")).expect("the state is JSON");
    assert_eq!(state.get("job_id").and_then(|j| j.as_str()), Some(job.as_str()));
    assert_eq!(
        state.get("status").and_then(|s| s.as_str()),
        Some("running"),
        "{}",
        json::write(&state)
    );
    assert!(state.get("events_so_far").and_then(|e| e.as_arr()).is_some());
    assert!(state.get("answers_so_far").and_then(|a| a.as_arr()).is_some());
    // The run completes; the resource then carries the run's result with the
    // path of the trace it kept.
    let reply = c.rpc(4, &poll_call(&job, 10_000));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    assert_eq!(body.get("status").and_then(|s| s.as_str()), Some("done"), "{}", json::write(&body));
    let trace = body
        .get("trace_path")
        .and_then(|t| t.as_str())
        .expect("the kept trace's path")
        .to_string();
    let reply = c.rpc(
        5,
        &format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"resources/read","params":{{"uri":"run://{job}"}}}}"#),
    );
    let contents = c
        .result_of(&reply)
        .get("contents")
        .and_then(|x| x.as_arr())
        .expect("contents")
        .to_vec();
    let state = json::parse(contents[0].get("text").and_then(|t| t.as_str()).expect("text")).expect("the state is JSON");
    assert_eq!(state.get("status").and_then(|s| s.as_str()), Some("done"));
    assert!(state.get("trace_path").and_then(|t| t.as_str()).is_some_and(|t| !t.is_empty()));
    // The trace resource reads the file whole.
    let reply = c.rpc(
        6,
        &format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"resources/read","params":{{"uri":"trace://{job}"}}}}"#),
    );
    let contents = c
        .result_of(&reply)
        .get("contents")
        .and_then(|x| x.as_arr())
        .expect("contents")
        .to_vec();
    let from_file = std::fs::read_to_string(&trace).expect("the trace file");
    assert_eq!(contents[0].get("text").and_then(|t| t.as_str()), Some(from_file.as_str()));
}

#[test]
fn subscribe_sees_the_runs_updates() {
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("subscribe");
    let net = temp.write(
        "net.json",
        &network(
            "signed",
            &[(
                "e",
                "\"run\"",
                r#"["sh", "-c", "cat > /dev/null; sleep 1.4; echo '{\"ok\": true}'"]"#,
                "",
            )],
        ),
    );
    let reply = c.rpc(2, &start_call(&net, plain_inputs()));
    let job = c.job_of(&reply);
    // The subscription stands while the run works; its changes are
    // announced (at most one per second per job, the run's end always).
    let reply = c.rpc(
        3,
        &format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"resources/subscribe","params":{{"uri":"run://{job}"}}}}"#),
    );
    assert!(c.result_of(&reply).keys().is_empty());
    // Wait for the run's own end; the announcements pass by the poll reply
    // (the client records them), and any that are still coming are read
    // from the wire — the terminal one may be on the wire after the reply.
    let reply = c.rpc(4, &poll_call(&job, 10_000));
    let (body, _) = c.tool_body(&reply);
    assert_eq!(body.get("status").and_then(|s| s.as_str()), Some("done"));
    let mut updates: Vec<String> = c
        .notes_of()
        .iter()
        .filter(|n| n.get("method").and_then(|m| m.as_str()) == Some("notifications/resources/updated"))
        .filter_map(|n| {
            n.get("params")
                .and_then(|p| p.get("uri"))
                .and_then(|u| u.as_str())
                .map(str::to_string)
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    while updates.len() < 2 && Instant::now() < deadline {
        match c.lines.recv_timeout(Duration::from_secs(2)) {
            Ok(line) => {
                let frame = c.frame_of(&line);
                if frame.get("method").and_then(|m| m.as_str()) == Some("notifications/resources/updated") {
                    if let Some(uri) = frame.get("params").and_then(|p| p.get("uri")).and_then(|u| u.as_str()) {
                        updates.push(uri.to_string());
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // At least two announcements, both for this run's uri (the second of the
    // running changes lands after the first's second passed, and the run's
    // end is announced against that limit).
    for uri in &updates {
        assert_eq!(uri, &format!("run://{job}"));
    }
    assert!(
        updates.len() >= 2,
        "a running change and the run's end are two announcements: {updates:?}"
    );
    // A run the server does not have is a refused subscription.
    let reply = c.rpc(
        5,
        r#"{"jsonrpc":"2.0","id":"ID","method":"resources/subscribe","params":{"uri":"run://9999"}}"#,
    );
    assert_eq!(
        reply.get("error").and_then(|e| e.get("code")).and_then(|x| x.as_i64()),
        Some(-32602)
    );
    // The unsubscribe takes it back off (and answers, whatever it removed).
    let reply = c.rpc(
        6,
        &format!(r#"{{"jsonrpc":"2.0","id":"ID","method":"resources/unsubscribe","params":{{"uri":"run://{job}"}}}}"#),
    );
    assert!(c.result_of(&reply).keys().is_empty());
}

#[test]
fn read_trace_returns_the_trace_file() {
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("trace");
    let net = temp.write(
        "net.json",
        &network(
            "tracey",
            &[("e", "\"run\"", r#"["sh", "-c", "cat > /dev/null; echo '{\"x\": 3}'"]"#, "")],
        ),
    );
    let reply = c.rpc(2, &run_call(&net, plain_inputs(), r#","keep_trace":true"#));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    let trace = body.get("trace_path").and_then(|t| t.as_str()).expect("trace_path").to_string();
    let reply = c.rpc(3, &read_trace_call(&trace));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    let header = body.get("header").expect("header");
    assert_eq!(header.get("format").and_then(|f| f.as_str()), Some("agent-neuron-trace"));
    assert_eq!(header.get("version").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(header.get("demo").and_then(|d| d.as_str()), Some("tracey"));
    // The header is the file's header: the writer's `ids` field rides with it.
    assert_eq!(
        header.get("ids").map(json::write),
        Some("{\"signal\":\"run-numbered\",\"trace\":\"run-numbered\"}".to_string()),
        "{}",
        json::write(header)
    );
    let events: Vec<&Value> = body
        .get("events")
        .and_then(|e| e.as_arr())
        .expect("events")
        .iter()
        .collect::<Vec<_>>();
    // The tool's events are the file's events, whole (a small trace is one
    // under the tool's event cap).
    let whole = std::fs::read_to_string(&trace).expect("the trace file");
    let file_events = json::parse(&whole)
        .expect("json")
        .get("events")
        .and_then(|e| e.as_arr())
        .expect("the file's events")
        .to_vec();
    assert_eq!(events.len(), file_events.len());
    assert!(events.iter().zip(file_events.iter()).all(|(a, b)| a.deep_eq(b)));
    // An unknown version is refused; so is a path the server cannot read.
    let other = temp.write("other.json", r#"{"format": "agent-neuron-trace", "version": 9, "events": []}"#);
    let reply = c.rpc(4, &read_trace_call(&other));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error, "{}", json::write(&body));
    assert!(body.get("error").and_then(|e| e.as_str()).is_some_and(|e| e.contains("not a v1")));
    let reply = c.rpc(5, &read_trace_call("/no/such/trace.json"));
    let (body, is_error) = c.tool_body(&reply);
    assert!(is_error, "{}", json::write(&body));
    assert!(body
        .get("error")
        .and_then(|e| e.as_str())
        .is_some_and(|e| e.contains("cannot read")));
}

#[test]
fn read_trace_reads_a_trace_that_has_no_ids_field() {
    // The `ids` field is optional for a reader (a v1 reader ignores a top
    // level field it does not know): the tool reads a trace without one and
    // carries the header as it stands.
    let mut c = Client::start();
    handshake(&mut c);
    let temp = Temp::new("trace-no-ids");
    let bare = temp.write(
        "bare.json",
        r#"{
  "format": "agent-neuron-trace",
  "version": 1,
  "demo": "bare",
  "neurons": [],
  "events": [],
  "summary": {}
}"#,
    );
    let reply = c.rpc(2, &read_trace_call(&bare));
    let (body, is_error) = c.tool_body(&reply);
    assert!(!is_error, "{}", json::write(&body));
    let header = body.get("header").expect("header");
    assert!(header.get("ids").is_none(), "{}", json::write(header));
}

#[test]
fn the_server_leaves_when_the_client_closes() {
    let mut c = Client::start();
    handshake(&mut c);
    let stdin = c.stdin.take().expect("stdin");
    drop(stdin); // close it: the client is gone
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "the server did not leave after EOF");
        match c.child.try_wait().expect("wait") {
            Some(status) => {
                assert!(status.success(), "the server left with {status}");
                break;
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    // And nothing comes off a protocol whose client left: a frame within
    // the window would be a line the server owed nobody.
    let extra = c.lines.recv_timeout(Duration::from_secs(2));
    assert!(extra.is_err(), "the server said more after it left: {extra:?}");
}
