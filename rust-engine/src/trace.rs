// The trace and the live stream (the `--trace` and `--stream` of `run`).
use crate::json::{write, Obj, Value};
use crate::net::{Kind, Network};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

pub const FORMAT: &str = "agent-neuron-trace";
pub const VERSION: i64 = 1;
pub const LIBRARY: &str = "0.0.0";
pub const ENGINE_NAME: &str = "agent-neuron-rust";

/// The trace header's kind of each network kind, as the format's `kind` lists
/// it (TRACE.md: a `network` is written `composed`).
fn kind_of(k: &Kind) -> &'static str {
    match k {
        Kind::Source => "source",
        Kind::Run(_) => "run",
        Kind::Serve(_) => "serve",
        Kind::Http(_) => "http",
        Kind::Human => "human",
        Kind::Network(_) => "composed",
    }
}

fn units_value(units: &crate::fixture::Units) -> Value {
    Value::Obj(units.iter().map(|(u, amt)| (u.clone(), Value::Int(*amt))).collect())
}

/// The trace file, per format v1 (docs/TRACE.md) and SPEC 7.1: the header
/// first, then neurons, links, events (one per line, so diffs read well), and
/// the demo's `summary` (the engine writes `{}`).
pub fn build(net: &Network, demo_fallback: &str, events: &[Value]) -> String {
    let demo = if net.name.is_empty() {
        demo_fallback.to_string()
    } else {
        net.name.clone()
    };
    let mut out = String::new();
    out.push_str("{\n");
    let kv = |k: &str, v: &Value| format!("  \"{}\": {}", k, write(v));
    let mut lines = vec![
        kv("format", &Value::Str(FORMAT.into())),
        kv("version", &Value::Int(VERSION)),
        kv("demo", &Value::Str(demo.clone())),
        kv("description", &Value::Str(net.description.clone())),
        kv("library", &Value::Str(LIBRARY.into())),
        kv("engine", &Value::Str(ENGINE_NAME.into())),
    ];
    if let Some(budget) = &net.budget {
        lines.push(kv("budget", &units_value(budget)));
    }
    // A neuron's `description` is never empty in a trace: the file's own
    // words, or `<kind> neuron` (SPEC 7.1).
    let mut neurons = String::from("  \"neurons\": [\n");
    for (i, n) in net.nodes.iter().enumerate() {
        let kind = kind_of(&n.kind);
        let mut o = Obj::new().str("id", n.id.clone()).str("kind", kind);
        let desc = if n.desc.is_empty() {
            format!("<{}> neuron", kind)
        } else {
            n.desc.clone()
        };
        o = o.str("description", desc);
        let mut options = Obj::new();
        if let Some(t) = n.opts.threshold {
            options = options.int("threshold", t as i64);
        }
        if let Some(w) = n.opts.within {
            options = options.int("within", w as i64);
        }
        if let Some(m) = n.opts.max_hops {
            options = options.int("maxHops", m as i64);
        }
        if let Value::Obj(entries) = options.build() {
            if !entries.is_empty() {
                o = o.set("options", Value::Obj(entries));
            }
        }
        if let Some(cost) = &n.opts.cost {
            o = o.set("cost", units_value(cost));
        }
        let comma = if i + 1 < net.nodes.len() { "," } else { "" };
        neurons.push_str(&format!("    {}{}\n", write(&o.build()), comma));
    }
    neurons.push_str("  ]");
    lines.push(neurons);
    let mut links = String::from("  \"links\": [\n");
    for (i, l) in net.links.iter().enumerate() {
        let o = Obj::new().str("from", l.from.clone()).str("to", l.to.clone()).build();
        let comma = if i + 1 < net.links.len() { "," } else { "" };
        links.push_str(&format!("    {}{}\n", write(&o), comma));
    }
    links.push_str("  ]");
    lines.push(links);
    let mut events_lines = String::from("  \"events\": [\n");
    for (i, ev) in events.iter().enumerate() {
        let comma = if i + 1 < events.len() { "," } else { "" };
        events_lines.push_str(&format!("    {}{}\n", write(ev), comma));
    }
    events_lines.push_str("  ]");
    lines.push(events_lines);
    lines.push("  \"summary\": {}".to_string());
    out.push_str(&lines.join(",\n"));
    out.push_str("\n}\n");
    out
}

pub fn write_trace(path: &str, net: &Network, demo_fallback: &str, events: &[Value]) -> Result<(), String> {
    let text = build(net, demo_fallback, events);
    std::fs::write(path, text).map_err(|e| format!("{}: cannot write ({})", path, e))
}

/// The events a trace file on disk holds, parsed back (only for the run's
/// events-vs-trace self-check; the format's own readers do not need this).
pub fn read_events(path: &str) -> Result<Vec<Value>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: cannot read ({})", path, e))?;
    let v = crate::json::parse(&text).map_err(|e| format!("{}: not JSON ({})", path, e))?;
    match v.get("events") {
        Some(Value::Arr(items)) => Ok(items.clone()),
        _ => Err(format!("{}: the trace has no events", path)),
    }
}

/// The live stream of `run --stream <path>`: a file, or a pipe (SPEC 9.1). A
/// file gets every event as it happens; a pipe needs a reader when the run
/// starts (fail-open: a missing reader is off, with a message on stderr); a
/// reader that stops reading is cut off after a short grace, with one
/// message. The stream never holds the run up.
pub struct Stream {
    file: Option<std::fs::File>,
    stale_since: Option<std::time::Instant>,
    cut: bool,
}

impl Stream {
    pub fn open(path: &str) -> Result<Stream, String> {
        use std::os::unix::fs::FileTypeExt;
        let meta = std::fs::metadata(path);
        let is_fifo = meta.as_ref().map(|m| m.file_type().is_fifo()).unwrap_or(false);
        if is_fifo {
            // A pipe with a reader when the run starts succeeds; without one
            // the stream is off (fail-open, 9.1), with its message.
            #[cfg(target_vendor = "apple")]
            const O_NONBLOCK: i32 = 0x0004;
            #[cfg(not(target_vendor = "apple"))]
            const O_NONBLOCK: i32 = 0x0800;
            const O_WRONLY: i32 = 1;
            let open = std::fs::OpenOptions::new()
                .custom_flags(O_WRONLY | O_NONBLOCK)
                .write(true)
                .open(path);
            match open {
                Ok(file) => Ok(Stream {
                    file: Some(file),
                    stale_since: None,
                    cut: false,
                }),
                Err(_) => Err(format!("the stream pipe {} has no reader; the stream is off (9.1)", path)),
            }
        } else {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)
                .map_err(|e| format!("{}: cannot write ({})", path, e))?;
            Ok(Stream {
                file: Some(file),
                stale_since: None,
                cut: false,
            })
        }
    }

    pub fn emit(&mut self, ev: &Value) {
        if self.cut {
            return;
        }
        let line = write(ev);
        let Some(file) = &mut self.file else { return };
        match file.write_all(line.as_bytes()).and_then(|_| file.write_all(b"\n")) {
            Ok(()) => {
                self.stale_since = None;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // A pipe whose buffer is full: grace, then cut off, with one
                // message (9.1). The run never waits on the stream.
                let since = *self.stale_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed().as_millis() as u64 > crate::host::STREAM_GRACE_MS {
                    self.cut = true;
                    eprintln!(
                        "the stream stopped being read; it is cut off after {} ms (the run is unaffected)",
                        crate::host::STREAM_GRACE_MS
                    );
                }
            }
            Err(e) => {
                // Any other failure is the stream's, never the run's.
                self.cut = true;
                let _ = e;
            }
        }
    }
}
