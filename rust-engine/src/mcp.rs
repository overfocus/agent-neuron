// The `mcp` command: the engine as an MCP server (Model Context Protocol's
// stable core subset) over stdio, newline-delimited JSON-RPC 2.0, hand-rolled
// on `json.rs` and the standard library.
//
// One request per line, one response per line; notifications (no `id`) are
// served without an answer; logs go to stderr only — stdout is the protocol.
// The subset: the `initialize` handshake, `ping`, `tools/list` and
// `tools/call` (five tools: `check_network`, `run_network`, `start_run`,
// `poll_run`, `read_trace`), and `resources/list`/`read`/`subscribe`/
// `unsubscribe` (the four spec resources, embedded at build, and the live
// `run://{job}` ones). No sampling, no elicitation, no prompts; `conform` and
// the fixtures are not exposed. A network with `human` neurons is refused on
// this surface: the desk is the person's page, not an MCP result.
use crate::fixture;
use crate::host;
use crate::json::{self, Obj, Value};
use crate::net::Network;
use crate::run;
use crate::trace;
use crate::{check, engine::Engine, runner};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const SERVER_NAME: &str = "agent-neuron";
const SERVER_VERSION: &str = "0.0.0";
/// The protocol versions this server speaks: the handshake answers with the
/// client's request when it is one of these, and with the first stable
/// protocol when it is not. Nothing else in the server branches on the
/// version: the result shapes are the stable core subset's own.
const SUPPORTED_PROTOCOLS: [&str; 2] = ["2024-11-05", "2025-06-18"];
const PROTOCOL_FALLBACK: &str = "2024-11-05";
const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_WAIT_MS: u64 = 30_000;
/// A tool result carries a run's events whole to this count; above it, the
/// first 299 events plus one `truncated` marker (the whole count rides in
/// the `summary` and the marker alike).
const EVENTS_SHOWN: usize = 299;
const EVENTS_WHOLE: usize = 300;
/// The rate of a running job's notifications, per job. The terminal change
/// is announced against it, as `Notice` says.
const NOTICE_INTERVAL: Duration = Duration::from_secs(1);

const SPEC_MD: &str = include_str!("../../docs/SPEC.md");
const CONFORMANCE_MD: &str = include_str!("../../docs/CONFORMANCE.md");
const TRACE_MD: &str = include_str!("../../docs/TRACE.md");
const SCHEMA_JSON: &str = include_str!("../../spec/network.schema.json");

pub fn main(args: &[String]) {
    if !args.is_empty() {
        eprintln!("usage: neuron-engine mcp");
        std::process::exit(2);
    }
    serve();
}

// ---------------------------------------------------------------------------
// The wire: one line in, at most one line out, logs on stderr
// ---------------------------------------------------------------------------

/// A protocol-level refusal (JSON-RPC's own errors): `code` and `message` of
/// the wire's `error` object. Tool failures a subject carried are not these —
/// they travel as results with `isError`.
struct Rpc {
    code: i64,
    message: String,
}

fn rpc_err(code: i64, message: impl Into<String>) -> Rpc {
    Rpc {
        code,
        message: message.into(),
    }
}

/// What the request loop and the notification thread share: the stdout lock
/// (both writers take it, so a line is never torn), the subscription set,
/// the jobs, and the temp directory the server's traces live in (removed
/// when the server exits by its own path).
struct Shared {
    out: Mutex<std::io::Stdout>,
    subs: Mutex<HashSet<String>>,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    next_job: AtomicU64,
    next_trace: AtomicU64,
    sync_traces: Mutex<HashMap<String, String>>, // trace://sync/{n} -> path
    notice: Mutex<mpsc::Sender<Notice>>,
    temp: PathBuf,
    initialized: AtomicBool,
}

/// A job's state change, for the notifications thread. The terminal change
/// (the run ended — done, refused, failed) is announced against the rate
/// limit, because it is the one change a subscriber must not miss; a run's
/// updates while it works are coarse, at most one per second per job.
struct Notice {
    job: u64,
    terminal: bool,
}

/// A job's live state: the events as they happen (the engine offers every
/// reported event to the stream's sink), and the run's final result once it
/// ended.
struct JobCore {
    events: Vec<Value>,
    final_result: Option<Value>,
}

/// One run the server carries as a job (`start_run`, `poll_run`).
struct Job {
    id: String,
    file: String,
    core: Arc<Mutex<JobCore>>,
    ended: Arc<(Mutex<bool>, Condvar)>,
}

/// A run the tool route carries (both run tools' arguments, in one reading).
struct RunSpec {
    file: String,
    inputs: Vec<fixture::Input>,
    timeout_ms: u64,
    keep_trace: bool,
}

/// What a job's thread hands the engine's stream: the live resource's state,
/// and the change notice that follows it.
struct Live {
    job: u64,
    core: Arc<Mutex<JobCore>>,
    notice: mpsc::Sender<Notice>,
}

/// One run through the route, as a tool result reads it.
struct Executed {
    result: Value,
    trace_path: Option<String>,
    is_error: bool,
}

struct Server {
    shared: Arc<Shared>,
}

pub fn serve() {
    // A signal that ends the server kills every live group first (decision
    // 0001): an interrupted server leaves no serve programs behind.
    host::install_signal_teardown();
    let temp = temp_dir();
    if let Err(e) = std::fs::create_dir_all(&temp) {
        eprintln!("mcp: the trace directory {temp:?} could not be created ({e}); keep_trace will fail");
    }
    let (notice, notices) = mpsc::channel::<Notice>();
    let shared = Arc::new(Shared {
        out: Mutex::new(std::io::stdout()),
        subs: Mutex::new(HashSet::new()),
        jobs: Mutex::new(HashMap::new()),
        next_job: AtomicU64::new(1),
        next_trace: AtomicU64::new(1),
        sync_traces: Mutex::new(HashMap::new()),
        notice: Mutex::new(notice),
        temp,
        initialized: AtomicBool::new(false),
    });
    // The notification writer: one thread owns every protocol line a job's
    // state change produces (the request loop writes its answers on the same
    // lock, so the two never tear a line).
    let wshared = shared.clone();
    host::spawn_thread("mcp-notifies", move || write_notices(wshared, notices));
    let mut server = Server { shared };
    eprintln!("mcp: agent-neuron {SERVER_VERSION} ready on stdin/stdout");
    let stdin = std::io::stdin();
    for line in BufReader::new(stdin.lock()).lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("mcp: stdin failed ({e}); the server closed");
                break;
            }
        };
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        server.frame(line);
    }
    // The exit: a run still open gets its groups killed at once (the polite
    // end needs a process to keep waiting — there is none after the exit),
    // the trace directory goes, and the process leaves.
    host::kill_live_groups();
    let _ = std::fs::remove_dir_all(&server.shared.temp);
    eprintln!("mcp: the client is gone; the server closed");
    std::process::exit(0);
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("agent-neuron-mcp-{}", std::process::id()))
}

