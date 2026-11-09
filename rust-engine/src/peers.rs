// The peers (decision 0013, docs/P2P.md P0): a run's peers file names the
// machines that execute some of its neurons, and this module carries one level
// P0 is the seam, not the protocol: the call is executed
// on the peer by the same `host` code a local call uses, and its outcome comes
// back as one `Ev::L0Done`, stamped by the engine when it reads the queue. The
// engine's decisions, its clock, its trace and its budgets stay where they
// were; only where a program runs moves.
//
// `peer --network <file>` is the other mode: the peer is *one neuron* whose
// inside is a whole network file. A call's payload is injected at the file's
// `entry` and the file's `exit` answers, so an outer network can place a
// `network` neuron on a peer and compose with it exactly as with a composed
// neuron run locally. The file is instance 0 of the per-call engine, so its own
// `--peers` placement works with plain neuron ids: a graph hosted by a peer can
// itself be spread over peers, and from outside it is still one neuron.
//
// The wire is newline-delimited JSON over TCP, `v` 1, one call at a time per
// connection: a `hello` (the peer's name and the run's token), then `call` /
// `done` pairs. The outcome is classified on the peer — one tagged object per
// `host::L0Out` variant — so the engine applies the same rules to it as to a
// local program's, down to the stderr head a failure carries.
//
// A std-only crate has no TLS: the token is a shared secret compared in constant
// time, and the wire is for a network the deployer already trusts (docs/P2P.md,
// security posture; a MAC per line is P1). A peer executes exactly what its
// driver's call names — that is what being a worker means — so the token gates
// a trusted network, not untrusted code. Cancellation is the socket's shutdown,
// as the http client's is; a peer that stops answering fails its call at the
// call's timeout, never retried. P0 carries `run` neurons only, and the worker
// holds a call's connection until its own backstop: the `cancel` message is P1.
use crate::engine::Engine;
use crate::host::{self, L0Out};
use crate::json::{self, Obj, Value};
use crate::net::{Kind, Link, NetFile, Network, Node};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A wire line's cap: a classified outcome can carry a whole 1 MiB answer plus
/// its JSON; 8 MiB is far past anything 3.1 permits, and a line over it fails
/// the call the way an over-long output does.
pub const WIRE_CAP: usize = 8 * 1024 * 1024;
/// A `hello`'s cap: a name and a token.
const HELLO_CAP: usize = 8 * 1024;
/// How long a peer gets to open and answer the handshake.
const CONNECT_MS: u64 = 5_000;
/// The window a cancel line has to be delivered before the socket is aborted
/// (finding 9, 0014): wide enough for a reading peer, short enough that a
/// stuck one does not hold the run.
const CANCEL_WRITE_MS: u64 = 500;
/// The worker's backstop past a call's own timeout: the engine's timeout owns
/// the call, and a worker that kept a program alive past it would leave work
/// nobody can report. P1's `cancel` makes this backstop rarely reached.
const BACKSTOP_MS: u64 = 5_000;

/// One peer: a name the peers file uses, where it listens, and the run's shared
/// secret.
#[derive(Debug, Clone)]
pub struct Peer {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub token: String,
}

/// The peers file, loaded: the machines, who runs which neuron, and this run's
/// own name on the wire. `live` is the driver side's table of calls in flight,
/// by the engine's level 0 index, so a cut can reach them.
pub struct Peers {
    /// This run's own name on the wire (`self` in the file, default `driver`).
    pub local: String,
    pub list: Vec<Peer>,
    /// Neuron id -> index into `list`.
    pub placement: HashMap<String, usize>,
    live: HashMap<usize, Call>,
}

/// One call in flight: the socket a cancel shuts down, and whether the thread
/// already reported (a late cancel must not be counted as a kill).
struct Call {
    sock: Arc<Mutex<Option<TcpStream>>>,
    cancelled: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

/// What one dispatch carries: the level 0 call as the local host would run it.
pub struct CallReq {
    /// The neuron's id, for the peer's own logs.
    pub node: String,
    pub argv: Vec<String>,
    pub cwd: Option<std::path::PathBuf>,
    pub env: Vec<String>,
    /// The one JSON value the engine writes to a program's stdin (or the
    /// payload a hosted graph is injected with).
    pub input: String,
    pub timeout: u64,
    /// What the driver expects the peer to be: `false` a program worker,
    /// `true` a peer that is one neuron. The handshake must agree.
    pub graph: bool,
}

/// A peer that is one neuron (decision 0013): a network file run per call,
/// injected at its `entry` and answered by its `exit`. The file is instance 0 of
/// the per-call engine, so its own `--peers` placement works with plain neuron
/// ids — a graph hosted by a peer can itself be spread over peers, and from
/// outside it is still one neuron.
pub struct Graph {
    net: Network,
    files: HashMap<String, NetFile>,
    base: PathBuf,
    entry: String,
    exit: Option<String>,
    /// The synthetic source a call's payload is injected at (the file itself
    /// has no source: 2.6).
    source: String,
    /// The graph's own peers file, when the worker was given one.
    peers: Option<String>,
}

impl Graph {
    /// Load the file a worker hosts. It must be composed-shaped: an `entry` to
    /// inject at, no `source` (the input comes from the call), no `ground` (2.6).
    pub fn open(file: &str, peers: Option<&str>) -> Result<Graph, String> {
        let path = std::path::Path::new(file);
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("{}: the working directory is unreadable ({})", file, e))?
                .join(path)
        };
        let (net, files) = crate::runner::load_network_tree(&abs).map_err(|e| format!("{}: {}", file, e.join("; ")))?;
        let Some(entry) = net.entry.clone() else {
            return Err(format!("{}: has no `entry`, so a call has nowhere to enter", file));
        };
        if net.nodes.iter().any(|n| matches!(n.kind, Kind::Source)) {
            return Err(format!(
                "{}: has a `source`; a network hosted as one neuron takes its input from the call",
                file
            ));
        }
        if net.ground.is_some() {
            return Err(format!("{}: declares a `ground`, which a composed file may not (2.6)", file));
        }
        let mut source = "__in".to_string();
        while net.node(&source).is_some() {
            source.push('_');
        }
        let base = abs.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        Ok(Graph {
            exit: net.exit.clone(),
            net,
            files,
            base,
            entry,
            source,
            peers: peers.map(|s| s.to_string()),
        })
    }
}

