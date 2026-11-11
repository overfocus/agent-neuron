// The engine's proxy for a confined neuron's named hosts (decision 0018, S2b).
//
// The sandbox holds a connection by port, never by host, so a neuron whose
// `may.connect` names a host is given one loopback port to reach — this
// proxy's — and the proxy opens a way out only to a `host:port` the neuron
// listed. One listener per neuron: the list a port enforces is that neuron's
// and nobody else's, and no other neuron's sandbox can reach the port.
//
// It speaks the two forms a proxy-aware program uses: `CONNECT host:port`
// (a tunnel, which is how TLS passes through untouched — the engine reads
// none of it) and a request with an absolute `http://` target, which is sent
// on as an ordinary request for the path, one request per connection. Anything else, and anything not listed, is refused. Standard library
// only.
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

const HEAD_CAP: usize = 16 * 1024;
const PATIENCE: Duration = Duration::from_secs(10);

/// Start the proxy of one neuron: a loopback listener that lives as long as
/// the engine, enforcing `allow` (entries `host:port`, as the `may` wrote
/// them). Returns its port.
pub fn start(neuron: &str, allow: Vec<String>) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("the proxy of '{}' cannot listen ({})", neuron, e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("the proxy of '{}' has no address ({})", neuron, e))?
        .port();
    let neuron = neuron.to_string();
    std::thread::Builder::new()
        .name(format!("proxy-{}", neuron))
        .spawn(move || {
            for client in listener.incoming().flatten() {
                let (neuron, allow) = (neuron.clone(), allow.clone());
                let _ = std::thread::Builder::new()
                    .name("proxy-conn".into())
                    .spawn(move || serve(client, &neuron, &allow));
            }
        })
        .map_err(|e| format!("a proxy cannot start ({})", e))?;
    Ok(port)
}

/// One connection from the confined program.
fn serve(mut client: TcpStream, neuron: &str, allow: &[String]) {
    let _ = client.set_read_timeout(Some(PATIENCE));
    let Some(head) = read_head(&mut client) else {
        return refuse(&mut client, "400 Bad Request");
    };
    let line = String::from_utf8_lossy(&head).lines().next().unwrap_or("").to_string();
    let mut parts = line.split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let tunnel = method == "CONNECT";
    let Some(wanted) = (if tunnel {
        authority(target, None)
    } else {
        target
            .strip_prefix("http://")
            .and_then(|rest| authority(rest.split('/').next().unwrap_or(""), Some(80)))
    }) else {
        return refuse(&mut client, "400 Bad Request");
    };
    if !allow.contains(&wanted) {
        // The run's own stderr, never the trace: a refusal here is the
        // program's failure to report (0018 point 7), and this line is for
        // whoever is working out why.
        let _ = writeln!(
            std::io::stderr(),
            "confine: '{}' was refused a connection to {}: its may does not list it",
            neuron,
            wanted
        );
        return refuse(&mut client, "403 Forbidden");
    }
    let Some(mut upstream) = dial(&wanted) else {
        return refuse(&mut client, "502 Bad Gateway");
    };
    let started = if tunnel {
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
    } else {
        upstream.write_all(&origin_form(&head))
    };
    if started.is_err() {
        return;
    }
    let _ = client.set_read_timeout(None);
    let (Ok(mut client_in), Ok(mut upstream_in)) = (client.try_clone(), upstream.try_clone()) else {
        return;
    };
    // Bytes both ways until either side is done; then both ends are closed.
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_in, &mut upstream);
        let _ = upstream.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut upstream_in, &mut client);
    let _ = client.shutdown(Shutdown::Both);
    let _ = up.join();
}

/// The request's head, through its blank line (and whatever came with it).
fn read_head(client: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        if head.len() > HEAD_CAP {
            return None;
        }
        match client.read(&mut buf) {
            Ok(n) if n > 0 => head.extend_from_slice(&buf[..n]),
            _ => return None,
        }
    }
    Some(head)
}

/// `host:port` in the form a `may` writes it: lower case, the port present
/// (`default` when the target left it out), nothing but a host before it.
fn authority(target: &str, default: Option<u16>) -> Option<String> {
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (target, default?),
    };
    let ok = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    (ok && port != 0).then(|| format!("{}:{}", host.to_ascii_lowercase(), port))
}

/// A request that named an absolute `http://host/path` target, as the server
/// itself expects it: the path alone on the request line (not every server
/// takes the absolute form), and `Connection: close`, so the program opens a
/// new connection — and is checked again — for its next request.
fn origin_form(head: &[u8]) -> Vec<u8> {
    let end = head.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(head.len());
    let text = String::from_utf8_lossy(&head[..end]).to_string();
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.splitn(3, ' ');
    let (method, target, version) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
        parts.next().unwrap_or("HTTP/1.1"),
    );
    let after = target.strip_prefix("http://").unwrap_or(target);
    let path = after.find('/').map(|i| &after[i..]).unwrap_or("/");
    let mut out = format!("{} {} {}\r\n", method, path, version);
    for line in lines.filter(|l| !l.is_empty()) {
        let name = line.split(':').next().unwrap_or("").to_ascii_lowercase();
        if name != "connection" && name != "proxy-connection" {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out.push_str("Connection: close\r\n\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(&head[end..]);
    bytes
}

fn dial(wanted: &str) -> Option<TcpStream> {
    wanted
        .to_socket_addrs()
        .ok()?
        .find_map(|addr| TcpStream::connect_timeout(&addr, PATIENCE).ok())
}

fn refuse(client: &mut TcpStream, status: &str) {
    let _ = client.write_all(format!("HTTP/1.1 {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", status).as_bytes());
    let _ = client.shutdown(Shutdown::Both);
}