/// One line to stdout, whole, flushed: everything the protocol says travels
/// through this or the request loop's call of it.
fn write_line(out: &Mutex<std::io::Stdout>, line: &str) {
    let Ok(mut lock) = out.lock() else { return };
    if let Err(e) = lock
        .write_all(line.as_bytes())
        .and_then(|_| lock.write_all(b"\n"))
        .and_then(|_| lock.flush())
    {
        eprintln!("mcp: stdout failed ({e}); the line was lost");
    }
}

/// The notifications thread: `notifications/resources/updated` with the
/// job's `uri`, at most one per second per job while the job runs, plus the
/// terminal change; and only for a client that subscribed to the uri (one
/// client, one subscription set).
fn write_notices(shared: Arc<Shared>, notices: mpsc::Receiver<Notice>) {
    let mut last: HashMap<u64, Instant> = HashMap::new();
    for n in notices {
        let uri = format!("run://{}", n.job);
        let subscribed = shared.subs.lock().map(|s| s.contains(&uri)).unwrap_or(false);
        if !subscribed {
            continue;
        }
        let waiting = last.get(&n.job).is_some_and(|t| t.elapsed() < NOTICE_INTERVAL);
        if waiting && !n.terminal {
            continue;
        }
        last.insert(n.job, Instant::now());
        let body = Obj::new()
            .str("jsonrpc", "2.0")
            .str("method", "notifications/resources/updated")
            .set("params", Obj::new().str("uri", uri).build())
            .build();
        write_line(&shared.out, &json::write(&body));
    }
}

impl Server {
    fn frame(&mut self, line: &str) {
        match json::parse(line) {
            Ok(req) => self.request(req),
            Err(e) => {
                eprintln!("mcp: a line was not JSON");
                self.reply(
                    Value::Null,
                    Err(rpc_err(-32700, format!("Parse error: the line is not JSON ({e})"))),
                );
            }
        }
    }

    fn reply(&mut self, id: Value, result: Result<Value, Rpc>) {
        let o = Obj::new().str("jsonrpc", "2.0").set("id", id);
        let body = match result {
            Ok(v) => o.set("result", v).build(),
            Err(rpc) => o
                .set("error", Obj::new().int("code", rpc.code).str("message", rpc.message).build())
                .build(),
        };
        write_line(&self.shared.out, &json::write(&body));
    }

    /// One parsed frame. Notifications (no `id`) are served without an
    /// answer; requests get one, whatever the method turned out to be.
    fn request(&mut self, req: Value) {
        let Value::Obj(entries) = &req else {
            self.reply(Value::Null, Err(rpc_err(-32600, "Invalid Request: the frame is not an object")));
            return;
        };
        let id = entries.iter().find(|(k, _)| k == "id").map(|(_, v)| v.clone());
        let version_ok = req.get("jsonrpc").and_then(|v| v.as_str()).is_some_and(|s| s == "2.0");
        if !version_ok {
            if let Some(id) = id {
                self.reply(id, Err(rpc_err(-32600, "Invalid Request: jsonrpc is \"2.0\"")));
            }
            return;
        }
        let Some(method) = req.get("method").and_then(|v| v.as_str()).map(str::to_string) else {
            if let Some(id) = id {
                self.reply(id, Err(rpc_err(-32600, "Invalid Request: the frame has no method")));
            }
            return;
        };
        if req.has("params") && !req.get("params").is_some_and(|p| p.is_obj()) {
            // params must be structured; a bare value is a bad frame, and a
            // notification with one is ignored as any other notification is.
            if let Some(id) = id {
                self.reply(id, Err(rpc_err(-32600, "Invalid Request: params is an object")));
            }
            return;
        }
        match id {
            None => {
                // A notification: `initialized` closes the handshake on the
                // server's side; `cancelled` is accepted and not acted on (a
                // tool call here runs to its own end); anything else unknown
                // is ignored, as JSON-RPC says a notification must be.
                if !matches!(method.as_str(), "notifications/initialized" | "notifications/cancelled") {
                    eprintln!("mcp: ignored the notification {method}");
                }
            }
            Some(id) => {
                eprintln!("mcp: {method}");
                let result = self.call(&method, req.get("params"));
                self.reply(id, result);
            }
        }
    }

    fn call(&mut self, method: &str, params: Option<&Value>) -> Result<Value, Rpc> {
        let initialized = self.shared.initialized.load(Ordering::SeqCst);
        match method {
            "initialize" => self.initialize(params, initialized),
            // Ping is part of the core protocol and needs no handshake.
            "ping" => Ok(Value::Obj(Vec::new())),
            "tools/list" if initialized => self.tools(),
            "tools/call" if initialized => self.tool_call(params),
            "resources/list" if initialized => self.resources(),
            "resources/read" if initialized => self.resource_read(params),
            "resources/subscribe" if initialized => self.resource_subscribe(params, true),
            "resources/unsubscribe" if initialized => self.resource_subscribe(params, false),
            "tools/list" | "tools/call" | "resources/list" | "resources/read" | "resources/subscribe" | "resources/unsubscribe" => {
                Err(rpc_err(-32600, "Invalid Request: initialize first"))
            }
            _ => Err(rpc_err(-32601, format!("Method not found: {method}"))),
        }
    }

    /// The handshake: the client's protocol version echoed when the server
    /// speaks it, else the first stable protocol's; the two capabilities
    /// (tools listed, resources read and subscribed, no list-changed
    /// notifications sent); the server's name and version. A second
    /// `initialize` is refused: negotiation happens once.
    fn initialize(&mut self, params: Option<&Value>, initialized: bool) -> Result<Value, Rpc> {
        if initialized {
            return Err(rpc_err(-32602, "the server is already initialized"));
        }
        self.shared.initialized.store(true, Ordering::SeqCst);
        let requested = params.and_then(|p| p.get("protocolVersion")).and_then(|v| v.as_str());
        let version = match requested {
            Some(v) if SUPPORTED_PROTOCOLS.contains(&v) => v.to_string(),
            _ => PROTOCOL_FALLBACK.to_string(),
        };
        Ok(Obj::new()
            .str("protocolVersion", version)
            .set("capabilities", capabilities())
            .set(
                "serverInfo",
                Obj::new().str("name", SERVER_NAME).str("version", SERVER_VERSION).build(),
            )
            .build())
    }

