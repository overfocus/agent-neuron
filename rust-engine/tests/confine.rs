// Confinement (SPEC 2.4, decision 0018, S2a), tested end to end through the
// engine binary on real processes: a program under `run --confine` is held to
// its neuron's `may` and to nothing more than the baseline.
//
// Every program here is an inline `/bin/sh -c` script that uses only system
// tools, so the tests need no interpreter tree and name no path of this
// machine. Each prints one JSON string saying what it managed to do.
#![cfg(target_os = "macos")]

use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "neuron-confine-{}-{}-{}",
            tag,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        Temp { dir }
    }
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
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

fn json_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A source and one neuron per entry: (id, kind, the sh script, the other
/// options as JSON members, such as `"may": {...}`).
fn network(entries: &[(&str, &str, &str, &str)]) -> String {
    let mut neurons = String::from("{\"id\": \"src\", \"source\": true}");
    let mut links = Vec::new();
    for (id, kind, script, opts) in entries {
        let opts = if opts.is_empty() { String::new() } else { format!(", {}", opts) };
        neurons.push_str(&format!(
            ",\n    {{\"id\": \"{}\", \"{}\": [\"/bin/sh\", \"-c\", {}]{}}}",
            id,
            kind,
            json_str(script),
            opts
        ));
        links.push(format!("{{\"from\": \"src\", \"to\": \"{}\"}}", id));
    }
    format!(
        "{{\"version\": 0, \"name\": \"confined\", \"neurons\": [\n    {}\n  ], \"links\": [{}]}}\n",
        neurons,
        links.join(", ")
    )
}

struct Run {
    out: Output,
}

impl Run {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out.stdout).to_string()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.out.stderr).to_string()
    }
}

/// One run of `net` with one injection, confined by a file with `tools`
/// (`None`: no `--confine` at all).
fn run(temp: &Temp, net: &str, tools: Option<&[&Path]>, extra: &[&str]) -> Run {
    let net_path = temp.write("net.json", net);
    let inputs = temp.path("inputs.jsonl");
    let mut f = std::fs::File::create(&inputs).expect("inputs");
    writeln!(f, "{{\"at\": 0, \"source\": \"src\", \"payload\": 1}}").expect("inputs line");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_neuron-engine"));
    cmd.arg("run").arg(&net_path).arg("--inputs").arg(&inputs);
    if let Some(tools) = tools {
        let list: Vec<String> = tools.iter().map(|t| json_str(&t.to_string_lossy())).collect();
        let confine = temp.write("confine.json", &format!("{{\"version\": 0, \"tools\": [{}]}}", list.join(", ")));
        cmd.arg("--confine").arg(confine);
    }
    for a in extra {
        cmd.arg(a);
    }
    cmd.current_dir(&temp.dir);
    Run {
        out: cmd.output().expect("the engine runs"),
    }
}

const READ_SECRET: &str = "if /bin/cat secret.txt >/dev/null 2>&1; then echo '\"read\"'; else echo '\"denied\"'; fi";

#[test]
fn without_the_flag_nothing_is_confined() {
    let temp = Temp::new("off");
    temp.write("secret.txt", "s");
    let r = run(&temp, &network(&[("a", "run", READ_SECRET, "")]), None, &[]);
    assert_eq!(r.stdout().trim(), "read", "{}", r.stderr());
}

#[test]
fn a_neuron_with_no_may_cannot_read_its_own_directory() {
    let temp = Temp::new("nomay");
    temp.write("secret.txt", "s");
    let r = run(&temp, &network(&[("a", "run", READ_SECRET, "")]), Some(&[]), &[]);
    assert_eq!(r.stdout().trim(), "denied", "{}", r.stderr());
}

/// The baseline shows a program the names in its working directory (it has to
/// be able to learn where it is) and none of their contents, nor anything deeper.
#[test]
fn a_neuron_with_no_may_sees_names_in_its_directory_and_no_contents() {
    let temp = Temp::new("names");
    temp.write("secret.txt", "s");
    temp.write("sub/inner.txt", "i");
    let script = "names=no; deeper=no; contents=no; /bin/ls | /usr/bin/grep -q secret.txt && names=yes; /bin/ls sub >/dev/null 2>&1 && deeper=yes; /bin/cat secret.txt >/dev/null 2>&1 && contents=yes; echo \"\\\"names:$names deeper:$deeper contents:$contents\\\"\"";
    let r = run(&temp, &network(&[("a", "run", script, "")]), Some(&[]), &[]);
    assert_eq!(r.stdout().trim(), "names:yes deeper:no contents:no", "{}", r.stderr());
}

