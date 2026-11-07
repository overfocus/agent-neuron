// The fixture file (CONFORMANCE section 2): one JSON object, closed at the
// top level; an unknown top-level field fails the fixture.
use crate::json::Value;
use std::collections::HashMap;

/// A named unit to an amount, in declared order (the order the file writes
/// them decides which short unit a `budget:` message names).
pub type Units = Vec<(String, i64)>;

#[derive(Debug, Clone)]
pub struct Input {
    pub at: u64,
    pub source: String,
    pub payload: Value,
    pub replaces: Option<String>,
    pub budget: Option<Units>,
}

#[derive(Debug, Clone)]
pub struct CallEntry {
    pub when: Option<Value>,
    pub after: u64,
    pub reply: Value,
    pub ignore_term: bool,
    pub leaves: bool,
    /// `onCancel`: `after` ms from the cancel, then this reply.
    pub on_cancel: Option<(u64, Value)>,
}

#[derive(Debug, Clone, Default)]
pub struct Prog {
    pub starts: Vec<Value>,
    pub calls: Vec<CallEntry>,
}

#[derive(Debug, Clone)]
pub struct Inject {
    pub after: u64,
    pub source: String,
    pub payload: Value,
    pub replaces: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub when: Value,
    pub inject: Inject,
}

#[derive(Debug, Clone)]
pub struct Fixture {
    pub description: String,
    pub network_path: String,
    pub notes: bool,
    pub finished: bool,
    pub stream_fails: bool,
    pub pending: Option<String>,
    pub ping_every: u64,
    pub refuses: bool,
    pub inputs: Vec<Input>,
    pub script: HashMap<String, Prog>,
    pub observer: Vec<Rule>,
    pub expect: Vec<Value>,
    pub expect_without: Option<Vec<Value>>,
    pub independent: bool,
}

pub fn load(path: &str) -> Result<Fixture, Vec<String>> {
    let text = std::fs::read_to_string(path).map_err(|e| vec![format!("{}: cannot read ({})", path, e)])?;
    let v = crate::json::parse(&text).map_err(|e| vec![format!("{}: not JSON ({})", path, e)])?;
    from_json(&v).map_err(|e| vec![format!("{}: {}", path, e.concat())])
}