    // -- The tools ----------------------------------------------------------

    /// The six tools, each with its closed schema (`required` marked).
    fn tools(&self) -> Result<Value, Rpc> {
        let tools = vec![
            tool(
                "check_network",
                "Check a network file tree against the closed schema (SPEC 2.7): the diagnostics, \
                 errors and warnings, each with its file, position and message; nothing runs.",
                file_schema(),
            ),
            tool(
                "describe_network",
                "The shape of a network file tree, for drawing it before any traffic: its name, \
                 description, entry, exit and budget, every neuron as a v1 trace declares it \
                 (TRACE.md) with effect: true and at: [x, y, z] where the file declares them, and every link; a composed neuron carries its inner network the same \
                 way under 'network'. The file is checked first, and a check with errors is this \
                 tool's error, with the diagnostics. Nothing runs; events never name inner neurons.",
                file_schema(),
            ),
            tool(
                "run_network",
                "Run a network synchronously with the given inputs through the engine's real \
                 route: the run's answers, events and summary. A refused run, or one that \
                 exceeds timeout_ms, is this tool's error. A network with human neurons is \
                 refused: the desk is the person's surface, not an MCP surface.",
                run_schema(),
            ),
            tool(
                "start_run",
                "Start a run with the same arguments as run_network and return at once with its \
                 job_id: the run goes in its own thread; watch it through the run://{job_id} \
                 resource and its updated notifications.",
                run_schema(),
            ),
            tool(
                "poll_run",
                "A run started with start_run: its result once it is over, or status running. \
                 wait_ms waits for the end, at most 30000.",
                poll_schema(),
            ),
            tool(
                "read_trace",
                "The records of a v1 trace file (TRACE.md): the header as the file holds it, \
                 and the events whole.",
                path_schema(),
            ),
        ];
        Ok(Obj::new().set("tools", Value::Arr(tools)).build())
    }