#[test]
fn read_grants_the_tree_it_names_and_no_other() {
    let temp = Temp::new("read");
    temp.write("data/deep/a.txt", "a");
    temp.write("other/b.txt", "b");
    let script = "a=no; b=no; /bin/cat data/deep/a.txt >/dev/null 2>&1 && a=yes; /bin/cat other/b.txt >/dev/null 2>&1 && b=yes; echo \"\\\"a:$a b:$b\\\"\"";
    let r = run(
        &temp,
        &network(&[("a", "run", script, "\"may\": {\"read\": [\"data\"]}")]),
        Some(&[]),
        &[],
    );
    assert_eq!(r.stdout().trim(), "a:yes b:no", "{}", r.stderr());
}

#[test]
fn write_grants_a_path_that_does_not_exist_yet_and_does_not_grant_reading_it() {
    let temp = Temp::new("write");
    let script = "w=no; s=no; r=no; /bin/mkdir out 2>/dev/null; { echo x > out/w.txt; } 2>/dev/null && w=yes; { echo y > stray.txt; } 2>/dev/null && s=yes; /bin/cat out/w.txt >/dev/null 2>&1 && r=yes; echo \"\\\"wrote:$w stray:$s readback:$r\\\"\"";
    let r = run(
        &temp,
        &network(&[("a", "run", script, "\"may\": {\"write\": [\"out\"]}")]),
        Some(&[]),
        &[],
    );
    assert_eq!(r.stdout().trim(), "wrote:yes stray:no readback:no", "{}", r.stderr());
    assert!(temp.path("out/w.txt").is_file(), "the granted write happened");
    assert!(!temp.path("stray.txt").exists(), "the stray write did not");
}

#[test]
fn a_neuron_with_no_may_cannot_write_anywhere() {
    let temp = Temp::new("nowrite");
    let script = "{ echo y > stray.txt; } 2>/dev/null && echo '\"wrote\"' || echo '\"denied\"'";
    let r = run(&temp, &network(&[("a", "run", script, "")]), Some(&[]), &[]);
    assert_eq!(r.stdout().trim(), "denied", "{}", r.stderr());
    assert!(!temp.path("stray.txt").exists());
}

#[test]
fn a_child_process_is_held_too() {
    let temp = Temp::new("child");
    temp.write("secret.txt", "s");
    let script = "/bin/sh -c '/bin/cat secret.txt' >/dev/null 2>&1 && echo '\"read\"' || echo '\"denied\"'";
    let r = run(&temp, &network(&[("a", "run", script, "")]), Some(&[]), &[]);
    assert_eq!(r.stdout().trim(), "denied", "{}", r.stderr());
}

/// The baseline lets a program signal what it started: without it a confined
/// program cannot stop its own child.
#[test]
fn a_confined_program_can_stop_its_own_child() {
    let temp = Temp::new("signal");
    let script = "/bin/sleep 30 & pid=$!; if kill $pid 2>/dev/null; then echo '\"stopped\"'; else echo '\"could not\"'; fi";
    let r = run(&temp, &network(&[("a", "run", script, "")]), Some(&[]), &[]);
    assert_eq!(r.stdout().trim(), "stopped", "{}", r.stderr());
}

#[test]
fn the_tools_of_the_confine_file_are_readable_by_every_program() {
    let temp = Temp::new("tools");
    let tools = Temp::new("tools-tree");
    tools.write("lib/x.txt", "x");
    let script = format!(
        "/bin/cat {} >/dev/null 2>&1 && echo '\"read\"' || echo '\"denied\"'",
        tools.path("lib/x.txt").to_string_lossy()
    );
    let net = network(&[("a", "run", &script, "")]);
    let held = run(&temp, &net, Some(&[]), &[]);
    assert_eq!(held.stdout().trim(), "denied", "{}", held.stderr());
    let granted = run(&temp, &net, Some(&[tools.dir.as_path()]), &[]);
    assert_eq!(granted.stdout().trim(), "read", "{}", granted.stderr());
}

