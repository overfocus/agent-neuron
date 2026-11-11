// The engine (SPEC sections 4-11), in one event loop, on either host.
//
// The event core never reads a clock and never touches the world: time is a
// count of milliseconds and every event comes out of one queue of wake-ups, in
// the order they were set (SPEC 6.2 rule 1). On the scripted host of
// CONFORMANCE 5 (the fixtures), programs are plans: a call's entry says when it
// ends and what it writes, and the wake-ups deliver those promises. On the real
// host (`host.rs`, the `run` command), programs are processes: the host starts
// them, reads their streams, and delivers what happened with the time it read
// it; the engine consumes those events at the same points (a reply line, a
// ready line, a ping's pong, a process's exit) and arms only its own wake-ups
// (timeouts, graces, restarts, pings).
//
// Structure: `plant` loads the network file(s) into one forest of instances
// (the outer network, then every composed file it names, one instance per
// `network` entry); `start_serves` starts every serve program and computes time
// zero (CONFORMANCE 4.2); `run` arms the injections (set before the run sets
// anything of its own, CONFORMANCE 4.1) and processes wake-ups until the run is
// over or the queue is empty (SPEC 6.2 rule 6). Events are `json::Value`s with
// opaque string ids; the runner renormalises both lists by first appearance.
// Events of the outer instance are reported; the inside of a composed neuron is
// discarded (sealing, SPEC 2.6). A call's reply wake-up is armed before its
// timeout, so a reply due at the timeout instant wins (SPEC 6.2 rule 7); every
// other same-instant order falls out of the arm order. On the real host an
// event's time is when the host read it, so a reply that made it to the queue
// by its timeout instant goes first.
use crate::fixture::{CallEntry, Input, Prog, Rule, Units};
use crate::host::{self, Host, Line};
use crate::json::{self, Obj, Value};
use crate::net::{Ground, Kind, NetFile, Network};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

/// The ground as the engine runs it (SPEC 4.5, decision 0012): the section of
/// the one network file that may declare it (2.6 refuses a composed one, so
/// there is at most one per run), the per-place state — a value and the
/// instant of its last write, an unwritten place reading 0 — and the one
/// 32-bit state of the pinned generator, seeded once by `ground.seed` here at
/// the plant and advanced one word per omitted-`to` reply.
struct GroundState {
    deposit: Option<crate::net::GroundDeposit>,
    evaporate: f64,
    select: bool,
    base: f64,
    power: f64,
    /// Place -> (value, instant of the last write). A read never changes an
    /// entry; a write stores the old value aged to its instant plus the
    /// amount (4.5).
    places: HashMap<String, (f64, i64)>,
    rng: u32,
}

impl GroundState {
    fn of(g: &Ground) -> GroundState {
        GroundState {
            deposit: g.deposit.clone(),
            evaporate: g.evaporate,
            select: g.select,
            base: g.base,
            power: g.power,
            places: HashMap::new(),
            // The generator is 32-bit: the seed rides its low 32 bits, the
            // same place the first add's wrap lands (4.5).
            rng: (g.seed as u64 & 0xFFFF_FFFF) as u32,
        }
    }

    /// The effective value of one entry at `t` (4.5): the stored value aged
    /// by `evaporate` a second since its write. Ageing for both a write (the
    /// old value, before the amount adds) and a read.
    fn effective(&self, (v, at): (f64, i64), t: i64) -> f64 {
        v * (-self.evaporate * (t - at) as f64 / 1000.0).exp()
    }
}

/// The staleness and sealing scope of a signal and the work it starts: the
/// outer instance is one scope; a composed neuron's inside is one scope per
/// call, so one call can never cut another (SPEC 2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ScopeKey(u32, usize);

/// A payload as the engine carries it: a JSON value or a text payload (a JSON
/// string is text: the one lossy spot, SPEC 7.2).
#[derive(Debug, Clone)]
pub(crate) enum PVal {
    Json(Value),
    Text(String),
}

impl PVal {
    /// The payload as a trace event carries it (7.2, TRACE.md): the whole
    /// value up to 2000 characters; a longer one is cut to its first 2000 and
    /// marked `truncated` (a JSON's preview stands in for its `data`). The
    /// run's printed answers carry the whole one (9.1, `as_full`).
    fn shaped(&self) -> Value {
        match self {
            PVal::Json(v) => {
                let text = json::write(v);
                if text.chars().count() > TRACE_PAYLOAD_CAP {
                    Obj::new()
                        .str("type", "json")
                        .str("preview", cut_chars(&text, TRACE_PAYLOAD_CAP))
                        .set("truncated", Value::Bool(true))
                        .build()
                } else {
                    Obj::new().str("type", "json").set("data", v.clone()).build()
                }
            }
            PVal::Text(s) => {
                if s.chars().count() > TRACE_PAYLOAD_CAP {
                    Obj::new()
                        .str("type", "text")
                        .str("text", cut_chars(s, TRACE_PAYLOAD_CAP))
                        .set("truncated", Value::Bool(true))
                        .build()
                } else {
                    Obj::new().str("type", "text").str("text", s.clone()).build()
                }
            }
        }
    }

    /// The whole value, as the run's answers carry it (the trace cuts long
    /// payloads; the run prints the whole one, TRACE.md and 9.1).
    pub(crate) fn as_full(&self) -> Value {
        match self {
            PVal::Json(v) => v.clone(),
            PVal::Text(s) => Value::Str(s.clone()),
        }
    }

