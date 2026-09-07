// `atlas serve`: local dev dashboard (REQ-F-016).
// One `TcpListener` serves the embedded single-file HTML page on `GET /`
// and pushes JSON task snapshots over a websocket on `/ws` (1s tick,
// tungstenite sync server). Binds loopback only unless `--public`.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::json;
use tungstenite::Message;

use crate::Store;

/// Embedded single-file dashboard (zero external assets).
pub const DASHBOARD_HTML: &str = include_str!("dashboard.html");

/// Marker strings the served page must contain (covered by tests).
pub const PAGE_MARKERS: &[&str] = &[
    "atlas-tasks",
    "atlas-history",
    "atlas-status",
    "/ws",
    "WebSocket",
];

/// Seconds between websocket snapshot pushes.
pub const TICK_SECS: u64 = 1;

/// Max events included in each snapshot / history panel.
pub const HISTORY_LIMIT: i64 = 50;

/// Max bytes of HTTP request head accepted per connection.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// Write timeout per snapshot push; a stuck client is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Config for one `atlas serve` run.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub host: String,
    pub port: u16,
    pub db: PathBuf,
    pub public: bool,
}

/// True for loopback-only hosts (dev default, no flag needed).
#[must_use]
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_matches(['[', ']']);
    h.eq_ignore_ascii_case("localhost")
        || h == "::1"
        || h.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Resolve the bind address, refusing non-loopback binds without
/// `--public` (dev-only default per REQ-F-016).
pub fn resolve_bind(host: &str, port: u16, public: bool) -> crate::Result<SocketAddr> {
    if !public && !is_loopback_host(host) {
        return Err(crate::AtlasError::InvalidState(format!(
            "refusing non-loopback bind '{host}' without --public"
        )));
    }
    // `SocketAddr::from_str` rejects hostnames, so map loopback names
    // to IPs first; anything else must already be a literal IP.
    let h = host.trim().trim_matches(['[', ']']);
    let ip: std::net::IpAddr = if h.eq_ignore_ascii_case("localhost") {
        "127.0.0.1".parse().expect("literal")
    } else {
        h.parse()
            .map_err(|_| crate::AtlasError::InvalidState(format!("invalid bind '{host}:{port}'")))?
    };
    Ok(SocketAddr::new(ip, port))
}

/// Current dashboard state: every task plus recent event history.
#[must_use]
pub fn snapshot_json(store: &Store) -> String {
    let tasks = store.list_tasks(None).unwrap_or_default();
    let events = store.recent_events(HISTORY_LIMIT).unwrap_or_default();
    json!({ "tasks": tasks, "events": events }).to_string()
}

/// Serve forever on one listener; returns only on accept failure.
pub fn run_server(cfg: &ServeConfig) -> crate::Result<()> {
    let addr = resolve_bind(&cfg.host, cfg.port, cfg.public)?;
    let listener = TcpListener::bind(addr)?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let db = cfg.db.clone();
                thread::spawn(move || handle_connection(stream, &db));
            }
            Err(_) => break,
        }
    }
    Ok(())
}

/// Read one raw HTTP request head (up to `\r\n\r\n`, capped).
fn read_head(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > MAX_HEAD_BYTES {
                    return None;
                }
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return None,
        }
    }
    Some(buf)
}

/// Minimal HTTP response writer (no external HTTP crate needed).
fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// Route one connection: page, websocket upgrade, or error status.
fn handle_connection(mut stream: TcpStream, db: &Path) {
    let Some(raw) = read_head(&mut stream) else {
        return;
    };
    let head = String::from_utf8_lossy(&raw);
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if method != "GET" {
        respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed",
        );
        return;
    }
    if path == "/" {
        respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            DASHBOARD_HTML.as_bytes(),
        );
        return;
    }
    if path == "/ws" {
        if head.to_lowercase().contains("upgrade: websocket") {
            serve_websocket(raw, stream, db);
        } else {
            respond(
                &mut stream,
                "426 Upgrade Required",
                "text/plain",
                b"websocket upgrade required",
            );
        }
        return;
    }
    respond(&mut stream, "404 Not Found", "text/plain", b"not found");
}

/// Stream replaying already-read handshake bytes, then the socket.
/// Lets tungstenite run its own server handshake on a connection that
/// was already peeked at for routing.
struct PrefixStream {
    prefix: Vec<u8>,
    pos: usize,
    inner: TcpStream,
}

impl Read for PrefixStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos < self.prefix.len() {
            let n = (self.prefix.len() - self.pos).min(out.len());
            out[..n].copy_from_slice(&self.prefix[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        self.inner.read(out)
    }
}

impl Write for PrefixStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Run the tungstenite server handshake, then push a fresh DB snapshot
/// each tick until the client disconnects.
fn serve_websocket(raw_head: Vec<u8>, stream: TcpStream, db: &Path) {
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let prefixed = PrefixStream {
        prefix: raw_head,
        pos: 0,
        inner: stream,
    };
    let mut ws = match tungstenite::accept(prefixed) {
        Ok(ws) => ws,
        Err(_) => return,
    };
    let Ok(store) = Store::open(db) else {
        let _ = ws.close(None);
        return;
    };
    loop {
        let msg = Message::Text(snapshot_json(&store).into());
        if ws.send(msg).is_err() {
            break;
        }
        thread::sleep(Duration::from_secs(TICK_SECS));
    }
    let _ = ws.close(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_contains_expected_markers() {
        assert!(DASHBOARD_HTML.contains("<!DOCTYPE html>"));
        for m in PAGE_MARKERS {
            assert!(DASHBOARD_HTML.contains(m), "missing marker {m}");
        }
    }

    #[test]
    fn ws_snapshot_parses_as_json_task_list() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("serve snap").expect("session");
        store.create_task(&s, "demo task", None, &[]).expect("task");
        store
            .record_event("task_created", "{\"id\":\"x\"}")
            .expect("event");
        let snap: serde_json::Value =
            serde_json::from_str(&snapshot_json(&store)).expect("snapshot is JSON");
        let tasks = snap
            .get("tasks")
            .expect("tasks key")
            .as_array()
            .expect("tasks array");
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].get("title").expect("title").as_str(),
            Some("demo task")
        );
        let events = snap
            .get("events")
            .expect("events key")
            .as_array()
            .expect("events array");
        // Newest first: our explicit record sorts ahead of the
        // session/task rows emitted by the setup calls above.
        assert!(!events.is_empty());
        assert_eq!(
            events[0].get("kind").expect("kind").as_str(),
            Some("task_created")
        );
    }

    #[test]
    fn public_bind_refused_without_flag() {
        assert!(resolve_bind("127.0.0.1", 8080, false).is_ok());
        assert!(resolve_bind("localhost", 8080, false).is_ok());
        assert!(resolve_bind("::1", 8080, false).is_ok());
        assert!(is_loopback_host("127.0.0.5"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        let err = resolve_bind("0.0.0.0", 8080, false).expect_err("must refuse");
        assert!(err.to_string().contains("--public"));
        assert!(resolve_bind("0.0.0.0", 8080, true).is_ok());
    }
}
