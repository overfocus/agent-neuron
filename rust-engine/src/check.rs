// The `check` command (SPEC 2.7): the diagnostics of a network file tree,
// without running anything.
//
// Each file is validated with the errors of net.rs; this layer adds 2.7's
// warnings (an `http` neuron — run performs its request over the network,
// which a check cannot —, an executable that cannot be found, a cost
// above the file's ceiling, a composed file whose neurons cost a unit the
// declaring neuron's cost does not name), walks every composed file the tree
// names (relative to the file that declares the neuron, 2.4), and returns the
// findings in declaration order, errors and warnings alike, each with the
// file, line and column where it can. Exit: 0 when no error, 1 when one is,
// 2 when the usage or the file itself is wrong.
use crate::net::{self, Diag, Scan, Span, NONE_SPAN};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn main(args: &[String]) {
    if args.len() != 1 || args[0].is_empty() {
        eprintln!("usage: neuron-engine check <file>");
        std::process::exit(2);
    }
    let path = Path::new(&args[0]);
    if !path.is_file() {
        eprintln!("{}: cannot read (the file does not exist or is not a file)", args[0]);
        std::process::exit(2);
    }
    let diags = tree(&args[0]);
    for d in &diags {
        println!("{}", d.line_of());
    }
    let errors = diags.iter().filter(|d| d.error).count();
    let warnings = diags.len() - errors;
    println!("checked {}: {} errors, {} warnings", args[0], errors, warnings);
    std::process::exit(if errors > 0 { 1 } else { 0 });
}

/// The findings of the whole tree one file starts, errors before the run.
pub fn tree(path: &str) -> Vec<Diag> {
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    visit(Path::new(path), false, &mut out, &mut seen);
    out
}

/// One file: its errors and warnings, then its composed files. A file already
/// visited (a tree that loops back on itself) is not visited again: its rules
/// do not change, so a second report would repeat one honest finding.
fn visit(path: &Path, composed: bool, out: &mut Vec<Diag>, seen: &mut HashSet<PathBuf>) {
    let name = path.to_string_lossy().to_string();
    if let Ok(real) = std::fs::canonicalize(path) {
        if !seen.insert(real) {
            return;
        }
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            out.push(Diag {
                error: true,
                file: Some(name.clone()),
                pos: None,
                message: format!("cannot read ({})", e),
            });
            return;
        }
    };
    let scan = Scan::new(&text);
    let mut found: Vec<Diag> = Vec::new();
    let net = net::check_text(&text, Some(&dir_of(path)), composed, &mut found);
    for d in &mut found {
        if d.file.is_none() {
            d.file = Some(name.clone());
        }
    }
    out.append(&mut found);
    out.append(&mut warnings(&net, &scan, &name, &dir_of(path)));
    // The composed files (the declaration is a name relative to this file).
    let neuron_spans = scan.array_spans("neurons");
    for (i, n) in net.nodes.iter().enumerate() {
        let inner = match &n.kind {
            crate::net::Kind::Network(rel) => rel.clone(),
            _ => continue,
        };
        let inner_path = dir_of(path).join(&inner);
        if !inner_path.is_file() {
            // 2.7: a `network` file that is missing is an error, at the key
            // that names it.
            let span = neuron_spans.get(i).copied().unwrap_or(NONE_SPAN);
            out.push(Diag {
                error: true,
                file: Some(name.clone()),
                pos: scan.at(span, "network"),
                message: format!("the network file '{}' is missing", inner),
            });
            continue;
        }
        // The fold (2.7, decision 0016): the composed neuron's `may` covers
        // what its file's neurons declare. A file with errors of its own
        // reports those, and is not folded.
        if let Ok(inner_text) = std::fs::read_to_string(&inner_path) {
            let mut inner_diags = Vec::new();
            let inner_net = net::check_text(&inner_text, Some(&dir_of(&inner_path)), true, &mut inner_diags);
            if !inner_diags.iter().any(|d| d.error) {
                if let Some(why) = net::may_fold_error(n, &inner_net) {
                    let span = neuron_spans.get(i).copied().unwrap_or(NONE_SPAN);
                    let key = if n.opts.may.is_some() { "may" } else { "network" };
                    out.push(Diag {
                        error: true,
                        file: Some(name.clone()),
                        pos: scan.at(span, key),
                        message: why,
                    });
                }
                // The effect fold (11.5): at `effect` when the entry states
                // it over a file with none, at `network` when it leaves it out.
                if let Some(why) = net::effect_fold_error(n, &inner_net) {
                    let span = neuron_spans.get(i).copied().unwrap_or(NONE_SPAN);
                    let key = if n.opts.effect { "effect" } else { "network" };
                    out.push(Diag {
                        error: true,
                        file: Some(name.clone()),
                        pos: scan.at(span, key),
                        message: why,
                    });
                }
            }
        }
        visit(&inner_path, true, out, seen);
    }
}

