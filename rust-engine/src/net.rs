// The network file: parse and validate (SPEC section 2, the closed schema).
//
// `conform` needs the validations whose lack is a refusal (the check errors of
// 2.7); the warnings of 2.7 (a cost above the file's ceiling, executable
// look-ups) and their positions come with the `check` command, which is not in
// M0. Every error is text.
use crate::fixture::Units;
use crate::json::{parse, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub const MAX_I: i64 = 9_007_199_254_740_991; // 2^53 - 1

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Source,
    Run(Vec<String>),
    Serve(Vec<String>),
    Http(String),
    Human,
    Network(String),
}

#[derive(Debug, Clone, Default)]
pub struct Opts {
    pub threshold: Option<u64>,
    pub within: Option<u64>,
    pub max_hops: Option<u64>,
    pub timeout: Option<u64>,
    pub env: Vec<String>,
    pub effect: bool,
    pub cost: Option<Units>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub desc: String,
    pub kind: Kind,
    pub opts: Opts,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub from: String,
    pub to: String,
}

/// One network file and where it lives: the real host needs the declaring
/// file's directory, because a neuron's `cwd` defaults to it (SPEC 2.4).
#[derive(Debug, Clone)]
pub struct NetFile {
    pub net: Network,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Network {
    pub name: String,
    pub description: String,
    pub budget: Option<Units>,
    pub entry: Option<String>,
    pub exit: Option<String>,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
}

impl Network {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn source(&self) -> bool {
        self.nodes.iter().any(|n| n.kind == Kind::Source)
    }
    /// Outgoing `to` targets of `id` in declaration order.
    pub fn out(&self, id: &str) -> Vec<String> {
        self.links.iter().filter(|l| l.from == id).map(|l| l.to.clone()).collect()
    }
}

/// Parse and validate a network file. `used_as_composed` adds the composed
/// restrictions of 2.6: the file MUST have `entry` and `exit`, MUST NOT
/// contain a `source` or a neuron with `effect`, and `exit` MUST NOT have
/// outgoing links.
pub fn load(path: &str, used_as_composed: bool) -> Result<Network, Vec<String>> {
    let text = std::fs::read_to_string(path).map_err(|e| vec![format!("{}: cannot read ({})", path, e)])?;
    let v = parse(&text).map_err(|e| vec![format!("{}: not JSON ({})", path, e)])?;
    check(&v, used_as_composed).map_err(|errs| errs.into_iter().map(|e| format!("{}: {}", path, e)).collect::<Vec<String>>())
}

pub fn check(v: &Value, used_as_composed: bool) -> Result<Network, Vec<String>> {
    let mut errs = Vec::new();
    let net = from_json(v, &mut errs);
    if errs.is_empty() {
        check_graph(&net, &mut errs);
        check_cycles(&net, &mut errs);
        check_budget(&net, &mut errs);
        composed_rules(&net, used_as_composed, &mut errs);
    }
    if errs.is_empty() {
        Ok(net)
    } else {
        Err(errs)
    }
}

fn whole_number(v: &Value) -> bool {
    match v.as_i64() {
        Some(i) => (0..=MAX_I).contains(&i),
        None => false,
    }
}

fn is_id(name: &str) -> bool {
    let mut cs = name.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && name.len() <= 64
}

/// A literal string: the schema's `$defs/literal` refuses `${...}` and
/// `{{...}}` where a literal is expected.
fn is_literal(s: &str) -> bool {
    !s.contains("${") && !s.contains("{{")
}

fn is_env_name(s: &str) -> bool {
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn command(v: &Value, errs: &mut Vec<String>, what: &str) -> Vec<String> {
    let mut out = Vec::new();
    match v.as_arr() {
        Some(items) if !items.is_empty() => {
            for item in items {
                match item.as_str() {
                    Some(s) if is_literal(s) => out.push(s.to_string()),
                    _ => errs.push(format!("{}: a command is an array of literal strings", what)),
                }
            }
        }
        _ => errs.push(format!("{}: a command is a non-empty array", what)),
    }
    out
}

fn units(v: &Value, errs: &mut Vec<String>, what: &str) -> Option<Units> {
    let entries = match v {
        Value::Obj(entries) => entries,
        _ => {
            errs.push(format!("{} is not an object", what));
            return None;
        }
    };
    let mut out: Units = Vec::new();
    for (u, amt) in entries {
        if !is_id(u) {
            errs.push(format!("{}: unit '{}' is not an id", what, u));
        }
        match amt {
            _ if whole_number(amt) => {}
            _ => errs.push(format!("{}: '{}' is not a whole number from 0 to 2^53 - 1", what, u)),
        }
        out.push((u.clone(), amt.as_i64().unwrap_or(0)));
    }
    if out.is_empty() {
        errs.push(format!("{} is empty", what));
    }
    Some(out)
}

fn from_json(v: &Value, errs: &mut Vec<String>) -> Network {
    let empty = || Network {
        name: String::new(),
        description: String::new(),
        budget: None,
        entry: None,
        exit: None,
        nodes: Vec::new(),
        links: Vec::new(),
    };
    if !v.is_obj() {
        errs.push("the network file is not a JSON object".into());
        return empty();
    }
    for k in v.keys() {
        if !matches!(
            k,
            "$schema" | "version" | "name" | "description" | "entry" | "exit" | "neurons" | "links" | "budget"
        ) {
            errs.push(format!("unknown top-level field '{}'", k));
        }
    }
    match v.get("version") {
        None => errs.push("version is required".into()),
        Some(Value::Int(0)) => {}
        Some(_) => errs.push("version must be 0".into()),
    }
    let name = match v.get("name") {
        None => String::new(),
        Some(Value::Str(s)) => {
            if s.is_empty() || s.len() > 64 {
                errs.push("name must be 1 to 64 characters".into());
            }
            s.clone()
        }
        Some(_) => {
            errs.push("name must be a string".into());
            String::new()
        }
    };
    let description = match v.get("description") {
        None => String::new(),
        Some(Value::Str(s)) => s.clone(),
        Some(_) => {
            errs.push("description must be a string".into());
            String::new()
        }
    };
    let budget = v.get("budget").and_then(|x| units(x, errs, "budget"));
    let mut entry = None;
    let mut exit = None;
    for (key, slot) in [("entry", &mut entry), ("exit", &mut exit)] {
        match v.get(key) {
            Some(Value::Str(s)) if is_id(s) => *slot = Some(s.clone()),
            Some(_) => errs.push(format!("{} must be a neuron id", key)),
            None => {}
        }
    }
    if entry.is_some() != exit.is_some() {
        errs.push("entry and exit are both or neither".into());
    }

    let mut nodes = Vec::new();
    let mut seen_ids = HashSet::new();
    match v.get("neurons") {
        Some(Value::Arr(items)) if !items.is_empty() => {
            for (i, nv) in items.iter().enumerate() {
                let node = neuron(nv, i, errs);
                if !node.id.is_empty() && !seen_ids.insert(node.id.clone()) {
                    errs.push(format!("the id '{}' is used twice", node.id));
                }
                nodes.push(node);
            }
        }
        Some(Value::Arr(_)) => errs.push("neurons is an empty array".into()),
        None => errs.push("neurons is required".into()),
        Some(_) => errs.push("neurons is a non-empty array".into()),
    }
    let mut links = Vec::new();
    if let Some(lv) = v.get("links") {
        match lv.as_arr() {
            Some(items) => {
                for (i, li) in items.iter().enumerate() {
                    links.push(link(li, i, errs));
                }
            }
            None => errs.push("links is an array".into()),
        }
    }
    Network {
        name,
        description,
        budget,
        entry,
        exit,
        nodes,
        links,
    }
}

fn link(lv: &Value, i: usize, errs: &mut Vec<String>) -> Link {
    if !lv.is_obj() {
        errs.push(format!("links[{}] is not an object", i));
        return Link {
            from: String::new(),
            to: String::new(),
        };
    }
    for k in lv.keys() {
        if k != "from" && k != "to" {
            errs.push(format!("links[{}]: unknown field '{}'", i, k));
        }
    }
    let mut get = |key: &str| -> String {
        match lv.get(key) {
            Some(Value::Str(s)) if is_id(s) => s.clone(),
            None => {
                errs.push(format!("links[{}]: '{}' is required", i, key));
                String::new()
            }
            Some(_) => {
                errs.push(format!("links[{}]: '{}' must be a neuron id", i, key));
                String::new()
            }
        }
    };
    let from = get("from");
    let to = get("to");
    Link { from, to }
}

fn neuron(nv: &Value, i: usize, errs: &mut Vec<String>) -> Node {
    if !nv.is_obj() {
        errs.push(format!("neurons[{}] is not an object", i));
        return empty_node(Kind::Source);
    }
    let id = match nv.get("id") {
        Some(Value::Str(s)) if is_id(s) => s.clone(),
        _ => {
            errs.push(format!("neurons[{}]: id is missing or not an id", i));
            String::new()
        }
    };
    let what = format!("neurons[{}] ({})", i, id);
    let desc = match nv.get("description") {
        None | Some(Value::Str(_)) => nv.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
        Some(_) => {
            errs.push(format!("{}: description must be a string", what));
            String::new()
        }
    };
    let kinds = ["source", "run", "serve", "http", "human", "network"]
        .iter()
        .filter(|k| nv.has(k))
        .count();
    if kinds != 1 {
        errs.push(format!("{}: a neuron has exactly one kind field", what));
    }
    let kind = if nv.has("source") {
        kind_value(nv, "source", Kind::Source, &what, errs)
    } else if nv.has("run") {
        kind_value(nv, "run", Kind::Run(Vec::new()), &what, errs)
    } else if nv.has("serve") {
        kind_value(nv, "serve", Kind::Serve(Vec::new()), &what, errs)
    } else if nv.has("http") {
        kind_value(nv, "http", Kind::Http(String::new()), &what, errs)
    } else if nv.has("human") {
        kind_value(nv, "human", Kind::Human, &what, errs)
    } else if nv.has("network") {
        kind_value(nv, "network", Kind::Network(String::new()), &what, errs)
    } else {
        Kind::Source
    };
    let is_source = matches!(kind, Kind::Source);
    if is_source {
        // Only a source: no options at all beyond id and description.
        for k in nv.keys() {
            if !matches!(k, "id" | "description" | "source") {
                errs.push(format!("{}: option '{}' is not allowed on a source", what, k));
            }
        }
        return Node {
            id,
            desc,
            kind,
            opts: Opts::default(),
        };
    }
    let is_network = matches!(kind, Kind::Network(_));
    let is_command = matches!(kind, Kind::Run(_) | Kind::Serve(_));
    for k in nv.keys() {
        if matches!(k, "id" | "description" | "source" | "run" | "serve" | "http" | "human" | "network") {
            continue;
        }
        let allowed = match k {
            "threshold" | "within" => !is_network,
            "maxHops" | "timeout" | "cost" => true,
            "effect" => !is_network,
            "env" | "cwd" => is_command,
            _ => false,
        };
        if !allowed {
            errs.push(format!("{}: option '{}' is not allowed here", what, k));
        }
    }
    let mut opts = Opts::default();
    for (key, min, slot) in [("threshold", 1u64, 0), ("maxHops", 0, 1)] {
        if let Some(x) = nv.get(key) {
            match x.as_u64() {
                Some(n) if n >= min => {
                    match slot {
                        0 => opts.threshold = Some(n),
                        _ => opts.max_hops = Some(n),
                    };
                }
                _ => errs.push(format!("{}: {} is an integer at least {}", what, key, min)),
            }
        }
    }
    if let Some(x) = nv.get("within") {
        match x.as_u64() {
            Some(n) if (1..=3_600_000).contains(&n) => opts.within = Some(n),
            _ => errs.push(format!("{}: within is an integer ms from 1 to 3600000", what)),
        }
        if opts.threshold.is_none() {
            errs.push(format!("{}: within is allowed only with threshold", what));
        }
    }
    if let Some(x) = nv.get("timeout") {
        let cap = if matches!(kind, Kind::Human) { 86_400_000 } else { 3_600_000 };
        match x.as_u64() {
            Some(n) if (1..=cap).contains(&n) => opts.timeout = Some(n),
            _ => {
                let cap_label = if matches!(kind, Kind::Human) {
                    "at most 86400000 (a day) on human"
                } else {
                    "at most 3600000 (an hour)"
                };
                errs.push(format!("{}: timeout is an integer ms at least 1 and {}", what, cap_label))
            }
        }
    }
    if let Some(x) = nv.get("env") {
        match x.as_arr() {
            Some(items) => {
                let mut seen = HashSet::new();
                for item in items {
                    match item.as_str() {
                        Some(s) if is_env_name(s) && !s.is_empty() => {
                            if !seen.insert(s.to_string()) {
                                errs.push(format!("{}: env names '{}' twice", what, s));
                            }
                        }
                        _ => errs.push(format!("{}: env is an array of unique names", what)),
                    }
                }
                opts.env = items
                    .iter()
                    .filter_map(|x| x.as_str())
                    .filter(|s| is_env_name(s))
                    .map(|s| s.to_string())
                    .collect();
            }
            None => errs.push(format!("{}: env is an array of names", what)),
        }
    }
    if let Some(x) = nv.get("cwd") {
        match x {
            Value::Str(s) if is_literal(s) => opts.cwd = Some(s.clone()),
            _ => errs.push(format!("{}: cwd is a path", what)),
        }
    }
    if let Some(x) = nv.get("effect") {
        match x.as_bool() {
            Some(true) => opts.effect = true,
            _ => errs.push(format!("{}: effect is true", what)),
        }
    }
    if let Some(x) = nv.get("cost") {
        opts.cost = units(x, errs, &format!("{}: cost", what));
    }
    Node { id, desc, kind, opts }
}

fn empty_node(kind: Kind) -> Node {
    Node {
        id: String::new(),
        desc: String::new(),
        kind,
        opts: Opts::default(),
    }
}

fn kind_value(nv: &Value, field: &str, placeholder: Kind, what: &str, errs: &mut Vec<String>) -> Kind {
    let v = nv.get(field).unwrap();
    match (field, v) {
        ("source", Value::Bool(true)) => Kind::Source,
        ("human", Value::Bool(true)) => Kind::Human,
        ("run", _) => Kind::Run(command(v, errs, what)),
        ("serve", _) => Kind::Serve(command(v, errs, what)),
        ("http", Value::Str(s)) if is_literal(s) && (s.starts_with("http://") || s.starts_with("https://")) => Kind::Http(s.clone()),
        ("network", Value::Str(s)) if is_literal(s) => Kind::Network(s.clone()),
        _ => {
            errs.push(format!("{}: bad {} value", what, field));
            placeholder
        }
    }
}

fn check_graph(net: &Network, errs: &mut Vec<String>) {
    let ids: HashSet<&str> = net.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut pairs: HashSet<(String, String)> = HashSet::new();
    for (i, l) in net.links.iter().enumerate() {
        if !ids.contains(l.from.as_str()) || !ids.contains(l.to.as_str()) {
            // Point at the end the file does not have, nearest-id suggestion
            // included (2.7).
            let unknown = if ids.contains(l.from.as_str()) { &l.to } else { &l.from };
            let near = nearest(&net.nodes, unknown);
            errs.push(format!(
                "links[{}]: '{}' names a neuron the file does not have (the nearest id is '{}')",
                i, unknown, near
            ));
            continue;
        }
        if let Some(n) = net.node(&l.to) {
            if n.kind == Kind::Source {
                errs.push(format!("links[{}]: a link into the source '{}'", i, l.to));
            }
        }
        if !pairs.insert((l.from.clone(), l.to.clone())) {
            errs.push(format!("links[{}]: the link {} -> {} is duplicated", i, l.from, l.to));
        }
    }
}

fn nearest(nodes: &[Node], unknown: &str) -> String {
    let mut best = ("", i64::MAX);
    for n in nodes {
        let d = edit_distance(n.id.as_str(), unknown);
        if d.1 < best.1 {
            best = (n.id.as_str(), d.1);
        }
    }
    best.0.to_string()
}

/// A small Levenshtein distance, only for the nearest-id suggestion.
fn edit_distance(a: &str, b: &str) -> (bool, i64) {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<i64> = (0..=b.len() as i64).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![(i + 1) as i64];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            let v = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            cur.push(v);
        }
        prev = cur;
    }
    (a.is_empty() || b.is_empty(), prev[b.len()])
}