    /// The bare value a program receives (SPEC 3.1); a text payload arrives as
    /// a JSON string.
    fn as_json_raw(&self) -> Value {
        match self {
            PVal::Json(v) => v.clone(),
            PVal::Text(s) => Value::Str(s.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WKind {
    Run,
    Serve,
    Human,
    Composed,
    /// An `http` neuron's one call: the request runs in its own thread (3.3);
    /// the engine's timeout and staleness cancel it.
    Http,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WSt {
    Held,
    Queued,
    Running,
}

/// Work: one input at one neuron until its reply (SPEC 1); a held input is
/// work waiting. `done` waits for the bookkeeping too: a cut level 0 call is
/// finished when its process group is gone, a cut level 1 call when its slot is
/// free (SPEC 6.2 rule 6).
struct Work {
    node: usize,
    sig: u64,
    trace: u64,
    kind: WKind,
    st: WSt,
    done: bool,
    /// The drop of the staleness cut was already reported (a threshold call
    /// reports one drop, for the crossing input).
    cut: bool,
    /// For a threshold call: the consumed held works, in arrival order.
    inputs: Vec<usize>,
    /// Level 1: the `re` of the answer. Level 0: the group index.
    re: u64,
    l0: Option<usize>,
    /// The matched call entry, when the program has started.
    entry: Option<CallEntry>,
    /// Effects: a started call is marked, never cut (11.2).
    stale_marked: bool,
    /// The outcome's event is out: a further event of the same call (a real
    /// process dying after its timeout, say) reports nothing; the commit of
    /// such a call still waits for its program to be over (10.4).
    reported: bool,
    /// Budget: the pool this work reserved from.
    pool: Option<u64>,
    reserved: bool,
    committed: bool,
    reserve_at: u64,
    /// Composed call: the inner root signal, its failure flag, whether `exit`
    /// has replied.
    inner_root: Option<u64>,
    fail_inner: bool,
    exit_replied: bool,
    /// `http`: the request's index in the engine's `https`.
    http: Option<usize>,
    /// `human` on the real host: the desk item of the question.
    desk: Option<String>,
}

impl Work {
    fn running(&self) -> bool {
        self.st == WSt::Running && !self.done
    }
    fn open(&self) -> bool {
        !self.done
    }
}

/// A level 0 process group (SPEC 3.1, 5.4): the leader and whatever it left
/// running in the group. On the real host, `gid` is the process group; the
/// scripted host's flags are the script's word on the same facts.
struct L0Proc {
    work: usize,
    /// The leader still lives (scripted: per the script; real: no exit seen).
    leader_alive: bool,
    survivor: bool,
    ignore_term: bool,
    /// The real process group (the leader's process group, its own pid).
    gid: Option<i32>,
    /// A real group whose over-long output was already killed at once.
    killed: bool,
    /// A call dispatched to a peer (decision 0013): no local process group,
    /// so `gid` stays `None` and the peer's outcome arrives as `L0Done`.
    remote: bool,
}

/// A level 1 slot: at most `maxInflight` run at once (SPEC 3.2).
#[derive(Clone, Copy)]
struct Slot {
    re: u64,
    work: usize,
    cancelled: bool,
}

/// What one `starts` entry scripted (CONFORMANCE 5.3).
struct StartSpec {
    ready: Option<u64>, // None: never says ready
    max_inflight: Option<u64>,
    ready_v: u64,
    first: Option<Value>,
    pong: Option<u64>, // None: never pongs
    end: Option<(u64, String)>,
    ignore_term: bool,
    leaves: bool,
    stderr: Option<String>,
}

impl StartSpec {
    fn from(v: &Value) -> StartSpec {
        let ready = match v.get("ready") {
            Some(Value::Bool(false)) => None,
            Some(x) => Some(x.as_u64().unwrap_or(0)),
            None => Some(0),
        };
        let pong = match v.get("pong") {
            Some(Value::Bool(false)) => None,
            Some(x) => Some(x.as_u64().unwrap_or(0)),
            None => Some(0),
        };
        let end = v.get("end").map(|e| {
            (
                e.get("after").and_then(|x| x.as_u64()).unwrap_or(0),
                e.get("how").and_then(|x| x.as_str()).unwrap_or("exit").to_string(),
            )
        });
        StartSpec {
            ready,
            max_inflight: v.get("maxInflight").and_then(|x| x.as_u64()),
            ready_v: v.get("readyV").and_then(|x| x.as_i64()).unwrap_or(1).max(0) as u64,
            first: v.get("first").cloned(),
            pong,
            end,
            ignore_term: v.get("ignoreTerm").and_then(|x| x.as_bool()).unwrap_or(false),
            leaves: v.get("leaves").and_then(|x| x.as_bool()).unwrap_or(false),
            stderr: v.get("stderr").and_then(|x| x.as_str()).map(|s| s.to_string()),
        }
    }
}

/// A level 1 process, one per serve neuron (per instance).
struct Proc {
    gen: usize, // 1-based: this process uses starts[gen - 1]
    spec: StartSpec,
    ready: bool,
    ready_at: u64,
    alive: bool,
    max_inflight: u64,
    next_re: u64,
    slots: Vec<Slot>,
    lines: u64,
    /// The real process group of this generation.
    gid: Option<i32>,
}

struct Serve {
    timeout: u64,
    procs: usize,
    down: bool,
    failed: bool, // five consecutive failed starts: the neuron is failed
    f: u64,
    queue: Vec<usize>,
    cur: Option<Proc>,
    ping_n: u64,
    ping_open: Option<u64>,
    /// A kill grace under way: the group is being killed; the note and the
    /// flag of whether someone may still be in it live here, not on `cur`,
    /// because the process record is gone (6.3). The real host's group id of
    /// that grace rides with it.
    grace: bool,
    grace_gen: usize,
    grace_survivor: bool,
    grace_gid: Option<i32>,
}

/// A signal: the engine sets all of its fields (SPEC 1); the trace is the
/// root signal's id.
#[derive(Clone)]
struct SigRec {
    from: usize,
    to: Vec<usize>,
    payload: PVal,
    hops: u64,
    reply_to: Option<u64>,
    cause: Option<u64>,
    replaces: Option<String>,
    trace: u64,
    inst: u32,
    owner_call: Option<usize>,
}

struct StaleEnt {
    sig: u64,
    works: Vec<usize>,
}

struct TraceInfo {
    exhausted: bool,
    finished: bool,
}

struct Inst {
    nodes: Vec<usize>,
    entry: Option<usize>,
    exit: Option<usize>,
    budget: Option<Units>,
}

struct INode {
    inst: u32,
    kind: Kind,
    display: String,
    threshold: u64,
    within: u64,
    max_hops: Option<u64>,
    timeout: u64,
    effect: bool,
    cost: Option<Units>,
    out: Vec<usize>,
    prog: String,
    /// The program of a `run` or `serve` neuron, and the working directory it
    /// runs in: the `cwd` option, else the directory of the file that declares
    /// the neuron (SPEC 2.4), and the `env` names (2.4). The real host reads
    /// them; the scripted host ignores them.
    cmd: Vec<String>,
    cwd: Option<PathBuf>,
    env: Vec<String>,
    /// The peer that executes this neuron's calls (decision 0013): `None` is
    /// this machine. Read from the peers file at the plant; P0 carries only
    /// the outer file's `run` neurons.
    peer: Option<usize>,
}

/// What a script answers a call with (CONFORMANCE 5.2), with the reply's `to`
/// resolved to node indexes.
#[derive(Debug, Clone)]
enum Outcome {
    Answer {
        payload: PVal,
        to: Option<Vec<usize>>,
        replaces: KeyThree,
    },
    Quiet,
    Fail {
        code: &'static str,
        msg: String,
    },
}

/// A `replaces` key in a reply: absent inherits the incoming signal's key,
/// `null` clears it, and a string of at most 256 bytes sets it (SPEC 5.2).
type KeyThree = Option<Option<String>>;

/// One level 0 program's outcome, as both hosts report it (SPEC 3.1): the
/// scripted host's reply object and the real host's streams both land on one
/// of these.
enum L0Out {
    Answer {
        payload: PVal,
    },
    Quiet,
    /// stdout carried more than 1 MiB: the error, then the kill.
    Overlong,
    Fail {
        code: &'static str,
        msg: String,
    },
}

/// An observer injection waiting for its `after` (SPEC 9).
#[derive(Clone)]
struct ObsInject {
    source: String,
    payload: Value,
    replaces: Option<String>,
    trigger: u64,
}

/// The wake-ups, one queue: at one instant they run in the order they were
/// set (SPEC 6.2 rule 1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Wake {
    Inject(usize),
    ObserverInject(usize),
    CallEnd { work: usize },
    CallTimeout { work: usize },
    Within { work: usize },
    L1Reply { work: usize },
    CancelReply { work: usize },
    EndAt { server: usize, gen: usize },
    StartDeadline { server: usize, gen: usize },
    ReadyAt { server: usize, gen: usize },
    L0Grace { proc: usize },
    ServeGrace { server: usize, gen: usize },
    RestartDue { server: usize },
    PingTick { server: usize, gen: usize },
    PongArrive { server: usize, gen: usize, n: u64 },
    PongDeadline { server: usize, gen: usize, n: u64 },
    SlotDeadline { server: usize, gen: usize, re: u64 },
}

pub struct RunOutcome {
    pub events: Vec<Value>,
    pub refused: Option<String>,
    pub harness: Option<String>,
    pub injected_traces: Vec<u64>,
    /// The whole answer payloads of the neurons with no outgoing links (a
    /// stale effect call's included), in the order they happened (the run
    /// prints these; the trace cuts long payloads). Read them through
    /// `printed_answers`; `PVal` is the engine's own shape.
    pub(crate) answers: Vec<(String, PVal)>,
    /// The run reached its deadline (a caller's `timeout_ms`) with work still
    /// open: the loop broke there, the teardown followed, and these are the
    /// facts of the run up to the deadline. Only the real host sets it.
    pub timed_out: bool,
}

/// One call to a peer-hosted graph (finding 3, 0014), the shape the worker's
/// connection reads as its `done` reply. The engine is planted once for the
/// worker process and the call enters its own composed boundary, so the
/// outcome is 2.6's per-call rules: the inside's `exit` replied, or the
/// inside was quiet, or it failed.
pub enum GraphOut {
    /// The first reply of the file's `exit` (2.6 step 2): the boundary's
    /// answer, the whole payload.
    Answer(Value),
    /// The inside settled with nothing running, queued or held for the call
    /// and `exit` had not replied (2.6 step 3).
    Quiet,
    /// An inner failure (2.6 step 3): the wire says the boundary's fixed
    /// `inner` message whatever `why` says — `why` is the cause for the
    /// worker's stderr only (sealing keeps it out of anything reported).
    Failed(Option<String>),
    /// The call's timeout cut it (2.6 step 4): the caller reported the
    /// timeout at the same instant; the reply only releases the connection.
    TimedOut,
    /// The wire's cancel cut the call (5.4's cut): the driver owns the end.
    Cancelled,
}

impl RunOutcome {
    /// The answers as the run prints them (4.4): a text payload as its text,
    /// the rest as their JSON value.
    pub fn printed_answers(&self) -> Vec<String> {
        self.answers
            .iter()
            .map(|(_, payload)| match payload {
                PVal::Text(text) => text.clone(),
                other => json::write(&other.as_full()),
            })
            .collect()
    }

    fn empty() -> RunOutcome {
        RunOutcome {
            events: Vec::new(),
            refused: None,
            harness: None,
            injected_traces: Vec::new(),
            answers: Vec::new(),
            timed_out: false,
        }
    }
}

pub struct Engine {
    inputs: Vec<Input>,
    pending_obs: Vec<ObsInject>,
    script: HashMap<String, Prog>,
    person: HashMap<String, Prog>,
    used: HashMap<String, Vec<u32>>,
    ping_every: u64,
    observer: Vec<Rule>,
    obs_fired: Vec<bool>,

    insts: Vec<Inst>,
    next_inst: u32,
    nodes: Vec<INode>,
    inner_inst_of: HashMap<usize, u32>, // composed node -> inner instance
    serves: HashMap<usize, Serve>,      // serve node -> the server state
    l0s: Vec<L0Proc>,

    now: u64,
    t_zero: u64,
    seq: u64,
    heap: BinaryHeap<Reverse<(u64, u64, Wake)>>,
    next_id: u64,
    events: Vec<Value>,
    sigs: HashMap<u64, SigRec>,
    works: Vec<Work>,
    groups: BTreeMap<(usize, u64), Vec<usize>>, // (node, trace) -> held works, arrival order
    pools: HashMap<u64, Units>,                 // trace -> the pool
    stale: BTreeMap<(ScopeKey, String, String), Vec<StaleEnt>>,
    traces: Vec<(u64, TraceInfo)>,
    injections: usize,
    obs_pending: usize,
    injected_roots: Vec<u64>,
    harness: Option<String>,
    refused: Option<String>,

    /// The world of the `run` command: real processes under `host.rs`. `None`
    /// is the scripted host of the conformance fixtures; every difference
    /// between the two hosts sits in one `is_real()` branch.
    real: Option<Host>,
    /// The first live-stream copy of each reported event (the `--stream` pipe
    /// or file; SPEC 9.1). Never holds the run up.
    stream: Option<crate::run::StreamSink>,
    /// False on the real host while its serve programs are still starting:
    /// nothing is reported before the engine's clock starts.
    reporting: bool,
    answers: Vec<(String, PVal)>,
    /// The desk (decision 0008, Appendix D): some on a run whose network has
    /// `human` neurons; `None` when it has none, or on the scripted host.
    desk: Option<crate::desk::Desk>,
    /// The http requests, in start order; the engine cancels one by index.
    https: Vec<crate::http::Cancel>,
    /// A hard deadline for the run, on the real host's clock (`set_deadline_ms`
    /// of a caller's `timeout_ms`): at the instant with work still open the
    /// loop breaks, the outcome says `timed_out`, and the teardown that every
    /// real run ends with runs as usual. `None` is a run with no deadline.
    deadline: Option<u64>,
    /// Whether this route opens a desk when the network has `human` neurons
    /// (decision 0008). The `run` route does (default); an MCP route does not:
    /// the desk is the person's surface, and such a route refuses at plant.
    wants_desk: bool,
    /// The people at the desk (decision 0010), when the run names them.
    grants: Option<crate::desk::Grants>,
    /// The ground of the network that declared one, if any (SPEC 4.5,
    /// decision 0012): 2.6 refuses a ground in a composed file, so only the
    /// outer network's — instance 0 — can ever sit here.
    ground: Option<GroundState>,
    /// The peers this run dispatches to (decision 0013), when `--peers` named
    /// any: the placement is read at the plant, the wire at each call.
    confine: Option<crate::confine::Confine>,
    peers: Option<crate::peers::Peers>,
    /// An inner failure's own wording, for a caller that runs ONE boundary
    /// over many calls (the peer's graph worker, finding 3, 0014): the wire
    /// says the boundary's fixed `inner` message; this is the cause for the
    /// worker's stderr only, and a diagnostic — nothing of the run's records
    /// reads it (sealing holds: inner events are never reported out).
    inner_why: Option<String>,
}

impl Engine {
    pub fn new(
        inputs: Vec<Input>,
        script: HashMap<String, Prog>,
        person: HashMap<String, Prog>,
        ping_every: u64,
        observer: Vec<Rule>,
    ) -> Engine {
        let mut used: HashMap<String, Vec<u32>> = HashMap::new();
        for (path, prog) in script.iter().chain(person.iter()) {
            used.insert(path.clone(), vec![0; prog.calls.len()]);
        }
        Engine {
            inputs,
            pending_obs: Vec::new(),
            script,
            person,
            used,
            ping_every,
            obs_fired: vec![false; observer.len()],
            observer,
            insts: Vec::new(),
            next_inst: 1,
            nodes: Vec::new(),
            inner_inst_of: HashMap::new(),
            serves: HashMap::new(),
            l0s: Vec::new(),
            now: 0,
            t_zero: 0,
            seq: 1_000_000,
            heap: BinaryHeap::new(),
            next_id: 1,
            events: Vec::new(),
            sigs: HashMap::new(),
            works: Vec::new(),
            groups: BTreeMap::new(),
            pools: HashMap::new(),
            stale: BTreeMap::new(),
            traces: Vec::new(),
            injections: 0,
            obs_pending: 0,
            injected_roots: Vec::new(),
            harness: None,
            refused: None,
            real: None,
            stream: None,
            reporting: false,
            answers: Vec::new(),
            desk: None,
            https: Vec::new(),
            deadline: None,
            wants_desk: true,
            grants: None,
            ground: None,
            peers: None,
            confine: None,
            inner_why: None,
        }
    }

    /// The real host: `run`'s engine. The clock is real time; the inputs are
    /// injected `at` ms after time zero (the last ready of the serve
    /// programs, CONFORMANCE 4.2's real analogue).
    pub fn new_real(inputs: Vec<Input>, ping_every: u64) -> Engine {
        let mut eng = Engine::new(inputs, HashMap::new(), HashMap::new(), ping_every, Vec::new());
        eng.real = Some(Host::new());
        eng.now = host::clock_ms();
        eng
    }

    /// Offer a copy of every reported event as it happens (9.1). The sink is
    /// the `run` command's; a slow or closed sink never changes the run.
    pub fn set_stream(&mut self, sink: crate::run::StreamSink) {
        self.stream = Some(sink);
    }

    /// A hard deadline for the run, `ms` from now on the real host's clock: a
    /// run still open at the instant stops as timed out (see the field). The
    /// deadline covers the plant and the serve start too, so a route may give
    /// a whole run's budget to one call of it.
    pub fn set_deadline_ms(&mut self, ms: u64) {
        self.deadline = Some(host::clock_ms().saturating_add(ms));
    }

    /// The people the desk serves (decision 0010): their tokens, roles and
    /// the human neurons each may answer. Set before the plant.
    pub fn set_grants(&mut self, grants: crate::desk::Grants) {
        self.grants = Some(grants);
    }

    /// The peers (decision 0013): the machines that execute some of this
    /// run's neurons, and the wire to them. Set before the plant; absent is a
    /// run whose every call is local.
    pub fn set_peers(&mut self, peers: crate::peers::Peers) {
        self.peers = Some(peers);
    }

    /// Confinement (decision 0018): every `run` and `serve` program this
    /// engine plants starts under the sandbox, held to its neuron's `may`.
    /// Set before the plant; absent is a run that confines nothing.
    pub fn set_confine(&mut self, confine: crate::confine::Confine) {
        self.confine = Some(confine);
    }

    /// No desk on this route: a network with `human` neurons is refused at
    /// plant (a person answers at a desk, and this route has none to offer).
    pub fn set_no_desk(&mut self) {
        self.wants_desk = false;
    }

    /// A run the engine refused at its start (a serve program that never said
    /// ready): the start spawned processes; they are stopped, silently.
    pub fn stop_after_refusal(&mut self) {
        self.desk_close();
        if let Some(peers) = self.peers.as_mut() {
            peers.abort_all();
        }
        if self.real.is_none() {
            return;
        }
        let gids: Vec<i32> = self
            .serves
            .values()
            .filter_map(|s| s.cur.as_ref())
            .filter_map(|p| p.gid)
            .filter(|g| host::group_alive(*g))
            .collect();
        self.host().end_refused(&gids);
    }

    fn is_real(&self) -> bool {
        self.real.is_some()
    }

    fn host(&mut self) -> &mut Host {
        self.real.as_mut().expect("the real host")
    }

    /// A harness failure (a call no script entry matches, say): the run
    /// unwinds and the runner reports the fixture as failed.
    fn harness_failure(&mut self, why: String) -> ! {
        if self.harness.is_none() {
            self.harness = Some(why);
        }
        panic!("harness failure");
    }

    /// Reported time: whole ms since the run started (CONFORMANCE 4.2).
    fn now_t(&self) -> i64 {
        self.now.saturating_sub(self.t_zero) as i64
    }

    /// Report an event of `inst`'s network; the inside of a composed neuron is
    /// never reported (sealing, SPEC 2.6). The observer (SPEC 9) reads the
    /// outer events as they are reported, and so does the run's stream. On the
    /// real host nothing is reported before the engine's clock starts (an
    /// engine reports from its run's time zero on; a pre-time-zero fact is a
    /// process fact of the start-up, and the run that follows never sees it).
    fn emit(&mut self, inst: u32, ev: Value) {
        if inst != 0 {
            return;
        }
        if self.real.is_some() && !self.reporting {
            return;
        }
        if let Some(stream) = &mut self.stream {
            stream(&ev);
        }
        self.observer_sees(&ev);
        self.events.push(ev);
    }

    // -- The observer (SPEC 9) ----------------------------------------------

    /// A rule fires once, on the first event of the outer network that has an
    /// id (or is a `finished`) and whose fields the `when` covers. The
    /// injection is pending for its `after`; its `inject` note is written just
    /// before the injected signal.
    fn observer_sees(&mut self, ev: &Value) {
        if matches!(ev.get("type"), Some(Value::Str(t)) if t == "note") {
            return; // notes are not shown to an observer
        }
        for rule in 0..self.observer.len() {
            if self.obs_fired[rule] {
                continue;
            }
            if ev.covers(&self.observer[rule].when) {
                self.obs_fired[rule] = true;
                let id_str = ev
                    .get("id")
                    .and_then(|v| v.as_str())
                    .or_else(|| ev.get("trace").and_then(|v| v.as_str()))
                    .unwrap_or("0");
                let trigger: u64 = id_str.parse().unwrap_or(0);
                self.pending_obs.push(ObsInject {
                    source: self.observer[rule].inject.source.clone(),
                    payload: self.observer[rule].inject.payload.clone(),
                    replaces: self.observer[rule].inject.replaces.clone(),
                    trigger,
                });
                self.obs_pending += 1;
                let j = self.pending_obs.len() - 1;
                let after = self.observer[rule].inject.after;
                self.arm(self.now + after, Wake::ObserverInject(j));
            }
        }
    }

    // -----------------------------------------------------------------------
    // The forest of instances
    // -----------------------------------------------------------------------

    /// Load the outer network and every composed file it names. On the virtual
    /// host (conformance) an `http` neuron refuses the run as before: the
    /// fixtures have no page for the network. On the real host the run
    /// performs http calls (M2), and only a `https://` one is refused, for a
    /// std-only engine has no TLS. A `human` neuron runs for real: the desk
    /// of decision 0008 is opened here, one page for the run, on 127.0.0.1
    /// only, and the run's stderr carries its address.
    pub fn plant(&mut self, outer: &Network, files: &HashMap<String, NetFile>, base: Option<&PathBuf>) -> Result<(), String> {
        self.plant_net(outer, files, 0, None, "", base.map(PathBuf::from).unwrap_or_default())?;
        // Placement, checked against the tree this engine planted (P2): a key
        // is a node path (`audit` at the top, `review/wrap` inside a composed
        // one), and only `run` neurons and whole networks travel. A composed
        // node's inside is not planted here, so a path into a placed one is a
        // mistake — that tree lives on the peer.
        if let Some(peers) = &self.peers {
            for path in peers.placement.keys() {
                match self.nodes.iter().find(|n| n.prog == *path) {
                    None => {
                        self.refused = Some(format!("the peers file places '{}', which this network does not declare", path));
                        return Ok(());
                    }
                    Some(n) if !matches!(n.kind, Kind::Run(_) | Kind::Serve(_) | Kind::Network(_)) => {
                        self.refused = Some(format!(
                            "the peers file places '{}', and this engine carries only `run` and `serve` neurons, and whole networks, to a peer",
                            path
                        ));
                        return Ok(());
                    }
                    Some(_) => {}
                }
            }
        }
        let bad: Option<String> = self.nodes.iter().find_map(|n| match &n.kind {
            Kind::Http(url) => {
                if !self.is_real() {
                    Some(format!(
                        "the network has an http neuron ('{}'), which this engine does not run",
                        n.display
                    ))
                } else if url.starts_with("https://") {
                    Some(format!(
                        "the http neuron '{}' names '{url}', and this engine has no TLS (it is std-only; http:// only)",
                        n.display
                    ))
                } else {
                    None
                }
            }
            _ => None,
        });
        if let Some(why) = bad {
            self.refused = Some(why);
            return Ok(());
        }
        if self.is_real() && self.nodes.iter().any(|n| matches!(n.kind, Kind::Human)) {
            if !self.wants_desk {
                // SPEC 3.4 with the M3 reading: a person answers at a desk,
                // and this route is not the desk's surface — an MCP result
                // never holds a person's page. The engine refuses before the
                // run starts, as the M1 `no desk for a person` refusal did.
                let who = self
                    .nodes
                    .iter()
                    .find(|n| matches!(n.kind, Kind::Human))
                    .map(|n| n.display.clone())
                    .unwrap_or_default();
                self.refused = Some(format!(
                    "the network has a human neuron ('{who}'), and this route has no desk for a person \
                     (the desk is the person's surface, not an MCP surface; 3.4)"
                ));
                return Ok(());
            }
            // The desk binds before anything runs, so the address is on
            // stderr before the first answer could matter. Appendix D opens
            // the page "at the first question"; this engine opens it at the
            // plant; with no questions it closes again, unanswered.
            let post = self.host().post_handle();
            match crate::desk::Desk::open(std::sync::Arc::new(move || post.post(host::Ev::Desk)), self.grants.take()) {
                Ok(desk) => self.desk = Some(desk),
                Err(why) => {
                    self.refused = Some(format!("the desk for the network's human neurons could not be opened ({why})"));
                }
            }
        }
        Ok(())
    }

    fn plant_net(
        &mut self,
        net: &Network,
        files: &HashMap<String, NetFile>,
        inst: u32,
        _owner: Option<usize>,
        prefix: &str,
        dir: PathBuf,
    ) -> Result<(), String> {
        self.insts.push(Inst {
            nodes: Vec::new(),
            entry: None,
            exit: None,
            budget: net.budget.clone(),
        });
        let mut local: HashMap<String, usize> = HashMap::new();
        if inst == 0 {
            // Only the outer network can carry a ground (4.5; 2.6 refuses a
            // composed one): its instance is 0 here, and the generator and
            // the per-place state start with the run.
            self.ground = net.ground.as_ref().map(GroundState::of);
        }
        for n in net.nodes.iter() {
            let node = self.nodes.len();
            local.insert(n.id.clone(), node);
            // A placed neuron (decision 0013, P2): the key is the node's path
            // in the tree (`audit`, `review/wrap`), so a composed node inside a
            // hosted graph can be placed too — nesting all the way down.
            let prog = format!("{}{}", prefix, n.id);
            let peer = self.peers.as_ref().and_then(|p| p.placement.get(&prog).copied());
            if let Kind::Network(_) = &n.kind {
                // Each `network` entry is its own instance (2.6), numbered
                // before this file's other composed nodes plant. A placed one
                // is planted by its peer and gets no instance here (0013).
                if peer.is_none() {
                    self.inner_inst_of.insert(node, self.next_inst);
                    self.next_inst += 1;
                }
            }
            let (cmd, env) = match &n.kind {
                Kind::Run(cmd) | Kind::Serve(cmd) => (cmd.clone(), n.opts.env.clone()),
                _ => (Vec::new(), Vec::new()),
            };
            // cwd: the `cwd` option (a relative one is relative to the
            // declaring file's directory: a value in the file means what the
            // file means), else that directory itself (SPEC 2.4).
            let cwd = n
                .opts
                .cwd
                .clone()
                .map(|c| {
                    if PathBuf::from(&c).is_absolute() {
                        PathBuf::from(&c)
                    } else {
                        dir.join(&c)
                    }
                })
                .or_else(|| Some(dir.clone()));
            // Confined (decision 0018): the command becomes the sandbox's,
            // with a profile built from this neuron's `may`. An entry the
            // sandbox cannot hold refuses the plant, before anything starts.
            let cmd = match &self.confine {
                Some(confine) if !cmd.is_empty() && peer.is_none() => {
                    confine.wrap(&prog, &cmd, cwd.as_deref().unwrap_or(&dir), n.opts.may.as_ref())?
                }
                _ => cmd,
            };
            self.nodes.push(INode {
                inst,
                peer,
                kind: n.kind.clone(),
                display: n.id.clone(),
                threshold: n.opts.threshold.unwrap_or(1),
                within: n.opts.within.unwrap_or(10_000),
                max_hops: n.opts.max_hops,
                timeout: n.opts.timeout.unwrap_or(30_000),
                effect: n.opts.effect,
                cost: n.opts.cost.clone(),
                out: Vec::new(),
                prog,
                cmd,
                cwd,
                env,
            });
        }
        for n in net.nodes.iter() {
            if let Kind::Network(p) = &n.kind {
                let node = local[&n.id];
                if self.nodes[node].peer.is_some() {
                    // Hosted by a peer: this engine keeps no inner instance
                    // (0013). The peer holds the file tree and plants it.
                    continue;
                }
                let inner_inst = self.inner_inst_of[&node];
                let file = files.get(p).ok_or_else(|| format!("the composed file '{}' is not loaded", p))?;
                let (inner, inner_dir) = (file.net.clone(), file.dir.clone());
                self.plant_net(
                    &inner,
                    files,
                    inner_inst,
                    Some(node),
                    // The instance path joins the parent's: diag/mid/e (5.1).
                    &format!("{}{}/", prefix, n.id),
                    inner_dir,
                )?;
            }
        }
        for l in net.links.iter() {
            if let (Some(&from), Some(&to)) = (local.get(&l.from), local.get(&l.to)) {
                self.nodes[from].out.push(to);
            }
        }
        let this = &mut self.insts[inst as usize];
        this.nodes = net.nodes.iter().map(|n| local[&n.id]).collect();
        this.entry = net.entry.as_ref().and_then(|id| local.get(id).copied());
        this.exit = net.exit.as_ref().and_then(|id| local.get(id).copied());
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The start-up phase (CONFORMANCE 4.2): every serve program in every
    // instance starts first, in the network's order; time zero is the instant
    // the last of them said ready.
    // -----------------------------------------------------------------------

    pub fn start_serves(&mut self) -> Result<(), String> {
        if self.refused.is_some() {
            return Ok(());
        }
        for node in 0..self.nodes.len() {
            if matches!(self.nodes[node].kind, Kind::Serve(_)) {
                if !self.serves.contains_key(&node) {
                    let s = self.new_serve(node);
                    self.serves.insert(node, s);
                }
                self.spawn_process(node, false);
            }
        }
        if self.is_real() {
            // The real start-up: wait for every serve program's first line.
            // The run's deadline (a caller's timeout_ms) covers the start-up
            // too (finding 5, 0014): no wait runs past it, and at the instant
            // the start is over for this caller — `run` stops there as timed
            // out, like any run still open at its deadline, after the
            // start's own cleanup.
            while self.harness.is_none() && self.refused.is_none() && self.any_serve_unready() {
                if self.deadline_due() {
                    break;
                }
                let next = self.next_wake();
                let cap = match (next, self.deadline) {
                    (Some(t), Some(d)) => Some(t.min(d)),
                    (t, d) => t.or(d),
                };
                if self.host_step(cap) {
                    continue;
                }
                match next {
                    Some(t) => {
                        self.now = t;
                        if self.deadline_due() {
                            break;
                        }
                        let Some(w) = self.pop() else { break };
                        self.fire(w);
                        self.wake_composed_calls();
                    }
                    None => {
                        // Only host events can arrive: wait for one, never
                        // past the run's deadline.
                        if !self.host_step(self.deadline) {
                            break;
                        }
                    }
                }
            }
        } else {
            while self.any_serve_unready() {
                let Some(Reverse((t, _, _))) = self.heap.peek().copied() else {
                    break;
                };
                self.now = t;
                let Some(w) = self.pop() else { break };
                self.fire(w);
                if self.harness.is_some() {
                    break;
                }
            }
        }
        if self.harness.is_none() && self.any_serve_unready() {
            if self.is_real() && self.refused.is_none() && self.deadline_due() {
                // The run's deadline covered the start-up (finding 5, 0014):
                // not a refusal — the caller's `run` stops as timed out at
                // once, after the start's cleanup, the way any run with work
                // still open at its deadline does.
            } else if let Some(why) = self.refused.clone() {
                return Err(why);
            } else {
                return Err("a serve program never became ready".into());
            }
        }
        self.t_zero = self
            .serves
            .values()
            .filter_map(|s| s.cur.as_ref())
            .filter(|p| p.ready)
            .map(|p| p.ready_at)
            .max()
            .unwrap_or(0);
        if self.is_real() {
            // The clock starts here (Appendix C): from now on, an event's
            // time is ms since time zero, and events are reported.
            self.reporting = true;
        }
        Ok(())
    }

    fn any_serve_unready(&self) -> bool {
        self.serves
            .iter()
            .any(|(_, s)| !s.failed && (s.cur.is_none() || !s.cur.as_ref().expect("cur").ready))
    }

    // -----------------------------------------------------------------------
    // The run
    // -----------------------------------------------------------------------

    pub fn run(&mut self) -> RunOutcome {
        // Injections the engine refuses to the producer (SPEC 4.1, 10.2) stop
        // the run before it starts.
        for inp in &self.inputs {
            if inp.replaces.as_ref().map(|k| k.len() > 256).unwrap_or(false) {
                self.refused = Some("an injection carries a replaces key longer than 256 bytes".into());
                break;
            }
            if let Some(units) = &inp.budget {
                if let Err(why) = self.injection_budget_ok(units) {
                    self.refused = Some(format!("an injection was refused to the producer: {}", why));
                    break;
                }
            }
        }
        if let Some(refused) = self.refused.take() {
            let mut outcome = RunOutcome::empty();
            outcome.refused = Some(refused);
            return outcome;
        }
        // Injections are set before the run starts, in listed order; at a tie
        // they go before every wake-up the run sets itself. The run's own arms
        // start at 1_000_000.
        for (i, inp) in self.inputs.iter().enumerate() {
            self.heap.push(Reverse((self.t_zero + inp.at, i as u64, Wake::Inject(i))));
            self.injections += 1;
        }
        let mut timed_out = false;
        while self.harness.is_none() {
            if self.run_over() {
                break;
            }
            if self.deadline_due() {
                timed_out = true;
                break;
            }
            let next = self.next_wake();
            if self.is_real() {
                // A host event due at or before the next wake-up goes first;
                // its time is when the host read it, so work that ended in the
                // same ms as its timeout (rule 7) is here by its stamp. A
                // deadline caps every wait the loop makes: the engine's pump
                // never blocks past it, and at the instant the run stops as
                // timed out, with the teardown after the loop.
                let cap = match (next, self.deadline) {
                    (Some(t), Some(d)) => Some(t.min(d)),
                    (t, d) => t.or(d),
                };
                if self.host_step(cap) {
                    // A process outcome may have emptied a composed call's
                    // inside: 2.6 step 3's quiet/inner error is due at this
                    // instant, and only a wake-up used to check it (the real
                    // host would wait for the composed call's timeout instead).
                    self.wake_composed_calls();
                    continue;
                }
                if self.deadline_due() {
                    timed_out = true;
                    break;
                }
                self.now = next.unwrap_or_else(|| self.now + 65_536);
                let Some(w) = self.pop() else {
                    // The heap is empty and nothing is due: only a process
                    // outcome can close the run. The pump waits to the
                    // deadline when one is set, and to nothing at all when
                    // none is (a process's end is the only closer).
                    if self.host_step(self.deadline) {
                        self.wake_composed_calls();
                        continue;
                    }
                    if self.deadline_due() {
                        timed_out = true;
                        break;
                    }
                    break;
                };
                self.fire(w);
                self.wake_composed_calls();
                continue;
            }
            let Some(next) = next else { break };
            self.now = next;
            let Some(w) = self.pop() else { break };
            self.fire(w);
            self.wake_composed_calls();
        }
        if self.is_real() {
            self.end_run_quietly();
        }
        RunOutcome {
            events: std::mem::take(&mut self.events),
            refused: self.refused.take(),
            harness: self.harness.take(),
            injected_traces: std::mem::take(&mut self.injected_roots),
            answers: std::mem::take(&mut self.answers),
            timed_out,
        }
    }

    /// Is the run's deadline due (set by a caller's `timeout_ms`)? Only the
    /// real host has one: the scripted host of the conformance run never sets
    /// a deadline, and its loop is never asked about one.
    fn deadline_due(&self) -> bool {
        matches!(self.deadline, Some(d) if self.now >= d)
    }

    /// A serve start the worker could not even take (decision 0013, P1): a
    /// start failure with the wire's own word, for a generation the engine
    /// still waits on; a late word about a generation already replaced is
    /// dropped, as its lines are (3.2's generation checks).
    fn host_serve_start_failed(&mut self, server: usize, gen: usize, why: String) {
        let valid = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref())
            .map(|p| p.gen == gen && !p.ready)
            .unwrap_or(false);
        if !valid {
            return;
        }
        self.start_failed(server, gen, why);
    }

    /// The end of a real run (SPEC 3.2's shutdown, Appendix C): stdin closed
    /// everywhere, then SIGTERM to what still lives after 2 s and SIGKILL 2 s
    /// later, silently; the run returns after the processes are gone. The
    /// groups of finished calls that left a member behind are stopped the
    /// same way (decision 0001: the engine kills a call's process group).
    fn end_run_quietly(&mut self) {
        self.desk_close();
        if let Some(peers) = self.peers.as_mut() {
            peers.abort_all();
        }
        let mut serve_gids: Vec<i32> = Vec::new();
        let mut close: Vec<(usize, usize)> = Vec::new();
        for (node, s) in self.serves.iter() {
            let Some(p) = s.cur.as_ref() else { continue };
            if let Some(g) = p.gid {
                close.push((*node, p.gen));
                serve_gids.push(g);
            }
        }
        for (node, gen) in &close {
            self.host().l1_close(*node, *gen);
        }
        // Finished calls' groups that left a member behind are stopped the
        // same way (decision 0001: the engine kills a call's process group).
        let l0_gids: Vec<i32> = self.l0s.iter().filter_map(|p| p.gid).filter(|g| host::group_alive(*g)).collect();
        self.host().end_run(&serve_gids);
        if !l0_gids.is_empty() {
            self.host().end_run(&l0_gids);
        }
    }

    /// The groups of a run that did not end politely (a harness failure
    /// unwinds): every serve program and every level 0 call's group stopped
    /// at once — term, then the kill for whatever stays. A group the run's
    /// own teardown ended is dead to this, the way `kill_group` reads dead
    /// ids.
    pub fn kill_leftovers(&mut self) {
        self.desk_close();
        if let Some(peers) = self.peers.as_mut() {
            peers.abort_all();
        }
        if self.real.is_none() {
            return;
        }
        let mut serve_gids: Vec<i32> = Vec::new();
        for s in self.serves.values() {
            let Some(p) = s.cur.as_ref() else { continue };
            if let Some(g) = p.gid {
                serve_gids.push(g);
            }
        }
        let l0_gids: Vec<i32> = self.l0s.iter().filter_map(|p| p.gid).collect();
        self.host().end_refused(&serve_gids);
        if !l0_gids.is_empty() {
            self.host().end_refused(&l0_gids);
        }
    }

    /// Is the run over (SPEC 6.2 rule 6): nothing running, queued or held, no
    /// injection still to come, and no cancelled work left behind (a cut
    /// level 0 group, a cancelled level 1 call's slot, a kill grace). A
    /// restart delay, an idle ping or a pong deadline is not work.
    fn run_over(&self) -> bool {
        self.injections == 0
            && self.obs_pending == 0
            && self.works.iter().all(|w| w.done)
            && self.groups.values().all(|list| list.is_empty())
            && !self.serves.values().any(|s| s.grace)
    }

    /// The run's next wake-up, or `None` when the heap is empty: the loop
    /// waits to that instant (or until a host event arrives first).
    fn next_wake(&self) -> Option<u64> {
        self.heap.peek().map(|Reverse((t, _, _))| *t)
    }

    fn arm(&mut self, t: u64, w: Wake) {
        self.seq += 1;
        self.heap.push(Reverse((t, self.seq, w)));
    }

    fn pop(&mut self) -> Option<Wake> {
        self.heap.pop().map(|Reverse((_, _, w))| w)
    }

    /// One event from the real host, if one is due before the engine's next
    /// wake-up (or, with `None` for the cap, one at all): the time is when the
    /// host read it. `true` when an event was taken.
    ///
    /// The cap of the wait is a generous bound, not a promise: the heap is
    /// never empty while anything is open (every running call has a timeout
    /// armed), so the fallback keeps the run from hanging on a process the
    /// host can no longer report on.
    fn host_step(&mut self, cap: Option<u64>) -> bool {
        let cap = cap.or_else(|| Some(self.now + 65_536));
        let Some(m) = self.real.as_ref().expect("the real host").pump(cap) else {
            return false;
        };
        self.now = self.now.max(m.t);
        match m.ev {
            host::Ev::L0Done { l0, out, started } => self.host_l0_done(l0, out, started),
            host::Ev::L1Line { server, gen, line } => self.consume_line(server, gen, line),
            host::Ev::L1Down { server, gen, exited } => self.host_l1_down(server, gen, exited),
            host::Ev::ServeStartFailed { server, gen, why } => self.host_serve_start_failed(server, gen, why),
            host::Ev::HttpDone { idx, out } => self.host_http_done(idx, out),
            host::Ev::Desk => self.drain_desk(),
        }
        true
    }
}

// ---------------------------------------------------------------------------
// The wake-ups
// ---------------------------------------------------------------------------

impl Engine {
    fn fire(&mut self, w: Wake) {
        match w {
            Wake::Inject(i) => self.fire_inject(i),
            Wake::ObserverInject(j) => self.fire_obs_inject(j),
            Wake::CallEnd { work } => self.fire_call_end(work),
            Wake::CallTimeout { work } => self.fire_call_timeout(work),
            Wake::Within { work } => self.fire_within(work),
            Wake::L1Reply { work } => self.fire_l1_reply(work),
            Wake::CancelReply { work } => self.fire_cancel_reply(work),
            Wake::EndAt { server, gen } => self.fire_end_at(server, gen),
            Wake::StartDeadline { server, gen } => self.fire_start_deadline(server, gen),
            Wake::ReadyAt { server, gen } => self.fire_ready_at(server, gen),
            Wake::L0Grace { proc } => self.fire_l0_grace(proc),
            Wake::ServeGrace { server, gen } => self.fire_serve_grace(server, gen),
            Wake::RestartDue { server } => self.fire_restart_due(server),
            Wake::PingTick { server, gen } => self.fire_ping_tick(server, gen),
            Wake::PongArrive { server, gen, n } => self.fire_pong_arrive(server, gen, n),
            Wake::PongDeadline { server, gen, n } => self.fire_pong_deadline(server, gen, n),
            Wake::SlotDeadline { server, gen, re } => self.fire_slot_deadline(server, gen, re),
        }
    }

    // -- The producer's injection (SPEC 4.1) --------------------------------

    fn fire_inject(&mut self, i: usize) {
        self.injections -= 1;
        let inp = self.inputs[i].clone();
        let Some(src) = self.find_source(&inp.source) else {
            self.harness_failure(format!("the injection names unknown source '{}'", inp.source));
        };
        // The trace and its pool: the budget's ceilings, lowered by the
        // injection's `budget` (10.2).
        let to: Vec<usize> = self.nodes[src].out.clone();
        let id = self.new_signal(
            src,
            to,
            payload_of(inp.payload.clone()),
            0,
            None,
            None,
            inp.replaces.clone(),
            None,
            0,
            None,
        );
        if let Some(pool) = self.outer_pool() {
            self.pools.insert(id, self.lowered(pool, inp.budget.as_ref()));
        }
        self.traces.push((
            id,
            TraceInfo {
                exhausted: false,
                finished: false,
            },
        ));
        // The producer's injections are both runs' common part: only the
        // observer's injections are the "traces that start at an injection"
        // of the two-run rule (CONFORMANCE 6).
        self.deliver_signal(id);
    }

    /// The observer's injection (SPEC 9): recorded as a note just before the
    /// injected signal, with the trigger of the event that fired the rule.
    fn fire_obs_inject(&mut self, j: usize) {
        self.obs_pending -= 1;
        let ObsInject {
            source,
            payload,
            replaces,
            trigger,
            ..
        } = self.pending_obs[j].clone();
        let Some(src) = self.find_source(&source) else {
            self.harness_failure(format!("the observer injects into unknown source '{}'", source));
        };
        self.emit(
            0,
            Obj::new()
                .str("type", "note")
                .int("t", self.now_t())
                .str("neuron", self.nodes[src].display.clone())
                .str("what", "inject")
                .str("trigger", trigger.to_string())
                .build(),
        );
        let to: Vec<usize> = self.nodes[src].out.clone();
        let id = self.new_signal(src, to, payload_of(payload), 0, None, None, replaces, None, 0, None);
        if let Some(pool) = self.outer_pool() {
            self.pools.insert(id, pool);
        }
        self.traces.push((
            id,
            TraceInfo {
                exhausted: false,
                finished: false,
            },
        ));
        self.injected_roots.push(id);
        self.deliver_signal(id);
    }

    fn find_source(&self, name: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.inst == 0 && n.display == name && matches!(n.kind, Kind::Source))
    }

    // -- A level 0 program's outcome (or a scripted person's) ---------------

    fn fire_call_end(&mut self, work: usize) {
        let state = self.works.get(work).map(|w| (w.running(), w.cut));
        let Some((running, cut)) = state else { return };
        if !running || cut {
            return;
        }
        let kind = self.works[work].kind;
        match kind {
            WKind::Human => {
                let reply = self.works[work].entry.as_ref().map(|e| e.reply.clone()).unwrap_or(Value::Null);
                self.person_replied(work, &reply);
                self.finish_check(None);
            }
            WKind::Run => {
                let reply = self.works[work].entry.as_ref().map(|e| e.reply.clone()).unwrap_or(Value::Null);
                let get = |k: &str| match &reply {
                    Value::Obj(fields) => fields.iter().find(|(f, _)| f == k).map(|(_, v)| v),
                    _ => None,
                };
                let out = if get("quiet").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
                    L0Out::Quiet
                } else if let Some(Value::Str(msg)) = get("error") {
                    L0Out::Fail {
                        code: "exit",
                        msg: format!("exit 1: {}", trim_500(msg)),
                    }
                } else if let Some(Value::Str(text)) = get("junk") {
                    L0Out::Fail {
                        code: "junk",
                        msg: format!("not JSON: {}", trim_2000(text)),
                    }
                } else if get("crash").is_some() {
                    let name = match get("crash") {
                        Some(Value::Str(s)) => s.clone(),
                        _ => "SIGKILL".to_string(),
                    };
                    L0Out::Fail {
                        code: "exit",
                        msg: format!("signal {name}: "),
                    }
                } else if get("overlong").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
                    L0Out::Overlong
                } else if get("hang").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
                    // Never ends by itself: only the run's other wake-ups
                    // reach it.
                    return;
                } else if let Some(p) = get("answer") {
                    L0Out::Answer {
                        payload: payload_of(p.clone()),
                    }
                } else {
                    self.harness_failure(format!("the reply of the level 0 call {} is not an outcome", work));
                };
                self.l0_finished(work, out);
                self.finish_check(None);
            }
            _ => {}
        }
    }