impl Peers {
    /// Load and validate a peers file: `{version: 0, self?, peers: [{name,
    /// address, token}], placement: {neuron id: peer name}}`, closed. Errors
    /// name the file, as `run` reports them.
    pub fn load(path: &str) -> Result<Peers, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: cannot read ({})", path, e))?;
        let v = json::parse(&text).map_err(|e| format!("{}: is not JSON ({})", path, e))?;
        let bad = |why: String| format!("{}: {}", path, why);
        if !v.is_obj() {
            return Err(bad("is not an object".into()));
        }
        for k in v.keys() {
            if !matches!(k, "version" | "self" | "peers" | "placement") {
                return Err(bad(format!("unknown field '{}'", k)));
            }
        }
        let Some(version) = v.get("version").and_then(Value::as_u64) else {
            return Err(bad("has no `version`".into()));
        };
        if version != 0 {
            return Err(bad(format!("has version {}, and this engine implements 0", version)));
        }
        let local = match v.get("self") {
            None => "driver".to_string(),
            Some(Value::Str(s)) if id_ok(s) => s.clone(),
            Some(_) => return Err(bad("`self` is not a name ([A-Za-z][A-Za-z0-9_-]{0,63})".into())),
        };
        let Some(list) = v.get("peers").and_then(Value::as_arr) else {
            return Err(bad("has no `peers` array".into()));
        };
        if list.is_empty() {
            return Err(bad("names no peer".into()));
        }
        let mut peers: Vec<Peer> = Vec::new();
        for (i, p) in list.iter().enumerate() {
            if !p.is_obj() {
                return Err(bad(format!("peers[{}] is not an object", i)));
            }
            for k in p.keys() {
                if !matches!(k, "name" | "address" | "token") {
                    return Err(bad(format!("peers[{}]: unknown field '{}'", i, k)));
                }
            }
            let name = p
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| bad(format!("peers[{}] has no `name`", i)))?
                .to_string();
            if !id_ok(&name) {
                return Err(bad(format!("peers[{}]: `name` is not a name ([A-Za-z][A-Za-z0-9_-]{{0,63}})", i)));
            }
            if peers.iter().any(|q| q.name == name) {
                return Err(bad(format!("peers[{}]: the name '{}' appears twice", i, name)));
            }
            let address = p
                .get("address")
                .and_then(Value::as_str)
                .ok_or_else(|| bad(format!("peers[{}] has no `address`", i)))?;
            let (host, port) = parse_address(address).map_err(|e| bad(format!("peers[{}]: {}", i, e)))?;
            let token = p
                .get("token")
                .and_then(Value::as_str)
                .ok_or_else(|| bad(format!("peers[{}] has no `token`", i)))?;
            if !token_ok(token) {
                return Err(bad(format!("peers[{}]: `token` is not 16 to 128 hex characters", i)));
            }
            peers.push(Peer {
                name,
                host,
                port,
                token: token.to_string(),
            });
        }
        let mut placement: HashMap<String, usize> = HashMap::new();
        let Some(table) = v.get("placement") else {
            return Err(bad("has no `placement`".into()));
        };
        let Value::Obj(pairs) = table else {
            return Err(bad("`placement` is not an object".into()));
        };
        for (node, who) in pairs.iter() {
            let who = who
                .as_str()
                .ok_or_else(|| bad(format!("placement.'{}' is not a peer name", node)))?;
            let Some(idx) = peers.iter().position(|p| p.name == who) else {
                return Err(bad(format!("placement.'{}' names '{}', which is not a peer", node, who)));
            };
            placement.insert(node.to_string(), idx);
        }
        Ok(Peers {
            local,
            list: peers,
            placement,
            live: HashMap::new(),
        })
    }

    /// Dispatch one level 0 call to peer `peer`: connect, handshake, send the
    /// call, read the outcome, post it as `Ev::L0Done`. A connect or handshake
    /// failure is the call failing to start (`exit 127`, Appendix C's rule).
    pub fn start(&mut self, post: host::Post, l0: usize, peer: usize, req: CallReq) -> Result<(), String> {
        let p = self.list.get(peer).ok_or("the placement names no peer")?.clone();
        let call = Call {
            sock: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicBool::new(false)),
        };
        let handle = Call {
            sock: call.sock.clone(),
            cancelled: call.cancelled.clone(),
            done: call.done.clone(),
        };
        let local = self.local.clone();
        self.live.insert(l0, call);
        host::spawn_thread("peer-call", move || {
            let (out, answered, started) = dispatch(&p, &local, &req, &handle);
            if handle.cancelled.load(Ordering::SeqCst) && !answered {
                // The socket went away under us: nothing to report, and the
                // grace owns the call's end (6.2 rule 6).
                return;
            }
            handle.done.store(true, Ordering::SeqCst);
            if let Some(s) = handle.sock.lock().ok().and_then(|mut g| g.take()) {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            post.post(host::Ev::L0Done { l0, out, started });
        });
        Ok(())
    }

    /// Cut a call (5.4): a `cancel` line asks the peer to stop the program and
    /// answer, and `true` says the call had not reported yet — the engine owes
    /// it the kill grace in case the peer never answers. The write never
    /// happens on the decision thread (finding 9, 0014): a peer that took its
    /// handshake and stopped reading holds the socket's write for the whole
    /// timeout, which would stall every other decision the run owes. A writer
    /// of its own carries the line, with a short window to deliver it in; a
    /// socket that cannot take the line promptly is aborted, and the grace
    /// owns the call's end either way.
    pub fn cancel(&mut self, l0: usize) -> bool {
        let Some(call) = self.live.get(&l0) else { return false };
        call.cancelled.store(true, Ordering::SeqCst);
        let pending = !call.done.load(Ordering::SeqCst);
        let sock = call.sock.clone();
        let line = Obj::new().str("type", "cancel").int("id", 1).build();
        host::spawn_thread("peer-cancel", move || {
            // Clone under the lock, write with no lock held: the dispatch
            // thread takes the lock too (to shut its socket down at the end),
            // and a hold across a stuck write would block it as well.
            let clone = sock.lock().ok().and_then(|slot| slot.as_ref().and_then(|s| s.try_clone().ok()));
            let Some(mut c) = clone else {
                // No socket to write on: nothing can be delivered, so the
                // socket is gone either way.
                if let Ok(mut slot) = sock.lock() {
                    if let Some(s) = slot.take() {
                        let _ = s.shutdown(std::net::Shutdown::Both);
                    }
                }
                return;
            };
            let _ = c.set_write_timeout(Some(Duration::from_millis(CANCEL_WRITE_MS)));
            if write_line(&mut c, &line).is_err() {
                // The line cannot be delivered promptly (a stuck peer): the
                // socket is aborted, and the kill grace owns the call's end.
                if let Ok(mut slot) = sock.lock() {
                    if let Some(s) = slot.take() {
                        let _ = s.shutdown(std::net::Shutdown::Both);
                    }
                }
            }
        });
        pending
    }

    /// The kill grace's end: the socket is gone and the entry dropped. `true`
    /// when the call had not reported, so a `kill` note is owed (6.3).
    pub fn abort(&mut self, l0: usize) -> bool {
        let Some(call) = self.live.remove(&l0) else { return false };
        call.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = call.sock.lock() {
            if let Some(s) = slot.take() {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
        !call.done.load(Ordering::SeqCst)
    }

    /// The call's outcome arrived: its entry is no longer needed (the thread
    /// that reported has already shut its socket down). Called for every
    /// remote call, so a long run does not hold one entry per finished call.
    pub fn forget(&mut self, l0: usize) {
        self.live.remove(&l0);
    }

    /// Every call still in flight stops now (a run that ends, politely or not).
    pub fn abort_all(&mut self) {
        let all: Vec<usize> = self.live.keys().copied().collect();
        for l0 in all {
            self.abort(l0);
        }
    }
}

/// The driver's thread body: one connection, one call. The returned `started`
/// says whether the call could have begun on the peer: nothing that fails
/// before the call line went out (connect, handshake, a refused send) started
/// the effect, and everything that fails after it — the wire dying, a timed
/// read, a malformed reply — is genuinely ambiguous. The peer's own `done`
/// message narrows that: a program call whose spawn was refused carries
/// `started: false` though the call was written (finding 8, 0014).
fn dispatch(p: &Peer, local: &str, req: &CallReq, call: &Call) -> (L0Out, bool, bool) {
    // `false` says the peer never answered: the call failed to start, or the
    // wire broke — not a program's outcome. `began` turns true when the call
    // line has gone out: from there the outcome is ambiguous, not unstarted.
    let began = std::cell::Cell::new(false);
    let fail = |why: String| {
        (
            L0Out::Exit {
                code: 127,
                stderr: format!("peer {}: {}", p.name, why),
            },
            false,
            began.get(),
        )
    };
    let Some(mut stream) = connect(p) else {
        return fail(format!("cannot reach {}:{}", p.host, p.port));
    };
    let watcher = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => return fail(format!("cannot use the connection ({})", e)),
    };
    if let Ok(mut slot) = call.sock.lock() {
        *slot = Some(watcher);
    }
    let read_timeout = req.timeout.saturating_add(BACKSTOP_MS).saturating_add(CONNECT_MS);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(read_timeout)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(CONNECT_MS)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(e) => return fail(format!("cannot use the connection ({})", e)),
    });
    let hello = Obj::new()
        .str("type", "hello")
        .int("v", 1)
        .str("peer", local.to_string())
        .str("token", p.token.clone())
        .build();
    if let Err(e) = write_line(&mut stream, &hello) {
        return fail(e);
    }
    let reply = match read_line(&mut reader, HELLO_CAP) {
        Ok(text) => match json::parse(&text) {
            Ok(v) => v,
            Err(e) => return fail(format!("the handshake is not JSON ({})", e)),
        },
        Err(e) => return fail(e),
    };
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        let why = reply.get("error").and_then(Value::as_str).unwrap_or("the handshake was refused");
        return fail(why.to_string());
    }
    // A peer that hosts a graph and one that runs programs speak the same wire
    // but not the same call: say which is which at the handshake, not by a
    // failure the graph would have to report as its own.
    let mode = reply.get("mode").and_then(Value::as_str).unwrap_or("program");
    let want = if req.graph { "graph" } else { "program" };
    if mode != want {
        return fail(format!(
            "peer {} speaks as a {} peer, and this call needs a {} peer",
            p.name, mode, want
        ));
    }
    // The call goes out only if no cancel for it is already in: `cancel` marks
    // the call before it writes the cancel line, and the worker refuses a call
    // whose id its cancels already hold either way (0014, finding 1).
    if call.cancelled.load(Ordering::SeqCst) {
        return fail("the call was cancelled before it was sent".into());
    }
    let call_line = Obj::new()
        .str("type", "call")
        .int("id", 1)
        .str("node", req.node.clone())
        .set("argv", Value::Arr(req.argv.iter().map(|a| Value::Str(a.clone())).collect()))
        .set_if("cwd", req.cwd.as_ref().map(|c| Value::Str(c.to_string_lossy().to_string())))
        .set("env", Value::Arr(req.env.iter().map(|a| Value::Str(a.clone())).collect()))
        .str("input", req.input.clone())
        .int("timeout", req.timeout as i64)
        .build();
    if let Err(e) = write_line(&mut stream, &call_line) {
        return fail(e);
    }
    began.set(true);
    let done = match read_line(&mut reader, WIRE_CAP) {
        Ok(text) => match json::parse(&text) {
            Ok(v) => v,
            Err(e) => return fail(format!("the outcome is not JSON ({})", e)),
        },
        Err(e) => return fail(e),
    };
    // A `started` of false says the peer took the call and never ran it (its
    // own spawn was refused, or its cancel was already in); a peer of this
    // wire always says so, so a missing field keeps the call's ambiguity.
    let peer_started = done.get("started").and_then(Value::as_bool).unwrap_or(true);
    match done.get("out") {
        Some(out) => match outcome_of_json(out) {
            Ok(o) => (o, true, began.get() && peer_started),
            Err(why) => fail(format!("the outcome is malformed ({})", why)),
        },
        None => fail("the outcome carries no `out`".into()),
    }
}

