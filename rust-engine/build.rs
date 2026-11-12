// Records the commit the engine is built from, for `neuron-engine --version`:
// the full hash of HEAD, with `-dirty` when the engine's own sources differ
// from that commit, or `unknown` when there is no git checkout (a source
// archive). Nothing else is generated.
use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// The engine's own files: a change to one of them is what `-dirty` means,
/// and what makes this script run again.
const SOURCES: [&str; 4] = ["src", "Cargo.toml", "Cargo.lock", "build.rs"];

fn main() {
    for path in SOURCES {
        println!("cargo:rerun-if-changed={path}");
    }
    let commit = match git(&["rev-parse", "HEAD"]) {
        Some(head) if !head.is_empty() => {
            // A commit moves HEAD, the branch it names, or the packed refs.
            let mut watched = vec!["HEAD".to_string(), "packed-refs".to_string()];
            if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
                watched.push(branch);
            }
            for name in watched {
                if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
                    if Path::new(&path).exists() {
                        println!("cargo:rerun-if-changed={path}");
                    }
                }
            }
            let mut status = vec!["status", "--porcelain", "--"];
            status.extend(SOURCES);
            match git(&status) {
                Some(changed) if changed.is_empty() => head,
                _ => format!("{head}-dirty"),
            }
        }
        _ => "unknown".to_string(),
    };
    println!("cargo:rustc-env=NEURON_ENGINE_COMMIT={commit}");
}