    /// A level 0 program is over (SPEC 3.1): the commit just before the
    /// outcome event (10.4), then the event, and for an over-long output the
    /// kill that follows its error. `done` waits for the bookkeeping too: a
    /// cut level 0 call is finished when its process group is gone.
    fn l0_finished(&mut self, work: usize, out: L0Out) {
        let node = self.works[work].node;
        match out {
            L0Out::Quiet => self.call_replied(work, Outcome::Quiet),
            L0Out::Answer { payload } => {
                self.call_replied(
                    work,
                    Outcome::Answer {
                        payload,
                        to: None,
                        replaces: None,
                    },
                );
            }
            L0Out::Fail { code, msg } => {
                self.fail_call(work, code, &msg);
            }
            L0Out::Overlong => {
                // The commit comes before the error (probe q2); the finished
                // event is due then the kill note, the step's last word (6.3).
                self.commit_call(work);
                self.emit_fail(work, "output", "output too large");
                self.works[work].done = true;
                self.finish_check(None);
                if let Some(l0) = self.works[work].l0 {
                    let killed = self.kill_l0_group_now(l0);
                    if killed {
                        self.emit(0, self.node_note(node, "kill"));
                    }
                }
                return;
            }
        }
        // The program is over (its output and exit arrived); with `leaves` a
        // member of the group survives, until the engine stops it silently at
        // the end of the run (5.4).
        if let Some(l0) = self.works[work].l0 {
            self.l0s[l0].leader_alive = false;
        }
        self.works[work].done = true;
        self.finish_check(None);
    }

    fn person_replied(&mut self, work: usize, reply: &Value) {
        let get = |k: &str| match reply {
            Value::Obj(fields) => fields.iter().find(|(f, _)| f == k).map(|(_, v)| v),
            _ => None,
        };
        if get("quiet").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
            self.call_replied(work, Outcome::Quiet);
        } else if let Some(Value::Str(text)) = get("answer") {
            let payload = PVal::Text(text.clone());
            self.call_replied(
                work,
                Outcome::Answer {
                    payload,
                    to: None,
                    replaces: None,
                },
            );
        } else if get("hang").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
            // The person never answers; the timeout or staleness ends the call.
        } else {
            self.harness_failure(format!("the person of work {} answers something that is not text", work));
        }
        self.works[work].done = true;
    }

    // -- Timeouts -----------------------------------------------------------

    /// The call's timeout (SPEC 6.1, 6.3): the outcome event, then the cancel;
    /// for level 0 the commit follows the cancel and term notes.
    fn fire_call_timeout(&mut self, work: usize) {
        let running = self.works.get(work).map(|w| w.running() && !w.cut).unwrap_or(false);
        if !running {
            return;
        }
        self.timeout_call(work);
        self.finish_check(None);
    }

    fn timeout_call(&mut self, work: usize) {
        let node = self.works[work].node;
        let msg = format!("timed out after {} ms", self.nodes[node].timeout);
        self.emit_fail(work, "timeout", &msg);
        match self.works[work].kind {
            WKind::Run => {
                self.emit(0, self.node_note(node, "cancel"));
                self.terminate_l0(self.works[work].l0.expect("l0"));
            }
            WKind::Human => {
                self.desk_set_status_work(work, crate::desk::Status::TimedOut);
                self.emit(0, self.node_note(node, "cancel"));
                self.commit_call(work);
                self.works[work].done = true;
            }
            WKind::Http => {
                self.emit(0, self.node_note(node, "cancel"));
                self.cancel_http(work);
                self.commit_call(work);
                self.works[work].done = true;
            }
            WKind::Serve => {
                // The cancel line; the slot is kept until the neuron replies
                // for it or the deadline passes (3.2); a commit waits for the
                // slot to free.
                self.emit(0, self.node_note(node, "cancel"));
                self.arm_on_cancel(work);
                if let Some(p) = self.proc_of(node) {
                    for slot in p.slots.iter_mut() {
                        if slot.work == work {
                            slot.cancelled = true;
                        }
                    }
                }
                let re = self.works[work].re;
                // The deadline carries the process generation: a slot
                // deadline is a decision about ONE call on ONE process (the
                // process it began on), never about the neuron's next one
                // (finding 7, 0014).
                let gen = self.proc_of(node).map(|p| p.gen).unwrap_or(0);
                self.arm(self.now + self.nodes[node].timeout, Wake::SlotDeadline { server: node, gen, re });
            }
            WKind::Composed => {
                self.emit(0, self.node_note(node, "cancel"));
                if let Some(l0) = self.works[work].l0 {
                    self.terminate_l0(l0);
                } else {
                    self.composed_cancel_inner(work);
                    self.commit_call(work);
                    self.works[work].done = true;
                }
            }
        }
    }

    /// When the call's entry carries `onCancel`, the cancel line the engine
    /// sends is answered `after` ms later (5.1); the scheduled reply is
    /// abandoned.
    fn arm_on_cancel(&mut self, work: usize) {
        if let Some(entry) = self.works[work].entry.clone() {
            if let Some((after, _)) = entry.on_cancel {
                self.arm(self.now + after, Wake::CancelReply { work });
            }
        }
    }

    // -- The within expiry of a held input ----------------------------------

    fn fire_within(&mut self, work: usize) {
        let state = self.works.get(work).map(|w| (w.st, w.done));
        if state != Some((WSt::Held, false)) {
            return;
        }
        let sig = self.works[work].sig;
        let (node, trace) = (self.works[work].node, self.works[work].trace);
        self.emit_drop(sig, node, "expired", None);
        self.works[work].done = true;
        self.remove_from_group(node, trace, work);
        self.finish_check(None);
    }
}

impl Engine {
    // -- Events -------------------------------------------------------------

    fn note_ev_extra(&self, node: usize, what: &str, line: Option<u64>, text: Option<String>) -> Value {
        let mut o = Obj::new()
            .str("type", "note")
            .int("t", self.now_t())
            .str("neuron", self.nodes[node].display.clone())
            .str("what", what);
        if let Some(line) = line {
            o = o.int("line", line as i64);
        }
        if let Some(text) = text {
            o = o.str("text", text);
        }
        o.build()
    }

    /// A note about a neuron's process: the events of the inside of a composed
    /// neuron are sealed (2.6), so the note carries its instance.
    fn node_note(&self, node: usize, what: &str) -> Value {
        self.note_ev_extra(node, what, None, None)
    }

    fn emit_node_note(&mut self, node: usize, what: &str) {
        let inst = self.nodes[node].inst;
        let ev = self.note_ev_extra(node, what, None, None);
        self.emit(inst, ev);
    }

    /// A drop (SPEC 7.2): the signal the work was for, plus neuron, reason,
    /// and the phase on a `replaced` drop.
    fn emit_drop(&mut self, sig: u64, node: usize, reason: &str, phase: Option<&str>) {
        let sigrec = self.sigs[&sig].clone();
        self.emit(
            sigrec.inst,
            self.sig_event(sig, "drop")
                .set("neuron", Value::Str(self.nodes[node].display.clone()))
                .str("reason", reason)
                .set_if("phase", phase.map(|p| Value::Str(p.to_string())))
                .set("payload", sigrec.payload.shaped())
                .build(),
        );
    }

    /// An error or, for a started effect call whose outcome cannot be known,
    /// an `unknown` with the same fields and the same code (11.3). A failure
    /// inside a composed neuron marks its call failed (2.6 step 3).
    fn emit_fail(&mut self, work: usize, code: &str, msg: &str) {
        // For an effect neuron every failure of a started call but the
        // neuron's own is `unknown` (11.3); `budget`, `start` and `neuron`
        // mean the call never ran or the neuron says its own word.
        let unknown = !matches!(code, "budget" | "start" | "neuron");
        self.emit_failure(work, code, msg, unknown);
    }

    /// A failure of a call that never started (a spawnFails, 5.2): never
    /// unknown, whatever the neuron's effect says.
    fn emit_fail_unstarted(&mut self, work: usize, code: &str, msg: &str) {
        self.emit_failure(work, code, msg, false);
    }

    fn emit_failure(&mut self, work: usize, code: &str, msg: &str, unknown: bool) {
        let node = self.works[work].node;
        let what = if self.nodes[node].effect && unknown { "unknown" } else { "error" };
        let drop_sig = self.fail_sig_of(work);
        let sigrec = self.sigs[&drop_sig].clone();
        self.emit(
            sigrec.inst,
            self.sig_event(drop_sig, what)
                .set("neuron", Value::Str(self.nodes[node].display.clone()))
                .str("message", msg)
                .str("code", code)
                .set("payload", sigrec.payload.shaped())
                .build(),
        );
        self.works[work].reported = true;
        if sigrec.inst != 0 {
            if let Some(cw) = sigrec.owner_call {
                // The wording the peer's worker logs when this instance's
                // call fails: the wire says the boundary's fixed `inner`,
                // and sealing keeps this out of the run's records.
                if self.inner_why.is_none() {
                    self.inner_why = Some(msg.to_string());
                }
                self.works[cw].fail_inner = true;
            }
        }
    }

    /// The signal an error or drop describes: for a threshold call, the
    /// crossing input (6.1, 5.4).
    fn fail_sig_of(&self, work: usize) -> u64 {
        if let Some(last) = self.works[work].inputs.last() {
            self.works[*last].sig
        } else {
            self.works[work].sig
        }
    }

    fn sig_event(&self, sig: u64, kind: &str) -> Obj {
        let rec = &self.sigs[&sig];
        let mut o = Obj::new()
            .str("type", kind)
            .int("t", self.now_t())
            .str("id", sig.to_string())
            .str("from", self.disp(rec.from))
            .set("to", Value::Arr(rec.to.iter().map(|&t| Value::Str(self.disp(t))).collect()))
            .str("trace", rec.trace.to_string());
        if let Some(rt) = rec.reply_to {
            o = o.str("replyTo", rt.to_string());
        }
        if let Some(c) = rec.cause {
            o = o.str("cause", c.to_string());
        }
        o.int("hops", rec.hops as i64)
    }

    fn emit_signal_event(&mut self, sig: u64) {
        let rec = self.sigs[&sig].clone();
        let mut ev = self.sig_event(sig, "signal").set("payload", rec.payload.shaped()).build();
        if let Some(key) = &rec.replaces {
            if let Value::Obj(fields) = &mut ev {
                fields.push(("replaces".into(), Value::Str(key.clone())));
            }
        }
        self.emit(rec.inst, ev);
    }

    fn finish_check(&mut self, excl: Option<u64>) {
        let pending: Vec<(u64, bool)> = self
            .traces
            .iter()
            .filter(|(trace, info)| !info.finished && Some(*trace) != excl)
            .filter(|(trace, _)| {
                !self.works.iter().any(|w| w.trace == *trace && !w.done)
                    && !self
                        .groups
                        .iter()
                        .any(|((node, t), list)| *t == *trace && !list.is_empty() && self.nodes[*node].inst == 0)
            })
            .map(|(trace, info)| (*trace, info.exhausted))
            .collect();
        for (trace, exhausted) in pending {
            if let Some(ix) = self.traces.iter().position(|(t, _)| *t == trace) {
                self.traces[ix].1.finished = true;
            }
            let mut o = Obj::new()
                .str("type", "finished")
                .int("t", self.now_t())
                .str("trace", trace.to_string());
            o = o.str("status", if exhausted { "exhausted" } else { "done" });
            if let Some(units) = self.outer_pool() {
                if let Some(pool) = self.pools.get(&trace) {
                    o = o.set("remaining", remaining_value(&units, pool));
                }
            }
            self.emit(0, o.build());
        }
    }

    fn disp(&self, node: usize) -> String {
        self.nodes[node].display.clone()
    }
}
// ---------------------------------------------------------------------------
// Signals, staleness, delivery, threshold, and starts
// ---------------------------------------------------------------------------

impl Engine {
    fn outer_pool(&self) -> Option<Units> {
        self.insts[0].budget.clone()
    }

    /// The trace's pool: the budget's ceilings, lowered by the injection's
    /// `budget`. A unit the injection leaves out keeps the ceiling.
    fn lowered(&self, ceiling: Units, units: Option<&Units>) -> Units {
        let mut out = ceiling;
        if let Some(units) = units {
            for (u, amt) in units {
                if let Some(entry) = out.iter_mut().find(|(bu, _)| bu == u) {
                    entry.1 = *amt;
                }
            }
        }
        out
    }

