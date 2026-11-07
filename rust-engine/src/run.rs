// The `run` command: one file, one inputs file, real processes (SPEC 9.1's
// command-line form; the answers of the neurons with no outgoing links).
use crate::engine::Engine;
use crate::fixture;
use crate::json::Value;
use crate::net::{NetFile, Network};
use std::collections::HashMap;
use std::path::Path;

pub struct RunOpts {
    pub inputs: String,
    pub ping_every: u64,
}

/// The inputs file: JSONL entries `{ at, source, payload, replaces?, budget? }`
/// (SPEC 4.1), the shape a fixture's `inputs` entries take.
pub fn read_inputs(path: &str) -> Result<Vec<fixture::Input>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: cannot read ({})", path, e))?;
    let mut inputs = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v = crate::json::parse(line).map_err(|e| format!("{} line {}: not JSON ({})", path, i + 1, e))?;
        let mut errs = Vec::new();
        let input = fixture::inputs_of(&v, i, &mut errs);
        if !errs.is_empty() {
            return Err(format!("{} line {}: {}", path, i + 1, errs.join("; ")));
        }
        inputs.push(input);
    }
    Ok(inputs)
}

/// One run, on real processes. Time zero is the instant the last serve
/// program said ready (CONFORMANCE 4.2's real analogue); the inputs are then
/// injected `at` ms from it, at real times; the engine returns after the
/// processes are gone (Appendix C), and never before its last event was
/// offered to the stream (9.1). On an engine that refuses (a serve program
/// that never said ready), the start's processes are stopped, silently.
///
/// Returns the run's printed answers — the whole payloads of the neurons with
/// no outgoing links, in the order they happened (4.4), a text payload as its
/// text and the rest as its JSON — the events, and the network's tree.
pub fn run_with_stream(
    file: &str,
    opts: &RunOpts,
    sink: Option<Box<dyn FnMut(&Value)>>,
) -> Result<(Vec<String>, Vec<Value>, Network, HashMap<String, NetFile>), String> {
    let inputs = read_inputs(&opts.inputs)?;
    let path = Path::new(file);
    let (outer, files) = crate::runner::load_network_tree(path).map_err(|e| e.join("; "))?;
    // The declaring file's directory is the default working directory of its
    // neurons (SPEC 2.4); the engine plants with it.
    let base = path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut eng = Engine::new_real(inputs, opts.ping_every);
    if let Err(e) = eng.plant(&outer, &files, Some(&base)) {
        return Err(format!("cannot run: {}", e));
    }
    if let Some(sink) = sink {
        eng.set_stream(sink);
    }
    if let Some(why) = eng.start_serves_err() {
        eng.stop_after_refusal();
        return Err(format!("the engine refused the run: {}", why));
    }
    let outcome = eng.run();
    if let Some(why) = &outcome.refused {
        return Err(format!("the engine refused the run: {}", why));
    }
    let printed: Vec<String> = outcome
        .answers
        .iter()
        .map(|(_, payload)| match payload {
            crate::engine::PVal::Text(text) => text.clone(),
            other => crate::json::write(&other.as_full()),
        })
        .collect();
    Ok((printed, outcome.events, outer, files))
}

/// The answers on stdout, one per line (already the printed form).
pub fn print_answers(answers: &[String]) {
    for line in answers {
        println!("{}", line);
    }
}