    /// `tools/call`: the result as `content` (one text block, the compact
    /// JSON of the body, byte-identical to it) and `structuredContent` (the
    /// body itself), with `isError` for a subject failure — a refused or
    /// timed-out run, a check that found errors, a thing the tool could not
    /// read. An unknown tool name is a protocol error, as the spec's error
    /// table has it.
    fn tool_call(&mut self, params: Option<&Value>) -> Result<Value, Rpc> {
        let Some(params) = params else {
            return Err(rpc_err(-32602, "Invalid params: the call carries no params"));
        };
        let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
            return Err(rpc_err(-32602, "Invalid params: the call names no tool"));
        };
        let args = params.get("arguments").cloned().unwrap_or(Value::Obj(Vec::new()));
        let (body, is_error) = match name {
            "check_network" => self.tool_check_network(&args),
            "describe_network" => self.tool_describe_network(&args),
            "run_network" => self.tool_run_network(&args),
            "start_run" => self.tool_start_run(&args),
            "poll_run" => self.tool_poll_run(&args),
            "read_trace" => self.tool_read_trace(&args),
            _ => return Err(rpc_err(-32602, format!("Invalid params: unknown tool '{name}'"))),
        };
        let envelope = Obj::new()
            .set(
                "content",
                Value::Arr(vec![Obj::new().str("type", "text").str("text", json::write(&body)).build()]),
            )
            .set("structuredContent", body)
            .set_if("isError", is_error.then_some(Value::Bool(true)))
            .build();
        Ok(envelope)
    }

    /// The schemas are closed objects; so is this reader, and an unknown
    /// field of the arguments is refused, with the tool's own name.
    fn unknown_field(&self, tool: &str, args: &Value, known: &[&str]) -> Option<String> {
        for key in args.keys() {
            if !known.contains(&key) {
                let list = known.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
                return Some(format!("{tool} takes {list}; unknown field '{key}'"));
            }
        }
        None
    }

    /// The diagnostics of a network file tree, the `check` command's whole
    /// surface (same error set and wordings, relative to the server's working
    /// directory): `ok` is the file's word, and a check that found errors is
    /// the tool's error.
    fn tool_check_network(&mut self, args: &Value) -> (Value, bool) {
        if let Some(why) = self.unknown_field("check_network", args, &["file"]) {
            return (fail_body(&why), true);
        }
        let Some(file) = args.get("file").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
            return (fail_body("check_network needs a network file ('file')"), true);
        };
        let diags = check::tree(file);
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        for d in &diags {
            if d.error {
                errors.push(diag_json(d));
            } else {
                warnings.push(diag_json(d));
            }
        }
        let ok = errors.is_empty();
        (
            Obj::new()
                .set("ok", Value::Bool(ok))
                .set("errors", Value::Arr(errors))
                .set("warnings", Value::Arr(warnings))
                .build(),
            !ok,
        )
    }

    /// The network's shape, checked first: a tree that does not check is the
    /// tool's error with the check's own diagnostics, as `check_network`
    /// gives them; one that does is `{ok, warnings, network}`.
    fn tool_describe_network(&mut self, args: &Value) -> (Value, bool) {
        if let Some(why) = self.unknown_field("describe_network", args, &["file"]) {
            return (fail_body(&why), true);
        }
        let Some(file) = args.get("file").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
            return (fail_body("describe_network needs a network file ('file')"), true);
        };
        let diags = check::tree(file);
        let errors: Vec<Value> = diags.iter().filter(|d| d.error).map(diag_json).collect();
        let warnings: Vec<Value> = diags.iter().filter(|d| !d.error).map(diag_json).collect();
        if !errors.is_empty() {
            return (
                Obj::new()
                    .set("ok", Value::Bool(false))
                    .set("errors", Value::Arr(errors))
                    .set("warnings", Value::Arr(warnings))
                    .build(),
                true,
            );
        }
        match describe(Path::new(file), file, false, &mut Vec::new()) {
            Ok(network) => (
                Obj::new()
                    .set("ok", Value::Bool(true))
                    .set("warnings", Value::Arr(warnings))
                    .set("network", network)
                    .build(),
                false,
            ),
            Err(why) => (fail_body(&why), true),
        }
    }

    /// The synchronous run: the engine's real route on this thread. A
    /// refusal (loading included) or a timeout is the tool's error.
    fn tool_run_network(&mut self, args: &Value) -> (Value, bool) {
        let spec = match run_args("run_network", args) {
            Ok(spec) => spec,
            Err(why) => return (fail_body(&why), true),
        };
        let n = self.shared.next_trace.fetch_add(1, Ordering::SeqCst);
        let mut eng = route_engine(&spec);
        let executed = run_route(&spec, None, &mut eng, self.shared.temp.join(format!("trace-sync-{n}.json")));
        if let Some(path) = executed.trace_path.clone() {
            let mut syncs = self.shared.sync_traces.lock().unwrap_or_else(|e| e.into_inner());
            syncs.insert(format!("trace://sync/{n}"), path);
        }
        (executed.result, executed.is_error)
    }

    /// The job: the same arguments, run in the job's own thread; the answer
    /// is the job's id at once.
    fn tool_start_run(&mut self, args: &Value) -> (Value, bool) {
        let spec = match run_args("start_run", args) {
            Ok(spec) => spec,
            Err(why) => return (fail_body(&why), true),
        };
        let n = self.shared.next_job.fetch_add(1, Ordering::SeqCst);
        let id = format!("{n}");
        let core = Arc::new(Mutex::new(JobCore {
            events: Vec::new(),
            final_result: None,
        }));
        let ended = Arc::new((Mutex::new(false), Condvar::new()));
        let job = Arc::new(Job {
            id: id.clone(),
            file: spec.file.clone(),
            core: core.clone(),
            ended: ended.clone(),
        });
        self.shared.jobs.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), job);
        let temp = self.shared.temp.clone();
        let job_of_thread = id.clone();
        let live = Live {
            job: n,
            core: core.clone(),
            notice: self.shared.notice.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        };
        host::spawn_thread(&format!("mcp-run-{id}"), move || {
            job_body(spec, job_of_thread, live, core, ended, temp)
        });
        eprintln!("mcp: run {id} started");
        (Obj::new().str("job_id", id).build(), false)
    }

    /// The job's state: the run's result once it is over, or `running`;
    /// `wait_ms` says how long to wait for the end first.
    fn tool_poll_run(&mut self, args: &Value) -> (Value, bool) {
        if let Some(why) = self.unknown_field("poll_run", args, &["job_id", "wait_ms"]) {
            return (fail_body(&why), true);
        }
        let Some(id) = args.get("job_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
            return (fail_body("poll_run needs a run's job_id ('job_id')"), true);
        };
        let wait_ms = match args.get("wait_ms") {
            None => 0u64,
            Some(v) => match v.as_u64() {
                Some(n) if n <= MAX_WAIT_MS => n,
                _ => {
                    return (
                        fail_body(&format!("poll_run's wait_ms is an integer in ms, at most {MAX_WAIT_MS}")),
                        true,
                    )
                }
            },
        };
        let Some(job) = self.locked_job(id) else {
            return (fail_body(&format!("poll_run: no job '{id}'")), true);
        };
        wait_done(&job.ended, wait_ms);
        let core = job.core.lock().unwrap_or_else(|e| e.into_inner());
        match &core.final_result {
            None => (Obj::new().str("status", "running").build(), false),
            Some(final_result) => {
                let failed = final_result.get("status").and_then(|s| s.as_str()).is_some_and(|s| s != "done");
                (final_result.clone(), failed)
            }
        }
    }

    fn locked_job(&self, id: &str) -> Option<Arc<Job>> {
        self.shared.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
    }

    /// The records of a v1 trace file, from the trace's own readers' wordings.
    fn tool_read_trace(&mut self, args: &Value) -> (Value, bool) {
        if let Some(why) = self.unknown_field("read_trace", args, &["path"]) {
            return (fail_body(&why), true);
        }
        let Some(path) = args.get("path").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
            return (fail_body("read_trace needs a trace file ('path')"), true);
        };
        match trace_records(path) {
            Ok((header, events)) => (Obj::new().set("header", header).set("events", Value::Arr(events)).build(), false),
            Err(why) => (fail_body(&why), true),
        }
    }

    // -- The resources ------------------------------------------------------

    /// The four spec resources (each read back byte for byte as it went in
    /// at build), every job's `run://{id}` live resource, and every kept
    /// trace's resource.
    fn resources(&self) -> Result<Value, Rpc> {
        let mut items = static_resources();
        let jobs = self.shared.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut ids: Vec<&String> = jobs.keys().collect();
        ids.sort();
        for id in ids {
            let job = &jobs[id];
            items.push(
                Obj::new()
                    .str("uri", format!("run://{id}"))
                    .str("name", format!("run {id}"))
                    .str("mimeType", "application/json")
                    .str("title", format!("run {id} of {}", job.file))
                    .build(),
            );
            if trace_path_of(job).is_some() {
                items.push(
                    Obj::new()
                        .str("uri", format!("trace://{id}"))
                        .str("name", format!("trace {id}"))
                        .str("mimeType", "application/json")
                        .str("title", format!("the trace of run {id}"))
                        .build(),
                );
            }
        }
        drop(jobs);
        let syncs = self.shared.sync_traces.lock().unwrap_or_else(|e| e.into_inner());
        let mut uris: Vec<&String> = syncs.keys().collect();
        uris.sort();
        for uri in uris {
            items.push(
                Obj::new()
                    .str("uri", uri.clone())
                    .str("name", uri.trim_start_matches("trace://").to_string())
                    .str("mimeType", "application/json")
                    .str("title", "the trace of a synchronous run")
                    .build(),
            );
        }
        Ok(Obj::new().set("resources", Value::Arr(items)).build())
    }

    /// The contents of one resource: every static one is the embedded text
    /// of this public repo; a running job's is the compact JSON of its
    /// state; a finished job's is its final result; a trace's is its file.
    fn resource_read(&mut self, params: Option<&Value>) -> Result<Value, Rpc> {
        let Some(uri) = params.and_then(|p| p.get("uri")).and_then(|v| v.as_str()) else {
            return Err(rpc_err(-32602, "Invalid params: resource read needs 'uri'"));
        };
        let (text, mime) = if let Some(known) = static_uris().iter().find(|u| **u == uri) {
            let text = match *known {
                "spec://SPEC.md" => SPEC_MD,
                "spec://CONFORMANCE.md" => CONFORMANCE_MD,
                "spec://TRACE.md" => TRACE_MD,
                _ => SCHEMA_JSON,
            };
            let mime = if uri.ends_with(".json") {
                "application/json"
            } else {
                "text/markdown"
            };
            (text.to_string(), mime.to_string())
        } else if uri.starts_with("trace://") {
            let path = self.trace_path_of_uri(uri)?;
            (
                std::fs::read_to_string(&path).map_err(|e| rpc_err(-32602, format!("{path}: cannot read ({e})")))?,
                "application/json".to_string(),
            )
        } else if let Some(id) = uri.strip_prefix("run://") {
            let Some(job) = self.locked_job(id) else {
                return Err(rpc_err(-32602, format!("Invalid params: unknown resource '{uri}'")));
            };
            (json::write(&job_state_body(&job)), "application/json".to_string())
        } else {
            return Err(rpc_err(-32602, format!("Invalid params: unknown resource '{uri}'")));
        };
        Ok(Obj::new()
            .set(
                "contents",
                Value::Arr(vec![Obj::new().str("uri", uri).str("mimeType", mime).str("text", text).build()]),
            )
            .build())
    }

    /// The path a trace resource reads: a job's kept trace
    /// (`trace://{job}`) or a synchronous run's (`trace://sync/{n}`).
    fn trace_path_of_uri(&self, uri: &str) -> Result<String, Rpc> {
        if let Some(rest) = uri.strip_prefix("trace://") {
            if rest.starts_with("sync/") {
                return self
                    .shared
                    .sync_traces
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(uri)
                    .cloned()
                    .ok_or_else(|| rpc_err(-32602, format!("Invalid params: unknown resource '{uri}'")));
            }
            let Some(job) = self.locked_job(rest) else {
                return Err(rpc_err(-32602, format!("Invalid params: unknown resource '{uri}'")));
            };
            return trace_path_of(&job).ok_or_else(|| rpc_err(-32602, format!("Invalid params: run '{rest}' kept no trace")));
        }
        Err(rpc_err(-32602, format!("Invalid params: unknown resource '{uri}'")))
    }

    /// Subscribe (or unsubscribe) one resource: the ones with updates of
    /// their own are the `run://` live ones; a static resource stands
    /// subscribed but no notification ever fires for it (it never changes);
    /// an unknown uri is refused.
    fn resource_subscribe(&mut self, params: Option<&Value>, add: bool) -> Result<Value, Rpc> {
        let Some(uri) = params.and_then(|p| p.get("uri")).and_then(|v| v.as_str()) else {
            return Err(rpc_err(-32602, "Invalid params: subscribe needs 'uri'"));
        };
        let known = static_uris().contains(&uri) || self.live_uri_exists(uri);
        if !known {
            return Err(rpc_err(-32602, format!("Invalid params: unknown resource '{uri}'")));
        }
        if add {
            self.shared.subs.lock().unwrap_or_else(|e| e.into_inner()).insert(uri.to_string());
            eprintln!("mcp: subscribed {uri}");
        } else {
            self.shared.subs.lock().unwrap_or_else(|e| e.into_inner()).remove(uri);
            eprintln!("mcp: unsubscribed {uri}");
        }
        Ok(Value::Obj(Vec::new()))
    }

    /// Is there a live run (or its kept trace) behind the uri? A
    /// subscription to a run the server does not carry would silently never
    /// fire, so one is refused.
    fn live_uri_exists(&self, uri: &str) -> bool {
        if let Some(id) = uri.strip_prefix("run://") {
            return self.locked_job(id).is_some();
        }
        if let Some(rest) = uri.strip_prefix("trace://") {
            if rest.starts_with("sync/") {
                return self.shared.sync_traces.lock().unwrap_or_else(|e| e.into_inner()).contains_key(uri);
            }
            return self.locked_job(rest).and_then(|j| trace_path_of(&j)).is_some();
        }
        false
    }
}