fn connect(p: &Peer) -> Option<TcpStream> {
    let addr = format!("{}:{}", p.host, p.port);
    let addrs: Vec<_> = addr.to_socket_addrs().ok()?.collect();
    for a in addrs {
        if let Ok(s) = TcpStream::connect_timeout(&a, Duration::from_millis(CONNECT_MS)) {
            return Some(s);
        }
    }
    None
}

/// The peer side: `neuron-engine peer --listen <host:port> --token <hex>
/// [--name <name>]`. One connection at a time; each connection carries one
/// handshake and then calls until the driver closes it. The listening line on
/// stderr is the readiness signal a host script waits for.
pub fn main(args: &[String]) {
    let mut listen: Option<String> = None;
    let mut token: Option<String> = None;
    let mut name = "peer".to_string();
    let mut network: Option<String> = None;
    let mut peers: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if i + 1 >= args.len() {
            usage();
        }
        match args[i].as_str() {
            "--listen" => listen = Some(args[i + 1].clone()),
            "--token" => token = Some(args[i + 1].clone()),
            "--name" => name = args[i + 1].clone(),
            "--network" => network = Some(args[i + 1].clone()),
            "--peers" => peers = Some(args[i + 1].clone()),
            _ => usage(),
        }
        i += 2;
    }
    let (Some(listen), Some(token)) = (listen, token) else { usage() };
    if !token_ok(&token) || !id_ok(&name) {
        usage();
    }
    // A peer without `--network` runs programs; with it, the peer *is* one
    // neuron whose inside is a graph.
    let graph = match &network {
        None => None,
        Some(file) => match Graph::open(file, peers.as_deref()) {
            Ok(g) => Some(Arc::new(g)),
            Err(why) => {
                eprintln!("peer {}: cannot host the network: {}", name, why);
                std::process::exit(1);
            }
        },
    };
    host::install_signal_teardown();
    let listener = match TcpListener::bind(&listen) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("peer {}: cannot listen on {} ({})", name, listen, e);
            std::process::exit(1);
        }
    };
    let bound = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| listen.clone());
    let mode = if graph.is_some() { "graph" } else { "program" };
    eprintln!("peer {}: listening on {} ({})", name, bound, mode);
    for stream in listener.incoming() {
        match stream {
            // One thread per connection, as the desk's page has: a driver that
            // went quiet must not keep the next one waiting to be served.
            Ok(s) => {
                let token = token.clone();
                let name = name.clone();
                let graph = graph.clone();
                host::spawn_thread("peer-conn", move || serve(s, &token, &name, graph));
            }
            // A probe that connected and left is not an error worth ending the
            // peer's service for (a test's readiness poll, a port scan).
            Err(_) => continue,
        }
    }
}

