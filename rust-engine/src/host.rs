// The real host: processes, pipes, process groups and signals.
//
// LEVEL 0 (SPEC 3.1): one process per call, in its own process group; the
// engine writes one JSON value to stdin and closes it; stdout is read with the
// 1 MiB cap; the call settles 100 ms after the process exits, even if a child
// still holds stdout open (decision 0001, Appendix C); an over-long output
// fails the call, and the group is killed at once.
//
// LEVEL 1 (SPEC 3.2): one long-lived process per neuron. A writer thread sends
// the engine's lines, so a process that never reads its stdin holds nobody up;
// one reader per stream forwards lines, stderr bytes and the process's exit
// into one queue with the time they were read; the engine waits on that queue
// up to its next wake-up. A serve process is noticed as down when it exits
// (after a 100 ms grace, so its last lines are read) or closes stdout.
//
// Everything is the standard library plus `kill`, `pipe`, `read`, `signal` and
// `write` (extern "C", POSIX): each child goes in its own process group at
// spawn (`process_group(0)`, stable on darwin and linux), and a group is
// signalled with `kill(-pgid, sig)`.
use crate::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// 1 MiB: a level 0 output or a level 1 line over this fails (SPEC 3.1, 3.2).
pub const CAP: usize = 1_048_576;
/// A call settles this long after its process exits (decision 0001).
pub const SETTLE_MS: u64 = 100;
/// SIGTERM to a group, then SIGKILL 2 s later (SPEC 3.1, 3.2, 6.3).
pub const GRACE_MS: u64 = 2000;
/// The stderr head kept for crash messages (SPEC 6.1 wants the first 500).
pub const STDERR_KEEP: usize = 4096;
/// How long a stream pipe's full buffer gets before its reader is cut off.
pub const STREAM_GRACE_MS: u64 = 1000;

#[derive(Debug)]
pub enum L0Out {
    /// stdout carried more than 1 MiB.
    Overlong,
    /// stdout carried one JSON value (whitespace around it allowed).
    Answer(json::Value),
    /// stdout was empty or whitespace only.
    Quiet,
    /// stdout was something else; its head, for the `not JSON:` message.
    Junk(String),
    /// non-zero exit: the code and the head of stderr.
    Exit { code: i32, stderr: String },
    /// killed by a signal: its number and the head of stderr.
    Signal { num: i32, stderr: String },
    /// A call carried by a peer whose graph failed as a whole: the composed
    /// neuron's `inner` code (6.1), with the inner cause as its message. Only a
    /// peer's outcome produces this; a local program never does.
    Inner(String),
}