// -- The run's arguments, the results, the route -------------------------------

fn fail_body(why: &str) -> Value {
    Obj::new().str("error", why).build()
}

fn refused_body(reason: String) -> Value {
    Obj::new()
        .str("status", "refused")
        .str("reason", reason)
        .set("answers", Value::Arr(Vec::new()))
        .set("events", Value::Arr(Vec::new()))
        .build()
}

/// One network file as `describe_network` gives it: `file` (the name as the
/// caller or the declaring file wrote it, never a resolved path), the header
/// fields the file sets, `neurons` in the trace's own shape (plus `effect:
/// true` where the file declares it) and `links`. A
/// composed neuron's inner file is resolved against its declaring file's
/// directory, as the runner resolves it; a file already open further up the
/// tree is not described again but marked `cycle`, so a tree that loops back
/// on itself ends.
fn describe(path: &Path, file: &str, composed: bool, open: &mut Vec<PathBuf>) -> Result<Value, String> {
    let net = crate::net::load(&path.to_string_lossy(), composed).map_err(|errs| errs.join("; "))?;
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    open.push(real);
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let mut neurons = Vec::new();
    for n in &net.nodes {
        let mut o = trace::neuron_value(n);
        // `effect` is not in the trace's neuron shape (TRACE.md), but a client
        // drawing the network needs to know which calls are never cut and end
        // `unknown` on a crash (SPEC 11): the description adds it, the trace
        // header stays as it is.
        if n.opts.effect {
            if let Value::Obj(ref mut entries) = o {
                entries.push(("effect".to_string(), Value::Bool(true)));
            }
        }
        // `at` (decision 0011), likewise outside the trace's shape: where the
        // file says a drawing puts the neuron.
        if let (Some(at), Value::Obj(ref mut entries)) = (n.at, &mut o) {
            entries.push(("at".to_string(), Value::Arr(at.iter().map(|v| json_number(*v)).collect())));
        }
        if let crate::net::Kind::Network(rel) = &n.kind {
            let inner_path = dir.join(rel);
            let inner_real = std::fs::canonicalize(&inner_path).unwrap_or_else(|_| inner_path.clone());
            let inner = if open.contains(&inner_real) {
                Obj::new().str("file", rel.clone()).set("cycle", Value::Bool(true)).build()
            } else {
                describe(&inner_path, rel, true, open)?
            };
            if let Value::Obj(ref mut entries) = o {
                entries.push(("network".to_string(), inner));
            }
        }
        neurons.push(o);
    }
    open.pop();
    let opt_str = |v: &Option<String>| v.clone().map(Value::Str);
    Ok(Obj::new()
        .str("file", file)
        .str("name", net.name.clone())
        .str("description", net.description.clone())
        .set_if("entry", opt_str(&net.entry))
        .set_if("exit", opt_str(&net.exit))
        .set_if("budget", net.budget.as_ref().map(trace::units_value))
        .set("neurons", Value::Arr(neurons))
        .set("links", Value::Arr(net.links.iter().map(trace::link_value).collect()))
        .build())
}