/// The warnings of 2.7 one file's tree carries (errors are net.rs's own).
fn warnings(net: &crate::net::Network, scan: &Scan, file: &str, dir: &Path) -> Vec<Diag> {
    let mut out: Vec<Diag> = Vec::new();
    let neuron_spans = scan.array_spans("neurons");
    for (i, n) in net.nodes.iter().enumerate() {
        let span = neuron_spans.get(i).copied().unwrap_or(NONE_SPAN);
        match &n.kind {
            crate::net::Kind::Http(http) => {
                // `http` runs for real (M2): `run` performs the request over
                // the network; `check` cannot reach it, and says so (2.7's
                // note, this engine's M2 choice).
                out.push(warn(
                    scan,
                    span,
                    "http",
                    file,
                    &format!(
                        "an http neuron ('{}') needs the network: run performs the request; check cannot reach it",
                        http
                    ),
                ));
            }
            crate::net::Kind::Run(cmd) | crate::net::Kind::Serve(cmd) => {
                let key = match &n.kind {
                    crate::net::Kind::Run(_) => "run",
                    _ => "serve",
                };
                if !findable(cmd.first().map(|s| s.as_str()).unwrap_or(""), n.opts.cwd.as_deref(), dir) {
                    out.push(warn(
                        scan,
                        span,
                        key,
                        file,
                        &format!(
                            "the executable '{}' of '{}' is not findable on PATH",
                            cmd.first().map(|s| s.as_str()).unwrap_or(""),
                            n.id
                        ),
                    ));
                }
            }
            _ => {}
        }
        // A cost above the file's ceiling in some unit: the neuron can never
        // run (2.7; 10.3 reserves and fails at once).
        if let (Some(budget), Some(cost)) = (&net.budget, &n.opts.cost) {
            let (_, probe) = crate::net::anchor_point(scan, span, "cost");
            for (u, amt) in cost {
                if budget.iter().any(|(bu, bamt)| bu == u && bamt < amt) {
                    out.push(warn(
                        scan,
                        probe,
                        u,
                        file,
                        &format!("the cost of '{}' is above the file's ceiling in '{}' (it can never run)", n.id, u),
                    ));
                }
            }
        }
        // A composed file with neurons costing a unit the declaring neuron's
        // cost does not name: those calls are always refused (10.5).
        if let crate::net::Kind::Network(rel) = &n.kind {
            let inner_units = units_of_files(&dir.join(rel), &mut HashSet::new());
            if let (Some(cost), Some(units)) = (&n.opts.cost, inner_units) {
                let lacks: Vec<String> = units.iter().filter(|u| !cost.iter().any(|(cu, _)| cu == *u)).cloned().collect();
                if !lacks.is_empty() {
                    let (_, cost_probe) = crate::net::anchor_point(scan, span, "cost");
                    for u in &lacks {
                        out.push(warn(
                            scan,
                            cost_probe,
                            u,
                            file,
                            &format!(
                                "the file '{}' has neurons costing the unit '{}' which the cost of '{}' does not name (those calls are always refused, 10.5)",
                                rel, u, n.id
                            ),
                        ));
                    }
                }
            }
        }
    }
    out
}

fn warn(scan: &Scan, span: Span, key: &str, file: &str, message: &str) -> Diag {
    Diag {
        error: false,
        file: Some(file.to_string()),
        pos: scan.at(span, key),
        message: message.to_string(),
    }
}

/// Is the command's program findable, as the engine would find it (2.3): a
/// name on `PATH`, a path with `/` relative to the neuron's working
/// directory, which is the `cwd` option over the declaring file's directory.
fn findable(name: &str, cwd: Option<&str>, dir: &Path) -> bool {
    if name.is_empty() {
        return false;
    }
    let base = match cwd {
        Some(c) => dir.join(c),
        None => dir.to_path_buf(),
    };
    if name.contains('/') {
        return base.join(name).exists();
    }
    match std::env::var_os("PATH") {
        Some(path) => std::env::split_paths(&path).any(|p| p.join(name).is_file()),
        None => false,
    }
}

/// The units every neuron of a composed file (and of the files it composes)
/// costs: the unit names a declaring cost should name for nothing inside to
/// be refused (10.5). A file that cannot be read or does not parse
/// contributes nothing (its errors are reported on their own).
fn units_of_files(path: &Path, seen: &mut HashSet<PathBuf>) -> Option<Vec<String>> {
    if let Ok(real) = std::fs::canonicalize(path) {
        if !seen.insert(real) {
            return None;
        }
    }
    let text = std::fs::read_to_string(path).ok()?;
    let mut diags = Vec::new();
    let net = net::check_text(&text, Some(&dir_of(path)), true, &mut diags);
    if diags.iter().any(|d| d.error) {
        return None;
    }
    let mut units: Vec<String> = Vec::new();
    for n in &net.nodes {
        if let Some(cost) = &n.opts.cost {
            for (u, _) in cost {
                if !units.contains(u) {
                    units.push(u.clone());
                }
            }
        }
        if let crate::net::Kind::Network(rel) = &n.kind {
            let more = units_of_files(&dir_of(path).join(rel), seen);
            if let Some(more) = more {
                for u in more {
                    if !units.contains(&u) {
                        units.push(u);
                    }
                }
            }
        }
    }
    Some(units)
}

fn dir_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}
