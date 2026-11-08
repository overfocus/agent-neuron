// The desk (decision 0008, SPEC Appendix D): one page per run, bound to
// 127.0.0.1 only, behind a random 128-bit token, closed when the run ends.
// Each call to a `human` neuron is an item; the person's answer becomes the
// call's reply (3.4: empty is quiet, anything else is a text payload).
//
// The surface, as 0008's rule list has it: the token is compared in constant
// time on every request; an exact `Host` (no DNS rebinding) and `Origin` (the
// page's own, required on a POST); answers by POST with a JSON body under a
// cap; a nonce CSP that fetches nothing anywhere but the page itself; every
// value from an input rendered as text, never as HTML. The page runs one
// inline script under the CSP's nonce.
//
// Grants (decision 0010): a run may name the people at the table, each with
// their own token, the roles they hold and the human neurons whose items they
// may answer. The desk checks every answer against them at the POST and logs
// a refusal on stderr; the run's token stays the owner's and answers anything.
// Who answered rides on the `desk answer:` line and in the shared record
// `/state` serves; it never enters the network, which still gets the
// person's text (3.4).
use crate::json::{self, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// A POST's JSON body cap: an edit's text is a paragraph, not a file.
pub const ANSWER_BODY_CAP: usize = 32_768;
/// A `why` line's cap: one line for the record, not an essay.
pub const WHY_CAP: usize = 500;
/// A request's head cap (request line and headers).
const HEAD_CAP: usize = 8_192;
/// How long a connection may sit without a request before it is dropped.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The accept loop's tick.
const TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Withdrawn,
    TimedOut,
    Answered,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Waiting => "waiting",
            Status::Withdrawn => "withdrawn",
            Status::TimedOut => "timed-out",
            Status::Answered => "answered",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Approve,
    Edit,
    No,
    /// One of the item's `options` (decision 0010): the reply is the
    /// option's `text` (else its `id`), computed at the POST.
    Choose,
}

/// What the engine registers when a human call starts (Appendix D: the
/// payload's `title` and `notes` are shown, never sent; the `draft`/`proposal`
/// pair is the diff convention; any other input is shown as its text or its
/// JSON, which the engine shapes into `text`). A host reading `/state` needs
/// more than the page does, so the item carries the whole ask too: its
/// `payload` (the input value as the engine received it) and `t` (the run's
/// millisecond at the ask).
pub struct NewItem {
    pub neuron: String,
    pub title: Option<String>,
    pub notes: Vec<String>,
    pub draft: Option<String>,
    pub proposal: Option<String>,
    pub text: Option<String>,
    pub payload: Value,
    pub t: u64,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Approve => "approve",
            Kind::Edit => "edit",
            Kind::No => "no",
            Kind::Choose => "choose",
        }
    }
}

// -- Grants (decision 0010) ---------------------------------------------------

/// A person at the table: an id (on the answer lines and the record), a name
/// (shown), the roles an item's `answerable_by` is matched against, and the
/// human neurons whose items they may answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub roles: Vec<String>,
    pub answers: Vec<String>,
}

/// The grants file: `{"version": 0, "people": [{id, name?, roles?, answers}]}`,
/// closed like the network file.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    pub people: Vec<Person>,
}

fn is_grant_id(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_lowercase())
        && s.len() <= 32
        && cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

