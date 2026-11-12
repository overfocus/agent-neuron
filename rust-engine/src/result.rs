//! A run-level result (decision 0022, `run --result`): one object written at
//! the end of a run, read off the events the run reported and adding nothing
//! to them. For each trace: its source, its key, the answers of the neurons
//! with no outgoing links, and every error, unknown and drop; and the totals.
//! A live run (decision 0020) keeps nothing, so it holds the totals only.

use crate::json::Value;
use std::collections::{HashMap, HashSet};

struct TraceRec {
    trace: String,
    source: Value,
    replaces: Option<Value>,
    status: Option<Value>,
    answers: Vec<(String, Value)>,
    failures: Vec<Value>,
    drops: Vec<Value>,
}

/// Folds a run's events, one at a time, into its result.
pub struct Tally {
    /// The neurons with no outgoing links: their answers are the run's (4.4).
    leaves: HashSet<String>,
    /// Whether the traces are kept (a run that ends) or only counted (live).
    detail: bool,
    recs: Vec<TraceRec>,
    index: HashMap<String, usize>,
    /// Where each kept answer sits, in the order the answers happened: the
    /// engine's whole payloads are put there by `fill` (an event's is cut).
    order: Vec<(usize, usize)>,
    traces: u64,
    answers: u64,
    errors: u64,
    unknowns: u64,
    drops: u64,
}

fn text(ev: &Value, key: &str) -> Option<String> {
    match ev.get(key) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn pick(ev: &Value, keys: &[&str]) -> Value {
    Value::Obj(keys.iter().filter_map(|k| ev.get(k).map(|v| (k.to_string(), v.clone()))).collect())
}

/// An event's payload as a bare value: what it carries of it (7.2).
fn bare(ev: &Value) -> Value {
    let Some(p) = ev.get("payload") else { return Value::Null };
    p.get("data")
        .or_else(|| p.get("text"))
        .or_else(|| p.get("preview"))
        .cloned()
        .unwrap_or(Value::Null)
}

impl Tally {
    pub fn new(leaves: HashSet<String>, detail: bool) -> Tally {
        Tally {
            leaves,
            detail,
            recs: Vec::new(),
            index: HashMap::new(),
            order: Vec::new(),
            traces: 0,
            answers: 0,
            errors: 0,
            unknowns: 0,
            drops: 0,
        }
    }

    fn rec(&mut self, ev: &Value) -> Option<&mut TraceRec> {
        if !self.detail {
            return None;
        }
        let i = *self.index.get(&text(ev, "trace")?)?;
        self.recs.get_mut(i)
    }

    pub fn see(&mut self, ev: &Value) {
        let Some(kind) = text(ev, "type") else { return };
        match kind.as_str() {
            "signal" => {
                // A root signal is its own trace (4.1).
                let (Some(id), Some(trace)) = (text(ev, "id"), text(ev, "trace")) else {
                    return;
                };
                if id != trace {
                    return;
                }
                self.traces += 1;
                if self.detail {
                    self.index.insert(trace.clone(), self.recs.len());
                    self.recs.push(TraceRec {
                        trace,
                        source: ev.get("from").cloned().unwrap_or(Value::Null),
                        replaces: ev.get("replaces").cloned(),
                        status: None,
                        answers: Vec::new(),
                        failures: Vec::new(),
                        drops: Vec::new(),
                    });
                }
            }
            "answer" => {
                let Some(from) = text(ev, "from") else { return };
                if !self.leaves.contains(&from) {
                    return;
                }
                self.answers += 1;
                let payload = bare(ev);
                if let Some(i) = text(ev, "trace").and_then(|t| self.index.get(&t).copied()) {
                    if self.detail {
                        self.order.push((i, self.recs[i].answers.len()));
                        self.recs[i].answers.push((from, payload));
                    }
                }
            }
            "error" | "unknown" => {
                if kind == "error" {
                    self.errors += 1;
                } else {
                    self.unknowns += 1;
                }
                let item = pick(ev, &["type", "neuron", "code", "message"]);
                if let Some(r) = self.rec(ev) {
                    r.failures.push(item);
                }
            }
            "drop" => {
                self.drops += 1;
                let item = pick(ev, &["neuron", "reason", "phase"]);
                if let Some(r) = self.rec(ev) {
                    r.drops.push(item);
                }
            }
            "finished" => {
                let status = ev.get("status").cloned();
                if let Some(r) = self.rec(ev) {
                    r.status = status;
                }
            }
            _ => {}
        }
    }

    /// The whole payloads of the answers, in the order they happened (what
    /// the run prints): they replace what the events carried, which is cut at
    /// 2000 characters. Left alone if the two lists do not line up.
    pub fn fill(&mut self, whole: &[(String, Value)]) {
        if whole.len() != self.order.len() {
            return;
        }
        if whole
            .iter()
            .zip(&self.order)
            .any(|((n, _), (r, a))| self.recs[*r].answers[*a].0 != *n)
        {
            return;
        }
        for ((_, payload), (r, a)) in whole.iter().zip(&self.order) {
            self.recs[*r].answers[*a].1 = payload.clone();
        }
    }

    /// No call ended in an error or an unknown.
    pub fn ok(&self) -> bool {
        self.errors == 0 && self.unknowns == 0
    }

    pub fn to_value(&self, network: &str) -> Value {
        let n = |v: u64| Value::Int(v as i64);
        let mut out = vec![
            ("network".to_string(), Value::Str(network.to_string())),
            ("ok".to_string(), Value::Bool(self.ok())),
            (
                "totals".to_string(),
                Value::Obj(vec![
                    ("traces".into(), n(self.traces)),
                    ("answers".into(), n(self.answers)),
                    ("errors".into(), n(self.errors)),
                    ("unknowns".into(), n(self.unknowns)),
                    ("drops".into(), n(self.drops)),
                ]),
            ),
        ];
        if self.detail {
            let traces = self
                .recs
                .iter()
                .map(|r| {
                    let mut t = vec![
                        ("trace".to_string(), Value::Str(r.trace.clone())),
                        ("source".to_string(), r.source.clone()),
                    ];
                    if let Some(k) = &r.replaces {
                        t.push(("replaces".into(), k.clone()));
                    }
                    if let Some(s) = &r.status {
                        t.push(("status".into(), s.clone()));
                    }
                    let answers = r
                        .answers
                        .iter()
                        .map(|(neuron, payload)| {
                            Value::Obj(vec![
                                ("neuron".into(), Value::Str(neuron.clone())),
                                ("payload".into(), payload.clone()),
                            ])
                        })
                        .collect();
                    t.push(("answers".into(), Value::Arr(answers)));
                    t.push(("failures".into(), Value::Arr(r.failures.clone())));
                    t.push(("drops".into(), Value::Arr(r.drops.clone())));
                    Value::Obj(t)
                })
                .collect();
            out.push(("traces".to_string(), Value::Arr(traces)));
        }
        Value::Obj(out)
    }
}