    fn injection_budget_ok(&self, units: &Units) -> Result<(), String> {
        let Some(ceiling) = self.outer_pool() else {
            return Err("the injection carries a budget but the file has none".into());
        };
        for (u, amt) in units {
            match ceiling.iter().find(|(bu, _)| bu == u) {
                None => return Err(format!("the injection names the unit '{}' which the file lacks", u)),
                Some((_, cap)) if *amt > *cap => {
                    return Err(format!("the injection raises '{}' above the ceiling of the file's budget", u))
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// Create a signal: record it, report it, then make every older signal of
    /// the same (sender, key) stale (SPEC 5.4) — its unfinished work drops in
    /// the order it was created (SPEC 6.2 rule 2a). `trace == None` makes the
    /// signal its own trace's root (an injection, a composed call's inside).
    #[allow(clippy::too_many_arguments)]
    fn new_signal(
        &mut self,
        from: usize,
        to: Vec<usize>,
        payload: PVal,
        hops: u64,
        reply_to: Option<u64>,
        cause: Option<u64>,
        replaces: Option<String>,
        trace: Option<u64>,
        inst: u32,
        owner_call: Option<usize>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let trace = trace.unwrap_or(id);
        let scope = ScopeKey(inst, owner_call.unwrap_or(0));
        self.sigs.insert(
            id,
            SigRec {
                from,
                to,
                payload,
                hops,
                reply_to,
                cause,
                replaces: replaces.clone(),
                trace,
                inst,
                owner_call,
            },
        );
        self.emit_signal_event(id);
        if let Some(key) = &replaces {
            let sender = self.disp(from);
            let list = self.stale.entry((scope, sender, key.clone())).or_default();
            let doomed: Vec<usize> = list.iter().flat_map(|e| e.works.clone()).collect();
            list.push(StaleEnt {
                sig: id,
                works: Vec::new(),
            });
            for work in doomed {
                self.cut_work(work);
            }
        }
        id
    }

    /// The (scope, sender display, key) a signal lives under, when it has a key.
    fn stale_key_of(&self, sig: u64) -> Option<(ScopeKey, String, String)> {
        let rec = &self.sigs[&sig];
        let key = rec.replaces.as_ref()?;
        Some((ScopeKey(rec.inst, rec.owner_call.unwrap_or(0)), self.disp(rec.from), key.clone()))
    }

    /// The staleness part of a signal's own step: the drops of the work the
    /// signal made stale, in the order that work was created. Called after the
    /// signal's own entry was appended.
    fn stale_and_cut(&mut self, sig: u64) {
        let Some((scope, sender, key)) = self.stale_key_of(sig) else {
            return;
        };
        let doomed: Vec<usize> = match self.stale.get(&(scope, sender, key)) {
            Some(list) => {
                let cut_len = list.len() - 1;
                list[..cut_len].iter().flat_map(|e| e.works.clone()).collect()
            }
            None => Vec::new(),
        };
        for work in doomed {
            self.cut_work(work);
        }
    }

    /// Register a work under its signal's (sender, key) entry, so a later
    /// signal of the same key cuts it.
    fn register_work(&mut self, sig: u64, work: usize) {
        let Some(skey) = self.stale_key_of(sig) else { return };
        if let Some(list) = self.stale.get_mut(&skey) {
            if let Some(ent) = list.iter_mut().find(|e| e.sig == sig) {
                ent.works.push(work);
            }
        }
    }

    /// The signal a drop or an error describes: for a threshold call, the
    /// crossing input (SPEC 5.4, 6.1).
    fn drop_sig_of(&self, work: usize) -> u64 {
        if let Some(last) = self.works[work].inputs.last() {
            self.works[*last].sig
        } else {
            self.works[work].sig
        }
    }

    /// A staleness cut (SPEC 5.4): the drop, then, for running work, the
    /// cancel. An effect call is marked, never cut (11.2). A threshold call
    /// reports one drop, for the crossing input.
    fn cut_work(&mut self, work: usize) {
        if self.works[work].done || self.works[work].cut {
            return;
        }
        match self.works[work].st {
            WSt::Held => {
                let sig = self.works[work].sig;
                let (node, trace) = (self.works[work].node, self.works[work].trace);
                self.emit_drop(sig, node, "replaced", Some("waiting"));
                self.works[work].done = true;
                self.remove_from_group(node, trace, work);
            }
            WSt::Queued => {
                let sig = self.works[work].sig;
                let node = self.works[work].node;
                self.emit_drop(sig, node, "replaced", Some("waiting"));
                self.works[work].done = true;
                if let Some(s) = self.serves.get_mut(&node) {
                    s.queue.retain(|&w| w != work);
                }
            }
            WSt::Running => {
                let node = self.works[work].node;
                if self.nodes[node].effect {
                    // No drop, no cancel: the call is marked and runs to its
                    // own end (11.2). Never `cut`: its reply is delivered and
                    // the forward alone is suppressed.
                    if self.works[work].stale_marked {
                        return; // already marked, once per call
                    }
                    self.works[work].stale_marked = true;
                    return;
                }
                let drop_sig = self.drop_sig_of(work);
                let kind = self.works[work].kind;
                self.emit_drop(drop_sig, node, "replaced", Some("running"));
                self.works[work].cut = true;
                self.emit_node_note(node, "cancel");
                match kind {
                    WKind::Run => {
                        self.terminate_l0(self.works[work].l0.expect("level 0 group"));
                    }
                    WKind::Serve => {
                        if let Some(re) = self.slot_re_of(work) {
                            if let Some(p) = self.proc_of(node) {
                                for slot in p.slots.iter_mut() {
                                    if slot.re == re {
                                        slot.cancelled = true;
                                    }
                                }
                                // The generation rides with the slot: the
                                // armed deadline belongs to this process only
                                // (finding 7, 0014).
                                let gen = p.gen;
                                let timeout = self.nodes[node].timeout;
                                self.arm(self.now + timeout, Wake::SlotDeadline { server: node, gen, re });
                            }
                        }
                        // The cancel line is answered as the entry's
                        // `onCancel` writes it, after `after` ms (5.1): the
                        // slot frees then, as with a timeout's cancel.
                        self.arm_on_cancel(work);
                        if let Some(re) = self.slot_re_of(work) {
                            self.send_cancel_line(node, re);
                        }
                    }
                    WKind::Human => {
                        // The question is withdrawn; a late answer finds the
                        // work cut and is discarded.
                        self.desk_set_status_work(work, crate::desk::Status::Withdrawn);
                        self.commit_call(work);
                        self.works[work].done = true;
                    }
                    WKind::Http => {
                        // The request's socket is shut down both ways; a late
                        // reply is discarded and frees nothing (3.3, 5.4).
                        self.cancel_http(work);
                        self.commit_call(work);
                        self.works[work].done = true;
                    }
                    WKind::Composed => {
                        // A composed instance on a peer has a level 0 entry for
                        // its cancel (0013); one planted here has none.
                        if let Some(l0) = self.works[work].l0 {
                            self.terminate_l0(l0);
                        } else {
                            self.composed_cancel_inner(work);
                            self.commit_call(work);
                            self.works[work].done = true;
                        }
                    }
                }
            }
        }
    }

    fn remove_from_group(&mut self, node: usize, trace: u64, work: usize) {
        if let Some(list) = self.groups.get_mut(&(node, trace)) {
            list.retain(|&w| w != work);
        }
    }

    // -- Delivery -----------------------------------------------------------

    /// A signal is delivered to each neuron in `to`, in order (SPEC 4.2), with
    /// the finished checks of 6.2 rules 2 and 8 around it: after the drops the
    /// signal caused (rule 2a), before its own delivery effects (the trace it
    /// carries is not finished meanwhile, 7.2), and again after the delivery.
    fn deliver_signal(&mut self, sig: u64) {
        self.stale_and_cut(sig);
        let trace = self.sigs[&sig].trace;
        self.finish_check(Some(trace));
        let targets = self.sigs[&sig].to.clone();
        for node in targets {
            self.deliver_to(node, sig);
        }
        self.finish_check(None);
    }

    fn deliver_to(&mut self, node: usize, sig: u64) {
        // The hops check comes first (SPEC 5.1).
        let hops = self.sigs[&sig].hops;
        if self.nodes[node].max_hops.map(|max| hops > max).unwrap_or(false) {
            self.emit_drop(sig, node, "hops", None);
            return;
        }
        if self.nodes[node].threshold > 1 {
            self.hold_or_cross(node, sig);
            return;
        }
        self.start_call(node, sig);
    }

    /// Threshold: inputs are grouped by (neuron, trace); a sender counts once:
    /// its newer input on the trace replaces its older held one (SPEC 5.2).
    fn hold_or_cross(&mut self, node: usize, sig: u64) {
        let trace = self.sigs[&sig].trace;
        let sender = self.sigs[&sig].from;
        let held: Vec<usize> = self.groups.get(&(node, trace)).cloned().unwrap_or_default();
        for h in held {
            if self.works[h].done || self.sigs[&self.works[h].sig].from != sender {
                continue;
            }
            if self.works[h].sig == sig {
                continue; // one signal is delivered once
            }
            let old_sig = self.works[h].sig;
            self.emit_drop(old_sig, node, "replaced", Some("waiting"));
            self.works[h].done = true;
            if let Some(group) = self.groups.get_mut(&(node, trace)) {
                group.retain(|&w| w != h);
            }
        }
        // Hold the input; its `within` wake-up is set when it is held.
        let work = self.make_call(node, sig, WSt::Held);
        self.arm(self.now + self.nodes[node].within, Wake::Within { work });
        let reached = {
            let list = self.groups.entry((node, trace)).or_default();
            list.push(work);
            list.len() as u64 >= self.nodes[node].threshold
        };
        if reached {
            self.threshold_act(node, trace);
        }
    }

    /// The threshold is met: the inputs leave the group and one call starts
    /// with all of them, in arrival order; the call answers the one that
    /// crossed (SPEC 5.2).
    fn threshold_act(&mut self, node: usize, trace: u64) {
        let inputs = self.groups.remove(&(node, trace)).unwrap_or_default();
        let crossing = *inputs.last().expect("the crossing input");
        let work = self.make_call(node, self.works[crossing].sig, WSt::Running);
        self.works[work].inputs = inputs.clone();
        for &h in &inputs {
            // The consumed inputs never expire (5.2) and report nothing.
            self.works[h].done = true;
            // Any input it was built from going stale cuts the call: the call
            // is registered under every entry its inputs are registered under.
            let sig = self.works[h].sig;
            let Some(skey) = self.stale_key_of(sig) else { continue };
            if let Some(list) = self.stale.get_mut(&skey) {
                if let Some(ent) = list.iter_mut().find(|e| e.sig == sig) {
                    ent.works.push(work);
                }
            }
        }
        self.start_new_call(work);
    }

    /// A call work: registered under its input's (sender, key) entry.
    fn make_call(&mut self, node: usize, sig: u64, st: WSt) -> usize {
        let rec = self.sigs[&sig].clone();
        let pool = if self.pools.contains_key(&rec.trace) {
            Some(rec.trace)
        } else {
            None
        };
        let work = Work {
            node,
            sig,
            trace: rec.trace,
            kind: match self.nodes[node].kind {
                Kind::Run(_) => WKind::Run,
                Kind::Serve(_) => WKind::Serve,
                Kind::Human => WKind::Human,
                Kind::Http(_) => WKind::Http,
                _ => WKind::Composed,
            },
            st,
            done: false,
            cut: false,
            inputs: Vec::new(),
            re: 0,
            l0: None,
            entry: None,
            stale_marked: false,
            reported: false,
            pool,
            reserved: false,
            committed: false,
            reserve_at: 0,
            inner_root: None,
            fail_inner: false,
            exit_replied: false,
            http: None,
            desk: None,
        };
        let ix = self.works.len();
        self.works.push(work);
        self.register_work(sig, ix);
        ix
    }
}
// ---------------------------------------------------------------------------
// Starts, replies, level 1, composed, budgets
// ---------------------------------------------------------------------------

impl Engine {
    /// A call starts (SPEC 5.1): the reserve check comes first, then the
    /// matched entry, the reserve, and the program.
    fn start_new_call(&mut self, work: usize) {
        let node = self.works[work].node;
        let kind = self.works[work].kind;
        match kind {
            WKind::Run => self.start_l0(work),
            WKind::Serve => {
                // A failed neuron refuses its inputs right away (3.2); a
                // process that is restarting keeps the call queued; a free
                // slot runs it, the rest queue in order.
                let failed = self.serves.get(&node).map(|s| s.failed).unwrap_or(false);
                if failed {
                    self.emit_fail(work, "start", "neuron failed to start");
                    self.works[work].done = true;
                    return;
                }
                if self.free_slot_of(node) {
                    self.signal_line(work);
                } else {
                    self.works[work].st = WSt::Queued;
                    if !self.serves.contains_key(&node) {
                        let s = self.new_serve(node);
                        self.serves.insert(node, s);
                    }
                    if let Some(s) = self.serves.get_mut(&node) {
                        s.queue.push(work);
                    }
                }
            }
            WKind::Human => self.start_human(work),
            WKind::Http => self.start_http(work),
            WKind::Composed => self.start_composed(work),
        }
    }

    fn start_call(&mut self, node: usize, sig: u64) {
        let work = self.make_call(node, sig, WSt::Running);
        self.start_new_call(work);
    }

    fn new_serve(&mut self, node: usize) -> Serve {
        Serve {
            timeout: self.nodes[node].timeout,
            procs: 0,
            down: true,
            failed: false,
            f: 0,
            queue: Vec::new(),
            cur: None,
            ping_n: 0,
            ping_open: None,
            grace: false,
            grace_gen: 0,
            grace_survivor: false,
            grace_gid: None,
        }
    }

    // -- Budgets (SPEC 10) --------------------------------------------------

    /// Can the pool cover every unit of the cost? If not, the reason is a
    /// `budget:` message naming the amount, the unit and what the pool has.
    fn check_cost(&mut self, work: usize) -> Result<(), String> {
        let node = self.works[work].node;
        let Some(cost) = self.nodes[node].cost.clone() else { return Ok(()) };
        let Some(trace) = self.works[work].pool else {
            return Ok(()); // the file has no budget: nothing is metered (10.7)
        };
        let Some(pool) = self.pools.get(&trace) else { return Ok(()) };
        for (u, amt) in &cost {
            let left = pool.iter().find(|(bu, _)| bu == u).map(|(_, v)| *v).unwrap_or(0);
            if left < *amt {
                return Err(format!("budget: needs {} {}, {} left", amt, u, left));
            }
        }
        Ok(())
    }

    /// Reserve the cost from the trace's pool (10.3), before anything else the
    /// call causes. A cost of 0 in every unit still reserves and commits.
    fn reserve_call(&mut self, work: usize) {
        let node = self.works[work].node;
        if self.works[work].committed {
            return;
        }
        if let Some(cost) = self.nodes[node].cost.clone() {
            if let Some(trace) = self.works[work].pool {
                if let Some(pool) = self.pools.get_mut(&trace) {
                    for (u, amt) in &cost {
                        if let Some(entry) = pool.iter_mut().find(|(bu, _)| bu == u) {
                            entry.1 -= amt;
                        }
                    }
                }
                self.works[work].reserved = true;
                self.works[work].reserve_at = self.now;
                let inst = self.nodes[node].inst;
                let mut o = Obj::new()
                    .str("type", "note")
                    .int("t", self.now_t())
                    .str("neuron", self.nodes[node].display.clone())
                    .str("what", "reserve");
                o = o.set("units", units_value(&cost));
                self.emit(inst, o.build());
            }
        }
    }

    /// The cost is committed when the program is over (10.4), just before the
    /// event that reports the outcome; a cut call's commit comes after its
    /// cancel and term and kill notes.
    fn commit_call(&mut self, work: usize) {
        if !self.works[work].reserved || self.works[work].committed {
            return;
        }
        let node = self.works[work].node;
        if let Some(cost) = self.nodes[node].cost.clone() {
            let ms = self.now.saturating_sub(self.works[work].reserve_at) as i64;
            let inst = self.nodes[node].inst;
            let o = Obj::new()
                .str("type", "note")
                .int("t", self.now_t())
                .str("neuron", self.nodes[node].display.clone())
                .str("what", "commit")
                .set("units", units_value(&cost))
                .int("ms", ms)
                .str("basis", "estimated")
                .build();
            self.emit(inst, o);
        }
        self.works[work].committed = true;
    }

    // -- Level 0 (SPEC 3.1) -------------------------------------------------

    fn start_l0(&mut self, work: usize) {
        let node = self.works[work].node;
        if let Err(why) = self.check_cost(work) {
            self.emit_fail(work, "budget", &why);
            self.mark_trace_refused(self.works[work].trace);
            self.works[work].done = true;
            return;
        }
        if self.is_real() {
            // The program starts now (10.3): the check, the reserve, the
            // spawn, and only the timeout wake-up. The outcome arrives from
            // the host, with the exit and the whole stdout read.
            self.reserve_call(work);
            let cmd = self.nodes[node].cmd.clone();
            let env = self.nodes[node].env.clone();
            let cwd = self.nodes[node].cwd.clone();
            let input = json::write(&self.input_value_of(work));
            let peer = self.nodes[node].peer;
            let l0 = self.l0s.len();
            self.l0s.push(L0Proc {
                work,
                leader_alive: true,
                survivor: false,
                ignore_term: false,
                gid: None,
                killed: false,
                remote: peer.is_some(),
            });
            self.works[work].l0 = Some(l0);
            self.works[work].st = WSt::Running;
            let started = match peer {
                // The call leaves this machine (decision 0013, P0): the peer
                // runs the same level 0 contract and classifies its outcome
                // the same way, and that outcome arrives as `L0Done`, stamped
                // by the queue like any program's.
                Some(peer) => {
                    let req = crate::peers::CallReq {
                        node: self.nodes[node].display.clone(),
                        argv: cmd.clone(),
                        cwd: cwd.clone(),
                        env: env.clone(),
                        input: input.clone(),
                        timeout: self.nodes[node].timeout,
                        graph: false,
                    };
                    let post = self.host().post_handle();
                    match self.peers.as_mut() {
                        Some(peers) => peers.start(post, l0, peer, req),
                        None => Err("no peers table for a placed neuron".into()),
                    }
                }
                None => self
                    .host()
                    .l0_begin(l0, &cmd, cwd.as_deref(), &env, input.as_bytes())
                    .map(|gid| self.l0s[l0].gid = Some(gid)),
            };
            if let Err(why) = started {
                // The program never ran: an error even for an effect neuron
                // (11.3 is for calls that started). A peer that cannot be
                // reached fails the call the same way a missing executable
                // does (Appendix C's `exit 127`).
                self.commit_call(work);
                self.emit_fail_unstarted(work, "exit", &format!("exit 127: {}", why));
                self.l0s[l0].leader_alive = false;
                self.works[work].done = true;
                self.finish_check(None);
            }
            if !self.works[work].done && (self.l0s[l0].gid.is_some() || self.l0s[l0].remote) {
                self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
            }
            return;
        }
        let prog = self.nodes[node].prog.clone();
        let entry = self.match_call_entry(&prog, work, false);
        self.works[work].entry = Some(entry.clone());
        self.reserve_call(work);
        let l0 = self.l0s.len();
        let ignore_term = entry.ignore_term;
        self.l0s.push(L0Proc {
            work,
            leader_alive: true,
            survivor: entry.leaves,
            ignore_term,
            gid: None,
            killed: false,
            // The scripted host has no wire: a call is either scripted here or
            // carried by a peer, never both.
            remote: false,
        });
        self.works[work].l0 = Some(l0);
        self.works[work].st = WSt::Running;
        // A program that cannot be started (spawnFails) fails at once (5.2).
        if let Some(spawn) = match &entry.reply {
            Value::Obj(fields) => fields.iter().find(|(f, _)| f == "spawnFails").map(|(_, v)| v.clone()),
            _ => None,
        } {
            let _ = spawn;
            self.commit_call(work);
            // The program never ran: an error even for an effect neuron
            // (11.3 is for calls that started).
            self.emit_fail_unstarted(work, "exit", "exit 127: no such executable");
            self.l0s[l0].leader_alive = false;
            self.works[work].done = true;
            return;
        }
        // The program's reply wake-up is armed before the call's timeout (6.2
        // rule 7); a `hang` never answers by itself.
        let hangs = reply_is_hang(&entry.reply);
        if !hangs {
            self.arm(self.now + entry.after, Wake::CallEnd { work });
        }
        self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
    }

    /// The level 0 group is terminated (SIGTERM): a `term` note when the group
    /// still had a live member, then the kill 2000 ms later if anything
    /// remains (6.3). On the real host the group is the OS's: a probe says
    /// who is still in it, and SIGTERM goes to the group now.
    fn terminate_l0(&mut self, l0: usize) {
        if self.is_real() {
            if self.l0s[l0].remote {
                // The peer's program is asked to stop the way a local group is
                // signalled: the request, a `term` note when the call had not
                // already reported, and the same 2 s grace before the kill
                // (6.3). The peer's confirmation is the group's death (6.2
                // rule 6).
                let node = self.works[self.l0s[l0].work].node;
                let open = self.peers.as_mut().map(|p| p.cancel(l0)).unwrap_or(false);
                if open {
                    self.emit_node_note(node, "term");
                    self.arm(self.now + host::GRACE_MS, Wake::L0Grace { proc: l0 });
                } else {
                    let work = self.l0s[l0].work;
                    self.commit_call(work);
                    self.works[work].done = true;
                }
                return;
            }
            let Some(gid) = self.l0s[l0].gid else {
                // Never spawned (a spawn failure): the call is over.
                let work = self.l0s[l0].work;
                self.commit_call(work);
                self.works[work].done = true;
                return;
            };
            let alive = host::group_alive(gid);
            if alive {
                let node = self.works[self.l0s[l0].work].node;
                self.emit_node_note(node, "term");
                host::term_group(gid);
                self.arm(self.now + host::GRACE_MS, Wake::L0Grace { proc: l0 });
            } else {
                // The program is gone: the cut call commits now (10.4).
                self.host().l0_forget(l0);
                let work = self.l0s[l0].work;
                self.commit_call(work);
                self.works[work].done = true;
            }
            return;
        }
        let alive = self.l0s[l0].leader_alive || self.l0s[l0].survivor;
        if alive {
            let node = self.works[self.l0s[l0].work].node;
            self.emit_node_note(node, "term");
        }
        if self.l0s[l0].leader_alive && !self.l0s[l0].ignore_term {
            self.l0s[l0].leader_alive = false;
        }
        let still = self.l0s[l0].leader_alive || self.l0s[l0].survivor;
        if still {
            self.arm(self.now + 2000, Wake::L0Grace { proc: l0 });
        } else {
            // The program is gone: the cut call commits now (10.4).
            let work = self.l0s[l0].work;
            self.commit_call(work);
            self.works[work].done = true;
        }
    }

    /// The SIGKILL of a level 0 group that ignored SIGTERM: the group is gone,
    /// the cut call commits (its ms include the grace). Real: the group is
    /// still alive by the OS's word; the scripted host's word is its flags.
    fn fire_l0_grace(&mut self, l0: usize) {
        let work = self.l0s[l0].work;
        if self.works[work].done {
            return;
        }
        let node = self.works[work].node;
        let killed: bool = if self.is_real() {
            if self.l0s[l0].remote {
                self.peers.as_mut().map(|p| p.abort(l0)).unwrap_or(false)
            } else {
                match self.l0s[l0].gid {
                    Some(gid) => {
                        let alive = host::group_alive(gid);
                        if alive {
                            host::kill_group(gid);
                        }
                        self.host().l0_forget(l0);
                        alive
                    }
                    None => false,
                }
            }
        } else {
            std::mem::replace(&mut self.l0s[l0].survivor, false) || std::mem::replace(&mut self.l0s[l0].leader_alive, false)
        };
        if killed {
            self.emit_node_note(node, "kill");
        }
        self.l0s[l0].leader_alive = false;
        self.commit_call(work);
        self.works[work].done = true;
        self.finish_check(None);
    }

    /// SIGKILL to a level 0 group at once (an over-long output, 6.3): `true`
    /// when the group still had a member, so the `kill` note follows the
    /// error. The script's word is its flags; the real host probes and kills.
    fn kill_l0_group_now(&mut self, l0: usize) -> bool {
        if self.is_real() {
            if self.l0s[l0].remote {
                let open = self.peers.as_mut().map(|p| p.abort(l0)).unwrap_or(false);
                self.l0s[l0].leader_alive = false;
                self.l0s[l0].killed = true;
                return open;
            }
            let Some(gid) = self.l0s[l0].gid else { return false };
            let alive = host::group_alive(gid);
            if alive {
                host::kill_group(gid);
            }
            self.host().l0_forget(l0);
            self.l0s[l0].leader_alive = false;
            self.l0s[l0].killed = true;
            return alive;
        }
        let alive = self.l0s[l0].leader_alive || self.l0s[l0].survivor;
        self.l0s[l0].leader_alive = false;
        self.l0s[l0].survivor = false;
        self.l0s[l0].killed = true;
        alive
    }

    /// A level 0 program's whole outcome arriving from the real host: the
    /// exit and the stdout head were read, the settle happened. A call whose
    /// outcome is not yet out takes it; one already reported (its timeout, an
    /// earlier error) commits only when its group is gone (10.4, 6.2 rule 6).
    /// A remote outcome that the peer never ran is a failure of a call that
    /// never started: `emit_fail_unstarted` keeps it an error even for an
    /// effect neuron, and the ambiguity of started work stays `unknown`
    /// (finding 8, 0014).
    fn host_l0_done(&mut self, l0: usize, out: host::L0Out, started: bool) {
        let Some(p) = self.l0s.get(l0) else { return };
        let work = p.work;
        let kind = self.works[work].kind;
        let still_running = self.works[work].running() && !self.works[work].cut && !self.works[work].reported;
        let gid = p.gid;
        let remote = p.remote;
        if remote {
            // The peer's call is over when its outcome arrives: the entry that
            // let a cancel reach the socket goes now (a long run must not hold
            // one per finished call).
            if let Some(peers) = self.peers.as_mut() {
                peers.forget(l0);
            }
        }
        self.l0s[l0].leader_alive = false;
        if kind == WKind::Composed {
            // A whole network answered from a peer (0013): the reply is the
            // composed neuron's own (2.6 step 2), not a program's outcome —
            // but only while the call is still eligible: one outcome per
            // call, and a late remote outcome after its timeout or cut is
            // cleanup only, like any other late program outcome (finding 4,
            // 0014).
            if still_running && !self.works[work].done {
                self.composed_remote_done(work, out);
                return;
            }
        }
        if still_running && !self.works[work].done {
            let out = l0_out_of(out);
            // The peer was never reached, the handshake was refused, or the
            // peer refused the program's own start: nothing ran there, and
            // the failure is an error whatever the effect says (11.3's
            // unstarted case).
            if let (L0Out::Fail { code, msg }, true) = (&out, remote && !started) {
                self.commit_call(work);
                self.emit_fail_unstarted(work, code, msg);
                self.works[work].done = true;
                self.finish_check(None);
                return;
            }
            self.l0_finished(work, out);
            self.finish_check(None);
            return;
        }
        if !self.works[work].done {
            let alive = gid.map(host::group_alive).unwrap_or(false);
            if !alive {
                self.host().l0_forget(l0);
                self.commit_call(work);
                self.works[work].done = true;
                self.finish_check(None);
            }
        }
    }

    /// One http request's outcome arriving from the host's queue (3.3): the
    /// response was read whole by the thread. A call whose outcome is already
    /// out (its timeout, a staleness cut) discards it: a late reply is
    /// discarded and frees nothing.
    fn host_http_done(&mut self, idx: usize, out: crate::http::HttpOut) {
        let Some(work) = self.works.iter().position(|w| w.http == Some(idx)) else {
            return;
        };
        let running = self.works[work].running() && !self.works[work].cut && !self.works[work].reported;
        if !running {
            return;
        }
        self.works[work].http = None;
        let out = match out {
            crate::http::HttpOut::Answer(v) => L0Out::Answer { payload: payload_of(v) },
            crate::http::HttpOut::Quiet => L0Out::Quiet,
            crate::http::HttpOut::OverCap => L0Out::Overlong,
            crate::http::HttpOut::Err(msg) => L0Out::Fail { code: "http", msg },
        };
        self.l0_finished(work, out);
        self.finish_check(None);
    }

    /// One of the run's desk items takes a status (answered, withdrawn, timed
    /// out), when the work carried one.
    fn desk_set_status_work(&self, work: usize, status: crate::desk::Status) {
        if let Some(id) = self.works[work].desk.as_deref() {
            if let Some(desk) = self.desk.as_ref() {
                desk.set_status(id, status);
            }
        }
    }

    // -- Humans (SPEC 3.4, CONFORMANCE 5.5) ---------------------------------

    fn start_human(&mut self, work: usize) {
        let node = self.works[work].node;
        if let Err(why) = self.check_cost(work) {
            self.emit_fail(work, "budget", &why);
            self.mark_trace_refused(self.works[work].trace);
            self.works[work].done = true;
            return;
        }
        if self.is_real() {
            // The person's question goes on the desk (Appendix D); the
            // engine's timeout and staleness end it as any call's.
            let item = self.desk_item_of(work);
            let Some(desk) = self.desk.as_ref() else {
                self.harness_failure("the run has a human neuron but no desk".into());
            };
            let id = desk.register(item);
            self.works[work].desk = Some(id);
            self.reserve_call(work);
            self.works[work].st = WSt::Running;
            self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
            return;
        }
        let prog = self.nodes[node].prog.clone();
        let entry = self.match_call_entry(&prog, work, true);
        self.works[work].entry = Some(entry.clone());
        self.reserve_call(work);
        self.works[work].st = WSt::Running;
        let hangs = reply_is_hang(&entry.reply);
        if !hangs {
            self.arm(self.now + entry.after, Wake::CallEnd { work });
        }
        self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
    }

    /// An `http` call (3.3): the check, the reserve, the request starting in
    /// its own thread, then only the timeout wake-up. The outcome arrives
    /// from the host's queue (3.3, 6.1); the timeout and staleness cancel it.
    fn start_http(&mut self, work: usize) {
        let node = self.works[work].node;
        if let Err(why) = self.check_cost(work) {
            self.emit_fail(work, "budget", &why);
            self.mark_trace_refused(self.works[work].trace);
            self.works[work].done = true;
            return;
        }
        if !self.is_real() {
            self.harness_failure("an http neuron runs only on the real host".into());
        }
        let url = match &self.nodes[node].kind {
            Kind::Http(url) => url.clone(),
            _ => unreachable!("an http call's kind"),
        };
        let timeout = self.nodes[node].timeout;
        let input = json::write(&self.input_value_of(work));
        let idx = self.https.len();
        let post = self.host().post_handle();
        self.reserve_call(work);
        self.works[work].st = WSt::Running;
        let cancel = crate::http::begin(post, idx, &url, input.as_bytes(), Duration::from_millis(timeout));
        self.https.push(cancel);
        self.works[work].http = Some(idx);
        self.arm(self.now + timeout, Wake::CallTimeout { work });
    }

    /// Shut a request's socket down both ways (a staleness cut or the
    /// call's timeout): what the thread saw after is discarded when it
    /// arrives, and frees nothing (5.4, 6.2 rule 6).
    fn cancel_http(&mut self, work: usize) {
        if let Some(idx) = self.works[work].http.take() {
            if let Some(cancel) = self.https.get(idx) {
                cancel.cancel();
            }
        }
    }

    // -- The desk (decision 0008, Appendix D) --------------------------------

    /// The desk item's shape (Appendix D): `title` and `notes` are shown,
    /// never sent; the `draft`/`proposal` pair is the diff convention; any
    /// other input is shown as its text, else its JSON. The neuron is named
    /// by its instance path (a human inside a composed instance is its own
    /// item: "outer/inner"). A host recording the run needs the ask itself:
    /// the item's `payload` is the input, whole, and `t` its run millisecond.
    fn desk_item_of(&self, work: usize) -> crate::desk::NewItem {
        let node = self.works[work].node;
        let input = self.input_value_of(work);
        let text_of = |v: Option<&Value>| v.and_then(|v| v.as_str()).map(str::to_string);
        let title = text_of(input.get("title"));
        let notes = match input.get("notes") {
            Some(Value::Arr(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<String>>(),
            _ => Vec::new(),
        };
        let (draft, proposal, text) = match (text_of(input.get("draft")), text_of(input.get("proposal"))) {
            (Some(d), Some(p)) => (Some(d), Some(p), None),
            (draft, proposal) => (draft, proposal, Some(display_text(&input))),
        };
        crate::desk::NewItem {
            neuron: self.nodes[node].prog.clone(),
            title,
            notes,
            draft,
            proposal,
            text,
            payload: input.clone(),
            t: self.now,
        }
    }

    /// The desk's accepted answers, taken and settled; each at its own now.
    fn drain_desk(&mut self) {
        while let Some(reply) = self.desk.as_ref().and_then(|d| d.try_take()) {
            self.desk_settle(reply);
        }
    }

    /// One answer the desk accepted (Appendix D): Approve sends the proposal,
    /// else the input if it is a string, else the input as JSON text; Edit
    /// sends the person's text (never empty: the desk refuses an empty one);
    /// No answers empty, which is quiet. An answer whose call was cut, or
    /// whose outcome is already out, is discarded (5.4, 4.3). The desk's POST
    /// handler reads the item's stored `payload` with the same
    /// `approve_text_of` rule, so what settled here is what `/state` reads
    /// back.
    fn desk_settle(&mut self, reply: crate::desk::Reply) {
        let Some(work) = self.works.iter().position(|w| w.desk.as_deref() == Some(reply.item.as_str())) else {
            return;
        };
        let running = self.works[work].running() && !self.works[work].cut && !self.works[work].reported;
        if !running {
            return;
        }
        let outcome = match reply.kind {
            crate::desk::Kind::Approve => {
                let input = self.input_value_of(work);
                Outcome::Answer {
                    payload: PVal::Text(crate::desk::approve_text_of(&input)),
                    to: None,
                    replaces: None,
                }
            }
            crate::desk::Kind::Edit | crate::desk::Kind::Choose => Outcome::Answer {
                payload: PVal::Text(reply.text),
                to: None,
                replaces: None,
            },
            crate::desk::Kind::No => Outcome::Quiet,
        };
        self.call_replied(work, outcome);
        self.finish_check(None);
    }

    /// The run is over (or the engine refused): the page says it is, and the
    /// desk closes.
    fn desk_close(&mut self) {
        if let Some(desk) = self.desk.take() {
            desk.mark_over();
            desk.close();
        }
    }

    /// The desk's address line, for the run's stderr: `desk: <url>`.
    pub fn desk_line(&self) -> Option<String> {
        self.desk.as_ref().map(|d| format!("desk: {}", d.url()))
    }

    /// One line per person of the grants: `desk person: {id, name, roles, url}`.
    pub fn desk_people_lines(&self) -> Vec<String> {
        self.desk.as_ref().map(|d| d.people_lines()).unwrap_or_default()
    }

    // -- The reply step (SPEC 4.3, 4.4) -------------------------------------

    /// A call that ends in a reply: the commit just before the outcome event,
    /// the outcome, its forward, then the slot of a level 1 call is freed and
    /// the queue drains. A reply of a composed neuron's `exit` completes its
    /// composed call instead (2.6 step 2): the first reply wins.
    fn call_replied(&mut self, work: usize, outcome: Outcome) {
        if let Some(cw) = self.exit_owner_of(work) {
            self.composed_exit_replied(work, cw, outcome);
            return;
        }
        let node = self.works[work].node;
        let sig = self.works[work].sig;
        let rec = self.sigs[&sig].clone();
        self.commit_call(work);
        // The call is over: its wake-ups (timeout, a late reply) are no-ops
        // from here, and its own forward can no longer cut it.
        self.works[work].done = true;
        self.works[work].reported = true;
        match outcome.clone() {
            Outcome::Fail { code, msg } => {
                self.emit_fail(work, code, &msg);
            }
            Outcome::Quiet => {
                let reply = self.reply_signal(work, sig);
                self.emit(rec.inst, self.sig_event(reply, "quiet").build());
            }
            Outcome::Answer { payload, to, replaces } => {
                let marked = self.works[work].stale_marked;
                let reply = self.reply_signal(work, sig);
                let mut ev = self.sig_event(reply, "answer").set("payload", payload.shaped()).build();
                if marked {
                    if let Value::Obj(entries) = &mut ev {
                        entries.push(("forwarded".into(), Value::Bool(false)));
                    }
                }
                // The run prints the answers of the neurons with no outgoing
                // links (SPEC 4.4); the whole payload, the trace cuts long
                // ones (TRACE.md).
                if self.nodes[node].out.is_empty() {
                    self.answers.push((self.disp(node), payload.clone()));
                }
                self.emit(rec.inst, ev);
                // The ground's deposit (4.5): the engine reads the reply's
                // payload for the named record fields — its own bookkeeping,
                // never a routing read — and a write is one `ground` event,
                // emitted immediately after its answer's event.
                self.ground_deposit(rec.inst, node, reply, &payload);
                // The forward is a signal on the same key and must not cut
                // the call that made it; the slot of a level 1 call frees
                // after the forward (6.2).
                if !marked {
                    // A stale effect answer is recorded but not forwarded
                    // (11.2); anything else is forwarded right after (4.4).
                    self.forward_answer(work, reply, &to, &replaces, payload);
                }
            }
        }
        if let Outcome::Fail { .. } = outcome {
            // nothing else
        }
        if self.works[work].kind == WKind::Serve {
            self.free_slot(work);
            self.drain_serve(node);
        }
        self.finish_check(None);
    }

    /// The reply signal: from the neuron to the input's sender (4.3).
    fn reply_signal(&mut self, work: usize, sig: u64) -> u64 {
        let rec = &self.sigs[&sig];
        let id = self.next_id;
        self.next_id += 1;
        self.sigs.insert(
            id,
            SigRec {
                from: self.works[work].node,
                to: vec![rec.from],
                payload: PVal::Json(Value::Null),
                hops: rec.hops + 1,
                reply_to: Some(sig),
                cause: None,
                replaces: None,
                trace: rec.trace,
                inst: rec.inst,
                owner_call: rec.owner_call,
            },
        );
        id
    }

    /// The forward: right after an answer that is not stale (4.4), the answer's
    /// key or the input's, along the outgoing links in declaration order (or
    /// the subset the reply names).
    fn forward_answer(&mut self, work: usize, reply: u64, to: &Option<Vec<usize>>, replaces: &KeyThree, payload: PVal) {
        let node = self.works[work].node;
        let incoming_key = self.sigs[&self.works[work].sig].replaces.clone();
        let rec = self.sigs[&reply].clone();
        let targets = match to {
            Some(subset) => {
                let named: HashSet<usize> = subset.iter().copied().collect();
                self.nodes[node].out.iter().copied().filter(|n| named.contains(n)).collect()
            }
            None => {
                // 4.4 and 4.5: an omitted `to` routes by the draw of 4.5 when
                // the network declares `select`, and along every link as at
                // v0.1 otherwise. A reply that names a `to` never draws.
                let select = self.nodes[node].inst == 0 && self.ground.as_ref().is_some_and(|g| g.select);
                if select {
                    match self.ground_draw(node) {
                        Some(link) => vec![link],
                        None => return, // an empty out-list or a total weight of 0
                    }
                } else {
                    self.nodes[node].out.clone()
                }
            }
        };
        if targets.is_empty() {
            return;
        }
        let key = match replaces {
            None => incoming_key,             // absent: inherit the input's key
            Some(None) => None,               // null: clear it
            Some(Some(s)) => Some(s.clone()), // a string: set it
        };
        let fsig = self.new_signal(
            node,
            targets,
            payload,
            rec.hops + 1,
            None,
            Some(reply),
            key,
            Some(rec.trace),
            rec.inst,
            rec.owner_call,
        );
        self.deliver_signal(fsig);
    }

    /// A reply's deposit (4.5, decision 0012): the engine checks every
    /// reply's payload for the record fields `deposit.on` (a string) and
    /// `deposit.add` (a finite number), and, with `deposit.by`, the replying
    /// neuron must be the one named — the engine's own bookkeeping, as
    /// staleness is keyed by `replaces`; nothing routes by it and nothing a
    /// program sees changes. A write ages the old value to the reply's
    /// instant before it adds, and is one `ground` event (7.2) with no
    /// signal fields, emitted right after its answer's event — `cause` is
    /// that answer's id. A reply without both fields, a `by` that is not the
    /// replying neuron, a `quiet`, or a failure: no deposit at all.
    fn ground_deposit(&mut self, inst: u32, node: usize, cause: u64, payload: &PVal) {
        if inst != 0 {
            return; // the ground is the outer network's (4.5, 2.6)
        }
        let (on, add, by) = match self.ground.as_ref().and_then(|g| g.deposit.clone()) {
            Some(d) => (d.on, d.add, d.by),
            None => return,
        };
        let fields = match payload {
            PVal::Json(Value::Obj(entries)) => entries.clone(),
            _ => return,
        };
        let place = match fields.iter().find(|(k, _)| *k == on).map(|(_, v)| v) {
            Some(Value::Str(s)) => s.clone(),
            _ => return,
        };
        let amount = match fields.iter().find(|(k, _)| *k == add).map(|(_, v)| v) {
            Some(Value::Int(n)) => *n as f64,
            Some(Value::Float(f)) if f.is_finite() => *f,
            _ => return,
        };
        if by.is_some_and(|b| b != self.nodes[node].display) {
            return;
        }
        let t = self.now_t();
        let ground = self.ground.as_mut().expect("a ground above");
        let value = match ground.places.get(&place) {
            Some(&ent) => ground.effective(ent, t) + amount,
            None => amount,
        };
        ground.places.insert(place.clone(), (value, t));
        let ev = Obj::new()
            .str("type", "ground")
            .int("t", t)
            .str("place", place.clone())
            .set("v", Value::Float(value))
            .str("cause", cause.to_string())
            .build();
        self.emit(inst, ev);
    }

    /// The draw of 4.5 (selection, opt-in): one word of the pinned
    /// generator — spent whatever the outcome, however few the links — and
    /// the link it picks: weights `max(0, base + effectiveValue)^power` from
    /// the values aged to the reply's instant, weights taken in the
    /// declaration order, the first cumulative weight above `u·total`
    /// winning, the last link when the rounding leaves none. `None` — an
    /// empty out-list, or a total weight of 0 (every trail dead) — forwards
    /// nothing, reported like a `to` of `[]` (4.4).
    fn ground_draw(&mut self, node: usize) -> Option<usize> {
        // The generator, one word (4.5): 32-bit wrap add and multiply,
        // zero-fill right shifts, the division by 2^32 unsigned.
        let word = {
            let ground = self.ground.as_mut().expect("a ground above");
            let s = ground.rng.wrapping_add(0x6D2B79F5);
            ground.rng = s;
            let t = (s ^ (s >> 15)).wrapping_mul(s | 1);
            let t = t ^ t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
            t ^ (t >> 14)
        };
        let u = word as f64 / 4294967296.0;
        let out = self.nodes[node].out.clone();
        if out.is_empty() {
            return None;
        }
        let t = self.now_t();
        let (weights, total) = {
            let ground = self.ground.as_ref().expect("a ground above");
            let mut weights = Vec::with_capacity(out.len());
            let mut total = 0.0;
            for &link in &out {
                let eff = ground
                    .places
                    .get(&self.nodes[link].display)
                    .copied()
                    .map(|ent| ground.effective(ent, t))
                    .unwrap_or(0.0);
                let w = (ground.base + eff).max(0.0).powf(ground.power);
                weights.push(w);
                total += w;
            }
            (weights, total)
        };
        if total == 0.0 {
            return None;
        }
        let bound = u * total;
        let mut cum = 0.0;
        for (i, w) in weights.iter().enumerate() {
            cum += *w;
            if cum > bound {
                return Some(out[i]);
            }
        }
        Some(out[out.len() - 1]) // the rounding left none: the last link
    }

    /// A failing call (SPEC 6.1): an error for the signal the call answers,
    /// nothing forwarded, nothing else affected.
    fn fail_call(&mut self, work: usize, code: &str, msg: &str) {
        self.commit_call(work);
        self.emit_fail(work, code, msg);
    }

    fn mark_trace_refused(&mut self, trace: u64) {
        if let Some(ix) = self.traces.iter().position(|(t, _)| *t == trace) {
            self.traces[ix].1.exhausted = true;
        }
    }
}
// ---------------------------------------------------------------------------
// Level 1 (SPEC 3.2): the engine's side of the protocol
// ---------------------------------------------------------------------------

/// The lines of one scripted reply, with the direct outcomes: nothing for a
/// `hang`, and the process's exit for a `crash`. (The real host's lines come
/// from its processes as `host::Line` events.)
fn script_lines(reply: &Value, re: u64) -> (Vec<Line>, bool, bool) {
    let mut lines: Vec<Line> = Vec::new();
    let mut crash = false;
    let mut hang = false;
    let fields = match reply {
        Value::Obj(fields) => fields.clone(),
        _ => return (lines, crash, hang),
    };
    let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v);
    if let Some(Value::Arr(also)) = get("also") {
        for item in also {
            match item {
                Value::Obj(obj) => lines.push(Line::Obj(merge_obj(obj.clone(), re))),
                Value::Str(s) => lines.push(Line::Text(s.clone())),
                _ => {}
            }
        }
    }
    if let Some(Value::Str(t)) = get("junk") {
        lines.push(Line::Text(t.clone()));
    }
    if get("overlong").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
        lines.push(Line::Overlong);
    }
    if let Some(Value::Obj(obj)) = get("send") {
        lines.push(Line::Obj(merge_obj(obj.clone(), re)));
    }
    if let Some(p) = get("answer") {
        let mut line = Obj::new().str("type", "answer").int("re", re as i64).set("payload", p.clone());
        if let Some(rv) = get("replaces") {
            // `false` scripts an explicit `null`; anything else is written as
            // given, so a bad shape is the protocol error of 3.2.
            match rv {
                Value::Str(s) => line = line.str("replaces", s.clone()),
                Value::Bool(false) => line = line.set("replaces", Value::Null),
                _ => line = line.set("replaces", rv.clone()),
            }
        }
        if let Some(Value::Arr(items)) = get("to") {
            line = line.set("to", Value::Arr(items.clone()));
        }
        lines.push(Line::Obj(line.build()));
    }
    if get("quiet").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
        lines.push(Line::Obj(Obj::new().str("type", "quiet").int("re", re as i64).build()));
    }
    if let Some(Value::Str(msg)) = get("error") {
        lines.push(Line::Obj(
            Obj::new()
                .str("type", "error")
                .int("re", re as i64)
                .str("message", msg.clone())
                .build(),
        ));
    }
    if get("crash").is_some() {
        crash = true;
    }
    if get("hang").map(|v| v == &Value::Bool(true)).unwrap_or(false) {
        hang = true;
    }
    (lines, crash, hang)
}

/// `{"re": R, ...obj}`: the object's fields, with `re` if the object leaves it
/// out. The object may override `re` (CONFORMANCE 5.2).
fn merge_obj(entries: Vec<(String, Value)>, re: u64) -> Value {
    if !entries.iter().any(|(k, _)| k == "re") {
        let mut with_re = vec![("re".to_string(), Value::Int(re as i64))];
        with_re.extend(entries);
        Value::Obj(with_re)
    } else {
        Value::Obj(entries)
    }
}

fn type_field(v: &Value) -> Option<&str> {
    v.get("type").and_then(|t| t.as_str())
}

fn is_protocol_object(v: &Value) -> bool {
    v.is_obj() && v.get("type").map(|t| t.as_str().is_some()).unwrap_or(false)
}

fn json_text(v: &Value) -> String {
    crate::json::write(v)
}

/// The trace event's payload cap (TRACE.md): a longer one is cut to its first
/// characters, marked `truncated`; the run's answers keep the whole value.
const TRACE_PAYLOAD_CAP: usize = 2000;

/// The first `n` characters of a string.
fn cut_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// A payload as the engine carries it: a JSON string is a text payload.
fn payload_of(v: Value) -> PVal {
    match v {
        Value::Str(s) => PVal::Text(s),
        other => PVal::Json(other),
    }
}

/// The desk item's display text (Appendix D): a string input is shown as its
/// text; anything else as its JSON.
fn display_text(input: &Value) -> String {
    match input {
        Value::Str(s) => s.clone(),
        other => json::write(other),
    }
}

fn trim_500(s: &str) -> String {
    s.chars().take(500).collect()
}

/// The engine's level 0 outcome of the host's reading of a process (SPEC 3.1,
/// 6.1): the message shape is the one the 6.1 table starts (`exit N:`,
/// `signal S:`, `not JSON:`), with the head's whitespace trimmed (3.1's
/// "trimmed").
fn l0_out_of(out: host::L0Out) -> L0Out {
    match out {
        host::L0Out::Overlong => L0Out::Overlong,
        host::L0Out::Quiet => L0Out::Quiet,
        host::L0Out::Answer(v) => L0Out::Answer { payload: payload_of(v) },
        host::L0Out::Junk(text) => L0Out::Fail {
            code: "junk",
            msg: format!("not JSON: {}", trim_2000(text.trim())),
        },
        host::L0Out::Exit { code, stderr } => L0Out::Fail {
            code: "exit",
            msg: format!("exit {}: {}", code, trim_500(stderr.trim())),
        },
        host::L0Out::Signal { num, stderr } => L0Out::Fail {
            code: "exit",
            msg: format!("signal {}: {}", host::signal_name(num), trim_500(stderr.trim())),
        },
        host::L0Out::Inner(msg) => L0Out::Fail { code: "inner", msg },
    }
}

fn trim_2000(s: &str) -> String {
    s.chars().take(2000).collect()
}

fn units_value(units: &Units) -> Value {
    Value::Obj(units.iter().map(|(u, amt)| (u.clone(), Value::Int(*amt))).collect())
}

fn remaining_value(ceiling: &Units, pool: &Units) -> Value {
    let mut entries = Vec::new();
    for (u, _) in ceiling {
        let amt = pool.iter().find(|(bu, _)| bu == u).map(|(_, v)| *v).unwrap_or(0);
        entries.push((u.clone(), Value::Int(amt)));
    }
    Value::Obj(entries)
}

fn backoff(f: u64) -> u64 {
    100u64.saturating_mul(1u64 << f.min(16)).min(5000)
}

fn reply_is_hang(reply: &Value) -> bool {
    matches!(reply, Value::Obj(fields) if fields.iter().any(|(f, v)| f == "hang" && v == &Value::Bool(true)))
}

impl Engine {
    // -- Level 1 slots and the queue ----------------------------------------

    fn free_slot_of(&self, node: usize) -> bool {
        match self.serves.get(&node) {
            Some(s) => match &s.cur {
                Some(p) => p.ready && (p.slots.len() as u64) < p.max_inflight,
                None => false,
            },
            None => false,
        }
    }

    fn proc_of(&mut self, node: usize) -> Option<&mut Proc> {
        self.serves.get_mut(&node).and_then(|s| s.cur.as_mut())
    }

    /// One line into a serve process's stdin (3.2): a local process's writer
    /// thread, a hosted one's wire queue — neither ever blocks the decision
    /// thread, and a process that cannot take the line loses it the same way.
    fn l1_write(&mut self, node: usize, gen: usize, line: String) {
        if self.nodes[node].peer.is_some() {
            if let Some(peers) = self.peers.as_mut() {
                peers.serve_send(node, gen, line);
            }
        } else {
            self.host().l1_send(node, gen, line);
        }
    }

    fn slot_index_of(&self, work: usize) -> Option<usize> {
        let node = self.works[work].node;
        let s = self.serves.get(&node)?;
        let p = s.cur.as_ref()?;
        p.slots.iter().position(|slot| slot.work == work)
    }

    fn slot_re_of(&self, work: usize) -> Option<u64> {
        let node = self.works[work].node;
        self.serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .and_then(|p| p.slots.iter().find(|s| s.work == work).map(|s| s.re))
    }

    fn slot_cancelled(&self, node: usize, work: usize) -> bool {
        self.serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .and_then(|p| p.slots.iter().find(|s| s.work == work))
            .map(|s| s.cancelled)
            .unwrap_or(false)
    }

    /// The level 1 call starts when its signal line is written (SPEC 10.3):
    /// the reserve check, then the entry, the reserve note, the line, and the
    /// reply and timeout wake-ups (the reply armed first, 6.2 rule 7). The
    /// real host writes the line through its writer thread and arms only the
    /// timeout: the reply arrives as a line, when the program sends it.
    fn signal_line(&mut self, work: usize) {
        let node = self.works[work].node;
        if let Err(why) = self.check_cost(work) {
            self.emit_fail(work, "budget", &why);
            self.mark_trace_refused(self.works[work].trace);
            self.works[work].done = true;
            if let Some(s) = self.serves.get_mut(&node) {
                s.queue.retain(|&w| w != work);
            }
            return;
        }
        let re;
        if self.is_real() {
            self.reserve_call(work);
            self.works[work].st = WSt::Running;
            let l1 = {
                let p = self.proc_of(node).expect("the process is up");
                let r = p.next_re;
                p.next_re += 1;
                p.slots.push(Slot {
                    re: r,
                    work,
                    cancelled: false,
                });
                (r, p.gen)
            };
            re = l1.0;
            self.works[work].re = re;
            // The envelope of 3.2: `type`, `re`, and the input as 3.1 (a
            // threshold's wrapper rides as its `payload` and `inputs` fields).
            let threshold = self.nodes[node].threshold;
            let input_value = self.input_value_of(work);
            let mut fields: Vec<(String, Value)> = vec![
                ("type".to_string(), Value::Str("signal".into())),
                ("re".to_string(), Value::Int(re as i64)),
            ];
            match &input_value {
                Value::Obj(entries) if threshold > 1 => fields.extend(entries.iter().cloned()),
                other => fields.push(("payload".to_string(), other.clone())),
            }
            let gen = l1.1;
            let line = json::write(&Value::Obj(fields));
            self.l1_write(node, gen, line);
            self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
            return;
        }
        let prog = self.nodes[node].prog.clone();
        let entry = self.match_call_entry(&prog, work, false);
        self.works[work].entry = Some(entry.clone());
        self.reserve_call(work);
        self.works[work].st = WSt::Running;
        re = {
            let p = self.proc_of(node).expect("the process is up");
            let r = p.next_re;
            p.next_re += 1;
            p.slots.push(Slot {
                re: r,
                work,
                cancelled: false,
            });
            r
        };
        self.works[work].re = re;
        let after = entry.after;
        self.arm(self.now + after, Wake::L1Reply { work });
        self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
    }

    /// The `cancel` line (3.2): the neuron SHOULD stop and MUST reply, which
    /// frees its slot. The real host sends it through the writer; the
    /// scripted host sends the line its entry's `onCancel` promises.
    fn send_cancel_line(&mut self, node: usize, re: u64) {
        if !self.is_real() {
            return;
        }
        let gen = self.proc_of(node).map(|p| p.gen).unwrap_or(0);
        if gen == 0 {
            return;
        }
        let line = json::write(&Obj::new().str("type", "cancel").int("re", re as i64).build());
        self.l1_write(node, gen, line);
    }

    /// A free slot takes the next waiting call, in the order it queued (3.2).
    fn drain_serve(&mut self, node: usize) {
        while self.free_slot_of(node) {
            let Some(s) = self.serves.get_mut(&node) else { break };
            let Some(next_index) = s.queue.iter().position(|&w| !self.works[w].done) else {
                break;
            };
            let next = s.queue.remove(next_index);
            self.signal_line(next);
        }
    }

    fn free_slot(&mut self, work: usize) {
        let node = self.works[work].node;
        if let Some(p) = self.proc_of(node) {
            p.slots.retain(|slot| slot.work != work);
        }
    }

    // -- A reply the script scheduled ----------------------------------------

    /// The wake-up armed at the signal write: the reply's lines (5.2). A
    /// `crash` is the process's exit at that instant; a `hang` never answers.
    fn fire_l1_reply(&mut self, work: usize) {
        // The process writes its scheduled lines whatever the engine did to
        // the call (a cut call's answers arrive and are discarded by their
        // `re`); a work that is done no longer has a program to answer.
        let running = self.works.get(work).map(|w| w.running()).unwrap_or(false);
        if !running {
            return;
        }
        let node = self.works[work].node;
        // A cancelled `re` writes its line all the same when the program
        // ignores the cancel: `reply_line` discards it and frees the slot
        // (3.2), which commits the cut call.
        let gen = self.proc_of(node).map(|p| p.gen).unwrap_or(0);
        let entry = self.works[work].entry.clone().expect("the call has an entry");
        let (lines, crash, hang) = script_lines(&entry.reply, self.works[work].re);
        if crash {
            // The process exits with status 1: nothing; the crash follows.
            self.process_down(node, true, false);
            return;
        }
        if hang {
            return;
        }
        for line in lines {
            self.consume_line(node, gen, line);
        }
    }

    /// The reply the script's `onCancel` promised (5.1): the reply for the
    /// cancelled `re` is discarded, but the slot frees, which commits the cut
    /// call and starts the next one in the queue.
    fn fire_cancel_reply(&mut self, work: usize) {
        let node = self.works[work].node;
        let Some(slot_ix) = self.slot_index_of(work) else { return };
        if !self.slot_index_cancelled(node, slot_ix) {
            return;
        }
        if self.works[work].done {
            return;
        }
        let Some(entry) = self.works[work].entry.clone() else { return };
        let Some((_, reply)) = entry.on_cancel else { return };
        let (lines, crash, _hang) = script_lines(&reply, self.works[work].re);
        let gen = self.proc_of(node).map(|p| p.gen).unwrap_or(0);
        if crash {
            self.process_down(node, true, false);
            return;
        }
        for line in lines {
            self.consume_line(node, gen, line);
        }
    }

    // -- One stdout line -----------------------------------------------------

    /// One line arrives at the engine: framing, junk, protocol errors, and the
    /// reply types of SPEC 3.2. On the real host every line before the
    /// process's `ready` is a first line: a valid one starts the process, and
    /// anything else is a failed start (Appendix C). A pong of a ping is the
    /// engine's own message (3.2).
    fn consume_line(&mut self, node: usize, gen: usize, line: Line) {
        let valid = self
            .serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .map(|p| p.gen == gen)
            .unwrap_or(false);
        if !valid {
            return; // a line of an older process, or no such process
        }
        // A new scope ends the process's borrow before the engine's own
        // fields are used again. (The line number counts every line, ready
        // included, from 1 per process, 3.2.)
        let line_no = {
            let p = self.proc_of(node).expect("the process is up");
            p.lines += 1;
            p.lines
        };
        let ready = self.proc_of(node).map(|p| p.ready).unwrap_or(false);
        if !ready && self.is_real() {
            match line {
                Line::Obj(v) => match self.ready_line_error(&v, 1) {
                    None => self.ready_ok(node, gen, v.get("maxInflight").and_then(|x| x.as_u64())),
                    Some(why) => self.start_failed(node, gen, why),
                },
                Line::Text(_) | Line::Overlong => {
                    self.start_failed(node, gen, "the first line is not a ready object".into());
                }
            }
            return;
        }
        match line {
            Line::Text(t) => {
                self.junk_line(node, line_no, t);
            }
            Line::Overlong => {
                self.overlong_line(node);
            }
            Line::Obj(v) => {
                if !is_protocol_object(&v) {
                    self.junk_line(node, line_no, json_text(&v));
                    return;
                }
                match type_field(&v).unwrap_or_default() {
                    "answer" | "quiet" | "error" => self.reply_line(node, v),
                    "pong" => self.pong_line(node, v),
                    _ => {} // an unknown message type is ignored
                }
            }
        }
    }

    /// A junk line (3.2): a note with the line number (from 1, ready included)
    /// and the first 2000 characters, then the failure when exactly one call
    /// is running at the neuron.
    fn junk_line(&mut self, node: usize, line_no: u64, text: String) {
        let inst = self.nodes[node].inst;
        self.emit(
            inst,
            self.note_ev_extra(node, "junk", Some(line_no), Some(text.chars().take(2000).collect())),
        );
        // "Running" counts every call that holds a slot, a cancelled one
        // included; a cancelled call is never failed. The failure's message
        // carries the line's first characters (probe p4).
        let running = self
            .serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .map(|p| p.slots.len())
            .unwrap_or(0);
        if running == 1 {
            let work = self
                .serves
                .get(&node)
                .and_then(|s| s.cur.as_ref())
                .and_then(|p| p.slots.first())
                .map(|s| s.work)
                .expect("one slot");
            if !self.slot_cancelled(node, work) && self.works[work].running() && !self.works[work].cut {
                self.commit_call(work);
                self.emit_fail(work, "junk", &format!("junk on stdout: {}", trim_2000(&text)));
                let node = self.works[work].node;
                self.free_slot(work);
                self.drain_serve(node);
                self.works[work].done = true;
                self.finish_check(None);
            }
        }
    }

    /// A line over 1 MiB (3.2): `output` with exactly one call running, else
    /// every running call fails with `crash`; the process is killed and the
    /// neuron restarted either way.
    fn overlong_line(&mut self, node: usize) {
        let slots = self
            .serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .map(|p| p.slots.clone())
            .unwrap_or_default();
        if slots.len() == 1 && !slots[0].cancelled {
            let work = slots[0].work;
            self.commit_call(work);
            self.emit_fail(work, "output", "output too large");
            self.works[work].done = true;
        } else {
            for slot in &slots {
                if slot.cancelled || !self.works[slot.work].running() || self.works[slot.work].cut {
                    continue;
                }
                self.commit_call(slot.work);
                self.emit_fail(slot.work, "crash", &self.crash_msg(node));
                self.works[slot.work].done = true;
            }
        }
        self.process_down(node, true, false);
    }

    fn crash_msg(&self, node: usize) -> String {
        let stderr = self
            .serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .and_then(|p| p.spec.stderr.clone())
            .unwrap_or_default();
        if stderr.is_empty() {
            "process failed".to_string()
        } else {
            format!("process failed: {}", trim_500(&stderr))
        }
    }

    /// An `answer`, `quiet` or `error` line: the reply for its `re`, a
    /// protocol error (6.1), or a crash when an `error` has no `re`.
    fn reply_line(&mut self, node: usize, value: Value) {
        if value.get("type").and_then(|v| v.as_str()) != Some("error") && value.get("re").and_then(|v| v.as_u64()).is_none() {
            return; // an answer or quiet without a usable re: discarded
        }
        if value.get("type").and_then(|v| v.as_str()) == Some("error") && value.get("re").is_none() {
            // Error without re: the process itself failed (crash).
            self.process_down(node, true, false);
            return;
        }
        let Some(re) = value.get("re").and_then(|v| v.as_u64()) else {
            return;
        };
        let slot = self
            .serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .and_then(|p| p.slots.iter().find(|s| s.re == re))
            .cloned();
        let Some(slot) = slot else { return }; // cancelled, finished or unknown re
        let work = slot.work;
        if slot.cancelled {
            // A reply for an re that was cancelled: discarded, but the slot
            // frees (3.2), which commits the cut call.
            self.commit_call(work);
            self.finish_check(None);
            self.works[work].done = true;
            self.free_slot(work);
            self.drain_serve(node);
            self.finish_check(None);
            return;
        }
        match value.get("type").and_then(|v| v.as_str()).unwrap_or_default() {
            "answer" => match self.validate_reply(&value, node) {
                Ok((payload, to, replaces)) => {
                    let payload = payload_of(payload);
                    self.call_replied(work, Outcome::Answer { payload, to, replaces });
                }
                Err(cause) => self.protocol_error(work, &cause),
            },
            "quiet" => self.call_replied(work, Outcome::Quiet),
            "error" => match value.get("message").and_then(|v| v.as_str()) {
                Some(msg) => self.call_replied(
                    work,
                    Outcome::Fail {
                        code: "neuron",
                        msg: msg.to_string(),
                    },
                ),
                None => self.protocol_error(work, "the error has no message"),
            },
            _ => {}
        }
    }

    fn protocol_error(&mut self, work: usize, cause: &str) {
        let node = self.works[work].node;
        self.commit_call(work);
        self.emit_fail(work, "protocol", &format!("protocol: {}", cause));
        self.free_slot(work);
        self.drain_serve(node);
        self.works[work].done = true;
        self.finish_check(None);
    }

    /// The validity of a reply: `payload` present, `replaces` a string or
    /// null of at most 256 bytes, `to` naming only outgoing links (3.2).
    /// The result's three states (5.2): absent inherits, `null` clears, and a
    /// string sets.
    fn validate_reply(&self, value: &Value, node: usize) -> Result<(Value, Option<Vec<usize>>, KeyThree), String> {
        let Some(payload) = value.get("payload") else {
            return Err("the answer has no payload".into());
        };
        let replaces: KeyThree = match value.get("replaces") {
            None => None, // inherited later, when the forward is built
            Some(Value::Null) => Some(None),
            Some(Value::Str(s)) => {
                if s.len() > 256 {
                    return Err("the replaces key is longer than 256 bytes".into());
                }
                Some(Some(s.clone()))
            }
            Some(_) => return Err("replaces is neither a string nor null".into()),
        };
        let to = match value.get("to") {
            None => None,
            Some(Value::Arr(items)) => {
                let mut named: Vec<usize> = Vec::new();
                for item in items {
                    match item.as_str() {
                        Some(id) => match self.nodes[node].out.iter().copied().find(|&t| self.nodes[t].display == id) {
                            Some(target) => {
                                if !named.contains(&target) {
                                    named.push(target);
                                }
                            }
                            None => return Err(format!("to names '{}', which is not an outgoing link", id)),
                        },
                        None => return Err("to is an array of neuron ids".into()),
                    }
                }
                Some(named)
            }
            Some(_) => return Err("to is an array of neuron ids".into()),
        };
        Ok((payload.clone(), to, replaces))
    }

    fn slot_index_cancelled(&self, node: usize, slot_ix: usize) -> bool {
        self.serves
            .get(&node)
            .and_then(|s| s.cur.as_ref())
            .and_then(|p| p.slots.get(slot_ix))
            .map(|s| s.cancelled)
            .unwrap_or(false)
    }
}
// ---------------------------------------------------------------------------
// The level 1 process lifecycle, and the composed neurons
// ---------------------------------------------------------------------------

impl Engine {
    /// Start a level 1 process: write the `hello` line, arm the start
    /// deadline, and on the scripted host arm the instant the program would
    /// say `ready` (5.3). A restart is preceded by a note (6.3). The real
    /// host spawns a process and writes `hello` to it; its ready line arrives
    /// through the stream, and only its start deadline is armed here.
    fn spawn_process(&mut self, node: usize, restart_note: bool) {
        let (timeout, gen) = {
            let s = self.serves.get_mut(&node).expect("the server");
            s.procs += 1;
            s.down = false;
            s.ping_open = None;
            (s.timeout, s.procs)
        };
        let spec = if self.is_real() {
            Value::Obj(Vec::new())
        } else {
            self.script
                .get(&self.nodes[node].prog)
                .and_then(|p| p.starts.get(gen - 1))
                .cloned()
                .unwrap_or(Value::Obj(Vec::new()))
        };
        let spec = StartSpec::from(&spec);
        if restart_note {
            self.emit_node_note(node, "restart");
        }
        let ready = spec.ready;
        {
            let s = self.serves.get_mut(&node).expect("the server");
            s.cur = Some(Proc {
                gen,
                spec,
                ready: false,
                ready_at: 0,
                alive: true,
                max_inflight: 1,
                next_re: 1,
                slots: Vec::new(),
                lines: 0,
                gid: None,
            });
        }
        if self.is_real() {
            let display = self.nodes[node].display.clone();
            let cmd = self.nodes[node].cmd.clone();
            let env = self.nodes[node].env.clone();
            let cwd = self.nodes[node].cwd.clone();
            let hello = json::write(&Obj::new().int("v", 1).str("type", "hello").str("neuron", &display).build());
            // The serve starts where the placement put it (decision 0013, P1):
            // one `serve_start` on the wire, the peer spawns and owns the
            // process for its life, and its lines and death come back through
            // the same queue a local process's do. What fails before a process
            // could exist arrives as a start failure with the wire's word.
            let spawn: Result<Option<i32>, String> = if let Some(peer) = self.nodes[node].peer {
                let req = crate::peers::ServeReq {
                    node: display.clone(),
                    hello: hello.clone(),
                    argv: cmd.clone(),
                    cwd: cwd.clone(),
                    env: env.clone(),
                    timeout: self.nodes[node].timeout,
                };
                let post = self.host().post_handle();
                match self.peers.as_mut() {
                    Some(peers) => peers.serve_start(post, node, gen, peer, req).map(|_| None),
                    None => Err("no peers table for a placed neuron".into()),
                }
            } else {
                self.host().l1_begin(node, gen, &hello, &cmd, cwd.as_deref(), &env).map(Some)
            };
            match spawn {
                Ok(gid) => {
                    if let Some(g) = gid {
                        if let Some(s) = self.serves.get_mut(&node) {
                            if let Some(p) = s.cur.as_mut() {
                                p.gid = Some(g);
                            }
                        }
                    }
                }
                Err(why) => {
                    // A start that never began: the first refuses the engine
                    // (3.2, Appendix C); a later one is a failed start — and,
                    // at five consecutive ones, the final failure of the
                    // neuron (finding 6, 0014).
                    let procs = self.serves.get(&node).map(|s| s.procs).unwrap_or(0);
                    if procs <= 1 {
                        self.refused = Some(format!(
                            "the serve program of '{}' failed its first start: {}",
                            self.nodes[node].display, why
                        ));
                    } else if let Some(s) = self.serves.get_mut(&node) {
                        s.f += 1;
                        s.down = true;
                        s.ping_open = None;
                        s.cur = None;
                        if s.f >= 5 {
                            self.serve_failed_at_limit(node);
                        } else {
                            let f = s.f;
                            self.arm(self.now + backoff(f), Wake::RestartDue { server: node });
                        }
                    }
                    return;
                }
            }
        }
        // The program writes `hello` at once. The scripted host's `ready`
        // line comes `ready` later, default 0 (the next instant); a `ready`
        // of `false` (never) arms nothing: only the start deadline waits
        // (5.3, never-ready). The ready wake-up is armed before the deadline
        // (3.2, probe p3): a ready at the deadline's instant is within
        // timeout. The real host's ready is its process's first line; no
        // wake-up invents it.
        if let Some(ready_ms) = ready {
            if !self.is_real() {
                self.arm(self.now + ready_ms, Wake::ReadyAt { server: node, gen });
            }
        }
        self.arm(self.now + timeout, Wake::StartDeadline { server: node, gen });
    }

    /// The start deadline of a process that never said `ready` (5.3): the
    /// first start refuses the engine; a later one is a failed start.
    fn fire_start_deadline(&mut self, server: usize, gen: usize) {
        let valid = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref())
            .map(|p| p.gen == gen && !p.ready)
            .unwrap_or(false);
        if !valid {
            return;
        }
        let timeout = self.serves.get(&server).map(|s| s.timeout).unwrap_or(0);
        self.start_failed(server, gen, format!("no ready within {} ms", timeout));
    }

    /// A start failed (no ready, a `ready` of the wrong shape, a spawn that
    /// never began): the first start refuses the engine (3.2, Appendix C:
    /// the engine does not restart a neuron whose first start fails); a later
    /// one is a failed start, with the backoff and the limit of 5. The real
    /// host's group is term'd at once, its grace armed if it may survive.
    /// The stderr head a serve process left (6.1's crash message): a local
    /// process's own head, or the one the worker's `down` carried for a
    /// hosted one.
    fn serve_stderr_head(&self, node: usize, gen: usize) -> String {
        if self.nodes[node].peer.is_some() {
            if let Some(peers) = &self.peers {
                return peers.serve_stderr(node, gen);
            }
        }
        match self.real.as_ref() {
            Some(host) => host.l1_stderr_head(node, gen),
            None => String::new(),
        }
    }

    /// A start failed (no ready, a `ready` of the wrong shape, a spawn that
    /// never began): the first start refuses the engine (3.2, Appendix C:
    /// the engine does not restart a neuron whose first start fails); a later
    /// one is a failed start, with the backoff and the limit of 5. The real
    /// host's group is term'd at once, its grace armed if it may survive.
    fn start_failed(&mut self, server: usize, gen: usize, reason: String) {
        let real = self.is_real();
        let procs = self.serves.get(&server).map(|s| s.procs).unwrap_or(0);
        if procs <= 1 {
            self.refused = Some(format!(
                "the serve program of '{}' failed its first start: {}",
                self.nodes[server].display, reason
            ));
            if real {
                if let Some(g) = self.serves.get(&server).and_then(|s| s.cur.as_ref()).and_then(|p| p.gid) {
                    host::kill_group(g); // silent: a refused run reports nothing
                }
            }
            return;
        }
        // A hosted serve's failed start drops its connection: the worker
        // stops the process stdin-first (3.2), and whatever of the start
        // attempt may have spawned is the peer's to end.
        if real && self.nodes[server].peer.is_some() {
            if let Some(peers) = self.peers.as_mut() {
                peers.serve_abort(server, gen);
            }
        }
        let (alive, ignore_term, survivor, gid) = if real {
            let filtered = self
                .serves
                .get(&server)
                .and_then(|s| s.cur.as_ref())
                .filter(|p| p.gen == gen)
                .map(|p| (p.alive, p.spec.ignore_term, p.spec.leaves, p.gid));
            match filtered {
                Some((a, i, s, g)) => (a, i, s, g),
                None => (false, false, false, None),
            }
        } else {
            let cur = self.serves.get(&server).and_then(|s| s.cur.as_ref()).filter(|p| p.gen == gen);
            match cur.map(|p| (p.alive, p.spec.ignore_term, p.spec.leaves)) {
                Some((a, i, s)) => (a, i, s, None),
                None => (false, false, false, None),
            }
        };
        if let Some(s) = self.serves.get_mut(&server) {
            s.f += 1;
        }
        let grace = if real {
            match gid {
                Some(g) => {
                    let is_alive = host::group_alive(g);
                    if is_alive || alive || survivor {
                        self.emit_node_note(server, "term");
                    }
                    host::term_group(g);
                    is_alive
                }
                None => {
                    if alive || survivor {
                        self.emit_node_note(server, "term");
                    }
                    false
                }
            }
        } else if alive || survivor {
            self.emit_node_note(server, "term");
            ignore_term || survivor
        } else {
            false
        };
        if grace {
            self.arm_terminate_grace(server, gen, grace);
        }
        if let Some(s) = self.serves.get_mut(&server) {
            s.down = true;
            s.ping_open = None;
            s.cur = None;
        }
        // Five consecutive failed starts: the neuron is failed and nothing
        // waits for a restart to free its queue (finding 6, 0014); before
        // the limit, the backoff schedules the next start.
        let f = self.serves.get(&server).map(|s| s.f).unwrap_or(0);
        if f >= 5 {
            self.serve_failed_at_limit(server);
        } else {
            self.arm(self.now + backoff(f), Wake::RestartDue { server });
        }
        self.finish_check(None);
    }

    /// The failed-start limit reached (3.2, SPEC: when `f` reaches 5 the
    /// neuron is failed): every input waiting for a process is failed with
    /// `start` (6.1), no restart is scheduled, and `start_new_call` refuses
    /// the neuron's later inputs. One finalizer for each failed-start path
    /// (the spawn that never began, a start that produced no ready, a death
    /// inside the limit), which before this change kept scheduling restarts
    /// past the limit for ever (finding 6, 0014).
    fn serve_failed_at_limit(&mut self, server: usize) {
        let queued: Option<Vec<usize>> = self.serves.get_mut(&server).map(|s| {
            s.failed = true;
            std::mem::take(&mut s.queue)
        });
        for queued in queued.unwrap_or_default() {
            let (done, st) = (self.works[queued].done, self.works[queued].st);
            if done || st != WSt::Queued {
                continue;
            }
            self.commit_call(queued);
            self.emit_fail(queued, "start", "neuron failed to start");
            self.works[queued].done = true;
        }
    }

    /// The first line said ready (the real host's path; the scripted host's
    /// `ready` arrives as a wake-up with its line counted there): the
    /// process's run clock starts now, its pings are armed, and whatever was
    /// queued runs (3.2).
    fn ready_ok(&mut self, server: usize, gen: usize, max: Option<u64>) {
        {
            let s = self.serves.get_mut(&server).expect("server");
            let p = s.cur.as_mut().expect("cur");
            p.ready = true;
            p.ready_at = self.now;
            p.max_inflight = max.unwrap_or(1).max(1);
        }
        self.arm(self.now + self.ping_every, Wake::PingTick { server, gen });
        self.drain_serve(server);
        self.finish_check(None);
    }

    /// An answer arrived for a ping (the real host's pong; the scripted one
    /// arrives as a wake-up): the ping is closed and the failed-start count
    /// clears (3.2). A pong for no ping of this process is ignored.
    fn pong_line(&mut self, node: usize, v: Value) {
        let Some(n) = v.get("n").and_then(|x| x.as_u64()) else { return };
        let Some(s) = self.serves.get_mut(&node) else { return };
        if s.ping_open != Some(n) {
            return;
        }
        s.ping_open = None;
        s.f = 0;
    }

    /// A level 1 process went down on the real host (Appendix C: noticed when
    /// it exits, after the settle, or when its stdout closes): a process past
    /// its ready is a crash; one before it never really started.
    fn host_l1_down(&mut self, server: usize, gen: usize, exited: bool) {
        let valid = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref())
            .map(|p| p.gen == gen)
            .unwrap_or(false);
        if !valid {
            return;
        }
        let ready = self.proc_of(server).map(|p| p.ready).unwrap_or(false);
        if ready {
            self.process_down(server, !exited, false);
        } else {
            let reason = if self.nodes[server].peer.is_some() {
                // The wire died, or the worker did: no ready crossed, and the
                // process's fate was never the driver's to see.
                "the peer's serve closed its wire before it was ready".to_string()
            } else if exited {
                "the process ended before it was ready".to_string()
            } else {
                "stdout closed before it was ready".to_string()
            };
            self.start_failed(server, gen, reason);
        }
    }

    /// The instant the program would write its first line (5.3): the `first`
    /// line, or the `ready` object built from `readyV` and `maxInflight`.
    fn fire_ready_at(&mut self, server: usize, gen: usize) {
        // The generation must still be the current one and not ready yet.
        let valid = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref())
            .map(|p| p.gen == gen && !p.ready)
            .unwrap_or(false);
        if !valid {
            return;
        }
        let (first, ready_v, max_inflight) = {
            let s = self.serves.get(&server).expect("server");
            let p = s.cur.as_ref().expect("cur");
            (p.spec.first.clone(), p.spec.ready_v, p.spec.max_inflight)
        };
        // Validation (3.2, CONFORMANCE 4.3): the first line is a valid
        // `ready` object, with `v` 1 and a `maxInflight` of at least 1.
        let reason = match &first {
            Some(line) => self.ready_line_error(line, ready_v),
            None => {
                if ready_v != 1 {
                    Some("ready.v differs from hello.v".to_string())
                } else if max_inflight.map(|mi| mi < 1).unwrap_or(false) {
                    Some("the ready's maxInflight is not an integer of at least 1".to_string())
                } else {
                    None
                }
            }
        };
        if reason.is_none() {
            // The process is ready; its run clock starts now.
            let max = max_inflight.unwrap_or(1);
            let now = self.now;
            let end_at = {
                let mut armed = None;
                if let Some(p) = self.proc_of(server) {
                    p.ready = true;
                    p.ready_at = now;
                    p.max_inflight = max;
                    p.lines += 1; // the ready line counts (from 1, per process)
                    if p.spec.end.is_some() {
                        armed = Some(p.spec.end.as_ref().map(|(ms, _)| *ms).unwrap_or(0));
                    }
                }
                armed
            };
            if let Some(ms) = end_at {
                self.arm(self.now + ms, Wake::EndAt { server, gen });
            }
            self.arm(self.now + self.ping_every, Wake::PingTick { server, gen });
            self.drain_serve(server);
            self.finish_check(None);
            return;
        }
        // A protocol error at the first line: the first start refuses the
        // engine; a later one is a failed start (3.2, Appendix C).
        self.start_failed(server, gen, reason.unwrap_or_default());
    }