/// A coordinate as JSON: a whole number as an integer, else a float.
fn json_number(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        Value::Int(v as i64)
    } else {
        Value::Float(v)
    }
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    Obj::new()
        .str("name", name)
        .str("description", description)
        .set("inputSchema", schema)
        .build()
}

fn req_keys(keys: &[&str]) -> Value {
    Value::Arr(keys.iter().map(|k| Value::Str((*k).to_string())).collect())
}

fn file_schema() -> Value {
    Obj::new()
        .str("type", "object")
        .set(
            "properties",
            Obj::new()
                .set(
                    "file",
                    Obj::new()
                        .str("type", "string")
                        .str(
                            "description",
                            "The network file, resolved as a path of the server's working directory",
                        )
                        .build(),
                )
                .build(),
        )
        .set("required", req_keys(&["file"]))
        .set("additionalProperties", Value::Bool(false))
        .build()
}

fn path_schema() -> Value {
    Obj::new()
        .str("type", "object")
        .set(
            "properties",
            Obj::new()
                .set(
                    "path",
                    Obj::new()
                        .str("type", "string")
                        .str(
                            "description",
                            "The trace file, resolved as a path of the server's working directory",
                        )
                        .build(),
                )
                .build(),
        )
        .set("required", req_keys(&["path"]))
        .set("additionalProperties", Value::Bool(false))
        .build()
}

fn poll_schema() -> Value {
    Obj::new()
        .str("type", "object")
        .set(
            "properties",
            Obj::new()
                .set(
                    "job_id",
                    Obj::new()
                        .str("type", "string")
                        .str("description", "The id start_run returned")
                        .build(),
                )
                .set(
                    "wait_ms",
                    Obj::new()
                        .str("type", "integer")
                        .int("minimum", 0)
                        .int("maximum", MAX_WAIT_MS as i64)
                        .int("default", 0)
                        .str("description", "How long to wait for the run's end")
                        .build(),
                )
                .build(),
        )
        .set("required", req_keys(&["job_id"]))
        .set("additionalProperties", Value::Bool(false))
        .build()
}

/// The closed schema of the two run tools' arguments: `file`, the inputs
/// (closed entries: `at`, `source`, `payload`, and the optional `replaces`
/// and `budget` of SPEC 4.1 — the keys the engine's own reader takes, and
/// no others), `timeout_ms` with its default and cap, `keep_trace`.
fn run_schema() -> Value {
    let input_entry = Obj::new()
        .str("type", "object")
        .set(
            "properties",
            Obj::new()
                .set(
                    "at",
                    Obj::new()
                        .str("type", "integer")
                        .int("minimum", 0)
                        .str("description", "ms from the run's time zero")
                        .build(),
                )
                .set(
                    "source",
                    Obj::new()
                        .str("type", "string")
                        .str("description", "The network's source neuron the injection names")
                        .build(),
                )
                .set(
                    "payload",
                    Obj::new().str("description", "Any JSON value: the signal's payload").build(),
                )
                .set(
                    "replaces",
                    Obj::new()
                        .str("type", "string")
                        .str("description", "A replaces key this signal carries (SPEC 5.2)")
                        .build(),
                )
                .set(
                    "budget",
                    Obj::new()
                        .str("type", "object")
                        .set("additionalProperties", Obj::new().str("type", "integer").build())
                        .str("description", "A budget object, unit to amount (SPEC 10.2)")
                        .build(),
                )
                .build(),
        )
        .set("required", req_keys(&["at", "source", "payload"]))
        .set("additionalProperties", Value::Bool(false))
        .build();
    Obj::new()
        .str("type", "object")
        .set(
            "properties",
            Obj::new()
                .set(
                    "file",
                    Obj::new()
                        .str("type", "string")
                        .str(
                            "description",
                            "The network file, resolved as a path of the server's working directory",
                        )
                        .build(),
                )
                .set(
                    "inputs",
                    Obj::new()
                        .str("type", "array")
                        .set("items", input_entry)
                        .str("description", "The producer's injections (SPEC 4.1)")
                        .build(),
                )
                .set(
                    "timeout_ms",
                    Obj::new()
                        .str("type", "integer")
                        .int("minimum", 1)
                        .int("maximum", MAX_TIMEOUT_MS as i64)
                        .int("default", DEFAULT_TIMEOUT_MS as i64)
                        .str("description", "How long the run may keep at work before it is torn down")
                        .build(),
                )
                .set(
                    "keep_trace",
                    Obj::new()
                        .str("type", "boolean")
                        .str("description", "Keep a v1 trace file of the run in the server's temp directory")
                        .build(),
                )
                .build(),
        )
        .set("required", req_keys(&["file", "inputs"]))
        .set("additionalProperties", Value::Bool(false))
        .build()
}

fn static_resources() -> Vec<Value> {
    [
        (
            "spec://SPEC.md",
            "SPEC.md",
            "text/markdown",
            "the agent-neuron SPEC.md (the text embedded at build)",
        ),
        (
            "spec://CONFORMANCE.md",
            "CONFORMANCE.md",
            "text/markdown",
            "the agent-neuron CONFORMANCE.md (the text embedded at build)",
        ),
        (
            "spec://TRACE.md",
            "TRACE.md",
            "text/markdown",
            "the agent-neuron TRACE.md (the text embedded at build)",
        ),
        (
            "spec://network.schema.json",
            "network.schema.json",
            "application/json",
            "the agent-neuron network schema (the text embedded at build)",
        ),
    ]
    .into_iter()
    .map(|(uri, name, mime, title)| {
        Obj::new()
            .str("uri", uri)
            .str("name", name)
            .str("mimeType", mime)
            .str("title", title)
            .build()
    })
    .collect()
}

fn static_uris() -> Vec<&'static str> {
    vec![
        "spec://SPEC.md",
        "spec://CONFORMANCE.md",
        "spec://TRACE.md",
        "spec://network.schema.json",
    ]
}

