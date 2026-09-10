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

/// Split one HTTP request head into `(method, path)` from its request line.
/// Pure: no I/O, safe to unit-test offline.
#[must_use]
pub fn parse_request_line(head: &str) -> (String, String) {
    let request_line = head.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    (method, path)
}

/// True when the request head asks for a websocket upgrade (case-insensitive).
/// Pure: no I/O, safe to unit-test offline.
#[must_use]
pub fn is_websocket_upgrade(head: &str) -> bool {
    head.to_lowercase().contains("upgrade: websocket")
}

/// Build one minimal HTTP/1.1 response (head + body) as bytes.
/// Pure: no I/O, safe to unit-test offline.
#[must_use]
pub fn build_response(status: &str, ctype: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    [head.as_bytes(), body].concat()
}

/// Current dashboard state: every task plus recent event history and SODP metadata (ADR-004).
#[must_use]
pub fn snapshot_json(store: &Store) -> String {
    let tasks = store.list_tasks(None).unwrap_or_default();
    let events = store.recent_events(HISTORY_LIMIT).unwrap_or_default();
    json!({
        "sodp_protocol": "4.0",
        "schema_version": crate::store::SCHEMA_VERSION,
        "tasks": tasks,
        "events": events
    }).to_string()
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
    let bytes = build_response(status, ctype, body);
    let _ = stream.write_all(&bytes);
    let _ = stream.flush();
}