/// A cycle in which no neuron has maxHops is an error (2.7): after removing
/// every maxHops neuron, a left-over cycle is exactly a cycle that never
/// passes a maxHops neuron.
fn check_cycles(net: &Network, errs: &mut Vec<String>) {
    let keep: HashSet<&str> = net
        .nodes
        .iter()
        .filter(|n| n.opts.max_hops.is_none())
        .map(|n| n.id.as_str())
        .collect();
    if !has_cycle(&keep, net) {
        return;
    }
    errs.push("a cycle in which no neuron has maxHops (every cycle must pass a neuron with maxHops)".into());
}

fn has_cycle(keep: &HashSet<&str>, net: &Network) -> bool {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for l in &net.links {
        if keep.contains(l.from.as_str()) && keep.contains(l.to.as_str()) {
            adj.entry(l.from.as_str()).or_default().push(l.to.as_str());
        }
    }
    let mut colour: HashMap<&str, u8> = HashMap::new(); // keyed by the node's name; 1 visiting, 2 done
    fn dfs<'a>(u: &'a str, adj: &HashMap<&'a str, Vec<&'a str>>, colour: &mut HashMap<&'a str, u8>) -> bool {
        if colour.get(u) == Some(&1) {
            return true;
        }
        if colour.get(u) == Some(&2) {
            return false;
        }
        colour.insert(u, 1);
        if let Some(nexts) = adj.get(u) {
            for v in nexts.clone() {
                if dfs(v, adj, colour) {
                    return true;
                }
            }
        }
        colour.insert(u, 2);
        false
    }
    for u in keep {
        if dfs(u, &adj, &mut colour) {
            return true;
        }
    }
    false
}