fn capabilities() -> Value {
    Obj::new()
        .set("tools", Obj::new().set("listChanged", Value::Bool(false)).build())
        .set(
            "resources",
            Obj::new()
                .set("subscribe", Value::Bool(true))
                .set("listChanged", Value::Bool(false))
                .build(),
        )
        .build()
}

/// One diagnostic as the check surface reports it: the file, the line and
/// column when the file carries a position, the message.
fn diag_json(d: &crate::net::Diag) -> Value {
    let (line, column) = match &d.pos {
        Some(p) => (Value::Int(p.line as i64), Value::Int(p.column as i64)),
        None => (Value::Null, Value::Null),
    };
    Obj::new()
        .set("file", d.file.clone().map(Value::Str).unwrap_or(Value::Null))
        .set("line", line)
        .set("col", column)
        .str("message", d.message.clone())
        .build()
}

/// The arguments of the two run tools: the file, the inputs read through the
/// engine's own validator (so the wordings are the engine's wordings), the
/// timeout with its default and cap, `keep_trace`. Unknown fields are
/// refused; so are missing ones, with the tool's own error wording.
fn run_args(tool: &str, args: &Value) -> Result<RunSpec, String> {
    for key in args.keys() {
        if !matches!(key, "file" | "inputs" | "timeout_ms" | "keep_trace") {
            return Err(format!(
                "{tool} takes 'file', 'inputs', 'timeout_ms', 'keep_trace'; unknown field '{key}'"
            ));
        }
    }
    let Some(file) = args.get("file").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
        return Err(format!("{tool} needs a network file ('file')"));
    };
    let raw = match args.get("inputs") {
        Some(Value::Arr(items)) => items.clone(),
        _ => return Err(format!("{tool} needs the producer's inputs ('inputs', an array)")),
    };
    let mut inputs = Vec::new();
    let mut errs = Vec::new();
    for (i, entry) in raw.iter().enumerate() {
        inputs.push(fixture::inputs_of(entry, i, &mut errs));
    }
    if !errs.is_empty() {
        return Err(errs.join("; "));
    }
    let timeout_ms = match args.get("timeout_ms") {
        None => DEFAULT_TIMEOUT_MS,
        Some(v) => match v.as_u64() {
            Some(n) if n >= 1 => n.min(MAX_TIMEOUT_MS),
            _ => {
                return Err(format!(
                    "{tool}'s timeout_ms is an integer in ms, at least 1 and at most {MAX_TIMEOUT_MS}"
                ))
            }
        },
    };
    let keep_trace = match args.get("keep_trace") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(|| format!("{tool}'s keep_trace is a boolean"))?,
    };
    Ok(RunSpec {
        file: file.to_string(),
        inputs,
        timeout_ms,
        keep_trace,
    })
}

/// The engine of an MCP route: the real engine, with no desk (a network with
/// `human` neurons is refused at plant, 3.4) and with the caller's deadline
/// (`timeout_ms`).
fn route_engine(spec: &RunSpec) -> Engine {
    let mut eng = Engine::new_real(spec.inputs.clone(), 10_000);
    eng.set_no_desk();
    eng.set_deadline_ms(spec.timeout_ms);
    eng
}

/// One run through the engine's real route — the shape of run.rs's
/// `run_with_stream`, with the MCP route's difference (no desk, a hard
/// deadline) carried in `eng`. The result is a body in the run result's own
/// shape, plus the trace path when `keep_trace` kept one.
fn run_route(spec: &RunSpec, live: Option<Live>, eng: &mut Engine, trace_out: PathBuf) -> Executed {
    if let Some(l) = live {
        let core = l.core.clone();
        let notice = l.notice.clone();
        let job = l.job;
        eng.set_stream(Box::new(move |ev| {
            let pushed = core.lock().map(|mut c| {
                c.events.push(ev.clone());
            });
            if pushed.is_ok() {
                let _ = notice.send(Notice { job, terminal: false });
            }
        }));
    }
    let path = Path::new(&spec.file);
    // The declaring file's directory is the default working directory of its
    // neurons (SPEC 2.4); a bare name resolves against the server's working
    // directory first, as the run command resolves a bare name.
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(e) => {
                return Executed {
                    result: refused_body(format!("cannot run: the working directory is unreadable ({e})")),
                    trace_path: None,
                    is_error: true,
                }
            }
        }
    };
    let (outer, files) = match runner::load_network_tree(&absolute) {
        Ok(tree) => tree,
        Err(diags) => {
            return Executed {
                result: refused_body(format!("cannot run: {}", diags.join("; "))),
                trace_path: None,
                is_error: true,
            }
        }
    };
    let base = absolute.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    if let Err(e) = eng.plant(&outer, &files, Some(&base)) {
        return Executed {
            result: refused_body(format!("cannot run: {e}")),
            trace_path: None,
            is_error: true,
        };
    }
    if let Some(why) = eng.start_serves_err() {
        eng.stop_after_refusal();
        return Executed {
            result: refused_body(format!("the engine refused the run: {why}")),
            trace_path: None,
            is_error: true,
        };
    }
    let outcome = eng.run();
    if let Some(why) = &outcome.refused {
        return Executed {
            result: refused_body(format!("the engine refused the run: {why}")),
            trace_path: None,
            is_error: true,
        };
    }
    let answers = outcome.printed_answers();
    let timed_out = outcome.timed_out;
    let events = outcome.events;
    let (shown, total_events) = shown_events(&events);
    let mut entries: Vec<(String, Value)> = Vec::new();
    entries.push(("status".to_string(), Value::Str(if timed_out { "failed" } else { "done" }.into())));
    if timed_out {
        entries.push(("reason".to_string(), Value::Str(format!("timed out at {} ms", spec.timeout_ms))));
    }
    entries.push(("summary".to_string(), demo_summary(&outer, &spec.file, total_events, answers.len())));
    entries.push(("answers".to_string(), Value::Arr(answers.into_iter().map(Value::Str).collect())));
    entries.push(("events".to_string(), shown));
    let mut executed = Executed {
        result: Value::Obj(entries),
        trace_path: None,
        is_error: timed_out,
    };
    if spec.keep_trace && !events.is_empty() {
        // The trace, with the P2 gate's self-check (the events read back are
        // the run's), as the run command writes its trace with. A trace the
        // check refuses is the tool's error: a trace was promised and none
        // was delivered.
        let out = trace_out.to_string_lossy().to_string();
        match run::write_trace_checked(&out, &events, &outer, None) {
            Ok(()) => {
                if let Value::Obj(list) = &mut executed.result {
                    list.push(("trace_path".to_string(), Value::Str(out.clone())));
                }
                executed.trace_path = Some(out);
            }
            Err(why) => {
                if let Value::Obj(list) = &mut executed.result {
                    list.push(("trace_error".to_string(), Value::Str(why)));
                }
                executed.is_error = true;
            }
        }
    }
    executed
}