/// A listener of the test's own, on a free loopback port, that takes
/// connections until the test ends.
fn listener() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for c in l.incoming() {
            drop(c);
        }
    });
    port
}

#[test]
fn connect_grants_the_loopback_port_it_names_and_no_other() {
    let temp = Temp::new("connect");
    let port = listener();
    let script = format!(
        "/usr/bin/nc -z -w 2 127.0.0.1 {} >/dev/null 2>&1 && echo '\"connected\"' || echo '\"refused\"'",
        port
    );
    let with = |may: &str| run(&temp, &network(&[("a", "run", &script, may)]), Some(&[]), &[]);
    let none = with("");
    assert_eq!(none.stdout().trim(), "refused", "{}", none.stderr());
    let other = with(&format!(
        "\"may\": {{\"connect\": [\"127.0.0.1:{}\"]}}",
        if port == 65535 { port - 1 } else { port + 1 }
    ));
    assert_eq!(other.stdout().trim(), "refused", "{}", other.stderr());
    let named = with(&format!("\"may\": {{\"connect\": [\"127.0.0.1:{}\"]}}", port));
    assert_eq!(named.stdout().trim(), "connected", "{}", named.stderr());
    let unconfined = run(&temp, &network(&[("a", "run", &script, "")]), None, &[]);
    assert_eq!(
        unconfined.stdout().trim(),
        "connected",
        "the probe itself works: {}",
        unconfined.stderr()
    );
}

#[test]
fn listen_grants_the_loopback_port_it_names_and_no_other() {
    let temp = Temp::new("listen");
    // A port that was free a moment ago.
    let port = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port();
    let script = format!(
        "/usr/bin/nc -l 127.0.0.1 {} >/dev/null 2>&1 & pid=$!; /bin/sleep 1; if kill -0 $pid 2>/dev/null; then kill $pid; echo '\"listening\"'; else echo '\"refused\"'; fi",
        port
    );
    let with = |may: &str| run(&temp, &network(&[("a", "run", &script, may)]), Some(&[]), &[]);
    let none = with("");
    assert_eq!(none.stdout().trim(), "refused", "{}", none.stderr());
    let other = with(&format!(
        "\"may\": {{\"listen\": [\"127.0.0.1:{}\"]}}",
        if port == 65535 { port - 1 } else { port + 1 }
    ));
    assert_eq!(other.stdout().trim(), "refused", "{}", other.stderr());
    let named = with(&format!("\"may\": {{\"listen\": [\"127.0.0.1:{}\"]}}", port));
    assert_eq!(named.stdout().trim(), "listening", "{}", named.stderr());
}

/// A level 1 neuron in sh: ready, then one reply per signal saying whether it
/// could read the secret, and a pong per ping.
const SERVE: &str = "read hello; echo '{\"v\":1,\"type\":\"ready\"}'; while read line; do re=$(echo \"$line\" | /usr/bin/sed -n 's/.*\"re\":\\([0-9]*\\).*/\\1/p'); n=$(echo \"$line\" | /usr/bin/sed -n 's/.*\"n\":\\([0-9]*\\).*/\\1/p'); case \"$line\" in *'\"type\":\"signal\"'*) if /bin/cat secret.txt >/dev/null 2>&1; then echo \"{\\\"type\\\":\\\"answer\\\",\\\"re\\\":$re,\\\"payload\\\":\\\"read\\\"}\"; else echo \"{\\\"type\\\":\\\"answer\\\",\\\"re\\\":$re,\\\"payload\\\":\\\"denied\\\"}\"; fi;; *'\"type\":\"ping\"'*) echo \"{\\\"type\\\":\\\"pong\\\",\\\"n\\\":$n}\";; esac; done";

#[test]
fn a_serve_neuron_is_held_as_a_run_neuron_is() {
    let temp = Temp::new("serve");
    temp.write("secret.txt", "s");
    let held = run(&temp, &network(&[("a", "serve", SERVE, "")]), Some(&[]), &[]);
    assert_eq!(held.stdout().trim(), "denied", "{}", held.stderr());
    let granted = run(
        &temp,
        &network(&[("a", "serve", SERVE, "\"may\": {\"read\": [\"secret.txt\"]}")]),
        Some(&[]),
        &[],
    );
    assert_eq!(granted.stdout().trim(), "read", "{}", granted.stderr());
}