fn usage() -> ! {
    eprintln!("usage: neuron-engine peer --listen <host:port> --token <hex> [--name <name>]");
    eprintln!("       neuron-engine peer --listen <host:port> --token <hex> --network <file> [--peers <json>]");
    std::process::exit(2);
}

/// One connection: the handshake, then calls until the driver closes it.
fn serve(stream: TcpStream, token: &str, name: &str, graph: Option<Arc<Graph>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(CONNECT_MS)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(CONNECT_MS)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut writer = stream;
    let Ok(line) = read_line(&mut reader, HELLO_CAP) else { return };
    let Ok(hello) = json::parse(&line) else {
        refuse(&mut writer, "the hello is not JSON");
        return;
    };
    if hello.get("type").and_then(Value::as_str) != Some("hello") {
        refuse(&mut writer, "the first line is not a hello");
        return;
    }
    if hello.get("v").and_then(Value::as_u64) != Some(1) {
        refuse(&mut writer, "this peer speaks wire version 1");
        return;
    }
    let got = hello.get("token").and_then(Value::as_str).unwrap_or("");
    if !token_eq(got, token) {
        refuse(&mut writer, "the token does not match");
        return;
    }
    let ok = Obj::new()
        .str("type", "hello")
        .int("v", 1)
        .str("peer", name.to_string())
        .str("mode", if graph.is_some() { "graph" } else { "program" })
        .set("ok", Value::Bool(true))
        .build();
    if write_line(&mut writer, &ok).is_err() {
        return;
    }
    // A call runs for its own timeout; the socket's read timeout is the
    // handshake's and must not cut a long call short.
    let _ = reader.get_ref().set_read_timeout(None);
    // A reader of its own, so a `cancel` line is seen while a call runs: the
    // running program is stopped and answered, instead of the driver waiting
    // out its kill grace (P1). A cancel is kept by the call's id and never
    // cleared: it is a decision about a call, not a phase of one, so a cancel
    // that arrives before (or with) its call still reaches it (finding 1,
    // 0014). The wire dying is the one unconditional stop, as the socket
    // shutdown's fallback has always been.
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let cancels = Arc::new(Mutex::new(HashSet::new()));
    let dead = Arc::new(AtomicBool::new(false));
    let reader_cancels = cancels.clone();
    let reader_dead = dead.clone();
    host::spawn_thread("peer-read", move || {
        let mut reader = reader;
        loop {
            match read_line(&mut reader, WIRE_CAP) {
                Ok(line) => match json::parse(&line) {
                    Ok(msg) if msg.get("type").and_then(Value::as_str) == Some("call") => {
                        if tx.send(Msg::Call(msg)).is_err() {
                            break;
                        }
                    }
                    Ok(msg) if msg.get("type").and_then(Value::as_str) == Some("cancel") => {
                        if let Ok(mut ids) = reader_cancels.lock() {
                            ids.insert(wire_call_id(&msg));
                        }
                        let _ = tx.send(Msg::Cancel);
                    }
                    _ => {
                        reader_dead.store(true, Ordering::SeqCst);
                        let _ = tx.send(Msg::Eof);
                        break;
                    }
                },
                Err(_) => {
                    reader_dead.store(true, Ordering::SeqCst);
                    let _ = tx.send(Msg::Eof);
                    break;
                }
            }
        }
    });
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Call(call) => {
                let id = wire_call_id(&call);
                // A call whose cancel is already in does not run: the check
                // and the reader's insert share the set's lock, so a cancel
                // recorded before this instant cannot be missed (0014, finding 1).
                if wire_cancelled(&cancels, &dead, id) {
                    // The call never ran: the outcome says so, and the driver
                    // reports an error for it, never an unknown (finding 8,
                    // 0014).
                    let reply = Obj::new()
                        .str("type", "done")
                        .int("id", id)
                        .set("out", cancelled_outcome(name))
                        .set("started", Value::Bool(false))
                        .build();
                    if write_line(&mut writer, &reply).is_err() {
                        return;
                    }
                    continue;
                }
                let done = match &graph {
                    Some(g) => execute_graph(g, &call, name, &cancels, &dead),
                    None => execute_program(&call, name, &cancels, &dead),
                };
                if write_line(&mut writer, &done).is_err() {
                    return;
                }
            }
            Msg::Cancel => continue,
            Msg::Eof => return,
        }
    }
}

