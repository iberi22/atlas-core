// Xavier adapter (REQ-F-019, ATLAS-09): optional CodeGraph + RAG memory.
//
// Degraded-first: every public path works with Xavier unreachable.
// All network I/O lives here and uses only std + serde_json.
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Default base URL when `ATLAS_XAVIER_URL` is unset.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8006";
/// Capabilities the adapter can use once Xavier answers a health ping.
pub const CAPABILITIES: &[&str] = &[
    "memories.search",
    "memories.add",
    "context.assemble",
    "memories.graph",
];

/// Runtime configuration: env overrides, sane offline defaults.
#[derive(Debug, Clone)]
pub struct XavierConfig {
    pub base_url: String,
    pub token: String,
    pub timeout_ms: u64,
}

impl Default for XavierConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl XavierConfig {
    /// `ATLAS_XAVIER_URL` overrides the base URL (tests point it at a stub);
    /// `XAVIER_TOKEN` carries auth; `ATLAS_XAVIER_TIMEOUT_MS` bounds every
    /// call so the CLI never hangs on a sick server (default 2500ms).
    pub fn from_env() -> Self {
        let base_url =
            std::env::var("ATLAS_XAVIER_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
        let token = std::env::var("XAVIER_TOKEN").unwrap_or_default();
        let timeout_ms = std::env::var("ATLAS_XAVIER_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2500);
        Self {
            base_url,
            token,
            timeout_ms,
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.max(100))
    }
}

/// One memory hit, parsed tolerantly from any Xavier search shape
/// (`full` items carry no score; `snippet`/`ids` items do).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryHit {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub kind: String,
}

/// Liveness report for `atlas xavier status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XavierStatus {
    pub reachable: bool,
    pub base_url: String,
    pub capabilities: Vec<String>,
    pub degraded: bool,
    pub detail: String,
}

/// One proposed backlog item from `atlas xavier draft`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftTask {
    pub title: String,
    /// Indices into the same draft list (chain order), empty = no deps.
    pub depends_on: Vec<usize>,
}

/// Failure modes: unreachable (degraded path) vs. bad payloads.
#[derive(Debug, thiserror::Error)]
pub enum XavierError {
    #[error("xavier unreachable at {0}: {1}")]
    Unreachable(String, String),
    #[error("bad xavier response: {0}")]
    BadResponse(String),
}

/// Network boundary. `HttpBackend` does real I/O; `StubBackend` is for
/// tests, demos, and offline use.
pub trait XavierBackend {
    fn check_status(&self) -> XavierStatus;
    fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>, XavierError>;
}

/// Real backend over blocking std TCP with a hard timeout per call.
pub struct HttpBackend {
    pub config: XavierConfig,
}

impl HttpBackend {
    pub fn new(config: XavierConfig) -> Self {
        Self { config }
    }

    fn get(&self, path: &str) -> Result<(u16, String), XavierError> {
        http_request(&self.config, "GET", path, None)
    }

    fn post(&self, path: &str, body: &str) -> Result<(u16, String), XavierError> {
        http_request(&self.config, "POST", path, Some(body))
    }
}

impl XavierBackend for HttpBackend {
    fn check_status(&self) -> XavierStatus {
        match self.get("/health") {
            Ok((code, _)) if (200..300).contains(&code) => XavierStatus {
                reachable: true,
                base_url: self.config.base_url.clone(),
                capabilities: CAPABILITIES.iter().map(|s| s.to_string()).collect(),
                degraded: false,
                detail: "health ping ok".to_owned(),
            },
            Ok((code, _)) => degraded(&self.config, format!("health returned HTTP {code}")),
            Err(XavierError::Unreachable(_, why)) => degraded(&self.config, why),
            Err(XavierError::BadResponse(why)) => degraded(&self.config, why),
        }
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>, XavierError> {
        // `snippet` mode returns scored hits with path; the parser also
        // accepts `full` and `ids` shapes if the server ignores the mode.
        let body = serde_json::json!({"query": query, "limit": limit, "mode": "snippet"});
        let raw =
            serde_json::to_string(&body).map_err(|e| XavierError::BadResponse(e.to_string()))?;
        let (code, text) = self.post("/v1/memories/search", &raw)?;
        if !(200..300).contains(&code) {
            return Err(XavierError::BadResponse(format!("HTTP {code}: {text}")));
        }
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| XavierError::BadResponse(e.to_string()))?;
        Ok(parse_search_payload(&value))
    }
}