/// Route one connection: page, websocket upgrade, or error status.
fn handle_connection(mut stream: TcpStream, db: &Path) {
    let Some(raw) = read_head(&mut stream) else {
        return;
    };
    let head = String::from_utf8_lossy(&raw);
    let (method, path) = parse_request_line(&head);
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
        if is_websocket_upgrade(&head) {
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

    // ---- helpers de test: sockets loopback efimeros, sin red externa ----

    fn temp_db(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "atlas_serve_{tag}_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// Par conectado (cliente, servidor) sobre 127.0.0.1 con puerto efimero.
    fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = TcpStream::connect(addr).expect("connect");
        let (server, _) = listener.accept().expect("accept");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        (client, server)
    }

    /// Atiende UNA conexion con `handle_connection` en un hilo.
    fn serve_once(listener: TcpListener, db: PathBuf) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            handle_connection(stream, &db);
        })
    }

    fn serve_once_addr(db: &Path) -> (TcpListener, SocketAddr, PathBuf) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        (listener, addr, db.to_path_buf())
    }

    /// Lee hasta EOF (el servidor cierra con `connection: close`).
    fn read_all(stream: &mut TcpStream) -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        out
    }

    fn split_head_body(resp: &[u8]) -> (&str, &[u8]) {
        let pos = resp
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("head terminator");
        (
            std::str::from_utf8(&resp[..pos]).expect("head utf8"),
            &resp[pos + 4..],
        )
    }

    // ---- funciones puras ----

    #[test]
    fn request_line_parses_method_and_path() {
        assert_eq!(
            parse_request_line("GET / HTTP/1.1\r\nHost: x\r\n\r\n"),
            ("GET".to_string(), "/".to_string())
        );
        assert_eq!(
            parse_request_line("GET /ws HTTP/1.1\r\n\r\n"),
            ("GET".to_string(), "/ws".to_string())
        );
        assert_eq!(
            parse_request_line("POST / HTTP/1.1\r\n\r\n"),
            ("POST".to_string(), "/".to_string())
        );
        assert_eq!(parse_request_line(""), (String::new(), String::new()));
        assert_eq!(
            parse_request_line("GET\r\n\r\n"),
            ("GET".to_string(), String::new())
        );
        // Espaciado extra no rompe el parseo.
        assert_eq!(
            parse_request_line("  GET   /a  HTTP/1.1\r\n\r\n"),
            ("GET".to_string(), "/a".to_string())
        );
    }

    #[test]
    fn upgrade_detect_is_case_insensitive() {
        assert!(is_websocket_upgrade(
            "GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n"
        ));
        assert!(is_websocket_upgrade(
            "GET /ws HTTP/1.1\r\nUPGRADE: WEBSOCKET\r\n\r\n"
        ));
        assert!(!is_websocket_upgrade("GET /ws HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert!(!is_websocket_upgrade(""));
    }

    #[test]
    fn response_bytes_carry_status_and_length() {
        let body = b"hello";
        let raw = build_response("200 OK", "text/plain", body);
        let (head, rest) = split_head_body(&raw);
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("content-type: text/plain\r\n"));
        assert!(head.contains("content-length: 5\r\n"));
        assert!(head.ends_with("connection: close"));
        assert_eq!(rest, body);
        // Cuerpo vacio: content-length 0 y sin bytes extra.
        let raw = build_response("404 Not Found", "text/plain", b"");
        let (head, rest) = split_head_body(&raw);
        assert!(head.contains("content-length: 0\r\n"));
        assert!(rest.is_empty());
    }

    #[test]
    fn loopback_hosts_cover_names_brackets_and_case() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(is_loopback_host("  localhost  "));
        assert!(is_loopback_host("[::1]"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(!is_loopback_host(""));
        assert!(!is_loopback_host("example.com"));
        assert!(is_loopback_host("[127.0.0.1]"));
    }

    #[test]
    fn bind_maps_localhost_and_rejects_garbage() {
        let a = resolve_bind("localhost", 8080, false).expect("localhost");
        assert_eq!(a.ip().to_string(), "127.0.0.1");
        assert_eq!(a.port(), 8080);
        let a = resolve_bind("[::1]", 1, false).expect("bracketed v6");
        assert_eq!(a.ip().to_string(), "::1");
        assert!(resolve_bind("not a host!!", 8080, true).is_err());
        assert!(resolve_bind("example.com", 8080, true).is_err());
        // Con --public el loopback sigue valiendo.
        assert!(resolve_bind("127.0.0.1", 8080, true).is_ok());
    }

    #[test]
    fn snapshot_has_sodp_envelope_and_empty_lists() {
        let store = Store::open_in_memory().expect("open");
        let snap: serde_json::Value =
            serde_json::from_str(&snapshot_json(&store)).expect("JSON");
        assert_eq!(
            snap.get("sodp_protocol").expect("sodp").as_str(),
            Some("4.0")
        );
        assert!(snap.get("schema_version").is_some());
        assert_eq!(
            snap.get("tasks").expect("tasks").as_array().expect("arr").len(),
            0
        );
        assert_eq!(
            snap.get("events").expect("events").as_array().expect("arr").len(),
            0
        );
    }

    #[test]
    fn snapshot_history_is_capped() {
        let store = Store::open_in_memory().expect("open");
        for i in 0..60 {
            store
                .record_event("probe", &format!("{{\"i\":{i}}}"))
                .expect("event");
        }
        let snap: serde_json::Value =
            serde_json::from_str(&snapshot_json(&store)).expect("JSON");
        let events = snap
            .get("events")
            .expect("events")
            .as_array()
            .expect("array");
        assert_eq!(events.len(), HISTORY_LIMIT as usize);
    }

    // ---- read_head sobre socket real ----

    #[test]
    fn read_head_returns_bytes_up_to_terminator() {
        let (mut client, mut server) = tcp_pair();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\nEXTRA")
            .expect("write");
        let head = read_head(&mut server).expect("some");
        let text = String::from_utf8_lossy(&head);
        assert!(text.contains("GET / HTTP/1.1"));
    }

    #[test]
    fn read_head_empty_when_peer_closes() {
        let (client, mut server) = tcp_pair();
        drop(client);
        let head = read_head(&mut server).expect("some");
        assert!(head.is_empty());
    }

    #[test]
    fn read_head_rejects_oversized_head() {
        let (mut client, mut server) = tcp_pair();
        client
            .write_all(&vec![b'A'; 20 * 1024])
            .expect("write");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown");
        assert!(read_head(&mut server).is_none());
    }

    // ---- handle_connection de punta a punta ----

    #[test]
    fn serves_dashboard_page() {
        let db = temp_db("page");
        let (listener, addr, db) = serve_once_addr(&db);
        let h = serve_once(listener, db);
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nconnection: close\r\n\r\n")
            .expect("write");
        let resp = read_all(&mut client);
        h.join().expect("server thread");
        let (head, body) = split_head_body(&resp);
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
        assert!(head.contains("text/html"), "{head}");
        assert_eq!(body, DASHBOARD_HTML.as_bytes());
    }

    #[test]
    fn rejects_non_get_method() {
        let db = temp_db("m405");
        let (listener, addr, db) = serve_once_addr(&db);
        let h = serve_once(listener, db);
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        client
            .write_all(b"POST / HTTP/1.1\r\nHost: x\r\ncontent-length: 0\r\n\r\n")
            .expect("write");
        let resp = read_all(&mut client);
        h.join().expect("server thread");
        let (head, body) = split_head_body(&resp);
        assert!(head.starts_with("HTTP/1.1 405"), "{head}");
        assert_eq!(body, b"method not allowed");
    }

    #[test]
    fn unknown_path_is_404() {
        let db = temp_db("m404");
        let (listener, addr, db) = serve_once_addr(&db);
        let h = serve_once(listener, db);
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        client
            .write_all(b"GET /nope HTTP/1.1\r\nHost: x\r\n\r\n")
            .expect("write");
        let resp = read_all(&mut client);
        h.join().expect("server thread");
        let (head, body) = split_head_body(&resp);
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        assert_eq!(body, b"not found");
    }

    #[test]
    fn ws_without_upgrade_is_426() {
        let db = temp_db("m426");
        let (listener, addr, db) = serve_once_addr(&db);
        let h = serve_once(listener, db);
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        client
            .write_all(b"GET /ws HTTP/1.1\r\nHost: x\r\n\r\n")
            .expect("write");
        let resp = read_all(&mut client);
        h.join().expect("server thread");
        let (head, body) = split_head_body(&resp);
        assert!(head.starts_with("HTTP/1.1 426"), "{head}");
        assert_eq!(body, b"websocket upgrade required");
    }

    #[test]
    fn oversized_head_gets_no_response() {
        let db = temp_db("big");
        let (listener, addr, db) = serve_once_addr(&db);
        let h = serve_once(listener, db);
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        client.write_all(&vec![b'B'; 20 * 1024]).expect("write");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown");
        let resp = read_all(&mut client);
        h.join().expect("server thread");
        assert!(resp.is_empty());
    }

    // ---- PrefixStream ----

    #[test]
    fn prefix_stream_replays_then_delegates() {
        let (mut client, server) = tcp_pair();
        let mut ps = PrefixStream {
            prefix: vec![1u8, 2, 3],
            pos: 0,
            inner: server,
        };
        let mut first = [0u8; 2];
        ps.read_exact(&mut first).expect("prefix read");
        assert_eq!(first, [1, 2]);
        // El resto del prefijo llega antes que el socket.
        client.write_all(&[9u8, 8]).expect("write");
        let mut rest = [0u8; 3];
        ps.read_exact(&mut rest).expect("mixed read");
        assert_eq!(rest, [3, 9, 8]);
    }

    #[test]
    fn prefix_stream_write_goes_to_socket() {
        let (mut client, server) = tcp_pair();
        let mut ps = PrefixStream {
            prefix: Vec::new(),
            pos: 0,
            inner: server,
        };
        ps.write_all(b"ping").expect("write");
        ps.flush().expect("flush");
        let mut buf = [0u8; 4];
        client.read_exact(&mut buf).expect("read");
        assert_eq!(&buf, b"ping");
    }

    // ---- websocket ----

    /// Lee UNA trama de texto servidor->cliente (sin mascara) y la parsea.
    fn read_ws_text_frame(stream: &mut TcpStream) -> serde_json::Value {
        let mut hdr = [0u8; 2];
        stream.read_exact(&mut hdr).expect("ws header");
        assert_eq!(hdr[0], 0x81, "FIN + opcode texto");
        let mut len = (hdr[1] & 0x7F) as u64;
        if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext).expect("ws ext16");
            len = u16::from_be_bytes(ext) as u64;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            stream.read_exact(&mut ext).expect("ws ext64");
            len = u64::from_be_bytes(ext);
        }
        let mut payload = vec![0u8; len as usize];
        stream.read_exact(&mut payload).expect("ws payload");
        serde_json::from_slice(&payload).expect("payload JSON")
    }

    #[test]
    fn websocket_pushes_json_snapshot() {
        // DB en disco con una tarea para que el snapshot traiga contenido.
        let db = temp_db("wsok");
        {
            let store = Store::open(&db).expect("create db");
            let s = store.create_session("ws snap").expect("session");
            store.create_task(&s, "ws task", None, &[]).expect("task");
        }
        let (listener, addr, db) = serve_once_addr(&db);
        let h = serve_once(listener, db);
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        client
            .write_all(
                b"GET /ws HTTP/1.1\r\n\
                  Host: x\r\n\
                  Upgrade: websocket\r\n\
                  Connection: Upgrade\r\n\
                  Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                  Sec-WebSocket-Version: 13\r\n\r\n",
            )
            .expect("handshake");
        // Respuesta 101 al handshake (byte a byte: cubre varias iteraciones).
        let mut raw = Vec::new();
        loop {
            let mut one = [0u8; 1];
            client.read_exact(&mut one).expect("read 101");
            raw.push(one[0]);
            if raw.len() >= 4 && raw[raw.len() - 4..] == *b"\r\n\r\n" {
                break;
            }
        }
        let head = String::from_utf8_lossy(&raw);
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        // Primera trama: snapshot JSON con la tarea sembrada.
        let snap = read_ws_text_frame(&mut client);
        let tasks = snap.get("tasks").expect("tasks").as_array().expect("arr");
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].get("title").expect("title").as_str(),
            Some("ws task")
        );
        assert!(snap.get("events").is_some());
        assert_eq!(
            snap.get("sodp_protocol").expect("sodp").as_str(),
            Some("4.0")
        );
        // Al soltar el cliente, el loop del servidor falla el send y sale.
        drop(client);
        h.join().expect("server thread");
        let _ = std::fs::remove_file(temp_db("wsok"));
    }

    #[test]
    fn websocket_rejects_garbage_handshake() {
        let (client, server) = tcp_pair();
        let db = temp_db("wsbad");
        // Prefijo que no es un handshake: `accept` falla y retorna.
        serve_websocket(b"GARBAGE\r\n\r\n".to_vec(), server, &db);
        drop(client);
    }

    // ---- run_server: solo ramas de error (el loop exitoso no retorna) ----

    #[test]
    fn run_server_refuses_non_loopback() {
        let cfg = ServeConfig {
            host: "0.0.0.0".into(),
            port: 8080,
            db: temp_db("rs1"),
            public: false,
        };
        assert!(run_server(&cfg).is_err());
    }

    #[test]
    fn run_server_rejects_invalid_bind() {
        let cfg = ServeConfig {
            host: "not a host!!".into(),
            port: 8080,
            db: temp_db("rs2"),
            public: true,
        };
        assert!(run_server(&cfg).is_err());
    }

    #[test]
    fn run_server_reports_port_conflict() {
        let holder = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = holder.local_addr().expect("addr").port();
        let cfg = ServeConfig {
            host: "127.0.0.1".into(),
            port,
            db: temp_db("rs3"),
            public: false,
        };
        assert!(run_server(&cfg).is_err());
    }

    #[test]
    fn read_head_none_on_read_error() {
        let (_client, server) = tcp_pair();
        let mut server = server;
        server
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("timeout");
        // Sin bytes del peer el read expira -> `Err` -> `None`.
        assert!(read_head(&mut server).is_none());
    }

    #[test]
    fn ws_frame_reader_handles_64bit_length() {
        let (mut client, server) = tcp_pair();
        let payload = format!("\"{}\"", "a".repeat(70_000));
        let h = thread::spawn(move || {
            let mut server = server;
            let len = payload.len() as u64;
            let mut frame = vec![0x81u8, 127];
            frame.extend_from_slice(&len.to_be_bytes());
            frame.extend_from_slice(payload.as_bytes());
            server.write_all(&frame).expect("write frame");
        });
        let v = read_ws_text_frame(&mut client);
        h.join().expect("server thread");
        assert_eq!(v.as_str().expect("str").len(), 70_000);
    }

    #[test]
    fn websocket_closes_when_db_wont_open() {
        // Directorio padre inexistente: `Store::open` falla tras el accept.
        let mut bad = std::env::temp_dir();
        bad.push(format!("atlas_serve_nope_{}", std::process::id()));
        bad.push("db.sqlite");
        let (listener, addr, _) = serve_once_addr(&bad);
        let db = bad.clone();
        let h = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            handle_connection(stream, &db);
        });
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        client
            .write_all(
                b"GET /ws HTTP/1.1\r\n\
                  Host: x\r\n\
                  Upgrade: websocket\r\n\
                  Connection: Upgrade\r\n\
                  Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                  Sec-WebSocket-Version: 13\r\n\r\n",
            )
            .expect("handshake");
        let mut raw = Vec::new();
        loop {
            let mut one = [0u8; 1];
            client.read_exact(&mut one).expect("read 101");
            raw.push(one[0]);
            if raw.len() >= 4 && raw[raw.len() - 4..] == *b"\r\n\r\n" {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 101"));
        // El servidor cierra al no poder abrir la DB: trama close (0x88) o EOF.
        let mut hdr = [0u8; 2];
        match client.read_exact(&mut hdr) {
            Ok(()) => assert_eq!(hdr[0], 0x88, "close frame"),
            Err(_) => {}
        }
        h.join().expect("server thread");
    }

    #[test]
    fn serve_consts_are_sane() {
        assert_eq!(TICK_SECS, 1);
        assert_eq!(HISTORY_LIMIT, 50);
        assert!(!PAGE_MARKERS.is_empty());
    }
}
