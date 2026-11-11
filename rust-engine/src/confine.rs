// Confining a program to its `may` (SPEC 2.4, decision 0018, S2a).
//
// `run --confine <file>` asks for it. Every `run` and `serve` program of the
// tree then starts under the platform's sandbox with a profile built from its
// neuron's `may`: the baseline every program gets, and nothing beyond it that
// the `may` does not name. A run the engine cannot hold to that is refused
// before any process starts — it fails closed.
//
// The mechanism is macOS's `sandbox-exec` and nothing else yet. Its limits are
// the ones the probes of 0018 found: files are held by path, and connections
// by port on `*` or `localhost` only. So a `connect` to a named host goes
// through the engine's own proxy (proxy.rs, S2b): the sandbox lets the program
// reach that one loopback port, and the proxy lets it out only to what its
// `may` lists.
use crate::json::{self, Value};
use crate::net::May;
use std::path::{Path, PathBuf};

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The `--confine` file: what belongs to the machine, not to the house rules.
/// `tools` are the trees this machine keeps its interpreters in; every
/// confined program may read them.
#[derive(Debug, Clone)]
pub struct Confine {
    tools: Vec<PathBuf>,
}

impl Confine {
    /// `{"version": 0, "tools": [absolute paths]}`, closed. Errors name the file.
    pub fn load(path: &str) -> Result<Confine, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("the confine file {}: cannot read ({})", path, e))?;
        let v = json::parse(&text).map_err(|e| format!("the confine file {}: not JSON ({})", path, e))?;
        let bad = |why: &str| format!("the confine file {}: {}", path, why);
        if !v.is_obj() {
            return Err(bad("not an object"));
        }
        for k in v.keys() {
            if !matches!(k, "version" | "tools") {
                return Err(bad(&format!("unknown field '{}'", k)));
            }
        }
        if v.get("version").and_then(|x| x.as_i64()) != Some(0) {
            return Err(bad("version must be 0"));
        }
        let mut tools = Vec::new();
        match v.get("tools") {
            None => {}
            Some(Value::Arr(items)) => {
                for item in items {
                    match item.as_str() {
                        Some(s) if Path::new(s).is_absolute() => tools.push(real(Path::new("/"), s)),
                        _ => return Err(bad("tools is an array of absolute paths")),
                    }
                }
            }
            Some(_) => return Err(bad("tools is an array of absolute paths")),
        }
        Ok(Confine { tools })
    }

    /// Can this engine confine a program on this machine at all?
    pub fn supported() -> Result<(), String> {
        if !cfg!(target_os = "macos") {
            return Err("this engine confines programs on macOS only (decision 0018)".into());
        }
        if !Path::new(SANDBOX_EXEC).is_file() {
            return Err(format!("{} is missing: this machine has no sandbox the engine knows", SANDBOX_EXEC));
        }
        Ok(())
    }

    /// The command that runs `cmd` confined to `may`, for the neuron `id`
    /// whose working directory is `cwd`. An entry of the `may` the sandbox
    /// cannot enforce is an error: the run is refused.
    ///
    /// A program that cannot be found is returned as it came: starting it
    /// fails exactly as it does unconfined (the `exit 127` of SPEC 6.1), and
    /// nothing runs.
    pub fn wrap(&self, id: &str, cmd: &[String], cwd: &Path, may: Option<&May>) -> Result<Vec<String>, String> {
        let Some(program) = cmd.first().and_then(|name| find(name, cwd)) else {
            return Ok(cmd.to_vec());
        };
        let (profile, proxy) = self.profile(id, cwd, may)?;
        let mut out = vec![SANDBOX_EXEC.to_string(), "-p".to_string(), profile];
        if let Some(port) = proxy {
            // The program is told where its proxy is, in the variables a
            // proxy-aware program reads (both cases: tools differ). One that
            // ignores them reaches no named host at all. The host's spawn
            // clears the environment, so they are set by `env`, which then
            // becomes the program.
            let url = format!("http://127.0.0.1:{}", port);
            out.push("/usr/bin/env".to_string());
            for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
                out.push(format!("{}={}", name, url));
            }
            out.push("NODE_USE_ENV_PROXY=1".to_string());
        }
        out.push(program.to_string_lossy().to_string());
        out.extend(cmd.iter().skip(1).cloned());
        Ok(out)
    }

    /// The sandbox profile of one neuron: the baseline of 0018 point 4, then
    /// what its `may` names. With a named host in `connect`, the neuron's
    /// proxy is started and its port returned beside the profile.
    fn profile(&self, id: &str, cwd: &Path, may: Option<&May>) -> Result<(String, Option<u16>), String> {
        // The baseline: the system's own base profile, the right to start
        // processes and to signal the ones it started (a program that cannot
        // stop its own child leaves it running), metadata of any path (never
        // contents), and the machine's tool trees. Not the files of the
        // working directory, not a temp directory, not HOME.
        let mut p = String::from(
            "(version 1)(deny default)(import \"system.sb\")(allow process-exec)(allow process-fork)(allow signal (target same-sandbox))(allow file-read-metadata)",
        );
        // The system's TLS configuration and trust roots: a client that
        // cannot read them does not start, and they are public.
        p.push_str("(allow file-read* (subpath \"/private/etc/ssl\"))");
        for tool in &self.tools {
            p.push_str(&format!("(allow file-read* (subpath {}))", quoted(tool)));
        }
        // The working directory itself, as a directory and no deeper: a
        // program must be able to learn where it is (an interpreter that
        // cannot ask for its working directory does not start), and that same
        // right shows it the names in the directory. Their contents stay shut.
        p.push_str(&format!("(allow file-read-data (literal {}))", quoted(&real(cwd, "."))));
        let none = May::default();
        let may = may.unwrap_or(&none);
        // A path names a file or a whole tree (0018 point 5), resolved through
        // symbolic links now: the sandbox matches real paths.
        for entry in &may.read {
            p.push_str(&format!("(allow file-read* (subpath {}))", quoted(&real(cwd, entry))));
        }
        for entry in &may.write {
            p.push_str(&format!("(allow file-write* (subpath {}))", quoted(&real(cwd, entry))));
        }
        // A loopback entry is held by the sandbox itself; every other host
        // is the proxy's to hold (0018 point 6).
        let mut named: Vec<String> = Vec::new();
        for entry in &may.connect {
            let (host, port) = entry.rsplit_once(':').unwrap_or((entry, ""));
            if matches!(host, "localhost" | "127.0.0.1") {
                p.push_str(&format!("(allow network-outbound (remote tcp \"localhost:{}\"))", port));
            } else {
                named.push(entry.clone());
            }
        }
        for entry in &may.listen {
            let (host, port) = entry.rsplit_once(':').unwrap_or((entry, ""));
            let local = match host {
                "localhost" | "127.0.0.1" => "localhost",
                "0.0.0.0" => "*",
                _ => {
                    return Err(format!(
                        "'{}' may listen '{}': a confined run holds a listener to localhost, 127.0.0.1 or 0.0.0.0 only (decision 0018)",
                        id, entry
                    ));
                }
            };
            p.push_str(&format!(
                "(allow network-bind (local tcp \"{l}:{port}\"))(allow network-inbound (local tcp \"{l}:{port}\"))",
                l = local,
                port = port
            ));
        }
        // The proxy is started last, once nothing else can refuse the plant.
        let proxy = if named.is_empty() {
            None
        } else {
            let port = crate::proxy::start(id, named)?;
            p.push_str(&format!("(allow network-outbound (remote tcp \"localhost:{}\"))", port));
            Some(port)
        };
        Ok((p, proxy))
    }
}

/// A path as the sandbox will see it: joined to `base` when relative, then
/// its longest existing part resolved through symbolic links and the rest
/// kept as written, so a program may create the file its `may` names.
fn real(base: &Path, entry: &str) -> PathBuf {
    let joined = if Path::new(entry).is_absolute() {
        PathBuf::from(entry)
    } else {
        base.join(entry)
    };
    let mut existing = joined.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(resolved) = std::fs::canonicalize(&existing) {
            let mut out = resolved;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent().map(Path::to_path_buf),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return joined,
        }
    }
}

/// A path as a profile string.
fn quoted(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", s)
}

/// Where the program is, as the engine would find it (SPEC 2.3): a name with
/// `/` relative to the working directory, a bare name on `PATH`.
fn find(name: &str, cwd: &Path) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.contains('/') {
        let p = if Path::new(name).is_absolute() {
            PathBuf::from(name)
        } else {
            cwd.join(name)
        };
        return p.is_file().then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}