    /// The kill grace of a terminated group (6.3): 2000 ms later the group is
    /// gone; the `kill` note is recorded then if someone may have survived
    /// the termination. The state lives on the server: the process record is
    /// already gone when the grace fires. The real host's group id rides
    /// with the grace; the scripted host's flags say who may have survived.
    fn arm_terminate_grace(&mut self, server: usize, gen: usize, survivor: bool) {
        let gid = if self.is_real() {
            self.serves
                .get(&server)
                .and_then(|s| s.cur.as_ref())
                .filter(|p| p.gen == gen)
                .and_then(|p| p.gid)
        } else {
            None
        };
        // On the real host the grace asks the OS who is left, so the flag
        // only says to ask; the script's flag is the script's word.
        let real = self.is_real();
        let the_survivor = if real { true } else { survivor };
        if let Some(s) = self.serves.get_mut(&server) {
            s.grace = true;
            s.grace_gen = gen;
            s.grace_survivor = the_survivor;
            s.grace_gid = gid;
        }
        self.arm(self.now + 2000, Wake::ServeGrace { server, gen });
    }

    fn ready_line_error(&self, line: &Value, ready_v: u64) -> Option<String> {
        let Some(t) = line.get("type").and_then(|v| v.as_str()) else {
            return Some("the first line is not a ready object".into());
        };
        if t != "ready" {
            return Some("the first line is not a ready object".into());
        }
        if line.get("v").and_then(|v| v.as_i64()) != Some(ready_v as i64) {
            return Some("ready.v differs from hello.v".into());
        }
        match line.get("maxInflight") {
            None => None,
            Some(x) => match x.as_u64() {
                Some(n) if n >= 1 => None,
                _ => Some("the ready's maxInflight is not an integer of at least 1".into()),
            },
        }
    }