#[test]
fn a_program_that_cannot_be_found_fails_as_it_does_unconfined() {
    let temp = Temp::new("missing");
    let net = "{\"version\": 0, \"neurons\": [{\"id\": \"src\", \"source\": true}, {\"id\": \"a\", \"run\": [\"no-such-program-of-this-test\"]}], \"links\": [{\"from\": \"src\", \"to\": \"a\"}]}";
    let trace = temp.path("trace.json");
    let r = run(&temp, net, Some(&[]), &["--trace", &trace.to_string_lossy()]);
    let text = std::fs::read_to_string(&trace).unwrap_or_default();
    assert!(
        text.contains("\"code\":\"exit\"") && text.contains("exit 127"),
        "{}\n{}",
        text,
        r.stderr()
    );
}

/// The refusals: each ends the run before any process starts. `mark` would
/// leave a file if it ran.
fn refused(tag: &str, bad_opts: &str, extra_file: Option<(&str, &str)>, extra: &[&str], want: &str) {
    let temp = Temp::new(tag);
    let mark = (
        "mark",
        "run",
        "echo x > marked.txt; echo '\"ran\"'",
        "\"may\": {\"write\": [\".\"]}",
    );
    let bad = ("bad", "run", "echo '\"ran\"'", bad_opts);
    let mut args: Vec<String> = Vec::new();
    if let Some((name, contents)) = extra_file {
        args.push(extra[0].to_string());
        args.push(temp.write(name, contents).to_string_lossy().to_string());
    }
    let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = run(&temp, &network(&[mark, bad]), Some(&[]), &args);
    assert_eq!(r.out.status.code(), Some(1), "{}", r.stderr());
    assert!(r.stderr().contains(want), "{}", r.stderr());
    assert_eq!(r.stdout().trim(), "", "nothing answered");
    assert!(!temp.path("marked.txt").exists(), "nothing started");
}

#[test]
fn a_listen_address_the_sandbox_cannot_tell_apart_is_refused() {
    refused(
        "addr",
        "\"may\": {\"listen\": [\"10.0.0.7:8080\"]}",
        None,
        &[],
        "'bad' may listen '10.0.0.7:8080': a confined run holds a listener to localhost, 127.0.0.1 or 0.0.0.0 only",
    );
}

#[test]
fn confine_with_peers_is_refused() {
    refused(
        "peers",
        "",
        Some(("peers.json", "{\"version\": 0, \"peers\": [], \"placement\": {}}")),
        &["--peers"],
        "--confine and --peers together",
    );
}

#[test]
fn a_confine_file_with_an_unknown_field_is_refused() {
    let temp = Temp::new("badfile");
    let net = temp.write("net.json", &network(&[("a", "run", "echo x > marked.txt; echo '\"ran\"'", "")]));
    let inputs = temp.write("inputs.jsonl", "{\"at\": 0, \"source\": \"src\", \"payload\": 1}\n");
    let confine = temp.write("confine.json", "{\"version\": 0, \"tools\": [], \"allow\": true}");
    let out = Command::new(env!("CARGO_BIN_EXE_neuron-engine"))
        .arg("run")
        .arg(&net)
        .arg("--inputs")
        .arg(&inputs)
        .arg("--confine")
        .arg(&confine)
        .current_dir(&temp.dir)
        .output()
        .expect("the engine runs");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown field 'allow'"));
    assert!(!temp.path("marked.txt").exists());
}

// -- The proxy (S2b): a named host -------------------------------------------
//
// `neuron.localhost` is a name the engine does not take for loopback (only
// `localhost` and `127.0.0.1` are held by the sandbox itself), so it travels
// the proxy's road as any named host does, and the system resolves it to this
// machine with no network.

