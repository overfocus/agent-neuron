//! `neuron-engine --version`: one line, the crate version and the commit the
//! binary was built from (`build.rs`).

use std::path::Path;
use std::process::Command;

#[test]
fn version_names_the_commit_the_binary_was_built_from() {
    let out = Command::new(env!("CARGO_BIN_EXE_neuron-engine")).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let line = String::from_utf8(out.stdout).unwrap();
    let words: Vec<&str> = line.trim_end_matches('\n').split(' ').collect();
    assert_eq!(words.len(), 3, "{line}");
    assert_eq!(words[0], "neuron-engine");
    assert_eq!(words[1], env!("CARGO_PKG_VERSION"));

    // In a checkout the commit is HEAD, `-dirty` when the engine's sources
    // are changed; outside one it is `unknown`.
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
        .output();
    match head {
        Ok(head) if head.status.success() => {
            let head = String::from_utf8(head.stdout).unwrap();
            assert_eq!(words[2].trim_end_matches("-dirty"), head.trim());
        }
        _ => assert_eq!(words[2], "unknown"),
    }
}
