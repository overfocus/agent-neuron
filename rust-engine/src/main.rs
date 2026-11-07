// The command line: `neuron-engine conform <dir> [--only <text>]` and
// `neuron-engine run <file> --inputs <jsonl> [--trace <out.json>] [--stream
// <path>] [--ping <ms>]`.
//
// `conform` is the contract of CONFORMANCE 8: one line per fixture, `pass`/
// `fail` (first difference, two more indented lines)/`pending`, a last summary
// line, exit 0/1/2. `run` is SPEC 9.1's command-line form: the answers of the
// neurons with no outgoing links on stdout, a live stream (a file or a pipe)
// that never holds the run up, and the trace file when asked, with the
// events-vs-trace-vs-stream self-check.
use rust_engine::run::{self, RunOpts};
use rust_engine::runner;
use rust_engine::trace;
use rust_engine::{check, host, json};
use std::path::Path;

fn usage() -> ! {
    eprintln!("usage: neuron-engine conform <dir> [--only <text>]");
    eprintln!("       neuron-engine run <file> --inputs <jsonl> [--trace <out.json>] [--stream <path>] [--ping <ms>]");
    eprintln!("       neuron-engine check <file>");
    std::process::exit(2);
}

struct Flags {
    named: Vec<(String, String)>,
    free: Vec<String>,
}

fn parse_flags(args: &[String]) -> Flags {
    let mut named = Vec::new();
    let mut free = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i].starts_with("--") {
            if i + 1 >= args.len() {
                usage();
            }
            named.push((args[i][2..].to_string(), args[i + 1].clone()));
            i += 2;
        } else {
            free.push(args[i].clone());
            i += 1;
        }
    }
    Flags { named, free }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    match args[0].as_str() {
        "conform" => conform(&args[1..]),
        "run" => run(&args[1..]),
        "check" => check::main(&args[1..]),
        _ => usage(),
    }
}

fn conform(args: &[String]) {
    if args.len() != 1 && args.len() != 3 {
        usage();
    }
    let dir = args[0].clone();
    let mut only: Option<String> = None;
    if args.len() == 3 {
        if args[1] != "--only" {
            usage();
        }
        only = Some(args[2].clone());
    }
    let fixtures = match runner::discover(&dir) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot read {}: {}", dir, e);
            std::process::exit(2);
        }
    };
    let fixtures: Vec<_> = fixtures
        .into_iter()
        .filter(|f| only.as_deref().map(|o| f.name.contains(o)).unwrap_or(true))
        .collect();
    if fixtures.is_empty() {
        std::process::exit(2);
    }
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut pending = 0usize;
    let mut exit = 0;
    for fx in &fixtures {
        match runner::run_fixture(Path::new(&dir), fx) {
            Ok(()) => {
                println!("pass {}", fx.name);
                passed += 1;
            }
            Err(runner::Result::Pending { reason }) => {
                println!("pending {}: {}", fx.name, reason);
                pending += 1;
            }
            Err(runner::Result::Failed { diff }) => {
                println!("fail {}", fx.name);
                for line in diff.lines() {
                    println!("  {}", line);
                }
                failed += 1;
                exit = 1;
            }
        }
    }
    println!("{} passed, {} failed, {} pending", passed, failed, pending);
    std::process::exit(exit);
}

fn run(args: &[String]) {
    let flags = parse_flags(args);
    if flags.free.len() != 1 {
        usage();
    }
    let file = flags.free[0].clone();
    let get = |name: &str| flags.named.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    if flags
        .named
        .iter()
        .any(|(n, _)| !matches!(n.as_str(), "inputs" | "trace" | "stream" | "ping"))
    {
        usage();
    }
    let Some(inputs) = get("inputs") else { usage() };
    let ping_every = match get("ping") {
        None => 10_000,
        Some(v) => match v.parse::<u64>() {
            Ok(n) if n >= 1 => n,
            _ => usage(),
        },
    };
    let trace_out = get("trace");
    let stream_path = get("stream");
    // A signal that ends the CLI kills every running group first (Appendix C,
    // decision 0001); a run's own end has the engine's shutdown after it.
    host::install_signal_teardown();
    // The stream is in place before the engine starts, so the first event is
    // offered too (9.1: the stream is the trace, event for event).
    let written: std::sync::Arc<std::sync::Mutex<Vec<String>>> = std::sync::Arc::default();
    let mut sink: Option<run::StreamSink> = None;
    if let Some(path) = &stream_path {
        match trace::Stream::open(path) {
            Ok(mut s) => {
                let keep = written.clone();
                sink = Some(Box::new(move |ev: &json::Value| {
                    let line = json::write(ev);
                    if let Ok(mut w) = keep.lock() {
                        w.push(line.clone());
                    }
                    s.emit(ev);
                }));
            }
            Err(why) => {
                // Fail-open (9.1): a missing reader is off, with one message
                // on stderr; the run never waits on the stream.
                eprintln!("{}", why);
            }
        }
    }
    let opts = RunOpts {
        inputs: inputs.clone(),
        ping_every,
    };
    match run::run_with_stream(&file, &opts, sink) {
        Ok(result) => {
            run::print_answers(&result.answers);
            if let Some(out) = &trace_out {
                // The stream's record rides only when a stream was asked for.
                let record = if stream_path.is_some() { Some(&written) } else { None };
                if let Err(why) = run::write_trace_checked(out, &result.events, &result.network, record) {
                    eprintln!("{}", why);
                    std::process::exit(1);
                }
            }
            let demo = if result.network.name.is_empty() {
                Path::new(&file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                result.network.name
            };
            eprintln!("ran {}: {} events", demo, result.events.len());
        }
        Err(why) => {
            eprintln!("{}", why);
            std::process::exit(1);
        }
    }
}
