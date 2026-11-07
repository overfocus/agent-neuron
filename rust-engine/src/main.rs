// The command line: `neuron-engine conform <dir> [--only <text>]`.
//
// Runs every fixture directory under <dir> recursively whose fixture.json
// exists, in path order (CONFORMANCE 1 and 8). Prints one line per fixture and
// a last summary line; exits 0 when nothing failed, 1 when one did (a harness
// failure counts), 2 for a usage error or when <dir> holds no fixture.
use rust_engine::runner;
use std::path::Path;

fn usage() -> ! {
    eprintln!("usage: neuron-engine conform <dir> [--only <text>]");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 && args.len() != 4 {
        usage();
    }
    if args[0] != "conform" {
        usage();
    }
    let dir = args[1].clone();
    let mut only: Option<String> = None;
    if args.len() == 4 {
        if args[2] != "--only" {
            usage();
        }
        only = Some(args[3].clone());
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