/// What the connection's reader hands the serving thread.
enum Msg {
    Call(Value),
    Cancel,
    Eof,
}

/// A call's id on the wire (the driver sends one on every call and cancel).
fn wire_call_id(v: &Value) -> i64 {
    v.get("id").and_then(Value::as_i64).unwrap_or(0)
}

/// Whether this call's cancel is already in, or the wire is gone. The set is
/// the reader's and the executors' only shared state, so a cancel is recorded
/// or read in one order: the final check before a spawn cannot miss a cancel
/// that arrived before it (0014, finding 1).
fn wire_cancelled(ids: &Mutex<HashSet<i64>>, dead: &AtomicBool, id: i64) -> bool {
    if dead.load(Ordering::SeqCst) {
        return true;
    }
    ids.lock().map(|ids| ids.contains(&id)).unwrap_or(false)
}

/// The outcome of a call refused because its cancel is already in: the same
/// classification a cancelled run reports (the driver's cut machinery owns the
/// call's end; the reply only releases the connection).
fn cancelled_outcome(name: &str) -> Value {
    outcome_json(&L0Out::Signal {
        num: 9,
        stderr: format!("peer {}: the driver cancelled the call", name),
    })
}

/// Run one program call here, in this host's code: the same spawn, the same
/// 1 MiB cap, the same settle and the same classification a local call gets.
fn execute_program(call: &Value, name: &str, cancels: &Mutex<HashSet<i64>>, dead: &AtomicBool) -> Value {
    let id = wire_call_id(call);
    // `started` says whether the program was ever spawned: a call refused for
    // its argv, for a cancel already in, or by the spawn itself never ran,
    // and the driver reports an error for it, never an unknown (finding 8,
    // 0014).
    let reply = |out: Value, started: bool| {
        Obj::new()
            .str("type", "done")
            .int("id", id)
            .set("out", out)
            .set("started", Value::Bool(started))
            .build()
    };
    let argv: Vec<String> = call
        .get("argv")
        .and_then(Value::as_arr)
        .map(|a| a.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    if argv.is_empty() {
        return reply(
            outcome_json(&L0Out::Exit {
                code: 127,
                stderr: format!("peer {}: the call names no program", name),
            }),
            false,
        );
    }
    let env: Vec<String> = call
        .get("env")
        .and_then(Value::as_arr)
        .map(|a| a.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let cwd = call.get("cwd").and_then(Value::as_str).map(std::path::PathBuf::from);
    let input = call.get("input").and_then(Value::as_str).unwrap_or("");
    let timeout = call.get("timeout").and_then(Value::as_u64).unwrap_or(30_000).min(3_600_000);
    // The spawn is the point of no return: a cancel already in for this id
    // stops the call here, with the check ordered against the reader's insert
    // (0014 finding 1). A cancel that arrives after this instant is caught by
    // the pump below, before the program can finish.
    if wire_cancelled(cancels, dead, id) {
        return reply(cancelled_outcome(name), false);
    }
    let mut host = host::Host::new();
    if let Err(why) = host.l0_begin(0, &argv, cwd.as_deref(), &env, input.as_bytes()) {
        return reply(outcome_json(&L0Out::Exit { code: 127, stderr: why }), false);
    }
    // The worker's backstop: the engine's timeout owns the call, so a program
    // still alive past it is work nobody will report — it is killed here.
    let due = host::clock_ms().saturating_add(timeout).saturating_add(BACKSTOP_MS);
    // The pump's cap is the cancel's poll: a 50 ms tick is the worst delay
    // between the driver's `cancel` and the program's SIGTERM.
    let mut cancelled = false;
    let out = loop {
        let tick = host::clock_ms().saturating_add(50).min(due);
        if let Some(m) = host.pump(Some(tick)) {
            if let host::Ev::L0Done { out, .. } = m.ev {
                break Some(out);
            }
            continue;
        }
        if wire_cancelled(cancels, dead, id) {
            cancelled = true;
            break None;
        }
        if host::clock_ms() >= due {
            break None;
        }
    };
    let out = match out {
        Some(out) => out,
        None => {
            if let Some(gid) = host.l0_gid(0) {
                host::term_group(gid);
                host::kill_group(gid);
            }
            L0Out::Signal {
                num: 9,
                stderr: if cancelled {
                    format!("peer {}: the driver cancelled the call", name)
                } else {
                    format!("peer {}: the call outlived its timeout", name)
                },
            }
        }
    };
    // An over-long output is killed at once, as the engine's own host does
    // (6.3); the outcome still reports `output too large`.
    if matches!(out, L0Out::Overlong) {
        if let Some(gid) = host.l0_gid(0) {
            host::kill_group(gid);
        }
    }
    reply(outcome_json(&out), true)
}

/// Run one call as a whole network: the payload enters the file's `entry`, the
/// file's `exit` answers (or an inner failure is `inner`, or nothing is quiet —
/// 2.6's rules, applied to a run of the file as instance 0).
fn execute_graph(g: &Graph, call: &Value, name: &str, cancels: &Mutex<HashSet<i64>>, dead: &AtomicBool) -> Value {
    let id = call.get("id").and_then(Value::as_i64).unwrap_or(0);
    // The flag rides the wire for uniformity; the composed boundary reports
    // `inner` failures, which 6.1 keeps an error whatever started.
    let reply = |out: Value, started: bool| {
        Obj::new()
            .str("type", "done")
            .int("id", id)
            .set("out", out)
            .set("started", Value::Bool(started))
            .build()
    };
    let inner = |why: String, started: bool| reply(Obj::new().str("kind", "inner").str("message", why).build(), started);
    // A call whose cancel is already in never plants an engine here (finding
    // 1, 0014). Mid-run, a cancel is still the socket shutdown plus the
    // graph's deadline: the engine-per-instance cancel handle is finding 3's.
    // Nothing ran, and the reply says so (finding 8, 0014); a graph's other
    // replies need no flag — the composed boundary reports an `inner`
    // failure, which 6.1 keeps an error whatever started.
    if wire_cancelled(cancels, dead, id) {
        return reply(cancelled_outcome(name), false);
    }
    // A hosted graph cannot be cancelled mid-run yet: its own engine owns the
    // loop, and a cancel here would leave that engine's processes behind. The
    // driver's socket shutdown plus the graph's deadline is the backstop (P1's
    // rest: a cancel handle the engine's loop reads).
    let text = call.get("input").and_then(Value::as_str).unwrap_or("");
    let payload = match json::parse(text) {
        Ok(v) => v,
        Err(e) => return inner(format!("the call's input is not JSON ({})", e), false),
    };
    let timeout = call.get("timeout").and_then(Value::as_u64).unwrap_or(30_000).min(3_600_000);
    // The file is instance 0 here, so its own placement (plain neuron ids) is
    // the one the engine reads: a graph can be spread over peers itself.
    let mut net = g.net.clone();
    net.nodes.push(Node {
        id: g.source.clone(),
        desc: String::new(),
        at: None,
        kind: Kind::Source,
        opts: Default::default(),
    });
    net.links.push(Link {
        from: g.source.clone(),
        to: g.entry.clone(),
    });
    let inputs = vec![crate::fixture::Input {
        at: 0,
        source: g.source.clone(),
        payload,
        replaces: None,
        budget: None,
    }];
    let mut eng = Engine::new_real(inputs, 10_000);
    // A person answers at a desk on their own machine; this surface has none
    // (3.4), so a graph with a human neuron is refused at its plant.
    eng.set_no_desk();
    // The driver's timeout owns the call; this is the backstop past it.
    eng.set_deadline_ms(timeout.saturating_add(BACKSTOP_MS));
    let failed = |why: String| {
        // Sealing (2.6): the inside's cause stays on the worker's stderr, and
        // the boundary says only what a composed neuron says. A log line that
        // cannot be written is dropped: a closed stderr pipe must not take the
        // connection (and the call) down with it.
        let _ = writeln!(std::io::stderr(), "peer {}: graph {:?} failed: {}", name, g.net.name, why);
        inner("no answer: an inner neuron failed".to_string(), true)
    };
    if let Some(p) = &g.peers {
        match Peers::load(p) {
            Ok(peers) => eng.set_peers(peers),
            Err(e) => return failed(format!("the graph's peers: {}", e)),
        }
    }
    if let Err(e) = eng.plant(&net, &g.files, Some(&g.base)) {
        return failed(format!("cannot plant: {}", e));
    }
    if let Some(why) = eng.start_serves_err() {
        eng.stop_after_refusal();
        return failed(format!("the graph refused the run: {}", why));
    }
    let outcome = eng.run();
    if let Some(why) = &outcome.refused {
        return failed(why.clone());
    }
    if outcome.timed_out {
        return failed(format!(
            "the graph did not settle within {} ms",
            timeout.saturating_add(BACKSTOP_MS)
        ));
    }
    // The exit's answer is the neuron's answer; a file with no `exit` answers
    // with its first sink, as a run prints them (4.4).
    let pick = match &g.exit {
        Some(x) => outcome.answers.iter().find(|(who, _)| who == x),
        None => outcome.answers.first(),
    };
    if let Some((_, payload)) = pick {
        return reply(Obj::new().str("kind", "answer").set("payload", payload.as_full()).build(), true);
    }
    // No answer and something failed inside: 2.6's `inner` (the message keeps
    // the inner cause, as a composed neuron's own error text does).
    if let Some(ev) = outcome
        .events
        .iter()
        .find(|e| e.get("type").and_then(Value::as_str) == Some("error"))
    {
        let msg = ev.get("message").and_then(Value::as_str).unwrap_or("an inner neuron failed");
        return failed(format!("an error event: {}", msg));
    }
    reply(Obj::new().str("kind", "quiet").build(), true)
}

/// A level 0 outcome as one wire object: the classification the engine applies,
/// not the raw streams, so both sides of the wire agree on what happened.
fn outcome_json(out: &L0Out) -> Value {
    match out {
        L0Out::Answer(v) => Obj::new().str("kind", "answer").set("payload", v.clone()).build(),
        L0Out::Quiet => Obj::new().str("kind", "quiet").build(),
        L0Out::Junk(text) => Obj::new().str("kind", "junk").str("text", text.clone()).build(),
        L0Out::Exit { code, stderr } => Obj::new()
            .str("kind", "exit")
            .int("code", *code as i64)
            .str("stderr", stderr.clone())
            .build(),
        L0Out::Signal { num, stderr } => Obj::new()
            .str("kind", "signal")
            .int("num", *num as i64)
            .str("stderr", stderr.clone())
            .build(),
        L0Out::Overlong => Obj::new().str("kind", "overlong").build(),
        L0Out::Inner(msg) => Obj::new().str("kind", "inner").str("message", msg.clone()).build(),
    }
}

fn outcome_of_json(v: &Value) -> Result<L0Out, String> {
    let kind = v.get("kind").and_then(Value::as_str).ok_or("no `kind`")?;
    let stderr = || v.get("stderr").and_then(Value::as_str).unwrap_or("").to_string();
    Ok(match kind {
        "answer" => L0Out::Answer(v.get("payload").cloned().ok_or("no `payload`")?),
        "quiet" => L0Out::Quiet,
        "junk" => L0Out::Junk(v.get("text").and_then(Value::as_str).unwrap_or("").to_string()),
        "exit" => L0Out::Exit {
            code: v.get("code").and_then(Value::as_i64).unwrap_or(1) as i32,
            stderr: stderr(),
        },
        "signal" => L0Out::Signal {
            num: v.get("num").and_then(Value::as_i64).unwrap_or(9) as i32,
            stderr: stderr(),
        },
        "overlong" => L0Out::Overlong,
        "inner" => L0Out::Inner(
            v.get("message")
                .and_then(Value::as_str)
                .unwrap_or("no answer: an inner neuron failed")
                .to_string(),
        ),
        other => return Err(format!("unknown kind '{}'", other)),
    })
}

// -- The small pieces of the wire --------------------------------------------

/// A peer name: the network file's id rule ([A-Za-z][A-Za-z0-9_-]{0,63}).
fn id_ok(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    s.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A token: 16 to 128 hex characters (8 to 64 bytes of shared secret), the
/// shape the desk's 128-bit token already has.
fn token_ok(s: &str) -> bool {
    (16..=128).contains(&s.len()) && s.len().is_multiple_of(2) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Constant-time comparison, as the desk's token check is: the time it takes
/// says nothing about how much of a guess was right.
fn token_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// `host:port`, with the `tcp://` scheme allowed and ignored (the only one
/// there is: a std-only crate has no TLS).
fn parse_address(s: &str) -> Result<(String, u16), String> {
    let bare = s.strip_prefix("tcp://").unwrap_or(s);
    let Some((host, port)) = bare.rsplit_once(':') else {
        return Err(format!("`address` '{}' is not host:port", s));
    };
    if host.is_empty() {
        return Err(format!("`address` '{}' names no host", s));
    }
    let port: u16 = port.parse().map_err(|_| format!("`address` '{}' has no port", s))?;
    if port == 0 {
        return Err(format!("`address` '{}' names port 0", s));
    }
    Ok((host.to_string(), port))
}

/// One line, its newline stripped, never more than `cap` bytes.
fn read_line(r: &mut BufReader<TcpStream>, cap: usize) -> Result<String, String> {
    let mut buf = Vec::new();
    let n = (&mut *r)
        .take(cap as u64 + 1)
        .read_until(b'\n', &mut buf)
        .map_err(|e| format!("the wire failed ({})", e))?;
    if n == 0 {
        return Err("the peer closed the connection".into());
    }
    if buf.len() > cap {
        return Err("a wire line is over its cap".into());
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    String::from_utf8(buf).map_err(|_| "a wire line is not UTF-8".into())
}

fn write_line(w: &mut TcpStream, v: &Value) -> Result<(), String> {
    let mut line = json::write(v);
    line.push('\n');
    w.write_all(line.as_bytes())
        .and_then(|_| w.flush())
        .map_err(|e| format!("the wire failed ({})", e))
}

fn refuse(w: &mut TcpStream, why: &str) {
    let v = Obj::new()
        .str("type", "hello")
        .int("v", 1)
        .set("ok", Value::Bool(false))
        .str("error", why)
        .build();
    let _ = write_line(w, &v);
}