#[derive(Debug)]
pub enum Line {
    Obj(json::Value),
    Text(String),
    Overlong,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum Ev {
    /// A level 0 program produced its whole outcome. `started` says whether
    /// the call ever began where it ran: a remote outcome that could not
    /// reach its worker, or was refused by it before anything could run,
    /// arrives with `false` (the driver's `run` marks it, 11.3's unstarted
    /// case; finding 8, 0014). A local program always ran.
    L0Done { l0: usize, out: L0Out, started: bool },
    /// A level 1 process wrote one line.
    L1Line { server: usize, gen: usize, line: Line },
    /// A level 1 process's stdout closed (`exited: false`) or the process
    /// exited (`exited: true`, after the settle).
    L1Down { server: usize, gen: usize, exited: bool },
    /// A serve start the worker could not even take (decision 0013, P1):
    /// unreachable, a handshake mode it does not speak, a spawn it rejected.
    /// The start failure carries the reason the wire gave, so the engine
    /// refuses or restarts for the same reasons a local start would — never
    /// for silence (the start deadline is the last word of a peer that says
    /// nothing at all, finding 5, 0014).
    ServeStartFailed { server: usize, gen: usize, why: String },
    /// An http request finished (SPEC 3.3): the engine finds the call by `idx`.
    HttpDone { idx: usize, out: crate::http::HttpOut },
    /// A tick from a side surface (the desk accepted an answer): the engine
    /// reads it when it takes the event.
    Desk,
    /// A live producer's injection (SPEC 4.1, decision 0020): injected when
    /// the engine takes it, at the stamp it was read with.
    Inject(Box<crate::fixture::Input>),
    /// The live producer is done: no injection is still to come (6.2 rule 6).
    InputsClosed,
}

pub struct HostMsg {
    pub t: u64,
    pub ev: Ev,
}

struct Shared {
    q: Mutex<Vec<HostMsg>>,
    cv: Condvar,
}

// -- The host clock ----------------------------------------------------------

static HOST_START: OnceLock<Instant> = OnceLock::new();

/// Whole milliseconds since the host was made: the engine's real clock. Every
/// reader thread stamps the events it pushes with this, so an event's time is
/// when it was read, not when the engine got to it.
pub fn clock_ms() -> u64 {
    let start = HOST_START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
}

pub fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

// -- Signals (the same POSIX numbers on darwin and linux) --------------------

const SIGINT: i32 = 2;
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;

/// The name of a signal a child died from (SPEC 6.1: the name, never the
/// number). The signals a platform numbers differently are configured here
/// (USR1, USR2, CHLD, CONT, STOP, TSTP, CLD, PROF, WINCH, POLL); the rest are
/// the POSIX numbers on every platform of this engine.
pub fn signal_name(num: i32) -> String {
    #[cfg(target_vendor = "apple")]
    let table: &[(i32, &str)] = &[
        (1, "SIGHUP"),
        (2, "SIGINT"),
        (3, "SIGQUIT"),
        (6, "SIGABRT"),
        (9, "SIGKILL"),
        (13, "SIGPIPE"),
        (14, "SIGALRM"),
        (15, "SIGTERM"),
        (20, "SIGCHLD"),
        (23, "SIGURG"),
        (28, "SIGWINCH"),
        (30, "SIGUSR1"),
        (31, "SIGUSR2"),
    ];
    #[cfg(not(target_vendor = "apple"))]
    let table: &[(i32, &str)] = &[
        (1, "SIGHUP"),
        (2, "SIGINT"),
        (3, "SIGQUIT"),
        (6, "SIGABRT"),
        (9, "SIGKILL"),
        (13, "SIGPIPE"),
        (14, "SIGALRM"),
        (15, "SIGTERM"),
        (10, "SIGUSR1"),
        (12, "SIGUSR2"),
        (16, "SIGSTKFLT"),
        (17, "SIGCHLD"),
        (18, "SIGCONT"),
        (23, "SIGURG"),
        (28, "SIGWINCH"),
    ];
    match table.iter().find(|(n, _)| *n == num) {
        Some((_, name)) => name.to_string(),
        None => format!("SIG{num}"),
    }
}

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn pipe(fds: *mut i32) -> i32;
    fn read(fd: i32, buf: *mut core::ffi::c_void, n: usize) -> isize;
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    fn write(fd: i32, buf: *const core::ffi::c_void, n: usize) -> isize;
}

/// Does the process group still have a member? A probe with signal 0 (POSIX).
/// A leader the engine has not reaped yet still counts, and a leftover zombie
/// grandchild can hold the answer true for a moment; both are honest readings
/// of "a live member" for an engine whose monitors reap what they see.
pub fn group_alive(gid: i32) -> bool {
    gid > 0 && unsafe { kill(-gid, 0) == 0 }
}

pub fn term_group(gid: i32) {
    if gid > 0 {
        unsafe { kill(-gid, SIGTERM) };
    }
}

pub fn kill_group(gid: i32) {
    if gid > 0 {
        unsafe { kill(-gid, SIGKILL) };
    }
}

// -- The live-group registry: the CLI kills everything on SIGINT/SIGTERM -----

static LIVE: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn register_group(gid: i32) {
    if let Ok(mut list) = LIVE.lock() {
        list.push(gid);
    }
}

fn unregister_group(gid: i32) {
    if let Ok(mut list) = LIVE.lock() {
        list.retain(|g| *g != gid);
    }
}

fn registered_groups() -> Vec<i32> {
    LIVE.lock().map(|l| l.clone()).unwrap_or_default()
}