fn from_json(v: &Value) -> Result<Fixture, Vec<String>> {
    let mut errs: Vec<String> = Vec::new();
    if !v.is_obj() {
        return Err(vec!["the fixture file is not a JSON object".into()]);
    }
    for k in v.keys() {
        if !matches!(
            k,
            "fixture"
                | "description"
                | "network"
                | "notes"
                | "finished"
                | "streamFails"
                | "pending"
                | "pingEvery"
                | "refuses"
                | "inputs"
                | "script"
                | "observer"
                | "expect"
                | "expectWithout"
                | "independent"
        ) {
            errs.push(format!("unknown top-level field '{}'", k));
        }
    }
    if !v.get("fixture").map(|x| *x == Value::Int(0)).unwrap_or(false) {
        errs.push("fixture must be 0".into());
    }
    if v.get("description").is_none() {
        errs.push("description is required".into());
    }
    if v.get("inputs").is_none() {
        errs.push("inputs is required".into());
    }
    if v.get("expect").is_none() {
        errs.push("expect is required".into());
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    let description = v.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
    let network_path = match v.get("network") {
        None => "network.json".to_string(),
        Some(Value::Str(s)) => s.clone(),
        Some(_) => {
            errs.push("network must be a path".into());
            String::new()
        }
    };
    let flag = |name: &str| -> Result<bool, String> {
        match v.get(name) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(format!("{} must be a boolean", name)),
        }
    };
    let notes = flag("notes").map_err(|e| vec![e])?;
    let finished = flag("finished").map_err(|e| vec![e])?;
    let stream_fails = flag("streamFails").map_err(|e| vec![e])?;
    let refuses = flag("refuses").map_err(|e| vec![e])?;
    let independent = flag("independent").map_err(|e| vec![e])?;
    let pending = match v.get("pending") {
        None => None,
        Some(Value::Str(s)) => Some(s.clone()),
        Some(_) => return Err(vec!["pending must be a string".into()]),
    };
    let ping_every = match v.get("pingEvery") {
        None => 10_000,
        Some(x) => match x.as_u64() {
            Some(n) if n >= 1 => n,
            _ => return Err(vec!["pingEvery is an integer in ms".into()]),
        },
    };
    let mut inputs = Vec::new();
    if let Some(Value::Arr(items)) = v.get("inputs") {
        for (i, iv) in items.iter().enumerate() {
            inputs.push(input(iv, i, &mut errs));
        }
    }
    let mut script = HashMap::new();
    match v.get("script") {
        None => {}
        Some(Value::Obj(entries)) => {
            for (path, pv) in entries {
                script.insert(path.clone(), prog(pv, path, &mut errs));
            }
        }
        Some(_) => errs.push("script is an object".into()),
    }
    let mut observer = Vec::new();
    match v.get("observer") {
        None => {}
        Some(Value::Arr(rules)) => {
            for (i, rv) in rules.iter().enumerate() {
                observer.push(rule(rv, i, &mut errs));
            }
        }
        Some(_) => errs.push("observer is an array of rules".into()),
    }
    let expect = match v.get("expect") {
        Some(Value::Arr(items)) => items.clone(),
        _ => {
            errs.push("expect is required (an array)".into());
            Vec::new()
        }
    };
    let expect_without = match v.get("expectWithout") {
        None => None,
        Some(Value::Arr(items)) => Some(items.clone()),
        Some(_) => {
            errs.push("expectWithout is an array".into());
            None
        }
    };
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Fixture {
        description,
        network_path,
        notes,
        finished,
        stream_fails,
        pending,
        ping_every,
        refuses,
        inputs,
        script,
        observer,
        expect,
        expect_without,
        independent,
    })
}

fn input(iv: &Value, i: usize, errs: &mut Vec<String>) -> Input {
    if !iv.is_obj() {
        errs.push(format!("inputs[{}] is not an object", i));
        return Input {
            at: 0,
            source: String::new(),
            payload: Value::Null,
            replaces: None,
            budget: None,
        };
    }
    for k in iv.keys() {
        if !matches!(k, "at" | "source" | "payload" | "replaces" | "budget") {
            errs.push(format!("inputs[{}]: unknown field '{}'", i, k));
        }
    }
    let at = match iv.get("at").and_then(|x| x.as_u64()) {
        Some(n) => n,
        None => {
            errs.push(format!("inputs[{}]: 'at' is an integer in ms", i));
            0
        }
    };
    let source = match iv.get("source") {
        Some(Value::Str(s)) => s.clone(),
        _ => {
            errs.push(format!("inputs[{}]: 'source' is required", i));
            String::new()
        }
    };
    let replaces = match iv.get("replaces") {
        None => None,
        Some(Value::Str(s)) => Some(s.clone()),
        Some(_) => {
            errs.push(format!("inputs[{}]: 'replaces' must be a string", i));
            None
        }
    };
    let payload = iv.get("payload").cloned().unwrap_or(Value::Null);
    let budget = match iv.get("budget") {
        None => None,
        Some(b) => units_of(b, &format!("inputs[{}]: budget", i), errs),
    };
    Input {
        at,
        source,
        payload,
        replaces,
        budget,
    }
}

/// Units of a budget-like object, in declared order.
pub fn units_of(v: &Value, what: &str, errs: &mut Vec<String>) -> Option<Units> {
    let entries = match v {
        Value::Obj(entries) => entries,
        _ => {
            errs.push(format!("{} is not an object", what));
            return None;
        }
    };
    let mut out = Vec::new();
    for (u, amt) in entries {
        match amt.as_i64() {
            Some(n) if n >= 0 => out.push((u.clone(), n)),
            _ => errs.push(format!("{}: '{}' is not a whole number", what, u)),
        }
    }
    if out.is_empty() {
        errs.push(format!("{} is empty", what));
    }
    Some(out)
}