    /// The process's scripted end (5.3): `exit` at once, `closeStdout` with the
    /// leader alive, `error` with the leader alive. All three are crashes.
    fn fire_end_at(&mut self, server: usize, gen: usize) {
        let state = self.serves.get(&server).and_then(|s| {
            s.cur
                .as_ref()
                .filter(|p| p.gen == gen)
                .map(|p| (p.ready, p.alive, p.spec.end.as_ref().map(|(_, h)| h.clone())))
        });
        let Some((ready, alive, how)) = state else { return };
        if !ready || !alive {
            return;
        }
        let alive_now = how.as_deref() != Some("exit");
        self.process_down(server, alive_now, false);
    }

    /// The SIGKILL of a level 1 group that survived SIGTERM (6.3). Real: the
    /// group still has a member by the OS's word, so the note and the SIGKILL.
    fn fire_serve_grace(&mut self, server: usize, gen: usize) {
        let (_grace, grace_survivor, grace_gid) = match self.serves.get(&server) {
            Some(s) if s.grace && s.grace_gen == gen => (true, s.grace_survivor, s.grace_gid),
            _ => return, // no grace, or one armed by an older generation
        };
        if self.is_real() {
            if grace_survivor {
                if let Some(gid) = grace_gid {
                    if host::group_alive(gid) {
                        self.emit_node_note(server, "kill");
                        host::kill_group(gid);
                    }
                }
            }
        } else if grace_survivor {
            self.emit_node_note(server, "kill");
        }
        if let Some(s) = self.serves.get_mut(&server) {
            s.grace = false;
            s.grace_survivor = false;
            s.grace_gid = None;
        }
        self.finish_check(None);
    }

