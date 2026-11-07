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
use crate::json::{self, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// A POST's JSON body cap: an edit's text is a paragraph, not a file.
pub const ANSWER_BODY_CAP: usize = 32_768;
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
        }
    }
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

    fn state_json(&self) -> Value {
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|i| {
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
                    .build()
            })
            .collect();
        json::Obj::new()
            .set("over", Value::Bool(self.over))
            .set("items", Value::Arr(items))
            .build()
    }
}

struct Shared {
    token: String,
    nonce: String,
    port: u16,
    book: Mutex<Book>,
}

impl Shared {
    fn book(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
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
    pub fn open(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Desk, String> {
        let listener = TcpListener::bind((IpAddr::from([127, 0, 0, 1]), 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let _ = listener.set_nonblocking(true);
        let token = random_hex(16);
        let nonce = random_hex(16);
        let serving = listener;
        let shared = Arc::new(Shared {
            token: token.clone(),
            nonce: nonce.clone(),
            port,
            book: Mutex::new(Book::default()),
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
            .is_some_and(|(_, v)| ct_eq(v.as_bytes(), shared.token.as_bytes()));
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
        let ok = req
            .header("x-desk-token")
            .is_some_and(|t| ct_eq(t.as_bytes(), shared.token.as_bytes()));
        if !ok {
            let _ = write_response(&mut stream, 401, PLAIN, "no state here\n");
            return;
        }
        let text = json::write(&shared.book().state_json());
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
        let ok = req
            .header("x-desk-token")
            .is_some_and(|t| ct_eq(t.as_bytes(), shared.token.as_bytes()));
        if !ok {
            let _ = write_response(&mut stream, 401, JSON, "{\"error\":\"the token does not match\"}");
            return;
        }
        match answer_post(&req.body) {
            Ok((id, kind, text)) => {
                let mut book = shared.book();
                match book.items.iter().find(|i| i.id == id).map(|i| i.status) {
                    Some(Status::Waiting) => {
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
                                Kind::Edit => text.clone(),
                                Kind::No => String::new(),
                            }
                        };
                        let note = json::Obj::new()
                            .str("item", &id)
                            .str("kind", kind.name())
                            .str("text", accepted)
                            .build();
                        eprintln!("desk answer: {}", json::write(&note));
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

/// The POST body: `{item, kind: approve|edit|no, text?}` (the wording is the
/// engine's own; 0008 fixes the three answers). An edit answers with the
/// person's text, never empty: an empty one is an error reply, and the item
/// keeps waiting (Appendix D). A `no` answers empty (quiet); its text, when
/// present, is ignored. Approve's text is computed on the engine's side.
fn answer_post(body: &[u8]) -> Result<(String, Kind, String), String> {
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
            _ => return Err(format!("the kind '{}' is not approve, edit or no", k)),
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
    Ok((item, kind, text))
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
</style>
</head>
<body>
<h1>agent-neuron <span>desk</span></h1>
<div id="over">The run is over.</div>
<div id="items"></div>
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

async function answer(item, kind, text) {
  const body = JSON.stringify({ item: item.id, kind: kind, text: text === undefined ? "" : text });
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
  card.appendChild(err);
}

function render(state) {
  const over = document.getElementById("over");
  over.style.display = state.over ? "block" : "none";
  const root = document.getElementById("items");
  root.textContent = "";
  if (state.items.length === 0) {
    if (state.over) { root.appendChild(el("p", null, "No questions were asked.")); }
    return;
  }
  for (const item of state.items) {
    const card = el("div", "card " + item.status);
    card.dataset.item = item.id;
    card.appendChild(el("div", "neuron", item.neuron));
    card.appendChild(el("h2", null, item.title || item.neuron));
    if (item.notes && item.notes.length) {
      const notes = el("ul", "notes");
      for (const note of item.notes) { notes.appendChild(el("li", null, note)); }
      card.appendChild(notes);
    }
    if (item.draft !== undefined && item.proposal !== undefined) {
      showPair(card, item);
    } else if (item.text !== undefined) {
      card.appendChild(el("div", "single", item.text));
    }
    buttonsFor(card, item);
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