/// An HTTP server of the test's own on a free loopback port. It answers `ok`
/// to a request for a path, and 404 to one whose target is anything else, as
/// a strict server does: the proxy must hand it the path, not the whole URL.
fn http_server() -> u16 {
    use std::io::Read as _;
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut c = c;
                let mut buf = [0u8; 2048];
                let n = c.read(&mut buf).unwrap_or(0);
                let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
                let reply: &[u8] = if line.split(' ').nth(1).is_some_and(|t| t.starts_with('/')) {
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = c.write_all(reply);
            });
        }
    });
    port
}

/// A script that fetches `url` with curl (extra flags first) and says what it
/// got, or curl's own exit code: 7 is a connection it could not make, 22 an
/// HTTP refusal, 56 a tunnel the proxy would not open. A curl that could not
/// even start says something else, so no test passes for that reason.
fn fetch(flags: &str, url: &str) -> String {
    format!(
        "got=$(/usr/bin/curl -q -s -f -m 5 {} {} 2>/dev/null) && echo \"\\\"got:$got\\\"\" || echo \"\\\"curl:$?\\\"\"",
        flags, url
    )
}

#[test]
fn a_named_host_is_reached_through_the_proxy_both_ways_a_program_asks() {
    let temp = Temp::new("proxy");
    let port = http_server();
    let may = format!("\"may\": {{\"connect\": [\"neuron.localhost:{}\"]}}", port);
    let url = format!("http://neuron.localhost:{}/", port);
    // A plain request with an absolute target, then a tunnel (CONNECT).
    for flags in ["", "-p"] {
        let r = run(&temp, &network(&[("a", "run", &fetch(flags, &url), &may)]), Some(&[]), &[]);
        assert_eq!(r.stdout().trim(), "got:ok", "flags '{}': {}", flags, r.stderr());
    }
}

#[test]
fn a_host_the_may_does_not_list_is_refused_by_the_proxy() {
    let temp = Temp::new("proxy-unlisted");
    let (listed, unlisted) = (http_server(), http_server());
    let may = format!("\"may\": {{\"connect\": [\"neuron.localhost:{}\"]}}", listed);
    for flags in ["", "-p"] {
        let script = fetch(flags, &format!("http://neuron.localhost:{}/", unlisted));
        let r = run(&temp, &network(&[("a", "run", &script, &may)]), Some(&[]), &[]);
        // 22: the proxy answered, and its answer was a refusal.
        assert_eq!(r.stdout().trim(), "curl:22", "flags '{}': {}", flags, r.stderr());
        assert!(
            r.stderr()
                .contains(&format!("confine: 'a' was refused a connection to neuron.localhost:{}", unlisted)),
            "{}",
            r.stderr()
        );
    }
}

#[test]
fn a_program_that_ignores_the_proxy_reaches_no_named_host() {
    let temp = Temp::new("proxy-ignored");
    let port = http_server();
    let may = format!("\"may\": {{\"connect\": [\"neuron.localhost:{}\"]}}", port);
    let url = format!("http://neuron.localhost:{}/", port);
    // curl told to use no proxy, and a tool that knows nothing of proxies.
    let direct = run(
        &temp,
        &network(&[("a", "run", &fetch("--noproxy '*'", &url), &may)]),
        Some(&[]),
        &[],
    );
    assert_eq!(direct.stdout().trim(), "curl:7", "{}", direct.stderr());
    // The same curl, left to use the proxy, gets through: the refusal above is the sandbox's.
    let via = run(&temp, &network(&[("a", "run", &fetch("", &url), &may)]), Some(&[]), &[]);
    assert_eq!(via.stdout().trim(), "got:ok", "{}", via.stderr());
    let nc = format!(
        "/usr/bin/nc -z -w 2 127.0.0.1 {} >/dev/null 2>&1 && echo '\"connected\"' || echo '\"refused\"'",
        port
    );
    let raw = run(&temp, &network(&[("a", "run", &nc, &may)]), Some(&[]), &[]);
    assert_eq!(raw.stdout().trim(), "refused", "{}", raw.stderr());
}

