// The `http` neuron's client (SPEC 3.3): a std-only HTTP/1.1 client, one
// request per call. The input value (3.1) is POSTed as `application/json` to
// the neuron's URL; the reply is read whole, under a size cap, and shaped per
// 7.2 on the engine's side. Redirects are not followed; there is no
// authentication; `https://` never reaches this file (the run refuses such a
// network at its start: a std-only engine has no TLS).
//
// The request runs in its own thread and reports through the engine's host
// queue — the same channel the process layer reports through — stamped when
// the thread pushed it. A cancel (staleness or the call's timeout) shuts the
// socket down both ways. The thread's deadline is the call's `timeout` as
// well, but the thread reports nothing at it: the engine owns the timeout (a
// call times out only when strictly longer, 6.1's `timed out after N ms`, the
// outcome at the same instant winning, 6.2 rule 7). What a cancelled or
// deadlined thread read (a late reply) is discarded and frees nothing.
use crate::host;
use crate::json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The response body's cap: the size 3.1 puts on a level 0 output and 3.2 on
/// a level 1 line. An http response body is the call's output; the engine
/// reports one over this cap as `output too large` (6.1's `output` row).
pub const BODY_CAP: usize = 1_048_576;
/// The response head's cap (status line and headers).
const HEAD_CAP: usize = 32_768;
/// A chunk size line's cap.
const CHUNK_LINE_CAP: usize = 256;

/// One request's whole outcome, as the client reports it (SPEC 3.3).
#[derive(Debug)]
pub enum HttpOut {
    /// A 2xx response whose body is one JSON value: an answer with it.
    Answer(json::Value),
    /// A 2xx response with no body (a 204, or any other 2xx with an empty
    /// body): quiet.
    Quiet,
    /// The response body over the cap: the engine reports `output too large`.
    OverCap,
    /// The request itself failed (a non-2xx, a connect failure, a non-JSON
    /// body): the message starts `http`, as 6.1's `http` row has it.
    Err(String),
}

/// A started request that a cancel can abort: the socket is shut down both
/// ways. A cancel that arrives while the name is resolving or the connection
/// opening cannot interrupt that step; the thread stops at its next check.
pub struct Cancel {
    sock: Arc<Mutex<Option<TcpStream>>>,
    cancelled: Arc<AtomicBool>,
}