    /// The process is down (a crash: exit, closeStdout, an error line without
    /// `re`, an over-long line, a ping unanswered, a cancelled slot never
    /// freed): every running call fails (code crash), the group is killed, and
    /// the neuron restarts with the backoff of SPEC 3.2. On the real host the
    /// group term and its grace follow what the OS still holds.
    fn process_down(&mut self, server: usize, leader_alive: bool, hang: bool) {
        let spec = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref())
            .map(|p| (p.spec.leaves, p.spec.ignore_term, p.gen, p.ready_at, p.slots.clone(), p.gid))
            .unwrap_or((false, false, 0, 0, Vec::new(), None));
        let (leaves, ignore_term, gen, ready_at, slots, gid) = spec;
        let crash_msg = if self.is_real() {
            let stderr = self.serve_stderr_head(server, gen);
            if stderr.trim().is_empty() {
                "process failed".to_string()
            } else {
                format!("process failed: {}", trim_500(&stderr))
            }
        } else {
            self.crash_msg(server)
        };
        // Every running call fails; the crashed call's program is over now.
        for slot in &slots {
            if !slot.cancelled {
                let work = slot.work;
                if self.works[work].running() && !self.works[work].cut {
                    self.commit_call(work);
                    self.emit_fail(work, "crash", &crash_msg);
                }
            }
        }
        // A cancelled call's slot frees when the process is gone; the cut call
        // commits then (10.4).
        for slot in &slots {
            if slot.cancelled && !self.works[slot.work].done {
                self.commit_call(slot.work);
                self.works[slot.work].done = true;
            } else if !slot.cancelled {
                self.works[slot.work].done = true;
            }
        }
        // What remains of the process group is killed with SIGTERM; the kill
        // grace follows 2000 ms later when a member remains (6.3). A hosted
        // serve has no group of the engine's own: dropping the connection is
        // the whole lever, and the worker ends the process stdin-first, then
        // term and kill (3.2, 6.3).
        if self.is_real() {
            if let Some(g) = gid {
                let alive = host::group_alive(g);
                if alive {
                    self.emit_node_note(server, "term");
                    host::term_group(g);
                }
                if alive {
                    self.arm_terminate_grace(server, gen, true);
                }
            } else if self.nodes[server].peer.is_some() {
                if let Some(peers) = self.peers.as_mut() {
                    peers.serve_abort(server, gen);
                }
            }
        } else {
            if leader_alive || leaves {
                self.emit_node_note(server, "term");
            }
            let grace = (leader_alive && ignore_term) || leaves;
            if grace {
                self.arm_terminate_grace(server, gen, grace);
            }
        }
        let now = self.now;
        // The failed-start count: a pong or a life of at least 1000 ms that
        // does not end in a hang clears it; a shorter life, or the hang
        // itself, counts the start (3.2). At the limit of five, the one
        // finalizer fails the neuron and its queue (finding 6, 0014).
        let at_limit = {
            let Some(s) = self.serves.get_mut(&server) else { return };
            if let Some(p) = s.cur.take() {
                drop(p);
            }
            s.down = true;
            s.ping_open = None;
            let life = now.saturating_sub(ready_at);
            if hang || life < 1000 {
                s.f += 1;
            } else {
                s.f = 0;
            }
            s.f >= 5
        };
        if at_limit {
            self.serve_failed_at_limit(server);
        } else {
            let f = self.serves.get(&server).map(|s| s.f).unwrap_or(0);
            self.arm(self.now + backoff(f), Wake::RestartDue { server });
        }
        self.finish_check(None);
    }

    /// The restart is due: spawn the new process — unless nothing but a kill
    /// grace keeps the run open, in which case it is skipped silently.
    fn fire_restart_due(&mut self, server: usize) {
        let (failed, down) = self.serves.get(&server).map(|s| (s.failed, s.down)).unwrap_or((true, true));
        if failed || !down {
            return;
        }
        let other_open = self.works.iter().any(|w| w.open())
            || self.groups.values().any(|l| !l.is_empty())
            || self.injections > 0
            || self.obs_pending > 0;
        if !other_open {
            return;
        }
        self.spawn_process(server, true);
    }

    // -- Pings (3.2, CONFORMANCE 4.4) ----------------------------------------

    fn fire_ping_tick(&mut self, server: usize, gen: usize) {
        let (is_gen, ready) = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref())
            .map(|p| (p.gen == gen, p.ready))
            .unwrap_or((false, false));
        if !is_gen || !ready {
            return;
        }
        // The next tick is due pingEvery after this one.
        self.arm(self.now + self.ping_every, Wake::PingTick { server, gen });
        let (busy, ping_open, timeout, ping_n) = {
            let s = self.serves.get(&server).expect("server");
            (
                s.cur.as_ref().map(|p| !p.slots.is_empty()).unwrap_or(true),
                s.ping_open,
                s.timeout,
                s.ping_n,
            )
        };
        if busy || ping_open.is_some() {
            return;
        }
        // A tick is skipped while an earlier ping is unanswered; a ping is
        // sent only when nothing is running.
        let n = ping_n + 1;
        let pong = {
            let s = self.serves.get_mut(&server).expect("server");
            s.ping_n = n;
            s.ping_open = Some(n);
            s.cur.as_ref().and_then(|p| p.spec.pong)
        };
        // `pong: false` scripts a neuron that never answers a ping: only the
        // deadline is armed (3.2), and it hangs the process. The real host
        // writes the line; the pong arrives as one (3.2).
        if self.is_real() {
            let gen = self.proc_of(server).map(|p| p.gen).unwrap_or(0);
            if gen > 0 {
                let line = json::write(&Obj::new().str("type", "ping").int("n", n as i64).build());
                self.l1_write(server, gen, line);
            }
        } else if let Some(pong) = pong {
            self.arm(self.now + pong, Wake::PongArrive { server, gen, n });
        }
        self.arm(self.now + timeout, Wake::PongDeadline { server, gen, n });
    }

    fn fire_pong_arrive(&mut self, server: usize, gen: usize, n: u64) {
        let matches = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref().map(|p| p.gen == gen && s.ping_open == Some(n)))
            .unwrap_or(false);
        if !matches {
            return;
        }
        let s = self.serves.get_mut(&server).expect("server");
        s.ping_open = None;
        s.f = 0; // a pong clears the failed count (3.2)
        if let Some(p) = s.cur.as_mut() {
            p.lines += 1; // the pong line counts
        }
    }

    /// No pong within timeout is a hang: the process is killed and restarted
    /// (3.2); a pong deadline is not work and does not keep a finished run
    /// open.
    fn fire_pong_deadline(&mut self, server: usize, gen: usize, n: u64) {
        let ok = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref().map(|p| p.gen == gen && s.ping_open == Some(n)))
            .unwrap_or(false);
        if !ok {
            return;
        }
        self.process_down(server, true, true);
    }

    /// A cancelled call's slot that the neuron never freed: the process is
    /// treated as hung (3.2, SPEC 6.2 rule 6). The deadline is exact — it
    /// belongs to one call on one process, so an old generation's armed
    /// deadline for the same `re` does nothing to the restarted process
    /// (finding 7, 0014).
    fn fire_slot_deadline(&mut self, server: usize, gen: usize, re: u64) {
        let cancelled = self
            .serves
            .get(&server)
            .and_then(|s| s.cur.as_ref())
            .filter(|p| p.gen == gen)
            .and_then(|p| p.slots.iter().find(|s| s.re == re).map(|s| s.cancelled))
            .unwrap_or(false);
        if !cancelled {
            return;
        }
        self.process_down(server, true, true);
    }

    // -- Composed neurons (SPEC 2.6, 10.5) -----------------------------------

    fn start_composed(&mut self, work: usize) {
        let node = self.works[work].node;
        if let Err(why) = self.check_cost(work) {
            self.emit_fail(work, "budget", &why);
            self.mark_trace_refused(self.works[work].trace);
            self.works[work].done = true;
            return;
        }
        if self.is_real() {
            if let Some(peer) = self.nodes[node].peer {
                // The instance lives on a peer (0013): the call is dispatched
                // whole and its exit's reply comes back as the outcome. It gets
                // a level 0 entry with no local group, as a placed program does,
                // so a cut reaches the peer through the same term/grace path.
                self.reserve_call(work);
                let sig = self.works[work].sig;
                let payload = self.sigs[&sig].payload.as_full();
                let l0 = self.l0s.len();
                self.l0s.push(L0Proc {
                    work,
                    leader_alive: true,
                    survivor: false,
                    ignore_term: false,
                    gid: None,
                    killed: false,
                    remote: true,
                });
                self.works[work].l0 = Some(l0);
                let req = crate::peers::CallReq {
                    node: self.nodes[node].display.clone(),
                    argv: Vec::new(),
                    cwd: self.nodes[node].cwd.clone(),
                    env: Vec::new(),
                    input: json::write(&payload),
                    timeout: self.nodes[node].timeout,
                    graph: true,
                };
                let post = self.host().post_handle();
                let started = match self.peers.as_mut() {
                    Some(peers) => peers.start(post, l0, peer, req),
                    None => Err("no peers table for a placed neuron".into()),
                };
                if let Err(why) = started {
                    self.commit_call(work);
                    self.emit_fail(work, "inner", &why);
                    self.l0s[l0].leader_alive = false;
                    self.works[work].done = true;
                    self.finish_check(None);
                    return;
                }
                self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
                return;
            }
        }
        self.reserve_call(work);
        // The input is delivered to `entry` as a new inner root signal: new
        // inner trace, hops 0, the same payload, the outer input's key.
        let sig = self.works[work].sig;
        let inner_inst = self.inner_inst_of[&node];
        let entry = self.insts[inner_inst as usize].entry.expect("entry");
        let rec = self.sigs[&sig].clone();
        let root = self.new_signal(
            node,
            vec![entry],
            rec.payload.clone(),
            0,
            None,
            None,
            rec.replaces.clone(),
            None,
            inner_inst,
            Some(work),
        );
        self.works[work].inner_root = Some(root);
        if let Some(units) = self.composed_pool_units(node, inner_inst) {
            self.pools.insert(root, units);
        }
        // The composed call's timeout is ordinary: armed before the inside's
        // own wake-ups (CONFORMANCE 4.1), so an inner reply due at its instant
        // loses (probe p1b).
        self.arm(self.now + self.nodes[node].timeout, Wake::CallTimeout { work });
        self.deliver_signal(root);
    }

    /// The pool of one call to the composed neuron (10.5): the units of the
    /// cost, each at the lower of the cost and the inner budget; a unit the
    /// inner budget has that the cost lacks is not in the pool.
    fn composed_pool_units(&self, node: usize, inner_inst: u32) -> Option<Units> {
        let cost = self.nodes[node].cost.clone();
        let inner_budget = self.insts[inner_inst as usize].budget.clone();
        match (cost, inner_budget) {
            (Some(c), Some(b)) => Some(
                c.iter()
                    .map(|(u, amt)| {
                        let cap = b.iter().find(|(bu, _)| bu == u).map(|(_, v)| *v);
                        (u.clone(), cap.map(|cap| cap.min(*amt)).unwrap_or(*amt))
                    })
                    .collect(),
            ),
            (Some(c), None) => Some(c),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// The node is its instance's `exit` and the call belongs to a composed
    /// call that has not been replied yet.
    fn exit_owner_of(&self, work: usize) -> Option<usize> {
        let w = &self.works[work];
        let node = w.node;
        let inst = self.nodes[node].inst as usize;
        if inst == 0 || self.insts[inst].exit != Some(node) {
            return None;
        }
        self.sigs[&w.sig].owner_call
    }

    /// The first reply of `exit` is the composed neuron's reply (2.6 step 2):
    /// the remaining inner work of the call is cancelled without reports, and
    /// the composed call commits and answers like any other.
    /// A composed call hosted by a peer answered (0013): the exit's reply is the
    /// neuron's reply (2.6 step 2); an `inner` failure is its error; a call
    /// already cut or reported takes nothing.
    fn composed_remote_done(&mut self, work: usize, out: host::L0Out) {
        if self.works[work].done || self.works[work].cut {
            // A late outcome after the cut or the timeout: the grace owns the
            // call's end (6.2 rule 6), and its report is already out.
            return;
        }
        match l0_out_of(out) {
            L0Out::Answer { payload } => self.composed_exit_replied(
                work,
                work,
                Outcome::Answer {
                    payload,
                    to: None,
                    replaces: None,
                },
            ),
            L0Out::Quiet => self.composed_exit_replied(work, work, Outcome::Quiet),
            L0Out::Fail { msg, .. } => {
                self.emit_fail(work, "inner", &msg);
                self.commit_call(work);
                self.works[work].done = true;
                self.finish_check(None);
            }
            L0Out::Overlong => {
                self.emit_fail(work, "inner", "no answer: the answer was over the wire cap");
                self.commit_call(work);
                self.works[work].done = true;
                self.finish_check(None);
            }
        }
    }

    fn composed_exit_replied(&mut self, work: usize, cw: usize, outcome: Outcome) {
        if self.works[cw].exit_replied {
            // A later reply of exit is discarded; the slot frees like any
            // other (3.2).
            let node = self.works[work].node;
            self.free_slot(work);
            self.drain_serve(node);
            self.works[work].done = true;
            return;
        }
        self.works[cw].exit_replied = true;
        self.composed_cancel_inner(cw);
        let node = self.works[work].node;
        self.free_slot(work);
        self.drain_serve(node);
        self.works[work].done = true;
        match outcome {
            Outcome::Fail { code, msg } => {
                // An inner failure: the composed call's error comes when the
                // inside has nothing left for the call. The exit's own
                // wording rides with the worker's diagnostic (the wire says
                // the boundary's fixed `inner`; sealing holds).
                let _ = code;
                if self.inner_why.is_none() {
                    self.inner_why = Some(msg.clone());
                }
                self.works[cw].fail_inner = true;
            }
            Outcome::Quiet => self.call_replied(cw, Outcome::Quiet),
            Outcome::Answer { payload, to, replaces } => {
                let _ = to;
                self.call_replied(
                    cw,
                    Outcome::Answer {
                        payload,
                        to: None,
                        replaces,
                    },
                );
            }
        }
    }

    fn composed_cancel_inner(&mut self, work: usize) {
        let Some(root) = self.works[work].inner_root else { return };
        let doomed: Vec<usize> = self
            .works
            .iter()
            .enumerate()
            .filter(|(_, w)| w.trace == root && w.open())
            .map(|(ix, _)| ix)
            .collect();
        for w in doomed {
            // Each open item goes through the same cut an outer staleness cut
            // gives it (5.4): the level 0 process signalled, the serve slot
            // cancelled until its neuron answers or the slot deadline passes,
            // the human withdrawn, the http socket shut down. The inside's
            // reports are sealed (2.6: cancelled without reports — `emit`
            // drops every inner event), and each work stays open until its
            // process or slot actually finishes (6.2 rule 6). A composed call
            // in the inside cascades to its own inside too (2.6 step 4) by
            // the same path (finding 2, 0014).
            self.cut_work(w);
        }
    }

    /// After each wake-up: does some composed call now have an inside with
    /// nothing running, queued or held and no reply from `exit`? (2.6 step 3)
    fn wake_composed_calls(&mut self) {
        let calls: Vec<usize> = self
            .works
            .iter()
            .enumerate()
            .filter(|(_, w)| w.kind == WKind::Composed && w.open() && !w.exit_replied)
            .map(|(ix, _)| ix)
            .collect();
        for cw in calls {
            let Some(root) = self.works[cw].inner_root else { continue };
            let inner_open = self.works.iter().any(|w| w.trace == root && w.open())
                || self.groups.iter().any(|((_, t), list)| *t == root && !list.is_empty());
            if inner_open {
                continue;
            }
            self.works[cw].exit_replied = true;
            if self.works[cw].fail_inner {
                self.call_replied(
                    cw,
                    Outcome::Fail {
                        code: "inner",
                        msg: "no answer: an inner neuron failed".to_string(),
                    },
                );
            } else {
                self.call_replied(cw, Outcome::Quiet);
            }
        }
    }
}
// ---------------------------------------------------------------------------
// One call to a hosted graph (finding 3, 0014)
// ---------------------------------------------------------------------------

impl Engine {
    /// A plant-time refusal (a file this route cannot host), for a caller
    /// that drives many calls against one engine: the refusal is about the
    /// instance, so it repeats for every call.
    pub(crate) fn refusal(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// One call to a peer-hosted graph (finding 3, 0014). The engine was
    /// planted once for the worker process, the hosted file sitting behind
    /// one composed boundary of instance 0 (`boundary`, the id peers.rs gave
    /// it), so a call arrives the way a composed node's call arrives here: a
    /// signal at the boundary starts the call, `start_composed` delivers the
    /// payload at the file's `entry` as a new inner root (new trace, hops 0,
    /// its own pool, 2.6 step 1), and the engine's own machinery finishes
    /// the call — the first reply of the file's `exit` is the boundary's
    /// reply and the remaining inner work is cancelled through the composed
    /// cut (`composed_exit_replied`, `composed_cancel_inner`, 2.6 step 2),
    /// an inside with nothing left is quiet, or `inner` after an inner
    /// failure (step 3, via `wake_composed_calls` — the loop gives it the
    /// same check after every wake-up the run's loop does), and a timeout of
    /// the call cuts it like any call's timeout (step 4). The engine
    /// persists: its serve programs and their state are shared by the
    /// instance's calls, isolated per call by the fresh root (2.6 step 4).
    ///
    /// `cancelled` is the wire's word: the connection's reader holds what a
    /// cancel saw, and this loop polls it one step at a time — the closure
    /// decides by THIS call's id, so a cancel held for another call never
    /// cuts this one. When it fires, the call is cut the way any staleness
    /// cut is (`cut_work`, 5.4) and the caller answers what the wire needs.
    pub fn hosted_call(&mut self, boundary: &str, payload: Value, timeout_ms: u64, cancelled: &dyn Fn() -> bool) -> GraphOut {
        let Some(node) = self
            .nodes
            .iter()
            .position(|n| n.inst == 0 && matches!(n.kind, Kind::Network(_)) && n.display == boundary)
        else {
            return GraphOut::Failed(Some(format!("no composed boundary '{boundary}' in the graph the worker hosts")));
        };
        if let Some(why) = self.refusal().map(|s| s.to_string()) {
            return GraphOut::Failed(Some(why));
        }
        // The call is one root, and the events of the calls before it are
        // their own: this call's harvest starts here (the caller's next
        // answer must not see a predecessor's leftovers).
        self.inner_why = None;
        self.events.clear();
        let sig = self.new_signal(node, vec![node], payload_of(payload), 0, None, None, None, None, 0, None);
        let work = self.make_call(node, sig, WSt::Running);
        // The call's timeout is the wire's: `start_composed` arms it with
        // the boundary's timeout (6.2), so the cut cancels the inside at it.
        self.nodes[node].timeout = timeout_ms;
        self.start_new_call(work);
        while !self.works[work].done {
            if cancelled() {
                // A cancel for this call is on the wire (the reader saw it
                // and the id is this call's): the cut a staleness cut would
                // give the call (5.4, 2.6 step 4). The reply is the caller's
                // cut machinery's own; the wire carries a stopped call.
                self.cut_work(work);
                return GraphOut::Cancelled;
            }
            if self.deadline_due() {
                // The worker's backstop (the caller's budget is the driver's
                // timeout plus its own): cut the call as its timeout would
                // (2.6 step 4) and say the run stopped here.
                self.cut_work(work);
                return GraphOut::TimedOut;
            }
            let next = self.next_wake();
            let cap = match (next, self.deadline) {
                (Some(t), Some(d)) => Some(t.min(d)),
                (t, d) => t.or(d),
            };
            if self.host_step(cap) {
                self.wake_composed_calls();
                continue;
            }
            if self.deadline_due() {
                self.cut_work(work);
                return GraphOut::TimedOut;
            }
            self.now = next.unwrap_or_else(|| self.now + 65_536);
            let Some(w) = self.pop() else {
                // The heap is empty and nothing is due: only a process
                // outcome can close the call. The pump waits to the deadline
                // when one is set, and a step at a time when none is.
                if self.host_step(self.deadline) {
                    self.wake_composed_calls();
                    continue;
                }
                if self.deadline_due() {
                    self.cut_work(work);
                    return GraphOut::TimedOut;
                }
                // The next event ends this instant; the loop's own checks
                // are the deadlines that matter.
                continue;
            };
            self.fire(w);
            self.wake_composed_calls();
        }
        // The call is done of itself. The boundary's answer is the inside's
        // first exit reply (2.6 step 2) — its own entry in `answers` (the
        // boundary is a sink by construction: the wrapper plants no links).
        // A quiet inside pushed nothing (step 3); an inner failure's fixed
        // message is the wire's word and its cause stays on stderr (sealing).
        let answers = std::mem::take(&mut self.answers);
        if answers.last().map(|(who, _)| who.as_str()) == Some(boundary) {
            return GraphOut::Answer(answers.last().expect("the boundary's answer").1.as_full());
        }
        if self.works[work].fail_inner {
            return GraphOut::Failed(self.inner_why.take());
        }
        if self.works[work].reported {
            // The call's own timeout (the machinery's cut): the caller
            // reported the timeout at the same instant.
            return GraphOut::TimedOut;
        }
        GraphOut::Quiet
    }
}
// ---------------------------------------------------------------------------
// Remaining glue: the input value of a call, staleness registration, and the
// composed exit hook
// ---------------------------------------------------------------------------

impl Engine {
    /// The input value a program sees for a call (SPEC 3.1): the bare payload,
    /// or `{"payload", "inputs"}` for a threshold above 1. For a threshold call
    /// the group's inputs travel in arrival order.
    fn input_value_of(&self, work: usize) -> Value {
        let w = &self.works[work];
        if self.nodes[w.node].threshold > 1 {
            let crossing = if w.inputs.is_empty() {
                w.sig
            } else {
                self.works[*w.inputs.last().expect("crossing")].sig
            };
            let inputs: Vec<Value> = if w.inputs.is_empty() {
                vec![self.sigs[&w.sig].payload.as_json_raw()]
            } else {
                w.inputs
                    .iter()
                    .map(|h| self.sigs[&self.works[*h].sig].payload.as_json_raw())
                    .collect()
            };
            let payload = self.sigs[&crossing].payload.as_json_raw();
            return Value::Obj(vec![("payload".into(), payload), ("inputs".into(), Value::Arr(inputs))]);
        }
        self.sigs[&w.sig].payload.as_json_raw()
    }

    /// The first unused entry whose `when` deeply equals the input value
    /// (CONFORMANCE 5.1); a call no entry matches is a harness failure.
    fn match_call_entry(&mut self, prog: &str, work: usize, person: bool) -> CallEntry {
        let value = self.input_value_of(work);
        let program = if person { self.person.get(prog) } else { self.script.get(prog) };
        let Some(program) = program else {
            let who = if person { "person" } else { "program" };
            self.harness_failure(format!("the script has no {} for '{}'", who, prog));
        };
        let calls = program.calls.clone();
        let key = prog.to_string();
        let count = calls.len();
        let uses = self.used.entry(key).or_insert_with(|| vec![0; count]);
        let picked = calls
            .iter()
            .enumerate()
            .find(|(i, e)| uses[*i] == 0 && (e.when.is_none() || e.when.as_ref().unwrap().deep_eq(&value)))
            .map(|(i, _)| i);
        let Some(i) = picked else {
            self.harness_failure(format!(
                "no {} script entry matches the call of '{}' (input {})",
                if person { "person" } else { "call" },
                prog,
                crate::json::write(&value)
            ));
        };
        uses[i] = 1;
        calls[i].clone()
    }
}
