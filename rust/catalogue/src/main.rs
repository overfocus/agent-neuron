//! Reads a query from stdin and prints a JSON array of matching track titles.
//! A query under 3 characters prints nothing (the neuron stays quiet). The
//! catalogue and the rule live in the library (src/lib.rs).
use catalogue::{search, to_json};
use std::io::{Read, Write};

fn main() {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("cannot read stdin: {e}");
        std::process::exit(1);
    }
    if let Some(hits) = search(&input) {
        if writeln!(std::io::stdout().lock(), "{}", to_json(&hits)).is_err() {
            std::process::exit(1);
        }
    }
}