/// The budget-related errors of 2.7 and 10.1: a cost naming a unit the file's
/// budget lacks, and a non-source neuron without a cost in a file that has a
/// budget (a composed neuron included, 10.5).
fn check_budget(net: &Network, errs: &mut Vec<String>) {
    let Some(budget) = &net.budget else { return };
    for n in &net.nodes {
        if matches!(n.kind, Kind::Source) {
            continue; // a source is never metered (10.1)
        }
        let Some(cost) = &n.opts.cost else {
            errs.push(format!(
                "a budget requires a cost on every non-source neuron ('{}' has none, 10.1)",
                n.id
            ));
            continue;
        };
        for (u, _) in cost {
            if !budget.iter().any(|(bu, _)| bu == u) {
                errs.push(format!(
                    "the cost of '{}' names the unit '{}' which the file's budget lacks",
                    n.id, u
                ));
            }
        }
    }
}

fn composed_rules(net: &Network, used_as_composed: bool, errs: &mut Vec<String>) {
    if !used_as_composed {
        return;
    }
    if net.source() {
        errs.push("a composed file contains a source".into());
    }
    if net.nodes.iter().any(|n| n.opts.effect) {
        errs.push("a composed file contains a neuron with effect (11.5)".into());
    }
    match (&net.entry, &net.exit) {
        (Some(e), Some(x)) => {
            if net.node(e).is_none() {
                errs.push(format!("entry names a neuron the file does not have ('{}')", e));
            }
            if net.node(x).is_none() {
                errs.push(format!("exit names a neuron the file does not have ('{}')", x));
            } else if !net.out(x).is_empty() {
                errs.push("exit has outgoing links".into());
            }
        }
        _ => errs.push("a composed file must have entry and exit".into()),
    }
}