fn degraded(config: &XavierConfig, why: String) -> XavierStatus {
    XavierStatus {
        reachable: false,
        base_url: config.base_url.clone(),
        capabilities: Vec::new(),
        degraded: true,
        detail: why,
    }
}

/// Deterministic canned backend: tests, demos, offline runs.
pub struct StubBackend {
    pub reachable: bool,
    pub base_url: String,
    /// Raw JSON value fed through the same tolerant parser as HTTP.
    pub search_json: serde_json::Value,
}

impl StubBackend {
    pub fn reachable_with(search_json: serde_json::Value) -> Self {
        Self {
            reachable: true,
            base_url: "stub://xavier".to_owned(),
            search_json,
        }
    }

    pub fn unreachable() -> Self {
        Self {
            reachable: false,
            base_url: "stub://xavier".to_owned(),
            search_json: serde_json::Value::Null,
        }
    }
}

impl XavierBackend for StubBackend {
    fn check_status(&self) -> XavierStatus {
        if self.reachable {
            XavierStatus {
                reachable: true,
                base_url: self.base_url.clone(),
                capabilities: CAPABILITIES.iter().map(|s| s.to_string()).collect(),
                degraded: false,
                detail: "stub backend".to_owned(),
            }
        } else {
            XavierStatus {
                reachable: false,
                base_url: self.base_url.clone(),
                capabilities: Vec::new(),
                degraded: true,
                detail: "stub backend unreachable".to_owned(),
            }
        }
    }

    fn search(&self, _query: &str, _limit: usize) -> Result<Vec<MemoryHit>, XavierError> {
        if !self.reachable {
            return Err(XavierError::Unreachable(
                self.base_url.clone(),
                "stub backend unreachable".to_owned(),
            ));
        }
        Ok(parse_search_payload(&self.search_json))
    }
}

/// Accept every known search envelope: `{results:[...]}` in `full`,
/// `snippet`, or `ids` shape, plus a bare array. Unknown fields are
/// ignored; missing score/path/snippet default to empty/zero.
pub fn parse_search_payload(value: &serde_json::Value) -> Vec<MemoryHit> {
    let items: &[serde_json::Value] = if let Some(arr) = value.as_array() {
        arr
    } else if let Some(arr) = value.get("results").and_then(|r| r.as_array()) {
        arr
    } else {
        return Vec::new();
    };
    items.iter().map(parse_hit).collect()
}

