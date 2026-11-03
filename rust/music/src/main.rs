//! `music`: a composed neuron. To the outside it is one program: a query on
//! stdin, one JSON answer on stdout. Inside is a tiny network of three neurons
//! (tracks, artist, lyrics) that cannot be reached from outside, and cannot
//! reach out: a handler gets the query text and returns text, nothing else.
//!
//! Silent below the threshold where no inner neuron acts (prints nothing, exit 0).
//! `music --inspect` also prints the inner trace on stderr: the view from inside
//! the neuron. The answer never contains inner ids or the trace. Nothing is
//! learned or kept between runs: each call is its own process.
//!
//! The inner rules share thresholds and data with the JavaScript neurons in
//! examples/typing-demo.mjs (change both together); the answer shapes differ:
//! artist is the catalogue name, lyrics is the first matching line or null.
//! The lyrics neuron also takes as long as the JavaScript one (250 ms) when it
//! acts, so the demo has slow work to drop; set MUSIC_NO_LATENCY=1 to skip the wait.
//! The catalogue is the `catalogue` library's.
use catalogue::{clean, escape, matches, to_json, utf16_len, ARTISTS, LYRICS};
use std::io::{Read, Write};

/// An inner signal. `reply_to` is the signal this one answers; `trace` is
/// shared by all the inner work one query causes.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub id: u32,
    pub from: &'static str,
    pub to: &'static str,
    pub reply_to: Option<u32>,
    pub trace: u32,
    pub text: String,
}

/// A neuron is an id and a handler. The handler returns a JSON fragment, or
/// None to stay quiet.
pub struct Neuron {
    pub id: &'static str,
    pub handle: fn(&str) -> Option<String>,
}

/// The boundary's two ends: where the query comes in, and where answers go.
const IN: &str = "in";
const OUT: &str = "out";

pub struct Network {
    neurons: Vec<Neuron>,
    next_id: u32,
    pub log: Vec<Signal>,
}

impl Network {
    pub fn new(neurons: Vec<Neuron>) -> Self {
        Network { neurons, next_id: 1, log: Vec::new() }
    }

    fn record(&mut self, from: &'static str, to: &'static str, reply_to: Option<u32>, trace: u32, text: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.log.push(Signal { id, from, to, reply_to, trace, text: text.to_string() });
        id
    }

    /// Deliver the query to every neuron, one signal each, and collect the
    /// answers as (neuron id, JSON fragment). Quiet neurons answer nothing.
    pub fn ask(&mut self, query: &str) -> Vec<(&'static str, String)> {
        let trace = self.next_id; // the first signal's id names the trace
        let mut answers = Vec::new();
        for i in 0..self.neurons.len() {
            let (id, handle) = (self.neurons[i].id, self.neurons[i].handle);
            let sent = self.record(IN, id, None, trace, query);
            if let Some(fragment) = handle(query) {
                self.record(id, OUT, Some(sent), trace, &fragment);
                answers.push((id, fragment));
            }
        }
        answers
    }
}

fn tracks(q: &str) -> Option<String> {
    matches(q).map(|hits| to_json(&hits))
}

/// Acts only on an exact artist name.
fn artist(q: &str) -> Option<String> {
    let q = q.to_lowercase();
    ARTISTS.iter().find(|a| a.to_lowercase() == q).map(|a| escape(a))
}

/// Acts from 5 characters; the first lyric line containing the query, if any.
fn lyrics(q: &str) -> Option<String> {
    let q = q.to_lowercase();
    if utf16_len(&q) < 5 {
        return None;
    }
    // simulated latency, as in the JavaScript lyrics neuron; only when it acts
    if std::env::var_os("MUSIC_NO_LATENCY").is_none() {
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Some(LYRICS.iter().find(|l| l.contains(&q)).map_or("null".to_string(), |l| escape(l)))
}

pub fn inner_network() -> Network {
    Network::new(vec![
        Neuron { id: "tracks", handle: tracks },
        Neuron { id: "artist", handle: artist },
        Neuron { id: "lyrics", handle: lyrics },
    ])
}

/// The composed neuron's whole interface: raw input in, an answer (or nothing) out.
/// The inner network is returned too, for `--inspect` and tests.
pub fn run(input: &str) -> (Option<String>, Network) {
    let mut net = inner_network();
    let answers = net.ask(clean(input));
    if answers.is_empty() {
        return (None, net);
    }
    let get = |id: &str| answers.iter().find(|(a, _)| *a == id).map_or("null", |(_, f)| f.as_str());
    let tracks = answers.iter().find(|(a, _)| *a == "tracks").map_or("[]", |(_, f)| f.as_str());
    let out = format!(r#"{{"tracks":{},"artist":{},"lyrics":{}}}"#, tracks, get("artist"), get("lyrics"));
    (Some(out), net)
}

fn main() {
    let inspect = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--inspect") => true,
        Some(other) => {
            eprintln!("unknown argument {other}: the only option is --inspect");
            std::process::exit(2);
        }
    };
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("cannot read stdin: {e}");
        std::process::exit(1);
    }
    let (answer, net) = run(&input);
    let failed = |_| std::process::exit(1);
    if let Some(answer) = &answer {
        writeln!(std::io::stdout().lock(), "{answer}").unwrap_or_else(failed);
    }
    if inspect {
        let mut err = std::io::stderr().lock();
        for s in &net.log {
            let reply = s.reply_to.map_or("-".to_string(), |r| r.to_string());
            writeln!(err, "signal {} from {} to {} replyTo {} trace {}", s.id, s.from, s.to, reply, s.trace)
                .unwrap_or_else(failed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_query_is_answered_by_all_three_on_one_trace() {
        let (answer, net) = run("lumen harbor\n");
        assert_eq!(
            answer.unwrap(),
            r#"{"tracks":["Glass Orchard","Ninefold Morning"],"artist":"Lumen Harbor","lyrics":"carry me home to the lumen harbor"}"#
        );
        // three queries in, three answers out
        assert_eq!(net.log.len(), 6);
        assert!(net.log.iter().all(|s| s.trace == net.log[0].trace));
        for q in net.log.iter().filter(|s| s.from == IN) {
            assert_eq!(q.reply_to, None);
            let a: Vec<_> = net.log.iter().filter(|s| s.reply_to == Some(q.id)).collect();
            assert_eq!(a.len(), 1);
            assert_eq!((a[0].from, a[0].to), (q.to, OUT));
        }
    }

    #[test]
    fn below_the_threshold_nothing_acts() {
        let (answer, net) = run("lu");
        assert_eq!(answer, None);
        // the query was delivered to all three, nobody answered
        assert_eq!(net.log.len(), 3);
        assert!(net.log.iter().all(|s| s.from == IN));
    }

    #[test]
    fn thresholds_and_answer_shapes() {
        // tracks from 3; artist only on an exact name; lyrics from 5
        let (a, _) = run("lum");
        assert_eq!(a.unwrap(), r#"{"tracks":["Glass Orchard","Ninefold Morning"],"artist":null,"lyrics":null}"#);
        let (a, _) = run("lumen");
        assert!(a.unwrap().contains(r#""lyrics":"carry me home"#));
        let (a, _) = run("zzzzzz");
        assert_eq!(a.unwrap(), r#"{"tracks":[],"artist":null,"lyrics":null}"#);
    }

    #[test]
    fn the_answer_holds_no_inner_ids() {
        let (a, _) = run("lumen harbor");
        let a = a.unwrap();
        assert!(!a.contains("trace") && !a.contains("replyTo") && !a.contains("\"id\""));
    }
}
