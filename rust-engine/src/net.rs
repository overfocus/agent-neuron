// The network file: parse and validate (SPEC section 2, the closed schema).
//
// The validations are the errors of 2.7 (and the composed restrictions of
// 2.6); `conform` and `run` need exactly those (a file that fails one refuses
// to run). The `check` command (check.rs) adds the warnings of 2.7 and wants
// the file, line and column where it can: the same rules run positioned here,
// over the file's raw text (a `Scan`), each message anchored to the quoted
// key of the construct it is about.
//
// What the positions cover: a key the scan finds in the text it is anchored
// to (a neuron's `id`, a link's `from`/`to`, an option field, a units unit),
// with the spans of the `neurons`/`links` arrays walked by brace and string
// state. What they do not: rules about the file as a whole (a required key
// that is absent, the cycle, the `budget`-demands-costs rule) anchor to the
// key they name when it is present in the text and to `1:1` otherwise, and a
// key the scan cannot find inside its span (an odd quote, an anchor that is
// only in the parsed value) falls back to the span's own start. Columns count
// characters, not bytes or graphemes.
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
    /// Where a drawing puts the neuron (SPEC 2.2, decision 0011): a
    /// descriptive field, like `desc`. Nothing the engine does reads it.
    pub at: Option<[f64; 3]>,
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

// -- Findings ----------------------------------------------------------------

/// A place in a file: the line first, then the column. Both 1-based; the
/// column counts characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub column: usize,
}

/// One finding of the check (2.7): an error stops a run (`check` exits 1 for
/// one), a warning does not. A position is present when the file's text named
/// the anchor; what sits at `1:1` is in the Scan documentation above.
#[derive(Debug, Clone)]
pub struct Diag {
    pub error: bool,
    pub file: Option<String>,
    pub pos: Option<Pos>,
    pub message: String,
}

impl Diag {
    /// The one line `check` prints: `error file:line:column: message`.
    pub fn line_of(&self) -> String {
        let lead = if self.error { "error" } else { "warning" };
        match (&self.file, &self.pos) {
            (Some(f), Some(p)) => format!("{} {}:{}:{}: {}", lead, f, p.line, p.column, self.message),
            (Some(f), None) => format!("{} {}: {}", lead, f, self.message),
            _ => format!("{}: {}", lead, self.message),
        }
    }
}

/// A construct's span in the file's text: a neuron object, a link, the whole
/// file. Diagnostics outside any such span sit at the file's (1, 1).
pub type Span = (usize, usize);

/// The file's own span (a message about the file, wherever its key sits).
pub const FILE_SPAN: Span = (0, usize::MAX);
/// No span known: the diagnostic's position becomes the file's (1, 1).
pub const NONE_SPAN: Span = (0, 0);

/// The file's raw text, searched for the quoted keys each diagnostic anchors
/// to. Offsets are in characters.
pub struct Scan {
    text: Vec<char>,
}

impl Scan {
    pub fn new(text: &str) -> Scan {
        Scan {
            text: text.chars().collect(),
        }
    }

    /// The text's length in characters.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// The offset of the first `token` (with its quotes) in [from, to). `None`
    /// when the span holds no such token.
    pub fn token(&self, from: usize, to: usize, token: &str) -> Option<usize> {
        let pat: Vec<char> = format!("\"{}\"", token).chars().collect();
        let hay = &self.text;
        let to = to.min(hay.len());
        if from >= to || to < pat.len() || token.len() + 2 > to - from {
            return None;
        }
        for i in from..=to - pat.len() {
            if hay[i..i + pat.len()] == pat[..] {
                return Some(i);
            }
        }
        None
    }