/// Every live group the process still knows of, killed at once: the MCP
/// server's stdin-EOF exit uses it (a client gone mid-run leaves no programs
/// behind, and the polite `end_run` discipline needs a process to keep
/// waiting — there is none after the exit). A group already reaped is a
/// no-op, the way `kill_group` reads dead ids.
pub fn kill_live_groups() {
    for gid in registered_groups() {
        kill_group(gid);
    }
}

// -- The self-pipe signal teardown (the CLI's SIGINT/SIGTERM, decision 0001) --

static SIGNAL_WRITE: AtomicI32 = AtomicI32::new(-1);

/// One handler for SIGINT and SIGTERM: it writes a byte into a pipe (all it
/// may do safely); a thread then kills every registered group and leaves, so
/// an interrupted run leaves no programs behind.
pub fn install_signal_teardown() {
    let mut fds = [0i32; 2];
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
        return; // without a pipe, the default disposition ends us cleanly
    }
    SIGNAL_WRITE.store(fds[1], Ordering::SeqCst);
    extern "C" fn handler(_sig: i32) {
        let fd = SIGNAL_WRITE.load(Ordering::SeqCst);
        if fd >= 0 {
            unsafe { write(fd, b"x".as_ptr() as *const core::ffi::c_void, 1) };
        }
    }
    unsafe {
        signal(SIGINT, handler);
        signal(SIGTERM, handler);
    }
    thread::Builder::new()
        .name("signal-teardown".into())
        .spawn(move || {
            let mut buf = [0u8; 1];
            let n = unsafe { read(fds[0], buf.as_mut_ptr() as *mut core::ffi::c_void, 1) };
            if n < 1 {
                return;
            }
            for gid in registered_groups() {
                kill_group(gid);
            }
            std::process::exit(130);
        })
        .ok();
}

// -- Spawning ----------------------------------------------------------------

/// One program: argument 0 on `PATH` unless it contains `/` (then relative to
/// the program's working directory), a minimal environment (`PATH` and only
/// the neuron's named variables, a name the engine lacks stays unset), the
/// working directory, and its own process group (SPEC 2.3, 2.4, 3.1).
fn spawn(cmd: &[String], cwd: Option<&Path>, env_names: &[String]) -> std::io::Result<std::process::Child> {
    let Some(name) = cmd.first() else {
        return Err(std::io::Error::other("an empty command"));
    };
    // A relative executable is relative to the program's working directory
    // (2.3); on darwin the spawn's own chdir happens around the exec, so the
    // engine resolves it against the cwd itself and passes an absolute path.
    let absolute = match cwd {
        Some(dir) if !Path::new(name).is_absolute() && name.contains('/') => dir.join(name),
        _ => Path::new(name).to_path_buf(),
    };
    let mut c = std::process::Command::new(absolute);
    c.args(&cmd[1..]);
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    c.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        c.env("PATH", path);
    }
    for env_name in env_names {
        if let Some(value) = std::env::var_os(env_name) {
            c.env(env_name, value);
        }
    }
    c.stdin(std::process::Stdio::piped());
    c.stdout(std::process::Stdio::piped());
    c.stderr(std::process::Stdio::piped());
    c.process_group(0);
    c.spawn()
}

/// The level 0 outcome of a status, the stdout bytes and an over-long mark:
/// an over-long output first, then a signal, then the code, then what stdout
/// was (SPEC 3.1, 6.1). A failure carries the stderr head; the engine trims
/// its message to 500 characters.
pub fn outcome_of(status: &std::process::ExitStatus, data: &[u8], over: bool, stderr: &str) -> L0Out {
    if over {
        return L0Out::Overlong;
    }
    if let Some(num) = status.signal() {
        return L0Out::Signal {
            num,
            stderr: stderr.to_string(),
        };
    }
    let code = status.code().unwrap_or(1);
    if code != 0 {
        return L0Out::Exit {
            code,
            stderr: stderr.to_string(),
        };
    }
    if data.iter().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
        return L0Out::Quiet;
    }
    match json::parse(&String::from_utf8_lossy(data)) {
        Ok(v) => L0Out::Answer(v),
        Err(_) => {
            let text: String = String::from_utf8_lossy(data).chars().take(2000).collect();
            L0Out::Junk(text)
        }
    }
}