/// Each neuron has a proxy of its own, and a sandbox that reaches no other:
/// `a` hands `b` the address of its proxy, and `b`, which may connect nowhere,
/// cannot use it.
#[test]
fn a_neuron_cannot_use_another_neurons_proxy() {
    let temp = Temp::new("proxy-other");
    let port = http_server();
    let a = "echo \"\\\"$http_proxy\\\"\"";
    let b = format!(
        "p=$(/usr/bin/sed 's/\"//g'); got=$(/usr/bin/curl -q -s -f -m 5 -x \"$p\" http://neuron.localhost:{}/ 2>/dev/null) && echo \"\\\"got:$got\\\"\" || echo \"\\\"curl:$?\\\"\"",
        port
    );
    let net = format!(
        "{{\"version\": 0, \"neurons\": [{{\"id\": \"src\", \"source\": true}}, {{\"id\": \"a\", \"run\": [\"/bin/sh\", \"-c\", {}], \"may\": {{\"connect\": [\"neuron.localhost:{}\"]}}}}, {{\"id\": \"b\", \"run\": [\"/bin/sh\", \"-c\", {}]}}], \"links\": [{{\"from\": \"src\", \"to\": \"a\"}}, {{\"from\": \"a\", \"to\": \"b\"}}]}}",
        json_str(a),
        port,
        json_str(&b)
    );
    let r = run(&temp, &net, Some(&[]), &[]);
    // 7: `b` could not connect to the proxy at all. The proxy never saw it.
    assert_eq!(r.stdout().trim(), "curl:7", "{}", r.stderr());
    assert!(!r.stderr().contains("was refused a connection"), "{}", r.stderr());
}

// -- A neuron named by its own file (SPEC 2.10, decision 0019) ----------------

/// A network whose one neuron `a` is named by `kit/a.neuron.json`, with
/// `entry` as the entry's other members.
fn named(temp: &Temp, own: &str, entry: &str) -> String {
    temp.write("kit/a.neuron.json", own);
    let entry = if entry.is_empty() { String::new() } else { format!(", {}", entry) };
    format!(
        "{{\"version\": 0, \"neurons\": [{{\"id\": \"src\", \"source\": true}}, {{\"id\": \"a\", \"neuron\": \"kit/a.neuron.json\"{}}}], \"links\": [{{\"from\": \"src\", \"to\": \"a\"}}]}}",
        entry
    )
}

/// The neuron's file is where its paths start: it runs in its own directory,
/// and its `may` is read from there. Unconfined first, so the path rule is
/// shown apart from the sandbox.
#[test]
fn a_neuron_file_runs_in_its_own_directory_and_its_may_is_read_from_there() {
    let temp = Temp::new("neuronfile");
    temp.write("kit/data/a.txt", "a");
    temp.write("data/a.txt", "the using file's, not the neuron's");
    let script = "/bin/pwd | /usr/bin/grep -q '/kit$' && where=kit || where=elsewhere; /bin/cat data/a.txt >/dev/null 2>&1 && r=yes || r=no; echo \"\\\"in:$where read:$r\\\"\"";
    let own = format!(
        "{{\"version\": 0, \"run\": [\"/bin/sh\", \"-c\", {}], \"may\": {{\"read\": [\"data\"]}}}}",
        json_str(script)
    );
    let open = run(&temp, &named(&temp, &own, ""), None, &[]);
    assert_eq!(open.stdout().trim(), "in:kit read:yes", "{}", open.stderr());
    let held = run(&temp, &named(&temp, &own, ""), Some(&[]), &[]);
    assert_eq!(held.stdout().trim(), "in:kit read:yes", "{}", held.stderr());
}

/// What the entry narrows is what the sandbox holds: the file may read its
/// data, the entry that names it keeps nothing, and the program reads nothing.
#[test]
fn a_narrowed_may_is_the_one_the_sandbox_holds() {
    let temp = Temp::new("neuronfile-narrow");
    temp.write("kit/data/a.txt", "a");
    let script = "/bin/cat data/a.txt >/dev/null 2>&1 && echo '\"read\"' || echo '\"denied\"'";
    let own = format!(
        "{{\"version\": 0, \"run\": [\"/bin/sh\", \"-c\", {}], \"may\": {{\"read\": [\"data\"]}}}}",
        json_str(script)
    );
    let narrowed = run(&temp, &named(&temp, &own, "\"may\": {}"), Some(&[]), &[]);
    assert_eq!(narrowed.stdout().trim(), "denied", "{}", narrowed.stderr());
}