    /// The 1-based line and column of a character offset.
    pub fn pos(&self, off: usize) -> Pos {
        let off = off.min(self.text.len());
        let mut line = 1;
        let mut column = 1;
        for (i, c) in self.text.iter().enumerate() {
            if i >= off {
                break;
            }
            if *c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        Pos { line, column }
    }

    /// Where a span diagnostic points: the quoted `key` inside it, else the
    /// span's own start.
    pub fn at(&self, span: Span, key: &str) -> Option<Pos> {
        Some(self.pos(self.token(span.0, span.1, key).unwrap_or(span.0)))
    }

    fn str_state(c: char, in_str: &mut bool, esc: &mut bool) {
        if *in_str {
            if *esc {
                *esc = false;
            } else if c == '\\' {
                *esc = true;
            } else if c == '"' {
                *in_str = false;
            }
        } else if c == '"' {
            *in_str = true;
        }
    }

    /// The spans of the elements of the array a key names (`"neurons": [` or
    /// `"links": [`), in order, objects and the rest alike, so the index of an
    /// element matches the index of the value the parser built. `[]` when the
    /// key is absent or its value is not an array. The first occurrence of
    /// the key in the file is the one consulted.
    pub fn array_spans(&self, key: &str) -> Vec<Span> {
        let hay = &self.text;
        let Some(t) = self.token(0, hay.len(), key) else {
            return Vec::new();
        };
        let Some(open) = self.open_bracket(t + key.len() + 2) else {
            return Vec::new();
        };
        // The matching close bracket, strings and nesting tracked.
        let mut end = hay.len();
        let mut depth = 0usize;
        let (mut in_str, mut esc) = (false, false);
        let mut i = open;
        while i < hay.len() {
            let c = hay[i];
            if in_str {
                Self::str_state(c, &mut in_str, &mut esc);
                i += 1;
                continue;
            }
            match c {
                '"' => in_str = true,
                '[' | '{' => depth += 1,
                ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        // The interior, split at top-level commas into element spans.
        let interior = open + 1;
        let mut spans = Vec::new();
        let (mut in_str, mut esc) = (false, false);
        let mut depth = 0usize;
        let mut start = interior;
        let mut have_any = false;
        let mut i = interior;
        while i < end {
            let c = hay[i];
            if in_str {
                Self::str_state(c, &mut in_str, &mut esc);
                i += 1;
                continue;
            }
            match c {
                '"' => in_str = true,
                '[' | '{' => depth += 1,
                ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    spans.push(self.trimmed(start, i));
                    start = i + 1;
                    have_any = true;
                }
                _ => {}
            }
            i += 1;
        }
        if have_any || hay[start.min(end)..end].iter().any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r')) {
            spans.push(self.trimmed(start, end));
        }
        spans
    }

    /// From `from`, past whitespace, the offset of a `[`.
    fn open_bracket(&self, from: usize) -> Option<usize> {
        let hay = &self.text;
        let mut i = from;
        while i < hay.len() {
            match hay[i] {
                ' ' | '\t' | '\n' | '\r' | ':' => i += 1,
                '[' => return Some(i),
                _ => return None,
            }
        }
        None
    }

    /// A span tight to its first and last non-whitespace character.
    fn trimmed(&self, from: usize, to: usize) -> Span {
        let hay = &self.text;
        let mut a = from;
        while a < to && matches!(hay[a], ' ' | '\t' | '\n' | '\r') {
            a += 1;
        }
        if a == to {
            return (a, a);
        }
        let mut b = to;
        while b > a && matches!(hay[b - 1], ' ' | '\t' | '\n' | '\r') {
            b -= 1;
        }
        (a, b)
    }

    /// The span of the value a key names, tight inside a `{...}` or `[...]`
    /// (where the values the units rules name are searched), or a point span
    /// for a scalar. `from` is the offset of the key's own `"key"` token.
    pub fn value_span(&self, from: usize, key: &str) -> Span {
        let hay = &self.text;
        let mut i = from + key.len() + 2;
        while i < hay.len() && matches!(hay[i], ' ' | '\t' | '\n' | '\r' | ':') {
            i += 1;
        }
        if i >= hay.len() {
            return (from, from);
        }
        let open = i;
        if hay[open] != '{' && hay[open] != '[' {
            return (open, open);
        }
        let mut depth = 0usize;
        let (mut in_str, mut esc) = (false, false);
        while i < hay.len() {
            let c = hay[i];
            if in_str {
                Self::str_state(c, &mut in_str, &mut esc);
                i += 1;
                continue;
            }
            match c {
                '"' => in_str = true,
                '[' | '{' => depth += 1,
                ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return (open + 1, i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        (open, open)
    }
}

/// An error at the anchor `key` inside `span`: the one push the validators
/// make. (Warnings are check.rs's; they never stop a run.)
fn push(span: Span, scan: &Scan, key: &str, message: &str, out: &mut Vec<Diag>) {
    out.push(Diag {
        error: true,
        file: None,
        pos: scan.at(span, key),
        message: message.to_string(),
    });
}

/// A message with no anchor at all (the text is not JSON: there is nothing to
/// point inside).
fn bare(message: String, out: &mut Vec<Diag>) {
    out.push(Diag {
        error: true,
        file: None,
        pos: None,
        message,
    });
}

/// Parse and validate a network file (SPEC section 2, the errors of 2.7).
/// `used_as_composed` adds the composed restrictions of 2.6: the file MUST
/// have `entry` and `exit`, MUST NOT contain a `source` or a neuron with
/// `effect`, and `exit` MUST NOT have outgoing links. The strings keep their
/// `path: message` shape for the runner.
pub fn load(path: &str, used_as_composed: bool) -> Result<Network, Vec<String>> {
    let text = std::fs::read_to_string(path).map_err(|e| vec![format!("{}: cannot read ({})", path, e)])?;
    let mut diags = Vec::new();
    let net = check_text(&text, used_as_composed, &mut diags);
    let errs: Vec<String> = diags
        .iter()
        .filter(|d| d.error)
        .map(|d| format!("{}: {}", path, d.message))
        .collect();
    if errs.is_empty() {
        Ok(net)
    } else {
        Err(errs)
    }
}

/// The check of one file's text: the network tree, with the findings in
/// `diags` (only their positions here; the caller names the file). The
/// deeper rules run whenever the text parsed — the tree the parser built is
/// best effort, so a file with a broken construct is still checked for what
/// it does hold; a degenerate one (no neurons, say) passes them vacuously.
pub fn check_text(text: &str, used_as_composed: bool, diags: &mut Vec<Diag>) -> Network {
    let v = match parse(text) {
        Ok(v) => v,
        Err(e) => {
            bare(format!("not JSON ({})", e), diags);
            return empty_network();
        }
    };
    let scan = Scan::new(text);
    let net = from_json(&v, &scan, diags);
    check_graph(&net, &scan, diags);
    check_cycles(&net, &scan, diags);
    check_budget(&net, &scan, diags);
    composed_rules(&net, used_as_composed, &scan, diags);
    net
}

fn empty_network() -> Network {
    Network {
        name: String::new(),
        description: String::new(),
        budget: None,
        entry: None,
        exit: None,
        nodes: Vec::new(),
        links: Vec::new(),
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

/// A command (the `run`/`serve` value); its diagnostics anchor to the kind's
/// field.
fn command(v: &Value, scan: &Scan, span: Span, kind: &str, what: &str, out: &mut Vec<Diag>) -> Vec<String> {
    let mut out_cmd = Vec::new();
    match v.as_arr() {
        Some(items) if !items.is_empty() => {
            for item in items {
                match item.as_str() {
                    Some(s) if is_literal(s) => out_cmd.push(s.to_string()),
                    _ => push(
                        span,
                        scan,
                        kind,
                        &format!("{}: a command is an array of literal strings", what),
                        out,
                    ),
                }
            }
        }
        _ => push(span, scan, kind, &format!("{}: a command is a non-empty array", what), out),
    }
    out_cmd
}

/// The units of a `budget` or a `cost`: the amounts are whole numbers from 0
/// to 2^53 - 1, the units are ids, the block is not empty (2.7, 10.1). The
/// diagnostics sit at the offending unit's key inside the block's span
/// (`probe`), or at `anchor` for a rule about the block itself.
fn units(v: &Value, what: &str, scan: &Scan, anchor: Span, probe: Span, key: &str, out: &mut Vec<Diag>) -> Option<Units> {
    let entries = match v {
        Value::Obj(entries) => entries,
        _ => {
            push(anchor, scan, key, &format!("{} is not an object", what), out);
            return None;
        }
    };
    let mut units: Units = Vec::new();
    for (u, amt) in entries {
        if !is_id(u) {
            push(probe, scan, u, &format!("{}: unit '{}' is not an id", what, u), out);
        }
        if !whole_number(amt) {
            push(
                probe,
                scan,
                u,
                &format!("{}: '{}' is not a whole number from 0 to 2^53 - 1", what, u),
                out,
            );
        }
        units.push((u.clone(), amt.as_i64().unwrap_or(0)));
    }
    if units.is_empty() {
        push(anchor, scan, key, &format!("{} is empty", what), out);
    }
    Some(units)
}

/// The key's anchor (a point span) and its value's probe span: the pair the
/// units rules and check.rs anchor to.
pub fn anchor_point(scan: &Scan, probe: Span, key: &str) -> (Span, Span) {
    let t = scan.token(probe.0, probe.1, key).unwrap_or(probe.0);
    ((t, t), scan.value_span(t, key))
}

fn from_json(v: &Value, scan: &Scan, out: &mut Vec<Diag>) -> Network {
    if !v.is_obj() {
        bare("the network file is not a JSON object".to_string(), out);
        return empty_network();
    }
    let hay = scan.len();
    for k in v.keys() {
        if !matches!(
            k,
            "$schema" | "version" | "name" | "description" | "entry" | "exit" | "neurons" | "links" | "budget"
        ) {
            push(FILE_SPAN, scan, k, &format!("unknown top-level field '{}'", k), out);
        }
    }
    match v.get("version") {
        None => push(FILE_SPAN, scan, "version", "version is required", out),
        Some(Value::Int(0)) => {}
        Some(_) => push(FILE_SPAN, scan, "version", "version must be 0", out),
    }
    let name = match v.get("name") {
        None => String::new(),
        Some(Value::Str(s)) => {
            if s.is_empty() || s.len() > 64 {
                push(FILE_SPAN, scan, "name", "name must be 1 to 64 characters", out);
            }
            s.clone()
        }
        Some(_) => {
            push(FILE_SPAN, scan, "name", "name must be a string", out);
            String::new()
        }
    };
    let description = match v.get("description") {
        None => String::new(),
        Some(Value::Str(s)) => s.clone(),
        Some(_) => {
            push(FILE_SPAN, scan, "description", "description must be a string", out);
            String::new()
        }
    };
    let (budget_anchor, budget_probe) = crate::net::anchor_point(scan, (0, hay), "budget");
    let budget = v
        .get("budget")
        .and_then(|x| units(x, "budget", scan, budget_anchor, budget_probe, "budget", out));
    let (entry, exit) = entry_exit(v, scan, hay, out);
    let neuron_spans = scan.array_spans("neurons");
    let mut nodes = Vec::new();
    let mut seen_ids = HashSet::new();
    match v.get("neurons") {
        Some(Value::Arr(items)) if !items.is_empty() => {
            for (i, nv) in items.iter().enumerate() {
                let span = neuron_spans.get(i).copied().unwrap_or(NONE_SPAN);
                let node = neuron(nv, i, scan, span, out);
                if !node.id.is_empty() && !seen_ids.insert(node.id.clone()) {
                    push(span, scan, "id", &format!("the id '{}' is used twice", node.id), out);
                }
                nodes.push(node);
            }
        }
        Some(Value::Arr(_)) => push(FILE_SPAN, scan, "neurons", "neurons is an empty array", out),
        None => push(FILE_SPAN, scan, "neurons", "neurons is required", out),
        Some(_) => push(FILE_SPAN, scan, "neurons", "neurons is a non-empty array", out),
    }
    let link_spans = scan.array_spans("links");
    let mut links = Vec::new();
    if let Some(lv) = v.get("links") {
        match lv.as_arr() {
            Some(items) => {
                for (i, li) in items.iter().enumerate() {
                    let span = link_spans.get(i).copied().unwrap_or(NONE_SPAN);
                    links.push(link(li, i, scan, span, out));
                }
            }
            None => push(FILE_SPAN, scan, "links", "links is an array", out),
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

/// The `entry`/`exit` pair: both a neuron id or both absent, and set together
/// (2.6 pairs them).
fn entry_exit(v: &Value, scan: &Scan, hay: usize, out: &mut Vec<Diag>) -> (Option<String>, Option<String>) {
    let mut entry = None;
    let mut exit = None;
    for (key, slot) in [("entry", &mut entry), ("exit", &mut exit)] {
        match v.get(key) {
            Some(Value::Str(s)) if is_id(s) => *slot = Some(s.clone()),
            Some(_) => push(FILE_SPAN, scan, key, &format!("{} must be a neuron id", key), out),
            None => {}
        }
    }
    if entry.is_some() != exit.is_some() {
        push(FILE_SPAN, scan, "entry", "entry and exit are both or neither", out);
    }
    let _ = hay;
    (entry, exit)
}

fn link(lv: &Value, i: usize, scan: &Scan, span: Span, out: &mut Vec<Diag>) -> Link {
    if !lv.is_obj() {
        push(span, scan, "links", &format!("links[{}] is not an object", i), out);
        return Link {
            from: String::new(),
            to: String::new(),
        };
    }
    for k in lv.keys() {
        if k != "from" && k != "to" {
            push(span, scan, k, &format!("links[{}]: unknown field '{}'", i, k), out);
        }
    }
    let mut get = |key: &str| -> String {
        match lv.get(key) {
            Some(Value::Str(s)) if is_id(s) => s.clone(),
            None => {
                push(span, scan, key, &format!("links[{}]: '{}' is required", i, key), out);
                String::new()
            }
            Some(_) => {
                push(span, scan, key, &format!("links[{}]: '{}' must be a neuron id", i, key), out);
                String::new()
            }
        }
    };
    let from = get("from");
    let to = get("to");
    Link { from, to }
}

fn neuron(nv: &Value, i: usize, scan: &Scan, span: Span, out: &mut Vec<Diag>) -> Node {
    if !nv.is_obj() {
        push(span, scan, "neurons", &format!("neurons[{}] is not an object", i), out);
        return empty_node(Kind::Source);
    }
    let id = match nv.get("id") {
        Some(Value::Str(s)) if is_id(s) => s.clone(),
        _ => {
            push(span, scan, "id", &format!("neurons[{}]: id is missing or not an id", i), out);
            String::new()
        }
    };
    let what = format!("neurons[{}] ({})", i, id);
    let desc = match nv.get("description") {
        None | Some(Value::Str(_)) => nv.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
        Some(_) => {
            push(span, scan, "description", &format!("{}: description must be a string", what), out);
            String::new()
        }
    };
    // `at` (decision 0011): three finite numbers, for a drawing. Kept on the
    // node and read by nothing that routes.
    let at = match nv.get("at") {
        None => None,
        Some(Value::Arr(xs)) if xs.len() == 3 => {
            let nums: Vec<f64> = xs
                .iter()
                .filter_map(|x| match x {
                    Value::Int(n) => Some(*n as f64),
                    Value::Float(f) => Some(*f),
                    _ => None,
                })
                .filter(|f| f.is_finite() && f.abs() <= 1_000_000.0)
                .collect();
            if nums.len() == 3 {
                Some([nums[0], nums[1], nums[2]])
            } else {
                push(
                    span,
                    scan,
                    "at",
                    &format!("{}: at is three numbers from -1000000 to 1000000", what),
                    out,
                );
                None
            }
        }
        Some(_) => {
            push(span, scan, "at", &format!("{}: at is three numbers [x, y, z]", what), out);
            None
        }
    };
    let kind_key = ["source", "run", "serve", "http", "human", "network"]
        .iter()
        .find(|k| nv.has(k))
        .copied()
        .unwrap_or("");
    if ids_of_kind(nv) != 1 {
        push(span, scan, "id", &format!("{}: a neuron has exactly one kind field", what), out);
    }
    let kind = kind_value(nv, kind_key, scan, span, &what, out);
    let is_source = matches!(kind, Kind::Source);
    if is_source {
        // Only a source: no options at all beyond id and description.
        for k in nv.keys() {
            if !matches!(k, "id" | "description" | "at" | "source") {
                push(span, scan, k, &format!("{}: option '{}' is not allowed on a source", what, k), out);
            }
        }
        return Node {
            id,
            desc,
            at,
            kind,
            opts: Opts::default(),
        };
    }
    let is_network = matches!(kind, Kind::Network(_));
    let is_command = matches!(kind, Kind::Run(_) | Kind::Serve(_));
    for k in nv.keys() {
        if matches!(
            k,
            "id" | "description" | "at" | "source" | "run" | "serve" | "http" | "human" | "network"
        ) {
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
            push(span, scan, k, &format!("{}: option '{}' is not allowed here", what, k), out);
        }
    }
    let mut opts = Opts::default();
    for (key, min, which) in [("threshold", 1u64, 0), ("maxHops", 0, 1)] {
        if let Some(x) = nv.get(key) {
            match x.as_u64() {
                Some(n) if n >= min => {
                    match which {
                        0 => opts.threshold = Some(n),
                        _ => opts.max_hops = Some(n),
                    };
                }
                _ => push(span, scan, key, &format!("{}: {} is an integer at least {}", what, key, min), out),
            }
        }
    }
    if let Some(x) = nv.get("within") {
        match x.as_u64() {
            Some(n) if (1..=3_600_000).contains(&n) => opts.within = Some(n),
            _ => push(
                span,
                scan,
                "within",
                &format!("{}: within is an integer ms from 1 to 3600000", what),
                out,
            ),
        }
        if opts.threshold.is_none() {
            push(
                span,
                scan,
                "within",
                &format!("{}: within is allowed only with threshold", what),
                out,
            );
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
                push(
                    span,
                    scan,
                    "timeout",
                    &format!("{}: timeout is an integer ms at least 1 and {}", what, cap_label),
                    out,
                )
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
                                push(span, scan, "env", &format!("{}: env names '{}' twice", what, s), out);
                            }
                        }
                        _ => push(span, scan, "env", &format!("{}: env is an array of unique names", what), out),
                    }
                }
                opts.env = items
                    .iter()
                    .filter_map(|x| x.as_str())
                    .filter(|s| is_env_name(s))
                    .map(|s| s.to_string())
                    .collect();
            }
            None => push(span, scan, "env", &format!("{}: env is an array of names", what), out),
        }
    }
    if let Some(x) = nv.get("cwd") {
        match x {
            Value::Str(s) if is_literal(s) => opts.cwd = Some(s.clone()),
            _ => push(span, scan, "cwd", &format!("{}: cwd is a path", what), out),
        }
    }
    if let Some(x) = nv.get("effect") {
        match x.as_bool() {
            Some(true) => opts.effect = true,
            _ => push(span, scan, "effect", &format!("{}: effect is true", what), out),
        }
    }
    if let Some(x) = nv.get("cost") {
        let (cost_anchor, cost_probe) = crate::net::anchor_point(scan, (span.0, span.1.min(scan.len())), "cost");
        opts.cost = units(x, &format!("{}: cost", what), scan, cost_anchor, cost_probe, "cost", out);
    }
    Node { id, desc, at, kind, opts }
}

/// How many kind fields the neuron carries.
fn ids_of_kind(nv: &Value) -> usize {
    ["source", "run", "serve", "http", "human", "network"]
        .iter()
        .filter(|k| nv.has(k))
        .count()
}

fn empty_node(kind: Kind) -> Node {
    Node {
        id: String::new(),
        desc: String::new(),
        at: None,
        kind,
        opts: Opts::default(),
    }
}

/// A kind's value: `source`/`human` true, a command an array, `http` an
/// `http(s)://...` string, `network` a name. The diagnostics sit at the kind's
/// field (2.7's closed schema).
fn kind_value(nv: &Value, field: &str, scan: &Scan, span: Span, what: &str, out: &mut Vec<Diag>) -> Kind {
    let kind_span = match scan.token(span.0, span.1.min(scan.len()), field) {
        Some(t) => (t, span.1),
        None => span,
    };
    let Some(v) = nv.get(field) else {
        return Kind::Source; // zero kind fields: counted above
    };
    match (field, v) {
        ("source", Value::Bool(true)) => Kind::Source,
        ("human", Value::Bool(true)) => Kind::Human,
        ("run", _) => Kind::Run(command(v, scan, kind_span, field, what, out)),
        ("serve", _) => Kind::Serve(command(v, scan, kind_span, field, what, out)),
        ("http", Value::Str(s)) if is_literal(s) && (s.starts_with("http://") || s.starts_with("https://")) => Kind::Http(s.clone()),
        ("network", Value::Str(s)) if is_literal(s) => Kind::Network(s.clone()),
        _ => {
            push(kind_span, scan, field, &format!("{}: bad {} value", what, field), out);
            if field == "run" {
                Kind::Run(Vec::new())
            } else if field == "serve" {
                Kind::Serve(Vec::new())
            } else if field == "http" {
                Kind::Http(String::new())
            } else if field == "network" {
                Kind::Network(String::new())
            } else {
                Kind::Source
            }
        }
    }
}

fn check_graph(net: &Network, scan: &Scan, out: &mut Vec<Diag>) {
    let ids: HashSet<&str> = net.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut pairs: HashSet<(String, String)> = HashSet::new();
    let link_spans = scan.array_spans("links");
    for (i, l) in net.links.iter().enumerate() {
        let span = link_spans.get(i).copied().unwrap_or(NONE_SPAN);
        if !ids.contains(l.from.as_str()) || !ids.contains(l.to.as_str()) {
            // Point at the end the file does not have, nearest-id suggestion
            // included (2.7).
            let (unknown, at_key) = if ids.contains(l.from.as_str()) {
                (&l.to, &l.to)
            } else {
                (&l.from, &l.from)
            };
            let near = nearest(&net.nodes, unknown);
            push(
                span,
                scan,
                at_key,
                &format!(
                    "links[{}]: '{}' names a neuron the file does not have (the nearest id is '{}')",
                    i, unknown, near
                ),
                out,
            );
            continue;
        }
        if let Some(n) = net.node(&l.to) {
            if n.kind == Kind::Source {
                push(span, scan, "to", &format!("links[{}]: a link into the source '{}'", i, l.to), out);
            }
        }
        if !pairs.insert((l.from.clone(), l.to.clone())) {
            push(
                span,
                scan,
                "from",
                &format!("links[{}]: the link {} -> {} is duplicated", i, l.from, l.to),
                out,
            );
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
/// passes a maxHops neuron. The links block is where the diagnostic points.
fn check_cycles(net: &Network, scan: &Scan, out: &mut Vec<Diag>) {
    let keep: HashSet<&str> = net
        .nodes
        .iter()
        .filter(|n| n.opts.max_hops.is_none())
        .map(|n| n.id.as_str())
        .collect();
    if !has_cycle(&keep, net) {
        return;
    }
    push(
        FILE_SPAN,
        scan,
        "links",
        "a cycle in which no neuron has maxHops (every cycle must pass a neuron with maxHops)",
        out,
    );
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
/// budget (a composed neuron included, 10.5). The diagnostics point at the
/// offending cost's unit, or at the neuron for a cost that is missing.
fn check_budget(net: &Network, scan: &Scan, out: &mut Vec<Diag>) {
    let Some(budget) = &net.budget else { return };
    let neuron_spans = scan.array_spans("neurons");
    for (i, n) in net.nodes.iter().enumerate() {
        if matches!(n.kind, Kind::Source) {
            continue; // a source is never metered (10.1)
        }
        let span = neuron_spans.get(i).copied().unwrap_or(NONE_SPAN);
        let Some(cost) = &n.opts.cost else {
            push(
                span,
                scan,
                "id",
                &format!("a budget requires a cost on every non-source neuron ('{}' has none, 10.1)", n.id),
                out,
            );
            continue;
        };
        for (u, _) in cost {
            if !budget.iter().any(|(bu, _)| bu == u) {
                push(
                    span,
                    scan,
                    u,
                    &format!("the cost of '{}' names the unit '{}' which the file's budget lacks", n.id, u),
                    out,
                );
            }
        }
    }
}

/// The rules a file used as a composed neuron adds (2.6). Each points at the
/// neuron (or the field) it is about.
fn composed_rules(net: &Network, used_as_composed: bool, scan: &Scan, out: &mut Vec<Diag>) {
    if !used_as_composed {
        return;
    }
    let neuron_spans = scan.array_spans("neurons");
    if net.source() {
        let first = net.nodes.iter().position(|n| n.kind == Kind::Source).unwrap_or(0);
        let span = neuron_spans.get(first).copied().unwrap_or(NONE_SPAN);
        push(span, scan, "source", "a composed file contains a source", out);
    }
    if let Some((i, _)) = net.nodes.iter().enumerate().find(|(_, n)| n.opts.effect) {
        let span = neuron_spans.get(i).copied().unwrap_or(NONE_SPAN);
        push(span, scan, "effect", "a composed file contains a neuron with effect (11.5)", out);
    }
    match (&net.entry, &net.exit) {
        (Some(e), Some(x)) => {
            if net.node(e).is_none() {
                push(
                    FILE_SPAN,
                    scan,
                    "entry",
                    &format!("entry names a neuron the file does not have ('{}')", e),
                    out,
                );
            }
            if net.node(x).is_none() {
                push(
                    FILE_SPAN,
                    scan,
                    "exit",
                    &format!("exit names a neuron the file does not have ('{}')", x),
                    out,
                );
            } else if !net.out(x).is_empty() {
                push(FILE_SPAN, scan, "exit", "exit has outgoing links", out);
            }
        }
        _ => push(FILE_SPAN, scan, "entry", "a composed file must have entry and exit", out),
    }
}