// -- A stream's shared head (a cap, a stopped flag) --------------------------

struct Head {
    data: Vec<u8>,
    over: bool,
    stop: bool,
}

fn new_head() -> Arc<Mutex<Head>> {
    Arc::new(Mutex::new(Head {
        data: Vec::new(),
        over: false,
        stop: false,
    }))
}

// -- The host ----------------------------------------------------------------

/// The process layer under the engine's loop: it starts every process, reads
/// every stream, and delivers what happened with the time it was read.
pub struct Host {
    shared: Arc<Shared>,
    /// Level 1 processes by (node, generation): the writer of stdin, the head
    /// of stderr, and whether the process has exited (the monitor's word).
    serves: HashMap<(usize, usize), Serve>,
    /// The process group of each level 0 call, by the engine's `l0` index.
    l0s: HashMap<usize, i32>,
}

struct Serve {
    tx: Option<std::sync::mpsc::Sender<WriteMsg>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    exited: Arc<AtomicBool>,
}

enum WriteMsg {
    Line(String),
    Close,
}

fn push_ev(shared: &Arc<Shared>, ev: Ev) {
    if let Ok(mut q) = shared.q.lock() {
        q.push(HostMsg { t: clock_ms(), ev });
        shared.cv.notify_all();
    }
}

/// A handle a side thread (an http request, the desk) holds: it posts events
/// into the engine's queue, stamped when they were posted, and wakes the
/// engine's wait.
#[derive(Clone)]
pub struct Post {
    shared: Arc<Shared>,
}

impl Post {
    /// Post one event now; the engine reads it at its next pump.
    pub fn post(&self, ev: Ev) {
        push_ev(&self.shared, ev);
    }
}

/// A named thread, so a panic names its work; the return is dropped (an http
/// request's thread, the desk's, are joined by nothing: the process's exit
/// ends them).
pub fn spawn_thread(name: &str, f: impl FnOnce() + Send + 'static) {
    thread::Builder::new().name(name.into()).spawn(f).ok();
}

/// One line, plus its newline, to a process's stdin, if the pipe is open:
/// `true` when the line is safely written and flushed.
fn write_stdin(stdin: &mut Option<std::process::ChildStdin>, bytes: &[u8]) -> bool {
    let Some(f) = stdin.as_mut() else { return true }; // closed: the lines are lost, not errors
    let ok = f.write_all(bytes).and_then(|_| f.write_all(b"\n")).and_then(|_| f.flush()).is_ok();
    if !ok {
        *stdin = None;
    }
    ok
}

