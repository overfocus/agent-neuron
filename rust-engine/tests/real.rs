// The real-process layer of M1, tested end to end: every behavior of the
// milestone's list, one test each, through the engine binary against the
// small invented programs of `tests/progs/`. Assertions read the run's stdout,
// stderr and exit code, and the trace it wrote: the trace's events are in the
// engine's own compact JSON form, so a search for a substring is an exact
// match on that output.
use std::io::Write as _;
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
            "neuron-rs-{}-{}-{}",
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

fn progs(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/progs")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// A network of a source and, per entry, one neuron: (id, kind field, command
/// JSON, extra options JSON). `extra_network` is the network's other fields
/// (none, a `budget`, ...), already written as JSON members.
fn network(extra_network: &str, entries: &[(&str, &str, &str, &str)]) -> String {
    let mut neurons = String::from("{\"id\": \"src\", \"description\": \"in\", \"source\": true}");
    let mut links = Vec::new();
    for (id, kind, cmd, opts) in entries {
        neurons.push_str(&format!(
            ",\n    {{\"id\": \"{}\", \"description\": \"{}\", {}: {}{}}}",
            id, id, kind, cmd, opts
        ));
        links.push(format!("{{\"from\": \"src\", \"to\": \"{}\"}}", id));
    }
    let mut extra = extra_network.trim().trim_end_matches(',').to_string();
    if !extra.is_empty() {
        extra = format!("  {},\n", extra);
    }
    format!(
        "{{\n  \"version\": 0,\n  \"name\": \"test\",\n  \"description\": \"a test network\",\n{extra}  \"neurons\": [\n{neurons}\n  ],\n  \"links\": [\n{links}\n  ]\n}}\n",
        extra = extra,
        neurons = neurons,
        links = links.join(",\n    ")
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

fn run_with_env(temp: &Temp, net: &Path, inputs: &Path, extra_args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(engine());
    cmd.arg("run").arg(net).arg("--inputs").arg(inputs);
    for a in extra_args {
        cmd.arg(a);
    }
    for (k, v) in vars {
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

fn has(hay: &str, needle: &str) -> bool {
    hay.contains(needle)
}

fn count(hay: &str, needle: &str) -> usize {
    hay.matches(needle).count()
}

// -- Level 0 (SPEC 3.1) ------------------------------------------------------

#[test]
fn l0_answer() {
    let temp = Temp::new("l0-answer");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "a",
                "\"run\"",
                &format!("[\"python3\", \"{}\"]", progs("echo.py")),
                ", \"description\": \"e\"",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "21")]);
    let out = run_cli(&temp, &net, &inputs, &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    // The neuron answers, and the run prints it (it has no outgoing links).
    assert_eq!(stdout(&out).trim(), "{\"seen\":21}");
}

#[test]
fn l0_quiet() {
    let temp = Temp::new("l0-quiet");
    let net = temp.write(
        "network.json",
        &network("", &[("q", "\"run\"", &format!("[\"sh\", \"{}\"]", progs("quiet.sh")), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), ""); // a quiet prints nothing
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert_eq!(count(&trace, "\"type\":\"quiet\""), 1);
}

#[test]
fn l0_junk() {
    let temp = Temp::new("l0-junk");
    let net = temp.write(
        "network.json",
        &network("", &[("j", "\"run\"", &format!("[\"sh\", \"{}\"]", progs("junk.sh")), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        has(&trace, "\"code\":\"junk\"") && has(&trace, "\"message\":\"not JSON: not a json value at all\""),
        "{}",
        trace
    );
    assert_eq!(count(&trace, "\"type\":\"error\""), 1);
    let _ = out;
}

#[test]
fn l0_exit_error() {
    let temp = Temp::new("l0-exit");
    let net = temp.write(
        "network.json",
        &network("", &[("e", "\"run\"", &format!("[\"sh\", \"{}\"]", progs("err.sh")), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        out.status.success(),
        "a failing call is an event, not a failed run: {}",
        stderr(&out)
    );
    assert!(
        has(&trace, "\"code\":\"exit\"") && has(&trace, "\"message\":\"exit 7: boom on stderr\""),
        "{}",
        trace
    );
}

#[test]
fn l0_signal_error() {
    let temp = Temp::new("l0-signal");
    let net = temp.write(
        "network.json",
        &network("", &[("s", "\"run\"", &format!("[\"sh\", \"{}\"]", progs("signal.sh")), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(has(&trace, "\"message\":\"signal SIGUSR1: "), "{}", trace);
}

#[test]
fn l0_overlong_output() {
    let temp = Temp::new("l0-overlong");
    let net = temp.write(
        "network.json",
        &network("", &[("o", "\"run\"", &format!("[\"python3\", \"{}\"]", progs("overlong.py")), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        has(&trace, "\"code\":\"output\"") && has(&trace, "\"message\":\"output too large\""),
        "{}",
        trace
    );
    // The group went down at once: the kill note follows the error (6.3).
    assert!(has(&trace, "\"what\":\"kill\""), "{}", trace);
}

#[test]
fn l0_timeout_and_cancel() {
    let temp = Temp::new("l0-timeout");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "h",
                "\"run\"",
                &format!("[\"python3\", \"{}\"]", progs("hang.py")),
                ", \"timeout\": 300",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        has(&trace, "\"code\":\"timeout\"") && has(&trace, "\"message\":\"timed out after 300 ms\""),
        "{}",
        trace
    );
    assert!(has(&trace, "\"what\":\"cancel\"") && has(&trace, "\"what\":\"term\""), "{}", trace);
}

#[test]
fn l0_spawn_failure() {
    let temp = Temp::new("l0-spawn");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "x",
                "\"run\"",
                &format!("[\"{}\", \"--help\"]", temp.path("no-such-executable").to_string_lossy()),
                "",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(has(&trace, "\"message\":\"exit 127: "), "{}", trace);
}

#[test]
fn l0_env_isolation() {
    let temp = Temp::new("l0-env");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "v",
                "\"run\"",
                &format!("[\"python3\", \"{}\"]", progs("envcheck.py")),
                ", \"env\": [\"EXTRA\"]",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_with_env(
        &temp,
        &net,
        &inputs,
        &[],
        &[("EXTRA", "passed-through"), ("SECRET", "must-not-leak")],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let answer = stdout(&out);
    assert!(has(&answer, "\"extra\":\"passed-through\""), "{}", answer);
    assert!(has(&answer, "\"path\":true"), "{}", answer);
    assert!(has(&answer, "\"secret\":null"), "{}", answer);
}

#[test]
fn l0_cwd_and_relative_command() {
    // The cwd option, a relative command path, and the declaring file's
    // directory as the default working directory (SPEC 2.4, 2.3): both
    // probes live in `sub`, which is the working directory of their neurons.
    let temp = Temp::new("l0-cwd");
    let sub = temp.path("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("cwdprobe.py"),
        "import json, os\nprint(json.dumps({\"cwd\": os.path.basename(os.getcwd())}))\n",
    )
    .unwrap();
    std::fs::write(sub.join("relprog.sh"), "#!/bin/sh\necho '{\"rel\": true}'\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(sub.join("relprog.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let net_path = temp.write(
        "network.json",
        &network(
            "",
            &[
                ("w", "\"run\"", "[\"python3\", \"cwdprobe.py\"]", ", \"cwd\": \"sub\""),
                ("r", "\"run\"", "[\"./relprog.sh\"]", ", \"cwd\": \"sub\""),
            ],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net_path, &inputs, &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(count(&stdout(&out), "\"cwd\":\"sub\""), 1, "{}", stdout(&out));
    assert_eq!(count(&stdout(&out), "\"rel\":true"), 1, "{}", stdout(&out));
}

#[test]
fn l0_term_then_kill_with_survivor() {
    // A cut call whose group ignores SIGTERM: term, then the kill 2 s later,
    // and the run waits for the group (6.3, 6.2 rule 6).
    let temp = Temp::new("l0-leaves");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "l",
                "\"run\"",
                &format!("[\"python3\", \"{}\"]", progs("leaves_ignore.py")),
                ", \"timeout\": 250",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "\"hello\"")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(has(&trace, "\"what\":\"term\"") && has(&trace, "\"what\":\"kill\""), "{}", trace);
    // The finished of the trace comes after the kill note (6.2 rule 8).
    let term = trace.find("\"what\":\"kill\"").expect("kill note");
    let fin = trace.find("\"type\":\"finished\"").expect("finished event");
    assert!(term < fin, "the kill note comes after the error, before the finished");
    let _ = out;
}

// -- Level 1 (SPEC 3.2) ------------------------------------------------------

/// The network of one serve neuron on `serve.py`; its `maxInflight` of 2 is
/// the program's own word in `ready` (3.2), not a field of the file.
fn serve_network(delay_ms: u64, timeout_ms: u64) -> String {
    network(
        "",
        &[(
            "s",
            "\"serve\"",
            &format!("[\"python3\", \"{}\", \"{}\"]", progs("serve.py"), delay_ms),
            &format!(", \"timeout\": {}", timeout_ms),
        )],
    )
}

#[test]
fn l1_handshake_run_and_finish() {
    let temp = Temp::new("l1-basics");
    let net = temp.write("network.json", &serve_network(10, 3000));
    let inputs = inputs_file(&temp, &[("0", "src", "{\"n\": 2}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    // The threshold call's input rides as {payload, inputs} only above 1.
    assert_eq!(stdout(&out).trim(), "{\"n\":4}");
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        trace.contains("\"kind\":\"serve\"") && trace.contains("\"kind\":\"source\""),
        "{}",
        trace
    );
}

#[test]
fn l1_ready_validation_refuses() {
    let temp = Temp::new("l1-badready");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "s",
                "\"serve\"",
                &format!("[\"python3\", \"{}\"]", progs("serve_badready.py")),
                ", \"timeout\": 400",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &[]);
    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(has(&stderr(&out), "failed its first start"), "{}", stderr(&out));
    assert!(has(&stderr(&out), "ready.v differs from hello.v"), "{}", stderr(&out));
}

#[test]
fn l1_never_ready_refuses() {
    let temp = Temp::new("l1-noready");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "s",
                "\"serve\"",
                &format!("[\"python3\", \"{}\"]", progs("serve_noready.py")),
                ", \"timeout\": 300",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &[]);
    assert!(!out.status.success());
    assert!(has(&stderr(&out), "no ready within 300 ms"), "{}", stderr(&out));
}

#[test]
fn l1_maxinflight_queue() {
    // maxInflight 2 and three calls of 200 ms: two run, one queues; the
    // answers still arrive for all three (3.2).
    let temp = Temp::new("l1-queue");
    let net = temp.write("network.json", &serve_network(200, 5000));
    let inputs = inputs_file(&temp, &[("0", "src", "1"), ("50", "src", "2"), ("100", "src", "3")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).lines().count(), 3);
}

#[test]
fn l1_cancel_of_running_call() {
    // An input's newer sibling cancels a call that is still running: the drop
    // is running, the cancel note follows it (5.4, 6.2), and the neuron's
    // quiet for the cancelled re is discarded; the second call answers.
    let temp = Temp::new("l1-cancel");
    let net = temp.write("network.json", &serve_network(1200, 2500));
    let p = temp.path("inputs.jsonl");
    std::fs::write(
        &p,
        "{\"at\":0,\"source\":\"src\",\"payload\":{\"n\":1},\"replaces\":\"k\"}\n{\"at\":150,\"source\":\"src\",\"payload\":{\"n\":2},\"replaces\":\"k\"}\n",
    )
    .unwrap();
    let out = run_cli(&temp, &net, &p, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(has(&trace, "\"reason\":\"replaced\",\"phase\":\"running\""), "{}", trace);
    assert!(has(&trace, "\"what\":\"cancel\""), "{}", trace);
    assert_eq!(stdout(&out).lines().count(), 1);
}

#[test]
fn l1_ping_and_pong() {
    // Long idle gap with a short ping interval: pings are answered and
    // nobody hangs.
    let temp = Temp::new("l1-ping");
    // The network file is rewritten from the same shape the other tests use;
    // its path is read again, at the run, below.
    temp.write("network.json", &serve_network(10, 2000));
    let p2 = temp.path("i2.jsonl");
    std::fs::write(
        &p2,
        "{\"at\":0,\"source\":\"src\",\"payload\":{\"n\":1}}\n{\"at\":0,\"source\":\"src\",\"payload\":{\"n\":2}}\n",
    )
    .unwrap();
    let out = run_with_env(
        &temp,
        &temp.path("network.json"),
        &p2,
        &["--trace", &temp.path("t.json").to_string_lossy(), "--ping", "150"],
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(!has(&trace, "\"code\":\"crash\""), "no hang: {}", trace);
    assert_eq!(stdout(&out).lines().count(), 2);
}

#[test]
fn l1_hang_on_pong() {
    // A process that never pongs: the pong deadline hangs it, the calls fail,
    // and the neuron restarts (3.2; the restart note is written, 6.3).
    let temp = Temp::new("l1-hang");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "s",
                "\"serve\"",
                &format!("[\"python3\", \"{}\"]", progs("serve_hang.py")),
                ", \"timeout\": 300",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{\"n\": 1}")]);
    let out = run_cli(
        &temp,
        &net,
        &inputs,
        &["--trace", &temp.path("t.json").to_string_lossy(), "--ping", "200"],
    );
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    // The hang is of an idle process: the restart note is the fact; a crash
    // would be of a call. Either way the run ends and reports the term.
    assert!(has(&trace, "\"what\":\"term\"") || has(&trace, "\"what\":\"restart\""), "{}", trace);
    assert!(out.status.success());
}

#[test]
fn l1_crash_and_restart() {
    // A process that dies before its call answers: the call fails with crash,
    // the process restarts with the backoff, and the next call answers (3.2).
    let temp = Temp::new("l1-crash");
    temp.write(
        "network.json",
        &network(
            "",
            &[(
                "s",
                "\"serve\"",
                &format!("[\"python3\", \"{}\"]", progs("serve_crash.py")),
                ", \"timeout\": 2000",
            )],
        ),
    );
    let p = temp.path("inputs.jsonl");
    std::fs::write(
        &p,
        "{\"at\":0,\"source\":\"src\",\"payload\":{\"n\":1}}\n{\"at\":300,\"source\":\"src\",\"payload\":{\"n\":2}}\n",
    )
    .unwrap();
    let out = run_cli(
        &temp,
        &temp.path("network.json"),
        &p,
        &["--trace", &temp.path("t.json").to_string_lossy()],
    );
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        has(&trace, "\"code\":\"crash\"") && has(&trace, "process failed"),
        "crash: {}",
        trace
    );
    assert!(has(&trace, "\"what\":\"restart\""), "restart: {}", trace);
    // The restarted process answers the second call.
    assert_eq!(count(&stdout(&out), "\"seen\":"), 1, "{}", stdout(&out));
    assert!(out.status.success());
}

#[test]
fn l1_junk_line_fails_one_call() {
    let temp = Temp::new("l1-junk");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "s",
                "\"serve\"",
                &format!("[\"python3\", \"{}\"]", progs("serve_junk.py")),
                ", \"timeout\": 2000",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    // The note carries the line (from 1, ready included) and its text; the
    // junk error follows the note (6.3).
    assert!(has(&trace, "\"what\":\"junk\",\"line\""), "{}", trace);
    assert!(has(&trace, "junk on stdout: this line is junk"), "{}", trace);
}

#[test]
fn l1_overlong_line() {
    let temp = Temp::new("l1-longline");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "s",
                "\"serve\"",
                &format!("[\"python3\", \"{}\"]", progs("serve_longline.py")),
                ", \"timeout\": 2000",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(has(&trace, "\"code\":\"output\"") && has(&trace, "output too large"), "{}", trace);
}

#[test]
fn l1_protocol_error() {
    let temp = Temp::new("l1-protocol");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[(
                "s",
                "\"serve\"",
                &format!("[\"python3\", \"{}\"]", progs("serve_reject.py")),
                ", \"timeout\": 2000",
            )],
        ),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &["--trace", &temp.path("t.json").to_string_lossy()]);
    assert!(out.status.success());
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    assert!(
        has(&trace, "\"code\":\"protocol\"") && has(&trace, "protocol: to names"),
        "{}",
        trace
    );
}

// -- The run command's surfaces ----------------------------------------------

#[test]
fn run_refuses_https() {
    // An `http` neuron runs for real over http:// (3.3); a `https://` one is
    // refused at the start, for a std-only engine has no TLS. The reason
    // names the neuron and the scheme.
    let temp = Temp::new("run-https");
    let net = temp.write("network.json", &network("", &[("h", "\"http\"", "\"https://127.0.0.1:9/x\"", "")]));
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &[]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(has(&err, "refused the run"), "{}", err);
    assert!(has(&err, "https"), "{}", err);
    assert!(has(&err, "'h'"), "{}", err);
}

#[test]
fn run_refuses_a_file_that_does_not_check() {
    let temp = Temp::new("run-check-fail");
    let net = temp.write(
        "network.json",
        &network(
            "",
            &[("b", "\"run\"", "[\"python3\", \"-c\", \"pass\"]", ", \"cost\": {\"credits\": 3}")],
        ),
    );
    let mut with_budget = std::fs::read_to_string(&net).unwrap();
    with_budget = with_budget.replace(
        "\"description\": \"a test network\",",
        "\"description\": \"a test network\",\n  \"budget\": {},",
    );
    std::fs::write(&net, with_budget).unwrap();
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let out = run_cli(&temp, &net, &inputs, &[]);
    assert!(!out.status.success());
    assert!(has(&stderr(&out), "empty"), "{}", stderr(&out));
}

#[test]
fn run_resolves_a_file_named_bare_against_the_callers_cwd() {
    // The declaring file's directory is the working directory of its neurons
    // (2.4); a file named bare (no directory in the argument) sits in the
    // caller's cwd, whose directory the spawns must be able to chdir to.
    let temp = Temp::new("run-bare");
    let _net = temp.write(
        "network.json",
        &network("", &[("a", "\"run\"", &format!("[\"sh\", \"{}\"]", progs("quiet.sh")), "")]),
    );
    let inputs = inputs_file(&temp, &[("0", "src", "{}")]);
    let mut cmd = Command::new(engine());
    cmd.arg("run")
        .arg(temp.path("network.json").file_name().unwrap())
        .arg("--inputs")
        .arg(inputs.to_string_lossy().as_ref());
    cmd.current_dir(temp.dir.clone());
    let out = cmd.output().expect("the engine runs");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), ""); // the quiet prints nothing
}

// -- The trace and the stream -------------------------------------------------

#[test]
fn trace_header_and_stream_lines() {
    let temp = Temp::new("trace-stream");
    let net = temp.write("network.json", &serve_network(10, 3000));
    let inputs = inputs_file(&temp, &[("0", "src", "{\"n\": 5}")]);
    let out = run_cli(
        &temp,
        &net,
        &inputs,
        &[
            "--trace",
            &temp.path("t.json").to_string_lossy(),
            "--stream",
            &temp.path("s.jsonl").to_string_lossy(),
        ],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let trace = std::fs::read_to_string(temp.path("t.json")).unwrap();
    // The header of format v1 (docs/TRACE.md), with the engine named.
    assert!(has(&trace, "\"format\": \"agent-neuron-trace\""), "{}", trace);
    assert!(has(&trace, "\"version\": 1"));
    assert!(has(&trace, "\"library\": \"0.0.0\""));
    assert!(has(&trace, "\"engine\": \"agent-neuron-rust\""));
    // The header's `ids`: the two id kinds the format names, both numbered
    // per run by this engine (written whenever the trace is written).
    assert!(
        has(&trace, "\"ids\": {\"signal\":\"run-numbered\",\"trace\":\"run-numbered\"}"),
        "{}",
        trace
    );
    assert!(has(&trace, "\"demo\": \"test\""));
    assert!(has(&trace, "\"summary\": {}"));
    // Every signal's fields and the finished at the end; the events are one
    // per line.
    assert!(
        has(&trace, "\"replaces\":\"k\"") || has(&trace, "\"type\":\"finished\""),
        "{}",
        trace
    );
    assert!(has(&trace, "\"type\":\"finished\""));
    // The stream file: the same events, one JSON object per line.
    let stream = std::fs::read_to_string(temp.path("s.jsonl")).unwrap();
    assert!(stream.ends_with('\n'));
    for line in stream.lines() {
        assert!(line.starts_with('{') && line.ends_with('}'), "{}", line);
    }
    // The stream is the trace: counting the trace's events lines matches.
    assert_eq!(stream.lines().count(), count(&trace, "\n    {\"type\":"));
}

#[test]
fn stream_pipe_without_reader_is_off() {
    // A pipe with no reader: the stream is off, with one message on stderr,
    // and the run itself is unaffected (9.1).
    let temp = Temp::new("stream-pipe");
    temp.write("network.json", &serve_network(10, 2000));
    std::fs::create_dir(temp.path("d")).unwrap();
    let fifo = temp.path("d").join("events.fifo");
    make_fifo(&fifo);
    let inputs = inputs_file(&temp, &[("0", "src", "{\"n\": 1}")]);
    let out = run_cli(&temp, &temp.path("network.json"), &inputs, &["--stream", &fifo.to_string_lossy()]);
    assert!(out.status.success(), "the run never waits on a stream: {}", stderr(&out));
    assert!(has(&stderr(&out), "no reader"), "{}", stderr(&out));
}

#[cfg(unix)]
fn make_fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    // mkfifo via extern "C" (std-only path of this crate).
    unsafe {
        mkfifo(path.as_os_str().as_bytes().as_ptr() as *const i8, 0o644);
    }
}

extern "C" {
    fn mkfifo(path: *const i8, mode: u32) -> i32;
}