impl Grants {
    pub fn load(path: &str) -> Result<Grants, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: cannot read ({e})"))?;
        Grants::parse(&text).map_err(|e| format!("{path}: {e}"))
    }

    pub fn parse(text: &str) -> Result<Grants, String> {
        let v = json::parse(text).map_err(|e| format!("not JSON ({e})"))?;
        if !v.is_obj() {
            return Err("the grants are not an object".into());
        }
        for k in v.keys() {
            if !matches!(k, "$schema" | "version" | "people") {
                return Err(format!("the grants have a field '{k}' they do not know"));
            }
        }
        if v.get("version").and_then(|n| n.as_i64()) != Some(0) {
            return Err("the grants need \"version\": 0".into());
        }
        let people = v.get("people").and_then(|p| p.as_arr()).ok_or("the grants need a people array")?;
        if people.is_empty() {
            return Err("the grants name nobody".into());
        }
        let mut out: Vec<Person> = Vec::new();
        for (i, p) in people.iter().enumerate() {
            let at = format!("people[{i}]");
            if !p.is_obj() {
                return Err(format!("{at} is not an object"));
            }
            for k in p.keys() {
                if !matches!(k, "id" | "name" | "roles" | "answers") {
                    return Err(format!("{at} has a field '{k}' the grants do not know"));
                }
            }
            let id = p.get("id").and_then(|x| x.as_str()).ok_or(format!("{at} has no id"))?;
            if !is_grant_id(id) {
                return Err(format!("{at}: the id '{id}' is not lowercase letters, digits, - or _ (at most 32)"));
            }
            if id == "owner" {
                return Err(format!("{at}: 'owner' is the run's own token, not a person"));
            }
            if out.iter().any(|q| q.id == id) {
                return Err(format!("{at}: the id '{id}' is there twice"));
            }
            let name = match p.get("name") {
                None => id.to_string(),
                Some(Value::Str(n)) if !n.trim().is_empty() && n.chars().count() <= 64 => n.clone(),
                Some(_) => return Err(format!("{at}: the name is not a string of 1 to 64 characters")),
            };
            let strings = |key: &str| -> Result<Vec<String>, String> {
                match p.get(key) {
                    None => Ok(Vec::new()),
                    Some(Value::Arr(xs)) => xs
                        .iter()
                        .map(|x| {
                            x.as_str()
                                .map(str::to_string)
                                .ok_or(format!("{at}: {key} holds a value that is not a string"))
                        })
                        .collect(),
                    Some(_) => Err(format!("{at}: {key} is not an array")),
                }
            };
            let roles = strings("roles")?;
            let answers = strings("answers")?;
            if answers.is_empty() {
                return Err(format!("{at}: '{id}' answers no neuron"));
            }
            out.push(Person {
                id: id.to_string(),
                name,
                roles,
                answers,
            });
        }
        Ok(Grants { people: out })
    }

    /// Every neuron a grant names must be one of the run's human neurons.
    pub fn check_against(&self, humans: &[String]) -> Result<(), String> {
        for p in &self.people {
            for n in &p.answers {
                if !humans.contains(n) {
                    return Err(format!(
                        "the grants let '{}' answer '{}', which is not a human neuron of the network",
                        p.id, n
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Person {
    /// May this person answer an item of `neuron` whose ask is `payload`?
    /// The neuron must be granted, and the payload's `answerable_by` (a
    /// role, or a list of roles), when it has one, must name a role of theirs.
    fn may_answer(&self, neuron: &str, payload: &Value) -> bool {
        if !self.answers.iter().any(|n| n == neuron) {
            return false;
        }
        match payload.get("answerable_by") {
            None => true,
            Some(Value::Str(r)) => self.roles.iter().any(|x| x == r),
            Some(Value::Arr(rs)) => rs.iter().any(|r| r.as_str().is_some_and(|r| self.roles.iter().any(|x| x == r))),
            Some(_) => false,
        }
    }

    fn public(&self) -> Value {
        json::Obj::new()
            .str("id", &self.id)
            .str("name", &self.name)
            .set("roles", Value::Arr(self.roles.iter().map(|r| Value::Str(r.clone())).collect()))
            .build()
    }
}

/// Who a request speaks for: the run's owner (the `desk:` token) or one of
/// the people the grants name (an index into them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    Owner,
    Person(usize),
}

/// One accepted answer, as the shared record keeps it.
struct Record {
    item: String,
    neuron: String,
    kind: Kind,
    text: String,
    by: Option<String>,
    option: Option<String>,
    why: Option<String>,
    ms: u64,
}

/// What an item's `options` say: `[{id, label?, text?}]`; the reply of a
/// choice is `text` when it is a string, else the `id`.
fn option_text(payload: &Value, option: &str) -> Option<String> {
    let opts = payload.get("options")?.as_arr()?;
    let o = opts.iter().find(|o| o.get("id").and_then(|i| i.as_str()) == Some(option))?;
    Some(match o.get("text") {
        Some(Value::Str(t)) => t.clone(),
        _ => option.to_string(),
    })
}

/// One answer the desk accepted and the engine has not settled yet.
pub struct Reply {
    pub item: String,
    pub kind: Kind,
    /// The person's text, for an edit (an empty one was refused at POST).
    pub text: String,
}

struct Item {
    id: String,
    neuron: String,
    title: Option<String>,
    notes: Vec<String>,
    draft: Option<String>,
    proposal: Option<String>,
    text: Option<String>,
    /// The ask, whole (the input payload as the engine received it), and the
    /// run's millisecond at the ask: what a host records a question from.
    payload: Value,
    t: u64,
    status: Status,
}

#[derive(Default)]
struct Book {
    next: usize,
    items: Vec<Item>,
    /// Whether the run is over (`/state`'s `over`).
    over: bool,
    /// Whether the desk is closed: nothing new is served.
    closed: bool,
    /// Accepted answers, waiting for the engine's settle.
    replies: Vec<Reply>,
    /// Every accepted answer, in order: the shared record `/state` serves.
    record: Vec<Record>,
}

impl Book {
    fn new_item(&mut self, item: NewItem) -> String {
        let id = format!("i-{}", self.next + 1);
        self.next += 1;
        self.items.push(Item {
            id: id.clone(),
            neuron: item.neuron,
            title: item.title,
            notes: item.notes,
            draft: item.draft,
            proposal: item.proposal,
            text: item.text,
            payload: item.payload,
            t: item.t,
            status: Status::Waiting,
        });
        id
    }

    fn set_status(&mut self, id: &str, status: Status) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = status;
        }
    }

    fn take_reply(&mut self) -> Option<Reply> {
        if self.replies.is_empty() {
            None
        } else {
            Some(self.replies.remove(0))
        }
    }

    fn state_json(&self, who: Who, people: &[Person]) -> Value {
        let granted = !people.is_empty();
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|i| {
                let can: Vec<&Person> = people.iter().filter(|p| p.may_answer(&i.neuron, &i.payload)).collect();
                let yours = match who {
                    Who::Owner => true,
                    Who::Person(k) => people[k].may_answer(&i.neuron, &i.payload),
                };
                json::Obj::new()
                    .str("id", &i.id)
                    .str("neuron", &i.neuron)
                    .set_if("title", i.title.as_ref().map(|t| Value::Str(t.clone())))
                    .set("notes", Value::Arr(i.notes.iter().map(|n| Value::Str(n.clone())).collect()))
                    .set_if("draft", i.draft.as_ref().map(|d| Value::Str(d.clone())))
                    .set_if("proposal", i.proposal.as_ref().map(|p| Value::Str(p.clone())))
                    .set_if("text", i.text.as_ref().map(|t| Value::Str(t.clone())))
                    .set("payload", i.payload.clone())
                    .int("t", i.t as i64)
                    .str("status", i.status.name())
                    .set("yours", Value::Bool(yours))
                    .set_if(
                        "answerable_by",
                        granted.then(|| Value::Arr(can.iter().map(|p| Value::Str(p.id.clone())).collect())),
                    )
                    .build()
            })
            .collect();
        let record: Vec<Value> = self
            .record
            .iter()
            .map(|r| {
                json::Obj::new()
                    .str("item", &r.item)
                    .str("neuron", &r.neuron)
                    .str("kind", r.kind.name())
                    .str("text", &r.text)
                    .set_if("by", r.by.clone().map(Value::Str))
                    .set_if("option", r.option.clone().map(Value::Str))
                    .set_if("why", r.why.clone().map(Value::Str))
                    .int("ms", r.ms as i64)
                    .build()
            })
            .collect();
        let me = match who {
            Who::Owner => json::Obj::new().str("id", "owner").str("name", "owner").build(),
            Who::Person(k) => people[k].public(),
        };
        let table: Vec<Value> = people
            .iter()
            .map(|p| {
                let waiting = self
                    .items
                    .iter()
                    .filter(|i| i.status == Status::Waiting && p.may_answer(&i.neuron, &i.payload))
                    .count();
                let mut v = p.public();
                if let Value::Obj(ref mut e) = v {
                    e.push(("waiting".into(), Value::Int(waiting as i64)));
                }
                v
            })
            .collect();
        json::Obj::new()
            .set("over", Value::Bool(self.over))
            .set("items", Value::Arr(items))
            .set("record", Value::Arr(record))
            .set_if("me", granted.then_some(me))
            .set_if("people", granted.then_some(Value::Arr(table)))
            .build()
    }
}

struct Shared {
    token: String,
    nonce: String,
    port: u16,
    book: Mutex<Book>,
    /// The people of the grants, and each one's own token, in the same order.
    people: Vec<Person>,
    tokens: Vec<String>,
    opened: Instant,
}

impl Shared {
    fn book(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The token's owner, compared in constant time against every token
    /// (the owner's and each person's), whichever matches.
    fn who(&self, token: &str) -> Option<Who> {
        let mut found = None;
        if ct_eq(token.as_bytes(), self.token.as_bytes()) {
            found = Some(Who::Owner);
        }
        for (k, t) in self.tokens.iter().enumerate() {
            if ct_eq(token.as_bytes(), t.as_bytes()) && found.is_none() {
                found = Some(Who::Person(k));
            }
        }
        found
    }

    fn by(&self, who: Who) -> Option<String> {
        match who {
            Who::Owner if self.people.is_empty() => None,
            Who::Owner => Some("owner".into()),
            Who::Person(k) => Some(self.people[k].id.clone()),
        }
    }
}

/// The desk. `open` binds the listener (127.0.0.1, port 0) and starts its
/// accept loop; the URL, with the token, goes to the run's stderr on the
/// caller's side.
pub struct Desk {
    shared: Arc<Shared>,
    url: String,
}

impl Desk {
    /// Bind the listener on 127.0.0.1 and start its accept loop. `wake` runs
    /// when an answer is accepted, so the engine's pump wakes at once.
    pub fn open(wake: Arc<dyn Fn() + Send + Sync>, grants: Option<Grants>) -> Result<Desk, String> {
        let listener = TcpListener::bind((IpAddr::from([127, 0, 0, 1]), 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let _ = listener.set_nonblocking(true);
        let token = random_hex(16);
        let nonce = random_hex(16);
        let serving = listener;
        let people = grants.map(|g| g.people).unwrap_or_default();
        let tokens = people.iter().map(|_| random_hex(16)).collect();
        let shared = Arc::new(Shared {
            token: token.clone(),
            nonce: nonce.clone(),
            port,
            book: Mutex::new(Book::default()),
            people,
            tokens,
            opened: Instant::now(),
        });
        let accept_book = shared.clone();
        let conn_book = shared.clone();
        std::thread::Builder::new()
            .name("desk-listen".into())
            .spawn(move || loop {
                if accept_book.book.lock().map(|b| b.closed).unwrap_or(true) {
                    break;
                }
                match serving.accept() {
                    Ok((stream, _)) => {
                        let shared = conn_book.clone();
                        let wake = wake.clone();
                        std::thread::Builder::new()
                            .name("desk-conn".into())
                            .spawn(move || serve(stream, shared, move || wake()))
                            .ok();
                    }
                    Err(_) => std::thread::sleep(TICK),
                }
            })
            .ok();
        Ok(Desk {
            shared,
            url: format!("http://127.0.0.1:{port}/?token={token}"),
        })
    }

    /// The page's address: `http://127.0.0.1:<port>/?token=<token>`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// One stderr line per person of the grants, with their own address:
    /// `desk person: {"id", "name", "roles", "url"}`.
    pub fn people_lines(&self) -> Vec<String> {
        let s = &self.shared;
        s.people
            .iter()
            .zip(&s.tokens)
            .map(|(p, t)| {
                let mut v = p.public();
                if let Value::Obj(ref mut e) = v {
                    e.push(("url".into(), Value::Str(format!("http://127.0.0.1:{}/?token={t}", s.port))));
                }
                format!("desk person: {}", json::write(&v))
            })
            .collect()
    }

    /// One question, asked in order (the ask order is the page's).
    pub fn register(&self, item: NewItem) -> String {
        self.book().new_item(item)
    }

    /// A settled or withdrawn call's item takes its status (the page says it
    /// is no longer waiting, and an answer to it is refused).
    pub fn set_status(&self, id: &str, status: Status) {
        self.book().set_status(id, status);
    }

    /// The next accepted answer, if one is waiting.
    pub fn try_take(&self) -> Option<Reply> {
        self.book().take_reply()
    }

    /// The run is over: `/state` says so; every still-waiting item is shown
    /// as waiting no more than the run itself.
    pub fn mark_over(&self) {
        self.book().over = true;
    }

    /// Close the listener: the page ends with the run (Appendix D). Requests
    /// already accepted finish; new connections are refused by the OS.
    pub fn close(&self) {
        self.book().closed = true;
    }

    fn book(&self) -> MutexGuard<'_, Book> {
        self.shared.book.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What an Approve sends (Appendix D): the input's `proposal` field if it is
/// a string, else the input itself if it is a string, else the input as JSON
/// text. Shared by the desk (recording the accepted answer against the item's
/// stored `payload`) and the engine (settling the call), so both read the
/// same rule.
pub(crate) fn approve_text_of(input: &Value) -> String {
    if let Some(Value::Str(p)) = input.get("proposal") {
        return p.clone();
    }
    if let Value::Str(s) = input {
        return s.clone();
    }
    json::write(input)
}

// One connection: parse what arrived, check the Host, the Origin and the
// token, and serve one of three routes (or refuse). One request per
// connection; every response says `Connection: close`.
fn serve(mut stream: TcpStream, shared: Arc<Shared>, wake: impl Fn() + Send + Sync) {
    // The listener is non-blocking (its accept loop polls the closed flag),
    // and on some platforms (macOS) an accepted socket inherits that: a read
    // before the request has arrived would fail at once and drop the
    // connection unanswered. Blocking, with the read timeout below, is what
    // the request reader expects.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));
    let Some(req) = read_request(&mut stream) else {
        return; // nothing readable, or over the caps: nothing is owed
    };
    let method = req.method.to_ascii_uppercase();
    let route = match req.path.split_once('?') {
        Some((r, _)) => r.to_string(),
        None => req.path.clone(),
    };
    // The Host must be exact (no DNS rebinding): the bound address or that
    // with its port.
    let host_ok = req.header("host").is_some_and(|h| is_own_host(&h, shared.port));
    if !host_ok {
        let _ = write_response(&mut stream, 403, PLAIN, "not this host\n");
        return;
    }
    // An Origin, when present, must be the page's own; a POST must have one
    // (a browser's fetch always sends it; a top-level navigation does not).
    let own = format!("http://127.0.0.1:{}", shared.port);
    match req.header("origin") {
        Some(o) if o == own => {}
        Some(_) => {
            let _ = write_response(&mut stream, 403, PLAIN, "not this origin\n");
            return;
        }
        None if method == "POST" => {
            let _ = write_response(&mut stream, 403, PLAIN, "an answer needs its origin\n");
            return;
        }
        None => {}
    }
    if method == "GET" && route == "/" {
        // The page: the token is in the query, compared in constant time.
        let ok = req
            .query
            .iter()
            .find(|(k, _)| k == "token")
            .is_some_and(|(_, v)| shared.who(v).is_some());
        if !ok {
            let _ = write_response(&mut stream, 403, PLAIN, "no page here\n");
            return;
        }
        let head = [
            ("content-type", "text/html; charset=utf-8"),
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ];
        let csp = format!(
            "default-src 'none'; script-src 'nonce-{}'; style-src 'nonce-{}'; connect-src 'self'",
            shared.nonce, shared.nonce
        );
        let _ = write_csp_response(&mut stream, 200, &head, &csp, &page(&shared.nonce));
        return;
    }
    if method == "GET" && route == "/state" {
        // The page's poll: the token rides in a header.
        let Some(who) = req.header("x-desk-token").and_then(|t| shared.who(&t)) else {
            let _ = write_response(&mut stream, 401, PLAIN, "no state here\n");
            return;
        };
        let text = json::write(&shared.book().state_json(who, &shared.people));
        let head = [
            ("content-type", "application/json; charset=utf-8"),
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ];
        let _ = write_csp_response(&mut stream, 200, &head, "", &text);
        return;
    }
    if method == "POST" && route == "/answer" {
        // The token in a header, compared in constant time.
        let Some(who) = req.header("x-desk-token").and_then(|t| shared.who(&t)) else {
            let _ = write_response(&mut stream, 401, JSON, "{\"error\":\"the token does not match\"}");
            return;
        };
        let head = [
            ("content-type", "application/json; charset=utf-8"),
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ];
        match answer_post(&req.body) {
            Ok(Posted {
                item: id,
                kind,
                text,
                option,
                why,
            }) => {
                let mut book = shared.book();
                // The grants (decision 0010): a person answers only the items
                // of the neurons they were granted, and only when the ask's
                // `answerable_by` names a role of theirs. A refusal is logged
                // on stderr, as the desk's other records are.
                if let (Who::Person(k), Some(item)) = (who, book.items.iter().find(|i| i.id == id)) {
                    let person = &shared.people[k];
                    if !person.may_answer(&item.neuron, &item.payload) {
                        let note = json::Obj::new()
                            .str("item", &id)
                            .str("neuron", &item.neuron)
                            .str("by", &person.id)
                            .str("why", "not granted")
                            .build();
                        eprintln!("desk refused: {}", json::write(&note));
                        drop(book);
                        let body = format!(
                            "{{\"error\":\"{} may not answer this item (it is not granted to them)\"}}",
                            escape_json(&person.name)
                        );
                        let _ = write_csp_response(&mut stream, 403, &head, "", &body);
                        return;
                    }
                }
                match book.items.iter().find(|i| i.id == id).map(|i| i.status) {
                    Some(Status::Waiting) => {
                        let (neuron, chosen) = {
                            let item = book.items.iter().find(|i| i.id == id).expect("the waiting item");
                            let chosen = option.as_deref().map(|o| option_text(&item.payload, o));
                            (item.neuron.clone(), chosen)
                        };
                        if let Some(None) = chosen {
                            drop(book);
                            let body = format!(
                                "{{\"error\":\"the item has no option '{}'\"}}",
                                escape_json(option.as_deref().unwrap_or(""))
                            );
                            let _ = write_csp_response(&mut stream, 400, &head, "", &body);
                            return;
                        }
                        let text = match chosen {
                            Some(Some(t)) => t,
                            _ => text,
                        };
                        // What the run will carry (an approve sends the
                        // proposal, else the input; an edit the person's
                        // text; a no empty), and the host's own channel for
                        // it: the run can end the moment this settles and
                        // the desk closes with it, so the answer goes out as
                        // one `desk answer:` line in the same breath. The
                        // item's `payload` holds the input value whole, the
                        // same rule the engine's settle reads.
                        let accepted = {
                            let item = book.items.iter().find(|i| i.id == id).expect("the waiting item");
                            match kind {
                                Kind::Approve => approve_text_of(&item.payload),
                                Kind::Edit | Kind::Choose => text.clone(),
                                Kind::No => String::new(),
                            }
                        };
                        let by = shared.by(who);
                        let note = json::Obj::new()
                            .str("item", &id)
                            .str("kind", kind.name())
                            .str("text", accepted.clone())
                            .set_if("by", by.clone().map(Value::Str))
                            .set_if("option", option.clone().map(Value::Str))
                            .set_if("why", why.clone().map(Value::Str))
                            .build();
                        eprintln!("desk answer: {}", json::write(&note));
                        let ms = shared.opened.elapsed().as_millis() as u64;
                        book.record.push(Record {
                            item: id.clone(),
                            neuron,
                            kind,
                            text: accepted,
                            by,
                            option,
                            why,
                            ms,
                        });
                        book.replies.push(Reply {
                            item: id.clone(),
                            kind,
                            text: text.clone(),
                        });
                        book.set_status(&id, Status::Answered);
                        drop(book);
                        wake();
                        let head = [
                            ("content-type", "application/json; charset=utf-8"),
                            ("cache-control", "no-store"),
                            ("x-content-type-options", "nosniff"),
                        ];
                        let _ = write_csp_response(&mut stream, 200, &head, "", "{\"ok\":true}");
                    }
                    Some(st) => {
                        let head = [
                            ("content-type", "application/json; charset=utf-8"),
                            ("cache-control", "no-store"),
                            ("x-content-type-options", "nosniff"),
                        ];
                        let body = format!("{{\"error\":\"the item is no longer waiting ({})\"}}", st.name());
                        let _ = write_csp_response(&mut stream, 410, &head, "", &body);
                    }
                    None => {
                        let head = [
                            ("content-type", "application/json; charset=utf-8"),
                            ("cache-control", "no-store"),
                            ("x-content-type-options", "nosniff"),
                        ];
                        let _ = write_csp_response(&mut stream, 404, &head, "", "{\"error\":\"no such item\"}");
                    }
                }
            }
            Err(why) => {
                let head = [
                    ("content-type", "application/json; charset=utf-8"),
                    ("cache-control", "no-store"),
                    ("x-content-type-options", "nosniff"),
                ];
                let body = format!("{{\"error\":\"{}\"}}", escape_json(&why));
                let _ = write_csp_response(&mut stream, 400, &head, "", &body);
            }
        }
        return;
    }
    let _ = write_response(&mut stream, 404, PLAIN, "nothing here\n");
}

fn is_own_host(host: &str, port: u16) -> bool {
    host == format!("127.0.0.1:{port}") || host == "127.0.0.1"
}

/// Constant-time equality of two byte strings (0008: the token is compared in
/// constant time): the difference folds over both lengths, whatever the
/// argument's length.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u32;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= (x ^ y) as u32;
    }
    diff == 0
}

/// A POST's answer, parsed.
struct Posted {
    item: String,
    kind: Kind,
    text: String,
    option: Option<String>,
    why: Option<String>,
}

/// The POST body: `{item, kind: approve|edit|no|choose, text?, option?, why?}`
/// (the wording is the engine's own; 0008 fixes the three answers, 0010 adds
/// `choose`). An edit answers with the person's text, never empty: an empty
/// one is an error reply, and the item keeps waiting (Appendix D). A `no`
/// answers empty (quiet); its text, when present, is ignored. Approve's text
/// is computed on the engine's side, and a choice's from the item's option.
/// `why`, on any kind, is one line for the record, never sent to the network.
fn answer_post(body: &[u8]) -> Result<Posted, String> {
    if body.len() > ANSWER_BODY_CAP {
        return Err(format!("the body is over {} bytes", ANSWER_BODY_CAP));
    }
    let text_str = std::str::from_utf8(body).map_err(|_| "the body is not UTF-8".to_string())?;
    let v = json::parse(text_str).map_err(|_| "the body is not one JSON value".to_string())?;
    let item = match v.get("item") {
        Some(Value::Str(s)) => s.clone(),
        _ => return Err("the body has no item".into()),
    };
    let kind = match v.get("kind") {
        Some(Value::Str(k)) => match k.as_str() {
            "approve" => Kind::Approve,
            "edit" => Kind::Edit,
            "no" => Kind::No,
            "choose" => Kind::Choose,
            _ => return Err(format!("the kind '{}' is not approve, edit, no or choose", k)),
        },
        _ => return Err("the body has no kind".into()),
    };
    let text = match v.get("text") {
        Some(Value::Str(s)) => s.clone(),
        Some(_) => return Err("the text is not a string".into()),
        None => String::new(),
    };
    if kind == Kind::Edit && text.trim().is_empty() {
        return Err("an edit answers with the person's text, and a blank edit is not an answer".into());
    }
    let option = match (kind, v.get("option")) {
        (Kind::Choose, Some(Value::Str(o))) => Some(o.clone()),
        (Kind::Choose, _) => return Err("a choice names one of the item's options".into()),
        (_, None) => None,
        (_, Some(_)) => return Err("only a choice names an option".into()),
    };
    let why = match v.get("why") {
        None => None,
        Some(Value::Str(w)) => {
            let w = w.trim();
            if w.contains('\n') || w.chars().count() > WHY_CAP {
                return Err(format!("a why is one line of at most {WHY_CAP} characters"));
            }
            (!w.is_empty()).then(|| w.to_string())
        }
        Some(_) => return Err("the why is not a string".into()),
    };
    Ok(Posted {
        item,
        kind,
        text,
        option,
        why,
    })
}

fn escape_json(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '"' => "\\\"".to_string(),
            '\\' => "\\\\".to_string(),
            '\n' => "\\n".to_string(),
            '\r' => String::new(),
            c => c.to_string(),
        })
        .collect()
}

// -- The request ------------------------------------------------------------

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<String> {
        self.headers.get(&name.to_ascii_lowercase()).cloned()
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut raw: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        if let Some(hit) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break hit;
        }
        if raw.len() > HEAD_CAP {
            return None;
        }
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next()?.to_string();
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let content_length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if content_length > ANSWER_BODY_CAP {
        return None;
    }
    let mut body = raw[head_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    body.truncate(content_length);
    let (path, query) = qsplit(&target);
    Some(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

fn qsplit(target: &str) -> (String, Vec<(String, String)>) {
    match target.split_once('?') {
        None => (target.to_string(), Vec::new()),
        Some((p, q)) => {
            let mut pairs = Vec::new();
            for pair in q.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    pairs.push((url_decode(k), url_decode(v)));
                } else if !pair.is_empty() {
                    pairs.push((url_decode(pair), String::new()));
                }
            }
            (p.to_string(), pairs)
        }
    }
}

/// Percent-decoding for the query's one value (the token) and its keys.
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 3 <= bytes.len() {
            match std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                Some(b) => {
                    out.push(b);
                    i += 3;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

// -- The response -----------------------------------------------------------

const PLAIN: [(&str, &str); 2] = [("content-type", "text/plain; charset=utf-8"), ("cache-control", "no-store")];
const JSON: [(&str, &str); 2] = [("content-type", "application/json; charset=utf-8"), ("cache-control", "no-store")];

fn write_response(
    stream: &mut TcpStream,
    code: u16,
    headers: impl IntoIterator<Item = (&'static str, &'static str)>,
    body: &str,
) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        410 => "Gone",
        _ => "OK",
    };
    let mut head = format!("HTTP/1.1 {code} {reason}\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!(
        "x-content-type-options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

/// The same response with one more header, the page's CSP.
fn write_csp_response(
    stream: &mut TcpStream,
    code: u16,
    headers: &[(&'static str, &'static str)],
    csp: &str,
    body: &str,
) -> std::io::Result<()> {
    let mut all: Vec<(&'static str, &'static str)> = headers.to_vec();
    if !csp.is_empty() {
        all.push(("content-security-policy", ""));
    }
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        410 => "Gone",
        _ => "OK",
    };
    let mut head = format!("HTTP/1.1 {code} {reason}\r\n");
    for (k, v) in &all {
        let value = if k == &"content-security-policy" { csp } else { v };
        head.push_str(&format!("{k}: {value}\r\n"));
    }
    head.push_str(&format!(
        "x-content-type-options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

// -- The page ----------------------------------------------------------------

/// The page: one inline `<style>` and one inline `<script>`, both under the
/// CSP's nonce; every input value is put into the DOM as text
/// (`.textContent`, `createTextNode`), never as HTML; nothing is fetched
/// anywhere but this page (`connect-src 'self'`, everything else `none`).
/// System fonts only. The look is minimal on purpose: decision 0008's theme
/// is the page's rule set, not a conformance item.
const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>agent-neuron desk</title>
<style nonce="@NONCE@">
  :root { color-scheme: dark; }
  body { margin: 0; padding: 24px; background: #000; color: #e5e7eb;
         font-family: ui-sans-serif, system-ui, "Helvetica Neue", Arial, sans-serif;
         font-size: 15px; line-height: 1.5; }
  h1 { font-size: 17px; margin: 0 0 4px; color: #39ff00; letter-spacing: .02em; }
  h1 span { color: #9ca3af; font-weight: normal; }
  #over { border: 1px solid #06b6d4; color: #06b6d4; display: none;
          margin: 12px 0 20px; padding: 6px 10px; max-width: 720px; }
  .card { border: 1px solid #1f2937; margin: 0 0 20px; max-width: 720px; padding: 14px 16px; }
  .card .neuron { color: #9ca3af; font-size: 13px; }
  .card h2 { font-size: 15px; margin: 2px 0 10px; color: #39ff00; }
  .notes { margin: 0 0 10px; padding-left: 18px; color: #9ca3af; font-size: 13px; }
  .pair { display: grid; gap: 12px; grid-template-columns: 1fr 1fr; }
  @media (max-width: 640px) { .pair { grid-template-columns: 1fr; } }
  .col { background: #060606; border: 1px solid #1f2937; padding: 8px 10px;
         white-space: pre-wrap; overflow-wrap: anywhere; font-size: 14px; }
  .col b { color: #9ca3af; display: block; font-size: 12px; font-weight: normal;
           margin-bottom: 4px; }
  .col del { background: rgba(226,86,86,.25); color: #f5a0a0; text-decoration: line-through; }
  .col ins { background: rgba(57,255,0,.18); color: #d6ffb8; text-decoration: none; }
  .single { background: #060606; border: 1px solid #1f2937; padding: 8px 10px;
            white-space: pre-wrap; overflow-wrap: anywhere; font-size: 14px; }
  .row { margin-top: 10px; }
  button { background: #060606; border: 1px solid #39ff00; color: #39ff00; cursor: pointer;
           font: inherit; padding: 4px 14px; }
  button.nah { border-color: #1f2937; color: #9ca3af; }
  button.send { border-color: #06b6d4; color: #06b6d4; }
  button:disabled { border-color: #1f2937; color: #4b5563; cursor: default; }
  textarea { background: #060606; border: 1px solid #1f2937; color: #e5e7eb; display: block;
             font: inherit; margin: 10px 0; min-height: 84px; padding: 6px 8px;
             width: calc(100% - 18px); }
  .state { color: #9ca3af; font-size: 12px; margin-left: 10px; }
  .waiting .state { color: #39ff00; }
  .err { color: #f5a0a0; font-size: 13px; margin-top: 6px; min-height: 18px; }
  #table { color: #9ca3af; font-size: 13px; margin: 0 0 16px; max-width: 720px; }
  #table .p { display: inline-block; margin-right: 14px; }
  #table .p.me { color: #e5e7eb; }
  #table .p.busy b { color: #39ff00; font-weight: normal; }
  .card.theirs { opacity: .62; }
  .card.theirs h2, .card.answered h2, .card.withdrawn h2, .card.timed-out h2 { color: #9ca3af; }
  .waits { color: #9ca3af; font-size: 13px; margin-top: 8px; }
  .options { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 10px; }
  .whyline { background: #060606; border: 1px solid #1f2937; color: #e5e7eb; font: inherit;
             margin-top: 8px; padding: 4px 8px; width: calc(100% - 18px); }
  .done { color: #06b6d4; font-size: 13px; margin-top: 8px; }
  h2.rec { color: #9ca3af; font-size: 14px; margin: 26px 0 8px; }
  #record { border: 1px solid #1f2937; color: #9ca3af; font-size: 13px; margin: 0;
            max-width: 720px; padding: 10px 10px 10px 30px; }
  #record .by { color: #e5e7eb; }
</style>
</head>
<body>
<h1>agent-neuron <span id="who">desk</span></h1>
<div id="table"></div>
<div id="over">The run is over.</div>
<div id="items"></div>
<h2 class="rec">Record</h2>
<ol id="record"></ol>
<script nonce="@NONCE@">
"use strict";
const TOKEN = new URLSearchParams(window.location.search).get("token") || "";
if (!TOKEN) {
  document.getElementById("items").textContent = "no token in the address";
}
const HEADERS = { "X-Desk-Token": TOKEN };

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) { e.className = cls; }
  if (text !== undefined) { e.textContent = text; }
  return e;
}

// The word diff of the draft/proposal convention: the common prefix and
// suffix first, then a bounded LCS over the middles, so a proposal reads by
// the words that changed. A plain side-by-side when either text is long.

function words(s) {
  const out = [];
  const re = /\S+|\s+/g;
  let m;
  while ((m = re.exec(s)) !== null) { out.push(m[0]); }
  return out;
}

function midDiff(aw, bw) {
  const n = aw.length, m = bw.length;
  const dp = [];
  for (let i = 0; i <= n; i++) { dp.push(new Uint32Array(m + 1)); }
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = aw[i] === bw[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const marks = [];
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (aw[i] === bw[j]) { marks.push({ w: aw[i], k: 0 }); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) { marks.push({ w: aw[i], k: 1 }); i++; }
    else { marks.push({ w: bw[j], k: 2 }); j++; }
  }
  while (i < n) { marks.push({ w: aw[i], k: 1 }); i++; }
  while (j < m) { marks.push({ w: bw[j], k: 2 }); j++; }
  return marks;
}

function fullDiff(a, b) {
  const aw = words(a), bw = words(b);
  let p = 0;
  while (p < aw.length && p < bw.length && aw[p] === bw[p]) { p++; }
  let s = 0;
  while (s < aw.length - p && s < bw.length - p && aw[aw.length - 1 - s] === bw[bw.length - 1 - s]) { s++; }
  const mid = midDiff(aw.slice(p, aw.length - s), bw.slice(p, bw.length - s));
  const head = aw.slice(0, p).map((w) => ({ w: w, k: 0 }));
  const tail = aw.slice(aw.length - s).map((w) => ({ w: w, k: 0 }));
  return head.concat(mid, tail);
}

function diffColumn(label, text, isProposal, marks) {
  const col = el("div", "col");
  col.appendChild(el("b", null, label));
  if (!marks) {
    col.appendChild(document.createTextNode(text));
    return col;
  }
  for (const mark of marks) {
    if (mark.k === 0) {
      col.appendChild(document.createTextNode(mark.w));
    } else if ((mark.k === 1) !== isProposal) {
      const e = el(isProposal ? "ins" : "del");
      e.textContent = mark.w;
      col.appendChild(e);
    }
  }
  return col;
}

const DIFF_LEN = 20000, DIFF_WORDS = 600;
function showPair(card, item) {
  const pair = el("div", "pair");
  const marks =
    item.draft.length < DIFF_LEN && item.proposal.length < DIFF_LEN &&
    words(item.draft).length < DIFF_WORDS && words(item.proposal).length < DIFF_WORDS
      ? fullDiff(item.draft, item.proposal)
      : null;
  if (marks) {
    pair.appendChild(diffColumn("draft", item.draft, false, marks));
    pair.appendChild(diffColumn("proposal", item.proposal, true, marks));
  } else {
    pair.appendChild(diffColumn("draft", item.draft, false, null));
    pair.appendChild(diffColumn("proposal", item.proposal, true, null));
  }
  card.appendChild(pair);
}

let PEOPLE = {};
let ITEMS = {};
function optionLabel(r) {
  const it = ITEMS[r.item];
  const opts = it && it.payload && Array.isArray(it.payload.options) ? it.payload.options : [];
  const o = opts.find((x) => String(x.id) === r.option);
  return o && o.label ? String(o.label) : (r.option || "");
}
function nameOf(id) { return (PEOPLE[id] && PEOPLE[id].name) || id; }

async function answer(item, kind, text, option) {
  const card = document.querySelector('[data-item="' + item.id + '"]');
  const whyEl = card ? card.querySelector(".whyline") : null;
  const why = whyEl ? whyEl.value.trim() : "";
  const msg = { item: item.id, kind: kind, text: text === undefined ? "" : text };
  if (option !== undefined) { msg.option = option; }
  if (why) { msg.why = why; }
  const body = JSON.stringify(msg);
  try {
    const r = await fetch("/answer", { method: "POST", headers: HEADERS, body: body });
    if (!r.ok) {
      const j = await r.json().catch(() => null);
      if (j && j.error) {
        const card = document.querySelector('[data-item="' + item.id + '"]');
        if (card) { card.querySelector(".err").textContent = j.error; }
      }
    }
  } catch (e) {
    const card = document.querySelector('[data-item="' + item.id + '"]');
    if (card) { card.querySelector(".err").textContent = "the desk is not answering"; }
  }
  poll();
}

function whyFor(card, ok) {
  const why = el("input", "whyline");
  why.placeholder = "optional: one line on why, for the record (never sent on)";
  why.maxLength = 500;
  why.disabled = !ok;
  card.appendChild(why);
}

function optionsFor(card, item, options) {
  const ok = item.status === "waiting";
  const row = el("div", "options");
  for (const o of options) {
    const b = el("button", null, String(o.label || o.id));
    b.disabled = !ok;
    b.addEventListener("click", () => answer(item, "choose", "", String(o.id)));
    row.appendChild(b);
  }
  const nah = el("button", "nah", "No");
  nah.disabled = !ok;
  nah.addEventListener("click", () => answer(item, "no"));
  row.appendChild(nah);
  card.appendChild(row);
  whyFor(card, ok);
  card.appendChild(el("div", "err"));
}

function buttonsFor(card, item) {
  const ok = item.status === "waiting";
  const row = el("div", "row");
  const b = el("button", null, "Approve");
  b.disabled = !ok;
  b.addEventListener("click", () => answer(item, "approve"));
  const nah = el("button", "nah", "No");
  nah.disabled = !ok;
  nah.addEventListener("click", () => answer(item, "no"));
  const ta = el("textarea");
  ta.disabled = !ok;
  ta.placeholder = "or your own text, and send it";
  ta.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) { answer(item, "edit", ta.value); }
  });
  const send = el("button", "send", "Send edit");
  send.disabled = !ok;
  send.addEventListener("click", () => answer(item, "edit", ta.value));
  const err = el("div", "err");
  row.appendChild(b);
  row.appendChild(nah);
  row.appendChild(ta);
  row.appendChild(send);
  card.appendChild(row);
  whyFor(card, ok);
  card.appendChild(err);
}

function recordLine(r) {
  const li = el("li");
  if (r.by) { li.appendChild(el("span", "by", nameOf(r.by))); li.appendChild(document.createTextNode(" ")); }
  let what = r.kind + " on " + r.neuron + " (" + r.item + ")";
  if (r.kind === "choose") { what = "chose " + optionLabel(r) + " on " + r.neuron + " (" + r.item + ")"; }
  if (r.text && r.kind !== "no" && r.kind !== "choose") { what += ": " + r.text; }
  if (r.why) { what += " - why: " + r.why; }
  li.appendChild(document.createTextNode(what));
  return li;
}

function renderTable(state) {
  const table = document.getElementById("table");
  table.textContent = "";
  PEOPLE = {};
  const me = state.me;
  const people = state.people || [];
  for (const p of people) { PEOPLE[p.id] = p; }
  const mine = state.items.filter((i) => i.status === "waiting" && i.yours).length;
  const who = me ? (me.name + (me.roles && me.roles.length ? " (" + me.roles.join(", ") + ")" : "")) : "desk";
  document.getElementById("who").textContent = who;
  document.title = (mine ? "(" + mine + ") " : "") + "desk" + (me ? " - " + me.name : "");
  if (!people.length) { return; }
  table.appendChild(document.createTextNode("At this table: "));
  for (const p of people) {
    const span = el("span", "p" + (me && me.id === p.id ? " me" : "") + (p.waiting ? " busy" : ""));
    span.appendChild(document.createTextNode(p.name + " "));
    span.appendChild(el("b", null, p.waiting ? p.waiting + " waiting" : "nothing waiting"));
    table.appendChild(span);
  }
}

function render(state) {
  renderTable(state);
  const over = document.getElementById("over");
  over.style.display = state.over ? "block" : "none";
  ITEMS = {};
  for (const it of state.items) { ITEMS[it.id] = it; }
  const recEl = document.getElementById("record");
  recEl.textContent = "";
  const record = state.record || [];
  for (const r of record) { recEl.appendChild(recordLine(r)); }
  if (!record.length) { recEl.appendChild(el("li", null, "Nothing decided yet.")); }
  const typing = document.activeElement && document.activeElement.closest ? document.activeElement.closest(".card") : null;
  if (typing && typing.dataset && state.items.some((i) => i.id === typing.dataset.item && i.status === "waiting")) {
    // A person mid-sentence keeps their card: the rest refreshes on the next poll.
    return;
  }
  const root = document.getElementById("items");
  root.textContent = "";
  if (state.items.length === 0) {
    if (state.over) { root.appendChild(el("p", null, "No questions were asked.")); }
    return;
  }
  for (const item of state.items) {
    const card = el("div", "card " + item.status + (item.yours === false ? " theirs" : ""));
    card.dataset.item = item.id;
    card.appendChild(el("div", "neuron", item.neuron));
    card.appendChild(el("h2", null, item.title || item.neuron));
    if (item.notes && item.notes.length) {
      const notes = el("ul", "notes");
      for (const note of item.notes) { notes.appendChild(el("li", null, note)); }
      card.appendChild(notes);
    }
    const hasOptions = item.payload && Array.isArray(item.payload.options) && item.payload.options.length > 0;
    if (item.draft !== undefined && item.proposal !== undefined) {
      showPair(card, item);
    } else if (item.text !== undefined && !hasOptions) {
      card.appendChild(el("div", "single", item.text));
    }
    const options = item.payload && Array.isArray(item.payload.options) ? item.payload.options : null;
    const done = record.find((r) => r.item === item.id);
    if (done) {
      card.appendChild(el("div", "done", "decided: " + recordLine(done).textContent));
    } else if (item.yours === false) {
      const ids = item.answerable_by || [];
      card.appendChild(el("div", "waits", item.status === "waiting"
        ? "waiting on " + (ids.length ? ids.map(nameOf).join(" or ") : "nobody at this table")
        : item.status));
    } else if (options && options.length) {
      optionsFor(card, item, options);
    } else {
      buttonsFor(card, item);
    }
    card.appendChild(el("span", "state", item.status));
    root.appendChild(card);
  }
}

let closed = false;
async function poll() {
  if (closed) { return; }
  try {
    const r = await fetch("/state", { headers: HEADERS });
    if (!r.ok) { throw new Error(String(r.status)); }
    const j = await r.json();
    if (j.over) { closed = true; render(j); return; }
    render(j);
  } catch (e) {
    // The desk is gone (the run ended, or the token is wrong): stop polling.
    closed = true;
    const over = document.getElementById("over");
    if (document.querySelectorAll(".card").length === 0) {
      over.textContent = "The page does not answer (the run ended, or the token is wrong).";
      over.style.display = "block";
    }
    return;
  }
  setTimeout(poll, 700);
}

poll();
</script>
</body>
</html>
"#;

fn page(nonce: &str) -> String {
    PAGE.replace("@NONCE@", nonce)
}

// -- Randomness --------------------------------------------------------------

/// Hex of `bytes` fresh bytes, from /dev/urandom; on a system without it, a
/// mix of the clock, the pid and the buffer's own address (a desk token is a
/// secret against the network it is not published on; /dev/urandom exists on
/// both platforms the engine builds for).
fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    let fresh = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| {
            use std::io::Read as _;
            f.read_exact(&mut buf).map(|_| true)
        })
        .unwrap_or_else(|_| {
            mix_fallback(&mut buf);
            true
        });
    let _ = fresh;
    let mut out = String::with_capacity(bytes * 2);
    for b in buf {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn mix_fallback(buf: &mut [u8]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ ((std::process::id() as u64) << 32)
        ^ (buf.as_ptr() as usize as u64);
    for b in buf.iter_mut() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *b = (seed >> 33) as u8;
    }
}