fn parse_hit(item: &serde_json::Value) -> MemoryHit {
    let str_at = |key: &str| {
        item.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let meta = item.get("metadata");
    let meta_str = |key: &str| {
        meta.and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let mut path = str_at("path");
    if path.is_empty() {
        path = meta_str("path");
    }
    if path.is_empty() {
        // `full`-mode items identify the source via user_id instead.
        path = str_at("user_id");
    }
    let mut snippet = str_at("snippet");
    if snippet.is_empty() {
        snippet = str_at("memory");
    }
    if snippet.is_empty() {
        snippet = str_at("content");
    }
    if snippet.is_empty() {
        snippet = str_at("text");
    }
    // Keep CLI output readable: first 240 chars, single line.
    let snippet: String = snippet.chars().take(240).collect();
    let snippet = snippet.replace(['\n', '\r'], " ");
    let mut kind = str_at("kind");
    if kind.is_empty() {
        kind = meta_str("kind");
    }
    MemoryHit {
        id: str_at("id"),
        path,
        score: item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        snippet,
        kind,
    }
}

/// Draft a backlog from a goal plus grounding hits. Deterministic:
/// task 0 scopes the goal, one task per hit grounds it in the cited
/// path, each step depends on the previous one (linear chain).
pub fn draft_from_hits(goal: &str, hits: &[MemoryHit]) -> Vec<DraftTask> {
    let goal = goal.trim();
    let mut out = vec![DraftTask {
        title: format!("Scope: {goal}"),
        depends_on: Vec::new(),
    }];
    for (i, hit) in hits.iter().enumerate() {
        let source = if hit.path.is_empty() {
            hit.id.clone()
        } else {
            hit.path.clone()
        };
        out.push(DraftTask {
            title: format!("Ground '{goal}' in {source}"),
            // Index of the previous draft item (scope = 0, earlier hits follow).
            depends_on: vec![i],
        });
    }
    out
}

/// Minimal blocking HTTP/1.0 client (no new deps): one connection per
/// call, `Connection: close`, whole call bounded by the config timeout.
fn http_request(
    config: &XavierConfig,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<(u16, String), XavierError> {
    let unreachable = |why: String| XavierError::Unreachable(config.base_url.clone(), why);
    let (host, port, base_path) = split_base_url(&config.base_url)?;
    let addr = format!("{host}:{port}");
    let sock: Vec<std::net::SocketAddr> = addr
        .to_socket_addrs()
        .map_err(|e| unreachable(e.to_string()))?
        .collect();
    let first = sock
        .into_iter()
        .next()
        .ok_or_else(|| unreachable("no route".to_owned()))?;
    let timeout = config.timeout();
    let mut stream =
        TcpStream::connect_timeout(&first, timeout).map_err(|e| unreachable(e.to_string()))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|e| unreachable(e.to_string()))?;
    let full_path = format!("{base_path}{path}");
    let mut req = format!("{method} {full_path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n");
    if !config.token.is_empty() {
        req.push_str(&format!(
            "Authorization: Bearer {}\r\nX-Xavier-Token: {}\r\n",
            config.token, config.token
        ));
    }
    if let Some(b) = body {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        ));
    } else {
        req.push_str("\r\n");
    }
    stream
        .write_all(req.as_bytes())
        .map_err(|e| unreachable(e.to_string()))?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| unreachable(e.to_string()))?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let code = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|n| n.parse::<u16>().ok())
        .ok_or_else(|| XavierError::BadResponse("no HTTP status line".to_owned()))?;
    let body_start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(text.len());
    Ok((code, text[body_start..].to_owned()))
}