fn prog(pv: &Value, path: &str, errs: &mut Vec<String>) -> Prog {
    if !pv.is_obj() {
        errs.push(format!("script['{}'] is not an object", path));
        return Prog::default();
    }
    let mut p = Prog::default();
    if let Some(Value::Arr(starts)) = pv.get("starts") {
        p.starts = starts.clone();
    } else if pv.has("starts") {
        errs.push(format!("script['{}']: starts is an array", path));
    }
    if pv.has("calls") && !matches!(pv.get("calls"), Some(Value::Arr(_))) {
        errs.push(format!("script['{}']: calls is an array", path));
    }
    if let Some(Value::Arr(calls)) = pv.get("calls") {
        for (i, cv) in calls.iter().enumerate() {
            p.calls.push(call_entry(cv, path, i, errs));
        }
    }
    for k in pv.keys() {
        if k != "starts" && k != "calls" {
            errs.push(format!("script['{}']: unknown field '{}'", path, k));
        }
    }
    p
}

fn call_entry(cv: &Value, path: &str, i: usize, errs: &mut Vec<String>) -> CallEntry {
    if !cv.is_obj() {
        errs.push(format!("script['{}'].calls[{}] is not an object", path, i));
        return CallEntry {
            when: None,
            after: 0,
            reply: Value::Null,
            ignore_term: false,
            leaves: false,
            on_cancel: None,
        };
    }
    for k in cv.keys() {
        if !matches!(k, "when" | "after" | "reply" | "ignoreTerm" | "leaves" | "onCancel") {
            errs.push(format!("script['{}'].calls[{}]: unknown field '{}'", path, i, k));
        }
    }
    let when = cv.get("when").cloned();
    let after = match cv.get("after") {
        Some(x) => match x.as_u64() {
            Some(n) => n,
            None => {
                errs.push(format!("script['{}'].calls[{}]: 'after' is an integer in ms", path, i));
                0
            }
        },
        None => 0,
    };
    let reply = match cv.get("reply") {
        Some(r) => r.clone(),
        None => {
            errs.push(format!("script['{}'].calls[{}]: 'reply' is required", path, i));
            Value::Null
        }
    };
    let ignore_term = cv.get("ignoreTerm").and_then(|x| x.as_bool()).unwrap_or(false);
    let leaves = cv.get("leaves").and_then(|x| x.as_bool()).unwrap_or(false);
    let on_cancel = match cv.get("onCancel") {
        None => None,
        Some(ov) if ov.is_obj() => Some((
            ov.get("after").and_then(|x| x.as_u64()).unwrap_or(0),
            ov.get("reply").cloned().unwrap_or(Value::Null),
        )),
        Some(_) => {
            errs.push(format!("script['{}'].calls[{}]: onCancel is an object", path, i));
            None
        }
    };
    CallEntry {
        when,
        after,
        reply,
        ignore_term,
        leaves,
        on_cancel,
    }
}

fn rule(rv: &Value, i: usize, errs: &mut Vec<String>) -> Rule {
    if !rv.is_obj() {
        errs.push(format!("observer[{}] is not an object", i));
        return Rule {
            when: Value::Null,
            inject: Inject {
                after: 0,
                source: String::new(),
                payload: Value::Null,
                replaces: None,
            },
        };
    }
    let when = rv.get("when").cloned().unwrap_or(Value::Null);
    let empty = Value::Obj(Vec::new());
    let iv = rv.get("inject").unwrap_or(&empty);
    let inject = Inject {
        after: iv.get("after").and_then(|x| x.as_u64()).unwrap_or(0),
        source: iv.get("source").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        payload: iv.get("payload").cloned().unwrap_or(Value::Null),
        replaces: iv.get("replaces").and_then(|x| x.as_str()).map(|s| s.to_string()),
    };
    Rule { when, inject }
}