fn demo_summary(outer: &Network, file: &str, events: usize, answers: usize) -> Value {
    let demo = if outer.name.is_empty() {
        Path::new(file)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        outer.name.clone()
    };
    Obj::new()
        .str("demo", demo)
        .int("events", events as i64)
        .int("answers", answers as i64)
        .build()
}

/// The events of a result body: whole at the cap, else the first 299 plus
/// one marker carrying the run's whole count (the summary says it too).
fn shown_events(events: &[Value]) -> (Value, usize) {
    if events.len() <= EVENTS_WHOLE {
        return (Value::Arr(events.to_vec()), events.len());
    }
    let mut shown: Vec<Value> = events[..EVENTS_SHOWN].to_vec();
    shown.push(Obj::new().int("truncated", events.len() as i64).build());
    (Value::Arr(shown), events.len())
}

/// A job: one run in the job's own thread, its state shared so the resource
/// and `poll_run` read it.
fn job_body(spec: RunSpec, id: String, live: Live, core: Arc<Mutex<JobCore>>, ended: Arc<(Mutex<bool>, Condvar)>, temp: PathBuf) {
    let n = id.parse::<u64>().unwrap_or(0);
    let _ = live.notice.send(Notice { job: n, terminal: false });
    let terminal = live.notice.clone();
    let trace_out = temp.join(format!("trace-{id}.json"));
    let mut eng = route_engine(&spec);
    // A harness failure (a call the engine cannot match, say) unwinds the
    // engine: caught here — its processes stopped, its desk closed — and the
    // job reports the failure instead of the server's thread ending in
    // silence.
    let executed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_route(&spec, Some(live), &mut eng, trace_out)))
        .unwrap_or_else(|_| {
            eng.kill_leftovers();
            Executed {
                result: refused_body("the engine's run ended in a harness failure; its note is on the server's stderr".to_string()),
                trace_path: None,
                is_error: true,
            }
        });
    let said = executed.result.get("status").and_then(|s| s.as_str()).unwrap_or("?").to_string();
    if let Ok(mut c) = core.lock() {
        c.final_result = Some(executed.result.clone());
    }
    // The terminal change goes out against the rate limit: the one change a
    // subscriber must not miss.
    let _ = terminal.send(Notice { job: n, terminal: true });
    if let Ok(mut e) = ended.0.lock() {
        *e = true;
        ended.1.notify_all();
    }
    eprintln!("mcp: run {id} ended: {said}");
    // The engine drops here; its own end already stopped every process it
    // started (the run's teardown), and a torn-down engine's groups are dead.
}

/// The body of a job's `run://{id}` resource: the running state while the
/// run goes, the run's final result with the job id in front once it ended.
fn job_state_body(job: &Job) -> Value {
    let core = job.core.lock().unwrap_or_else(|e| e.into_inner());
    match &core.final_result {
        None => Obj::new()
            .str("job_id", job.id.clone())
            .str("status", "running")
            .set("events_so_far", Value::Arr(core.events.clone()))
            .set("answers_so_far", Value::Arr(Vec::new()))
            .build(),
        Some(final_result) => {
            let mut entries = vec![("job_id".to_string(), Value::Str(job.id.clone()))];
            if let Value::Obj(rest) = final_result {
                entries.extend(rest.clone());
            }
            Value::Obj(entries)
        }
    }
}

/// The path of the trace a job kept, from its final result (a job that kept
/// none has none to read).
fn trace_path_of(job: &Job) -> Option<String> {
    let core = job.core.lock().unwrap_or_else(|e| e.into_inner());
    core.final_result
        .as_ref()
        .and_then(|r| r.get("trace_path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Wait up to `ms` on a job's end.
fn wait_done(ended: &(Mutex<bool>, Condvar), ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    let (mx, cv) = ended;
    let mut guard = mx.lock().unwrap_or_else(|e| e.into_inner());
    while !*guard {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let (next, timeout) = cv.wait_timeout(guard, deadline - now).unwrap_or_else(|e| e.into_inner());
        guard = next;
        if timeout.timed_out() {
            return;
        }
    }
}

/// A trace file's records: `format` and `version` checked first (a v1
/// agent-neuron trace), then the events; the header as the file holds it.
fn trace_records(path: &str) -> Result<(Value, Vec<Value>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: cannot read ({e})"))?;
    let v = json::parse(&text).map_err(|e| format!("{path}: not JSON ({e})"))?;
    let not_a_trace = || {
        format!(
            "{path}: not a v1 agent-neuron trace (a v1 trace has 'format' = \"{}\" and 'version' = {})",
            trace::FORMAT,
            trace::VERSION
        )
    };
    if !v.is_obj() {
        return Err(not_a_trace());
    }
    let format_ok = v.get("format").and_then(|f| f.as_str()).is_some_and(|f| f == trace::FORMAT);
    let version_ok = v.get("version").and_then(|f| f.as_i64()).is_some_and(|f| f == trace::VERSION);
    if !format_ok || !version_ok {
        return Err(not_a_trace());
    }
    let events = match v.get("events") {
        Some(Value::Arr(items)) => items.clone(),
        _ => return Err(format!("{path}: the trace has no events")),
    };
    let header = match &v {
        Value::Obj(entries) => Value::Obj(entries.iter().filter(|(k, _)| k != "events").cloned().collect()),
        _ => unreachable!("checked above"),
    };
    Ok((header, events))
}

#[cfg(test)]
mod probe {
    use super::*;

    #[test]
    fn the_refusals_carry_the_trace_readers_own_wordings() {
        let why = trace_records("/no/such/file.json").unwrap_err();
        assert!(why.starts_with("/no/such/file.json: cannot read"), "{why}");
        assert_eq!(trace::VERSION, 1);
        assert_eq!(trace::FORMAT, "agent-neuron-trace");
    }
}