/// Split `http://host:port[/prefix]` into parts. Only plain http is
/// supported: atlas talks to a loopback dev server, never TLS here.
fn split_base_url(base: &str) -> Result<(String, u16, String), XavierError> {
    let bad = |why: &str| XavierError::BadResponse(format!("bad base url '{base}': {why}"));
    let rest = base
        .strip_prefix("http://")
        .ok_or_else(|| bad("need http://"))?;
    let (authority, prefix) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_owned()),
        None => (rest, String::new()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_owned(), p.parse::<u16>().map_err(|_| bad("bad port"))?),
        None => (authority.to_owned(), 80),
    };
    if host.is_empty() {
        return Err(bad("empty host"));
    }
    Ok((host, port, prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::channel;
    use std::thread;

    fn closed_port_config() -> XavierConfig {
        // Port 9 (discard) is virtually never bound; timeout kept tiny so
        // the degraded test stays fast even if the SYN hangs.
        XavierConfig {
            base_url: "http://127.0.0.1:9".to_owned(),
            token: String::new(),
            timeout_ms: 300,
        }
    }

    #[test]
    fn degraded_status_green_without_server() {
        let backend = HttpBackend::new(closed_port_config());
        let status = backend.check_status();
        assert!(!status.reachable);
        assert!(status.degraded);
        assert!(status.capabilities.is_empty());
    }

    #[test]
    fn degraded_search_green_without_server() {
        let backend = HttpBackend::new(closed_port_config());
        let err = backend
            .search("anything", 3)
            .expect_err("must be unreachable");
        assert!(matches!(err, XavierError::Unreachable(..)));
    }

    #[test]
    fn status_reports_reachable_against_stub_server() {
        let (port, _guard) = serve_stub("{\"status\":\"ok\"}", 200);
        let backend = HttpBackend::new(XavierConfig {
            base_url: format!("http://127.0.0.1:{port}"),
            token: String::new(),
            timeout_ms: 2000,
        });
        let status = backend.check_status();
        assert!(status.reachable);
        assert!(!status.degraded);
        assert_eq!(
            status.capabilities,
            CAPABILITIES
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn status_reports_unreachable_state_explicitly() {
        let backend = StubBackend::unreachable();
        let status = backend.check_status();
        assert!(!status.reachable && status.degraded);
        assert!(backend.search("q", 1).is_err());
    }

    #[test]
    fn draft_parsing_with_stubbed_snippet_json() {
        let payload = serde_json::json!({
            "count": 2,
            "workspace_id": "ws",
            "mode": "snippet",
            "results": [
                {"id": "m1", "snippet": "verifier promotes tasks", "score": 0.91,
                 "path": "docs/verifier.md", "kind": "doc"},
                {"id": "m2", "snippet": "dag edges", "score": 0.42,
                 "path": "src/dag.rs", "kind": "code"}
            ]
        });
        let backend = StubBackend::reachable_with(payload);
        let hits = backend.search("verify", 5).expect("stub search");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "docs/verifier.md");
        assert!((hits[0].score - 0.91).abs() < 1e-6);
        let draft = draft_from_hits("ship verifier", &hits);
        assert_eq!(draft.len(), 3);
        assert_eq!(draft[0].depends_on, Vec::<usize>::new());
        assert_eq!(draft[1].depends_on, vec![0]);
        assert_eq!(draft[2].depends_on, vec![1]);
        assert!(draft[1].title.contains("docs/verifier.md"));
    }

    #[test]
    fn draft_parsing_accepts_full_and_ids_shapes() {
        let full = serde_json::json!({"status": "ok", "results": [
            {"id": "a", "memory": "some content here", "user_id": "u1", "metadata": {}}
        ]});
        let hits = parse_search_payload(&full);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "some content here");
        let ids = serde_json::json!({"status": "ok",
            "results": [{"id": "b", "score": 0.5, "path": "p.md"}]});
        let hits = parse_search_payload(&ids);
        assert_eq!(hits[0].score, 0.5);
        assert_eq!(hits[0].path, "p.md");
        // Empty goal still drafts exactly the scope task.
        assert_eq!(draft_from_hits("g", &[]).len(), 1);
    }

    /// Tiny single-shot HTTP stub on std TcpListener. Returns the bound
    /// port; the guard thread is joined on drop via channel close.
    fn serve_stub(body: &'static str, code: u16) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = channel::<()>();
        let handle = thread::spawn(move || {
            let _ = tx.send(());
            // Serve a few requests (status + search + retries).
            for mut s in listener.incoming().take(4).flatten() {
                let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let reason = if code == 200 { "OK" } else { "ERROR" };
                let resp = format!(
                    "HTTP/1.0 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        rx.recv().expect("stub ready");
        (port, handle)
    }

    #[test]
    fn search_against_stub_server_parses_results() {
        let body = r#"{"status":"ok","results":[{"id":"s1","snippet":"hit text","score":0.7,"path":"a/b.md","kind":"doc"}]}"#;
        // Leak to 'static for the stub thread (test-only).
        let body_static: &'static str = Box::leak(body.to_owned().into_boxed_str());
        let (port, _guard) = serve_stub(body_static, 200);
        let backend = HttpBackend::new(XavierConfig {
            base_url: format!("http://127.0.0.1:{port}"),
            token: String::new(),
            timeout_ms: 2000,
        });
        let hits = backend.search("q", 3).expect("stub search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "s1");
        assert_eq!(hits[0].path, "a/b.md");
    }
}