/// One level 1 line, framed and read: a JSON object with a string `type` is a
/// protocol object; anything else is other text (junk at the engine, SPEC 3.2).
fn push_line(shared: &Arc<Shared>, node: usize, gen: usize, bytes: &[u8]) {
    let line = match bytes.last() {
        Some(b'\r') => &bytes[..bytes.len() - 1],
        _ => bytes,
    };
    if line.is_empty() {
        return; // a blank line is ignored
    }
    let text = String::from_utf8_lossy(line).to_string();
    match json::parse(&text) {
        Ok(v) => {
            let protocol = v.is_obj() && v.get("type").and_then(|t| t.as_str()).is_some();
            let ev_line = if protocol { Line::Obj(v) } else { Line::Text(text) };
            push_ev(
                shared,
                Ev::L1Line {
                    server: node,
                    gen,
                    line: ev_line,
                },
            );
        }
        Err(_) => push_ev(
            shared,
            Ev::L1Line {
                server: node,
                gen,
                line: Line::Text(text),
            },
        ),
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    /// A handle for side threads (the http client, the desk): they post their
    /// events into this engine's queue.
    pub fn post_handle(&self) -> Post {
        Post {
            shared: self.shared.clone(),
        }
    }

    pub fn new() -> Host {
        HOST_START.get_or_init(Instant::now);
        Host {
            shared: Arc::new(Shared {
                q: Mutex::new(Vec::new()),
                cv: Condvar::new(),
            }),
            serves: HashMap::new(),
            l0s: HashMap::new(),
        }
    }

    /// The next event: one already queued whose time is at or before the
    /// absolute due time `t`, else a wait for one until `t`. `None` means the
    /// wake-up at `t` is due and nothing arrived by then; a `t` of `None`
    /// waits until one event arrives, however long.
    pub fn pump(&self, t: Option<u64>) -> Option<HostMsg> {
        let mut q = self.shared.q.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if !q.is_empty() {
                // An event stamped before the wake's instant goes first; one
                // stamped at it is a tie with the call's own wake-ups, and
                // rule 7 gives those (timeout included, armed at the call's
                // start) the instant: strict.
                if t.map(|t| q[0].t < t).unwrap_or(true) {
                    return Some(q.remove(0));
                }
                return None; // a later event: the wake-up at `t` goes first
            }
            let Some(t) = t else {
                q = self.shared.cv.wait(q).unwrap_or_else(|e| e.into_inner());
                continue;
            };
            let now = clock_ms();
            if now >= t {
                return None;
            }
            let (guard, timeout) = self
                .shared
                .cv
                .wait_timeout(q, Duration::from_millis(t - now))
                .unwrap_or_else(|e| e.into_inner());
            q = guard;
            if timeout.timed_out() {
                // A last look: an event may have been pushed while its
                // notification raced the timeout; the stamp decides, and a
                // tie belongs to the wake-up.
                if !q.is_empty() && q[0].t < t {
                    return Some(q.remove(0));
                }
                return None;
            }
        }
    }

    // -- Level 0 (SPEC 3.1) --------------------------------------------------

    /// Start one level 0 program for the call the engine indexes at `l0`:
    /// spawn it in its own group, write the input to stdin and close it in a
    /// thread of its own (a program that never reads a long input holds
    /// nobody up), read stdout with the 1 MiB cap, and settle 100 ms after
    /// the process exits before the whole outcome is delivered (decision
    /// 0001). `Ok(gid)` is the process group; a spawn failure is the error
    /// text (an `exit 127:` at once, Appendix C).
    pub fn l0_begin(&mut self, l0: usize, cmd: &[String], cwd: Option<&Path>, env_names: &[String], input: &[u8]) -> Result<i32, String> {
        let mut child = spawn(cmd, cwd, env_names).map_err(|e| e.to_string())?;
        let gid = child.id() as i32;
        register_group(gid);
        self.l0s.insert(l0, gid);
        let stdin = child.stdin.take().expect("piped stdin");
        let input = input.to_vec();
        thread::Builder::new()
            .name("l0-stdin".into())
            .spawn(move || {
                let mut stdin = stdin;
                let _ = stdin.write_all(&input);
                drop(stdin); // closing ends the one JSON value
            })
            .ok();
        let stdout = child.stdout.take().expect("piped stdout");
        let head = new_head();
        let head_reader = head.clone();
        let reader_events = self.shared.clone();
        thread::Builder::new()
            .name("l0-read".into())
            .spawn(move || {
                let mut f = stdout;
                let mut buf = [0u8; 8192];
                let head = head_reader;
                let over_reported = AtomicBool::new(false);
                loop {
                    let stopped = head.lock().map(|h| h.stop).unwrap_or(true);
                    if stopped {
                        break;
                    }
                    match f.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut h = head.lock().unwrap_or_else(|e| e.into_inner());
                            if h.over {
                                drop(h);
                                continue; // the tail is dropped; only the drain runs
                            }
                            if h.data.len() + n > CAP {
                                let room = CAP - h.data.len();
                                h.data.extend_from_slice(&buf[..room.min(n)]);
                                h.over = true;
                                drop(h);
                                // The over-long cut reports at once: the
                                // engine kills the group now, whatever the
                                // process does after it wrote (6.3, 3.1).
                                if !over_reported.swap(true, Ordering::SeqCst) {
                                    push_ev(
                                        &reader_events,
                                        Ev::L0Done {
                                            l0,
                                            out: L0Out::Overlong,
                                            started: true,
                                        },
                                    );
                                }
                            } else {
                                h.data.extend_from_slice(&buf[..n]);
                            }
                        }
                    }
                }
            })
            .ok();
        let stderrf = child.stderr.take().expect("piped stderr");
        let stderr_head = Arc::new(Mutex::new(Vec::new()));
        let stderr_reader = stderr_head.clone();
        thread::Builder::new()
            .name("l0-stderr".into())
            .spawn(move || {
                let mut f = stderrf;
                let mut buf = [0u8; 8192];
                let head = stderr_reader;
                let mut room = STDERR_KEEP;
                while let Ok(n) = f.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if room == 0 {
                        continue; // logs nobody reads again
                    }
                    if let Ok(mut h) = head.lock() {
                        h.extend_from_slice(&buf[..n.min(room)]);
                    }
                    room = room.saturating_sub(n);
                }
            })
            .ok();
        let monitor = self.shared.clone();
        thread::Builder::new()
            .name("l0-wait".into())
            .spawn(move || {
                let mut child = child;
                let status = child.wait();
                sleep_ms(SETTLE_MS);
                if let Ok(mut h) = head.lock() {
                    h.stop = true;
                }
                let (data, over) = match head.lock() {
                    Ok(h) => (h.data.clone(), h.over),
                    Err(_) => (Vec::new(), false),
                };
                let stderr = match stderr_head.lock() {
                    Ok(h) => String::from_utf8_lossy(&h).to_string(),
                    Err(_) => String::new(),
                };
                let out = match status {
                    Ok(st) => outcome_of(&st, &data, over, &stderr),
                    Err(e) => L0Out::Exit {
                        code: 127,
                        stderr: e.to_string(),
                    },
                };
                unregister_group(gid);
                push_ev(&monitor, Ev::L0Done { l0, out, started: true });
            })
            .ok();
        Ok(gid)
    }

    pub fn l0_gid(&self, l0: usize) -> Option<i32> {
        self.l0s.get(&l0).copied()
    }

    /// The group of a level 0 call matters again only if the engine signals
    /// it; the monitor thread owns the child and stays until the exit.
    pub fn l0_forget(&mut self, l0: usize) {
        self.l0s.remove(&l0);
    }

    // -- Level 1 (SPEC 3.2) --------------------------------------------------

    /// Start one long-lived process: spawn it in its own group, write the
    /// `hello` line, and read its stdout as protocol lines and its stderr as
    /// logs. `Ok(gid)` is the process group; a spawn failure the reason.
    pub fn l1_begin(
        &mut self,
        node: usize,
        gen: usize,
        hello: &str,
        cmd: &[String],
        cwd: Option<&Path>,
        env_names: &[String],
    ) -> Result<i32, String> {
        let mut child = spawn(cmd, cwd, env_names).map_err(|e| e.to_string())?;
        let gid = child.id() as i32;
        register_group(gid);
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderrf = child.stderr.take().expect("piped stderr");
        let (tx, rx) = std::sync::mpsc::channel::<WriteMsg>();
        let stderr_head: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let exited = Arc::new(AtomicBool::new(false));
        self.serves.insert(
            (node, gen),
            Serve {
                tx: Some(tx),
                stderr: stderr_head.clone(),
                exited: exited.clone(),
            },
        );
        let exited_reader = exited.clone();
        let hello = hello.to_string();
        thread::Builder::new()
            .name("l1-write".into())
            .spawn(move || {
                let mut stdin = Some(stdin);
                let _ = write_stdin(&mut stdin, hello.as_bytes());
                for m in rx {
                    match m {
                        WriteMsg::Line(line) => {
                            if !write_stdin(&mut stdin, line.as_bytes()) {
                                // A broken pipe (the process closed or died): the
                                // engine's steps stay right; these lines are lost
                                // to a process that cannot read.
                                stdin = None;
                            }
                        }
                        WriteMsg::Close => {
                            // Closing stdin is the shutdown (SPEC 3.2).
                            stdin.take();
                        }
                    }
                }
            })
            .ok();
        let reader = self.shared.clone();
        thread::Builder::new()
            .name("l1-read".into())
            .spawn(move || {
                let mut f = stdout;
                let mut buf = [0u8; 8192];
                let mut line: Vec<u8> = Vec::new();
                let mut drain = false; // after an over-long line: skip to the newline
                loop {
                    match f.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            for &b in &buf[..n] {
                                if drain {
                                    if b == b'\n' {
                                        drain = false;
                                        line.clear();
                                    }
                                    continue;
                                }
                                if b == b'\n' {
                                    push_line(&reader, node, gen, &line);
                                    line.clear();
                                    continue;
                                }
                                if line.len() >= CAP {
                                    // The line is past the cap with no end in
                                    // sight: one over-long line marks it, and
                                    // the tail's bytes are dropped.
                                    push_ev(
                                        &reader,
                                        Ev::L1Line {
                                            server: node,
                                            gen,
                                            line: Line::Overlong,
                                        },
                                    );
                                    drain = true;
                                    line.clear();
                                    continue;
                                }
                                line.push(b);
                            }
                        }
                    }
                }
                if !line.is_empty() {
                    push_line(&reader, node, gen, &line);
                }
                // The stdout is closed; the process may still live (a helper
                // holding the stream open). The engine hears it after a
                // settle, but only when the monitor has not already reported
                // the exit: its event carries the settled truth.
                for _ in 0..(SETTLE_MS / 10) {
                    sleep_ms(10);
                    if exited_reader.load(Ordering::SeqCst) {
                        return;
                    }
                }
                push_ev(
                    &reader,
                    Ev::L1Down {
                        server: node,
                        gen,
                        exited: false,
                    },
                );
            })
            .ok();
        thread::Builder::new()
            .name("l1-stderr".into())
            .spawn(move || {
                let mut f = stderrf;
                let mut buf = [0u8; 8192];
                let head = stderr_head;
                let mut room = STDERR_KEEP;
                while let Ok(n) = f.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if room == 0 {
                        continue;
                    }
                    if let Ok(mut h) = head.lock() {
                        h.extend_from_slice(&buf[..n.min(room)]);
                    }
                    room = room.saturating_sub(n);
                }
            })
            .ok();
        let monitor = self.shared.clone();
        thread::Builder::new()
            .name("l1-wait".into())
            .spawn(move || {
                let mut child = child;
                let _ = child.wait();
                exited.store(true, Ordering::SeqCst);
                sleep_ms(SETTLE_MS);
                unregister_group(gid);
                push_ev(
                    &monitor,
                    Ev::L1Down {
                        server: node,
                        gen,
                        exited: true,
                    },
                );
            })
            .ok();
        Ok(gid)
    }

    /// Write one line to a level 1 process's stdin: never a blocking step.
    pub fn l1_send(&mut self, node: usize, gen: usize, line: String) {
        if let Some(s) = self.serves.get(&(node, gen)) {
            if let Some(tx) = &s.tx {
                let _ = tx.send(WriteMsg::Line(line));
            }
        }
    }

    /// The `shutdown` of SPEC 3.2: stdin is closed; the process exits.
    pub fn l1_close(&mut self, node: usize, gen: usize) {
        if let Some(s) = self.serves.get_mut(&(node, gen)) {
            if let Some(tx) = s.tx.take() {
                let _ = tx.send(WriteMsg::Close);
            }
        }
    }

    /// The head of stderr a level 1 process has written, for a crash message.
    pub fn l1_stderr_head(&self, node: usize, gen: usize) -> String {
        match self.serves.get(&(node, gen)) {
            Some(s) => {
                let buf = s.stderr.lock().unwrap_or_else(|e| e.into_inner());
                String::from_utf8_lossy(&buf).to_string()
            }
            None => String::new(),
        }
    }

    /// Has the process of (node, gen) already exited, by the monitor's word?
    /// The engine reads it to know who is still of the process group.
    pub fn l1_exited(&self, node: usize, gen: usize) -> bool {
        self.serves
            .get(&(node, gen))
            .map(|s| s.exited.load(Ordering::SeqCst))
            .unwrap_or(true)
    }

    /// The end of a run (Appendix C): stdin closed (by the caller); then, for
    /// every process still of a group, SIGTERM after 2 s and SIGKILL 2 s
    /// later, silently; the host returns after the processes are gone.
    pub fn end_run(&mut self, serve_gids: &[i32]) {
        // The shutdown of 3.2 works like this: the stdin is closed and the
        // processes have the 2 s to end on their own; what stays after that
        // gets the term, and what stays after 2 s more, the kill.
        let mut alive: Vec<i32> = serve_gids.iter().copied().filter(|g| group_alive(*g)).collect();
        let deadline = clock_ms() + GRACE_MS;
        while clock_ms() < deadline && !alive.is_empty() {
            sleep_ms(50);
            alive = alive.iter().copied().filter(|g| group_alive(*g)).collect();
        }
        if !alive.is_empty() {
            for gid in &alive {
                term_group(*gid);
            }
        }
        // The kill 2 s later for whatever remains (a group member may ignore
        // the term; the leader ends with the rest of the group).
        let deadline = clock_ms() + GRACE_MS;
        alive.retain(|gid| group_alive(*gid));
        while clock_ms() < deadline && !alive.is_empty() {
            sleep_ms(50);
            alive = alive.iter().copied().filter(|g| group_alive(*g)).collect();
        }
        for gid in &alive {
            kill_group(*gid);
        }
        // A SIGKILL group is gone almost at once; a bounded wait keeps the
        // promise that the run returns after the processes are gone.
        for _ in 0..50 {
            alive.retain(|gid| group_alive(*gid));
            if alive.is_empty() {
                break;
            }
            sleep_ms(100);
        }
    }

    /// A run the engine refused before it started the work: term everything
    /// at once (no notes: nothing was reported), kill what stays.
    pub fn end_refused(&mut self, gids: &[i32]) {
        let mine: Vec<i32> = gids.iter().copied().filter(|g| group_alive(*g)).collect();
        for gid in &mine {
            term_group(*gid);
        }
        sleep_ms(500);
        for gid in &mine {
            if group_alive(*gid) {
                kill_group(*gid);
            }
        }
        sleep_ms(200);
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// The real host's level 0 round trip on one call of `echo`, asserted so
    /// a spawning failure names itself here and not hidden in a run.
    #[test]
    fn l0_echo_round_trip() {
        let input = b"{\"n\": 1}";
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let cases: Vec<(&str, Vec<String>)> = vec![
            (
                "no cwd",
                vec!["/bin/sh".into(), "-c".into(), "read -r x; echo \"{\\\"o\\\": $x}\"".into()],
            ),
            ("with cwd", vec!["/bin/sh".into(), "-c".into(), "echo '{\"o\": \"cwd-ok\"}'".into()]),
        ];
        for (what, cmd) in cases {
            let mut h = Host::new();
            let with_dir = what.starts_with("with");
            let cwd = if with_dir {
                Some(std::path::PathBuf::from(dir.clone()))
            } else {
                None
            };
            let gid = h.l0_begin(0, &cmd, cwd.as_deref(), &[], input).expect("the call starts");
            let _ = gid;
            let Some(m) = h.pump(Some(clock_ms() + 5000)) else {
                panic!("{}: no outcome in 5 s", what);
            };
            match m.ev {
                Ev::L0Done { l0: 0, out, .. } => match out {
                    L0Out::Answer(_) => {
                        // (the answer's shape is the probe's own business)
                    }
                    L0Out::Junk(text) => panic!("{}: junk: {}", what, text),
                    L0Out::Exit { code, stderr } => panic!("{}: exit {}: {}", what, code, stderr),
                    L0Out::Signal { num, stderr } => panic!("{}: signal {}: {}", what, num, stderr),
                    L0Out::Overlong => panic!("{}: overlong", what),
                    other => panic!("{}: quiet {:?}", what, other),
                },
                other => panic!("wrong ev: {:?}", other),
            }
        }
    }
}