impl Cancel {
    fn new() -> Cancel {
        Cancel {
            sock: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Shut the socket down both ways and mark the request cancelled.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.sock.lock() {
            if let Some(s) = slot.take() {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

/// Begin one request on its own thread. `deadline` is the call's whole time
/// budget (the neuron's `timeout`): the connect gets what remains of it, and
/// every step after checks it. `idx` indexes the call on the engine's side.
pub fn begin(post: host::Post, idx: usize, url: &str, body: &[u8], deadline: Duration) -> Cancel {
    let cancel = Cancel::new();
    let thread_cancel = Cancel {
        sock: cancel.sock.clone(),
        cancelled: cancel.cancelled.clone(),
    };
    let (body, url) = (body.to_vec(), url.to_string());
    host::spawn_thread("http", move || {
        let out = request(&url, &body, deadline, &thread_cancel);
        // A cancelled request, and one that gave up at its deadline with
        // nothing read, report nothing: the engine's timeout owns the call.
        if thread_cancel.cancelled.load(Ordering::SeqCst) {
            return;
        }
        if let Some(out) = out {
            post.post(host::Ev::HttpDone { idx, out });
        }
    });
    cancel
}

fn cancelled(c: &Cancel) -> bool {
    c.cancelled.load(Ordering::SeqCst)
}

/// The request's remaining time: `left()` is what is still there of the
/// call's budget; a `None` ends the effort with nothing read (the engine's
/// timeout owns the outcome).
struct Budget {
    started: std::time::Instant,
    deadline: Duration,
}

impl Budget {
    fn left(&self) -> Option<Duration> {
        let spend = self.started.elapsed();
        if spend >= self.deadline {
            None
        } else {
            Some(self.deadline - spend)
        }
    }
}

/// Parse the URL and run the POST against it; `None` is the deadline ending
/// the effort with nothing read (no outcome to push).
fn request(url: &str, body: &[u8], deadline: Duration, cancel: &Cancel) -> Option<HttpOut> {
    let budget = Budget {
        started: std::time::Instant::now(),
        deadline,
    };
    let Some((host, port, path)) = parse_url(url) else {
        return Some(HttpOut::Err(format!(
            "http: the URL cannot be used as a plain http request: '{url}'"
        )));
    };
    // The name's resolution is the one step a cancel cannot interrupt: the
    // platform's resolver blocks, and std does not abort it.
    budget.left()?;
    let addrs: Vec<SocketAddr> = match (host.as_str(), port).to_socket_addrs() {
        Ok(it) => it.collect(),
        Err(e) => return Some(HttpOut::Err(format!("http: cannot resolve '{host}': {e}"))),
    };
    if addrs.is_empty() {
        return Some(HttpOut::Err(format!("http: cannot resolve '{host}' (no addresses)")));
    }
    let mut stream: Option<TcpStream> = None;
    let mut last = "nothing answered".to_string();
    for addr in addrs {
        let budget = budget.left()?;
        if cancelled(cancel) {
            return None;
        }
        match TcpStream::connect_timeout(&addr, budget) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = format!("{} ({addr}: {e})", last),
        }
    }
    let Some(mut s) = stream else {
        return Some(HttpOut::Err(format!("http: cannot connect to {host}:{port}: {last}")));
    };
    if let Ok(mut slot) = cancel.sock.lock() {
        if cancelled(cancel) {
            let _ = s.shutdown(std::net::Shutdown::Both);
            return None;
        }
        *slot = Some(s.try_clone().expect("a socket handle"));
    }
    let _ = s.set_nodelay(true);
    let head_budget = budget.left()?;
    let _ = s.set_read_timeout(Some(head_budget));
    let _ = s.set_write_timeout(Some(head_budget));
    let head = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        path,
        host_with_port(&host, port),
        body.len()
    );
    let full = [head.as_bytes(), body].concat();
    if let Err(e) = s.write_all(&full) {
        return Some(HttpOut::Err(format!("http: the request's write failed: {e}")));
    }
    read_response(&mut s, &budget, cancel)
}

fn host_with_port(host: &str, port: u16) -> String {
    if port == 80 {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

/// `http://host[:port][/path]` only — the caller never hands this `https://`
/// (the run refuses one), and a URL that needs user info or escapes is
/// refused here as unusable (3.3 has no authentication).
fn parse_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    if rest.contains('@') || rest.contains('\\') || rest.contains(' ') {
        return None;
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() {
        return None;
    }
    let (host, port) = match authority.rfind(':') {
        Some(i) => {
            let digit = authority[i + 1..].chars().all(|c| c.is_ascii_digit());
            if !digit {
                return None;
            }
            let p: u16 = authority[i + 1..].parse().ok()?;
            (authority[..i].to_string(), p)
        }
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

/// The response: head to its blank line, body as the head says, under the
/// cap. Whatever read past the head's blank line rides in the body: the head
/// reader's surplus is never thrown away. The deadline holds throughout; at
/// it the client stops and returns `None` (the engine's timeout owns the
/// call).
fn read_response(s: &mut TcpStream, budget: &Budget, cancel: &Cancel) -> Option<HttpOut> {
    let mut head: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let head_done = loop {
        if let Some(hit) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break hit;
        }
        if head.len() > HEAD_CAP {
            return Some(HttpOut::Err(format!("http: the response head is over {HEAD_CAP} bytes")));
        }
        let budget = budget.left()?;
        let _ = s.set_read_timeout(Some(budget));
        match s.read(&mut buf) {
            Ok(0) => return Some(HttpOut::Err("http: the server closed before a response head".into())),
            Ok(n) => head.extend_from_slice(&buf[..n]),
            Err(e) => {
                if cancelled(cancel) {
                    return None;
                }
                return Some(HttpOut::Err(format!("http: the response head could not be read: {e}")));
            }
        }
    };
    // The body starts at the head's blank line; whatever the head reader
    // already read rides with it into the body's source.
    let over = head.split_off(head_done + 4);
    let mut src = Src { sock: s, carry: over };
    let text = String::from_utf8_lossy(&head[..head_done]);
    let mut lines = text.split("\r\n").filter(|l| !l.is_empty());
    let status_line = lines.next().unwrap_or("").to_string();
    let status: u16 = match status_line.split_whitespace().nth(1).and_then(|c| c.parse().ok()) {
        Some(c) => c,
        None => return Some(HttpOut::Err(format!("http: unreadable status line '{}'", trim(&status_line, 120)))),
    };
    let rest_of_line = status_line.splitn(3, ' ').nth(2).unwrap_or("").trim().to_string();
    if (200..300).contains(&status) && status == 204 {
        return Some(HttpOut::Quiet); // a 204 carries no body (RFC 9110 6.4.2)
    }
    if !(200..300).contains(&status) {
        // No redirects (3.3): a non-2xx is the error of its status; the body
        // is not read.
        let why = if rest_of_line.is_empty() {
            format!("{status}")
        } else {
            format!("{status} {}", trim(&rest_of_line, 80))
        };
        return Some(HttpOut::Err(format!("http: the server answered {why}")));
    }
    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    for h in lines {
        let Some((k, v)) = h.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim().to_ascii_lowercase();
        if k == "content-length" {
            content_length = v.parse().ok();
        } else if k == "transfer-encoding" && v.contains("chunked") {
            chunked = true;
        } else if k == "content-encoding" && !v.is_empty() && v != "identity" {
            return Some(HttpOut::Err(format!(
                "http: the server's encoding '{v}' cannot be read by a plain client"
            )));
        }
    }
    let loaded = if chunked {
        read_chunked(&mut src, budget, cancel)
    } else {
        read_plain(&mut src, content_length, budget, cancel)
    };
    // `None` is the deadline or a cancel ending the effort with nothing read
    // (the engine's timeout owns it); `Err` is an outcome to push.
    let loaded = loaded?;
    let body = match loaded {
        Ok(None) => return Some(HttpOut::Quiet), // an empty 2xx body
        Ok(Some(body)) => body,
        Err(out) => return Some(out),
    };
    Some(shape_body(status, &body).unwrap_or(HttpOut::Quiet))
}

/// The response body's source: the socket, plus whatever the head reader
/// already read past the head. `fill` never blocks while bytes are buffered.
struct Src<'a> {
    sock: &'a mut TcpStream,
    carry: Vec<u8>,
}

impl<'a> Src<'a> {
    /// One read into `buf`: the buffered bytes first, else the socket (whose
    /// read timeout was set by the caller).
    fn fill(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.carry.is_empty() {
            let n = buf.len().min(self.carry.len());
            buf[..n].copy_from_slice(&self.carry[..n]);
            self.carry.drain(..n);
            return Ok(n);
        }
        self.sock.read(buf)
    }
}

/// A done body: `None` when empty (quiet); otherwise `Some(2xx: the answer)`.
fn shape_body(status: u16, body: &[u8]) -> Option<HttpOut> {
    let _ = status;
    if body.is_empty() {
        return None;
    }
    let text = match std::str::from_utf8(body) {
        Ok(t) => t,
        Err(_) => return Some(HttpOut::Err("http: the body is not UTF-8".into())),
    };
    match json::parse(text.trim()) {
        Ok(v) => Some(HttpOut::Answer(v)),
        Err(_) => Some(HttpOut::Err(format!(
            "http: the server's 2xx body is not one JSON value: {}",
            trim(text, 200)
        ))),
    }
}

/// The body under a `Content-Length`, or to EOF without one; `Err` is an
/// outcome to push, `Ok(None)` an empty body.
fn read_plain(src: &mut Src, content_length: Option<usize>, budget: &Budget, cancel: &Cancel) -> Option<Result<Option<Vec<u8>>, HttpOut>> {
    if let Some(w) = content_length {
        if w > BODY_CAP {
            return Some(Err(HttpOut::OverCap));
        }
    }
    let mut body: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        if content_length.map(|w| body.len() >= w).unwrap_or(false) {
            break;
        }
        let budget = budget.left()?;
        let _ = src.sock.set_read_timeout(Some(budget));
        let n = match src.fill(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                if cancelled(cancel) {
                    return None;
                }
                return Some(Err(HttpOut::Err(format!("http: the body could not be read: {e}"))));
            }
        };
        if n == 0 {
            // EOF: with Connection: close, that is the whole body; a body
            // that ends before its length never completes.
            break;
        }
        if body.len() + n > BODY_CAP {
            return Some(Err(HttpOut::OverCap));
        }
        body.extend_from_slice(&buf[..n]);
    }
    if content_length.map(|w| body.len() < w).unwrap_or(false) {
        return Some(Err(HttpOut::Err("http: the body ended before its length".into())));
    }
    Some(Ok((!body.is_empty()).then_some(body)))
}

/// A `Transfer-Encoding: chunked` body, de-chunked under the cap.
fn read_chunked(src: &mut Src, budget: &Budget, cancel: &Cancel) -> Option<Result<Option<Vec<u8>>, HttpOut>> {
    let mut body: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let mut carry: Vec<u8> = Vec::new();
    let mut read_bytes = |src: &mut Src, carry: &mut Vec<u8>| -> Option<Result<(), HttpOut>> {
        let budget = budget.left()?;
        let _ = src.sock.set_read_timeout(Some(budget));
        match src.fill(&mut buf) {
            Ok(0) => Some(Err(HttpOut::Err("http: the chunked body ended mid-chunk".into()))),
            Ok(n) => {
                carry.extend_from_slice(&buf[..n]);
                Some(Ok(()))
            }
            Err(e) => {
                if cancelled(cancel) {
                    None
                } else {
                    Some(Err(HttpOut::Err(format!("http: the chunked body could not be read: {e}"))))
                }
            }
        }
    };
    loop {
        // One chunk line at a time: a size line, the chunk, its \r\n.
        let line = loop {
            if let Some(hit) = carry.windows(2).position(|w| w == b"\r\n") {
                break carry.drain(..hit + 2).collect::<Vec<u8>>();
            }
            if carry.len() > CHUNK_LINE_CAP {
                return Some(Err(HttpOut::Err("http: a chunk's size line is over its cap".into())));
            }
            match read_bytes(src, &mut carry)? {
                Ok(()) => {}
                Err(e) => return Some(Err(e)),
            }
        };
        let line_text = String::from_utf8_lossy(&line);
        let size_part = line_text.trim().split(';').next().unwrap_or("").trim();
        let size = match usize::from_str_radix(size_part, 16) {
            Ok(n) => n,
            Err(_) => return Some(Err(HttpOut::Err(format!("http: unreadable chunk size '{}'", trim(size_part, 60))))),
        };
        if size == 0 {
            break; // the last chunk; trailers are not read
        }
        while carry.len() < size + 2 {
            match read_bytes(src, &mut carry)? {
                Ok(()) => {}
                Err(e) => return Some(Err(e)),
            }
        }
        if body.len() + size > BODY_CAP {
            return Some(Err(HttpOut::OverCap));
        }
        body.extend_from_slice(&carry[..size]);
        carry.drain(..size);
        while carry.starts_with(b"\r\n") || carry.starts_with(b"\r") {
            let take = if carry.starts_with(b"\r\n") { 2 } else { 1 };
            carry.drain(..take);
        }
    }
    Some(Ok((!body.is_empty()).then_some(body)))
}

fn trim(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.trim().to_string()
    } else {
        let head: String = s.chars().take(n).collect();
        let _ = head;
        format!("{}…", s.chars().take(n).collect::<String>().trim_end())
    }
}
