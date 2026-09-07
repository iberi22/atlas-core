// SQLite-backed task-DAG store: versioned schema plus graph operations.
// Single-file DB (WAL), append-only events, cycle-safe edges (REQ-F-002/004).
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AtlasError;
pub use crate::error::Result;
use crate::model::{
    Checkpoint, CiRecord, DodItem, Event, EvidenceItem, ForgeIssue, ForgePr, IssueState, PrState,
    Session, Task, TaskState, TickSummary,
};

/// Current schema revision tracked in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i32 = 4;

/// Max consecutive failures before a task escalates to FAILED (REQ-F-009).
pub const MAX_RETRIES: i64 = 3;

const SCHEMA_UP: &str = "
CREATE TABLE IF NOT EXISTS sessions(
    id TEXT PRIMARY KEY,
    goal TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    status TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks(
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    agent TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_session ON tasks(session_id);
CREATE TABLE IF NOT EXISTS edges(
    child_id TEXT NOT NULL REFERENCES tasks(id),
    parent_id TEXT NOT NULL REFERENCES tasks(id),
    PRIMARY KEY(child_id, parent_id)
);
CREATE INDEX IF NOT EXISTS idx_edges_parent ON edges(parent_id);
CREATE TABLE IF NOT EXISTS events(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    payload TEXT NOT NULL,
    idempotency_key TEXT,
    processed INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_events_idem ON events(idempotency_key);
CREATE TABLE IF NOT EXISTS checkpoint(
    id INTEGER PRIMARY KEY CHECK(id = 1),
    last_event_id INTEGER NOT NULL,
    snapshot TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS metrics(
    task_id TEXT PRIMARY KEY REFERENCES tasks(id),
    duration_ms INTEGER NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    outcome TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS dod_items(
    task_id TEXT NOT NULL REFERENCES tasks(id),
    n INTEGER NOT NULL,
    text TEXT NOT NULL,
    checked INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(task_id, n)
);
CREATE TABLE IF NOT EXISTS evidence(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_evidence_task ON evidence(task_id);
CREATE TABLE IF NOT EXISTS verify_decisions(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    event_id INTEGER,
    passed INTEGER NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS forge_issues(
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'OPEN',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS forge_prs(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    base TEXT NOT NULL DEFAULT 'main',
    branch TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'OPEN',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS forge_ci(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pr_id INTEGER NOT NULL REFERENCES forge_prs(id),
    head_sha TEXT NOT NULL,
    profile TEXT NOT NULL DEFAULT 'fast',
    passed INTEGER NOT NULL,
    evidence TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_forge_ci_head ON forge_ci(head_sha);
CREATE INDEX IF NOT EXISTS idx_forge_ci_pr ON forge_ci(pr_id);
";

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Seconds since the Unix epoch; never panics, falls back to zero.
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Short unique id with a readable prefix (no external id crate needed).
fn new_id(prefix: &str) -> String {
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{:x}{:x}{n:x}", std::process::id(), now_secs())
}

/// The DAG store; wraps one SQLite connection to a single file.
pub struct Store {
    conn: Connection,
    /// Grafo internado cacheado: la arista se carga de SQLite UNA vez y se
    /// reusa en todas las consultas de traversal mientras los conteos de
    /// `tasks`/`edges` no cambien (cualquier escritura — API o SQL directo —
    /// muta un conteo y dispara recarga). SQLite sigue siendo la fuente de
    /// verdad y la persistencia; esto es solo un snapshot de lectura.
    graph: RwLock<GraphSnapshot>,
}

/// Snapshot internado del grafo: ids únicos + sesión por nodo + aristas como
/// pares de índices. Cero `String` por arista; el cómputo (Kahn/BFS del
/// núcleo `dag`) corre puro en memoria.
#[derive(Default)]
struct GraphSnapshot {
    loaded: bool,
    tasks: i64,
    edges: i64,
    ids: Vec<String>,
    sessions: Vec<String>,
    index: HashMap<String, u32>,
    pairs: Vec<(u32, u32)>,
}

impl Store {
    /// Open (creating) the DB file and run pending migrations.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // WAL is a no-op on `:memory:`; failures there are safe to ignore.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let store = Self {
            conn,
            graph: RwLock::new(GraphSnapshot::default()),
        };
        store.migrate()?;
        Ok(store)
    }

    /// In-memory DB, used by unit tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn,
            graph: RwLock::new(GraphSnapshot::default()),
        };
        store.migrate()?;
        Ok(store)
    }

    /// Apply pending migrations based on `PRAGMA user_version`.
    pub fn migrate(&self) -> Result<()> {
        let version: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version <= 0 {
            // Fresh DB: full v3 schema in one batch.
            self.conn.execute_batch(SCHEMA_UP)?;
            self.conn
                .pragma_update(None, "user_version", SCHEMA_VERSION)?;
            return Ok(());
        }
        if version < 2 {
            // v1 -> v2: idempotency keys + processed flags on events,
            // retry counter on tasks, durable dispatcher checkpoint.
            self.conn.execute_batch(
                "ALTER TABLE events ADD COLUMN idempotency_key TEXT;
                 ALTER TABLE events ADD COLUMN processed INTEGER NOT NULL DEFAULT 0;
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_events_idem ON events(idempotency_key);
                 ALTER TABLE tasks ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
                 CREATE TABLE IF NOT EXISTS checkpoint(
                     id INTEGER PRIMARY KEY CHECK(id = 1),
                     last_event_id INTEGER NOT NULL,
                     snapshot TEXT NOT NULL,
                     updated_at INTEGER NOT NULL
                 );",
            )?;
        }
        if version < 3 {
            // v2 -> v3: verifier tables (DoD checklist, evidence log,
            // promotion decisions).
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS dod_items(
                     task_id TEXT NOT NULL REFERENCES tasks(id),
                     n INTEGER NOT NULL,
                     text TEXT NOT NULL,
                     checked INTEGER NOT NULL DEFAULT 0,
                     PRIMARY KEY(task_id, n)
                 );
                 CREATE TABLE IF NOT EXISTS evidence(
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     task_id TEXT NOT NULL REFERENCES tasks(id),
                     text TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_evidence_task ON evidence(task_id);
                 CREATE TABLE IF NOT EXISTS verify_decisions(
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     task_id TEXT NOT NULL REFERENCES tasks(id),
                     event_id INTEGER,
                     passed INTEGER NOT NULL,
                     reason TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );",
            )?;
        }
        if version < 4 {
            // v3 -> v4: forge loop (issues, PRs, fast-CI evidence, REQ-F-017).
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS forge_issues(
                     id TEXT PRIMARY KEY,
                     title TEXT NOT NULL,
                     body TEXT NOT NULL DEFAULT '',
                     state TEXT NOT NULL DEFAULT 'OPEN',
                     created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS forge_prs(
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     title TEXT NOT NULL,
                     base TEXT NOT NULL DEFAULT 'main',
                     branch TEXT NOT NULL,
                     state TEXT NOT NULL DEFAULT 'OPEN',
                     created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS forge_ci(
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     pr_id INTEGER NOT NULL REFERENCES forge_prs(id),
                     head_sha TEXT NOT NULL,
                     profile TEXT NOT NULL DEFAULT 'fast',
                     passed INTEGER NOT NULL,
                     evidence TEXT NOT NULL DEFAULT '',
                     created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_forge_ci_head ON forge_ci(head_sha);
                 CREATE INDEX IF NOT EXISTS idx_forge_ci_pr ON forge_ci(pr_id);",
            )?;
        }
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// Drop every table and reset the schema version (tests only).
    pub fn migrate_down(&self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS forge_ci;
             DROP TABLE IF EXISTS forge_prs;
             DROP TABLE IF EXISTS forge_issues;
             DROP TABLE IF EXISTS verify_decisions;
             DROP TABLE IF EXISTS evidence;
             DROP TABLE IF EXISTS dod_items;
             DROP TABLE IF EXISTS checkpoint;
             DROP TABLE IF EXISTS metrics;
             DROP TABLE IF EXISTS events;
             DROP TABLE IF EXISTS edges;
             DROP TABLE IF EXISTS tasks;
             DROP TABLE IF EXISTS sessions;",
        )?;
        self.conn.pragma_update(None, "user_version", 0)?;
        Ok(())
    }

    /// Current `PRAGMA user_version`.
    pub fn schema_version(&self) -> Result<i32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    // -- sessions ----------------------------------------------------------

    /// Create a session (REQ-F-001) and return its id.
    pub fn create_session(&self, goal: &str) -> Result<String> {
        let id = new_id("s_");
        self.conn.execute(
            "INSERT INTO sessions(id, goal, started_at, status) VALUES(?1, ?2, ?3, 'active')",
            params![id, goal, now_secs()],
        )?;
        self.record_event("session_started", &format!("{{\"id\":\"{id}\"}}"))?;
        Ok(id)
    }

    pub fn get_session(&self, id: &str) -> Result<Session> {
        self.conn
            .query_row(
                "SELECT id, goal, started_at, status FROM sessions WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Session {
                        id: r.get(0)?,
                        goal: r.get(1)?,
                        started_at: r.get(2)?,
                        status: r.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AtlasError::NotFound(format!("session {id}")))
    }

    /// Most recently started session, used as the CLI default.
    pub fn latest_session(&self) -> Result<Option<Session>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, goal, started_at, status FROM sessions ORDER BY started_at DESC, id DESC LIMIT 1",
                [],
                |r| {
                    Ok(Session {
                        id: r.get(0)?,
                        goal: r.get(1)?,
                        started_at: r.get(2)?,
                        status: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, goal, started_at, status FROM sessions ORDER BY started_at, id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Session {
                id: r.get(0)?,
                goal: r.get(1)?,
                started_at: r.get(2)?,
                status: r.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(AtlasError::Db)
    }

    pub fn set_session_status(&self, id: &str, status: &str) -> Result<Session> {
        let before = self.get_session(id)?;
        let _ = before;
        let n = self.conn.execute(
            "UPDATE sessions SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        if n == 0 {
            return Err(AtlasError::NotFound(format!("session {id}")));
        }
        self.record_event(
            "session_status",
            &format!("{{\"id\":\"{id}\",\"status\":\"{status}\"}}"),
        )?;
        self.get_session(id)
    }

    // -- tasks -------------------------------------------------------------

    fn require_task_row(&self, id: &str) -> Result<Task> {
        self.conn
            .query_row(
                "SELECT id, session_id, title, state, agent, attempts, created_at, updated_at
                 FROM tasks WHERE id = ?1",
                params![id],
                |r| {
                    let state: String = r.get(3)?;
                    Ok(Task {
                        id: r.get(0)?,
                        session_id: r.get(1)?,
                        title: r.get(2)?,
                        state: state.parse().map_err(|_| {
                            rusqlite::Error::InvalidColumnType(
                                3,
                                "state".to_owned(),
                                rusqlite::types::Type::Text,
                            )
                        })?,
                        agent: r.get(4)?,
                        attempts: r.get(5)?,
                        created_at: r.get(6)?,
                        updated_at: r.get(7)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AtlasError::NotFound(format!("task {id}")))
    }

    pub fn get_task(&self, id: &str) -> Result<Task> {
        self.require_task_row(id)
    }

    /// Create a task; tasks with incomplete parents start BLOCKED, otherwise READY.
    pub fn create_task(
        &self,
        session_id: &str,
        title: &str,
        agent: Option<&str>,
        depends_on: &[&str],
    ) -> Result<String> {
        self.get_session(session_id)?;
        for parent in depends_on {
            self.require_task_row(parent)?;
        }
        let id = new_id("t_");
        // Insert first as PENDING so the row exists, then attach edges.
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO tasks(id, session_id, title, state, agent, created_at, updated_at)
             VALUES(?1, ?2, ?3, 'PENDING', ?4, ?5, ?6)",
            params![id, session_id, title, agent, now, now],
        )?;
        for parent in depends_on {
            self.insert_edge(&id, parent)?;
        }
        self.refresh_state(&id)?;
        self.record_event("task_created", &format!("{{\"id\":\"{id}\"}}"))?;
        Ok(id)
    }

    /// Attach an extra dependency to an existing task (cycle-checked).
    pub fn add_dependency(&self, child: &str, parent: &str) -> Result<()> {
        self.require_task_row(child)?;
        self.require_task_row(parent)?;
        self.insert_edge(child, parent)?;
        self.refresh_state(child)?;
        Ok(())
    }

    /// Insert one edge after rejecting cycles; duplicate edges are ignored.
    fn insert_edge(&self, child: &str, parent: &str) -> Result<()> {
        if self.would_cycle(child, parent)? {
            return Err(AtlasError::Cycle {
                child: child.to_owned(),
                parent: parent.to_owned(),
            });
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO edges(child_id, parent_id) VALUES(?1, ?2)",
            params![child, parent],
        )?;
        Ok(())
    }

    /// True when `parent` already (transitively) depends on `child`.
    /// Snapshot en memoria + `dag::reachable_idx` (núcleo puro, sin CTE
    /// recursiva; SQLite queda como store, ADR-004).
    fn would_cycle(&self, child: &str, parent: &str) -> Result<bool> {
        if child == parent {
            return Ok(true);
        }
        let snap = self.snapshot()?;
        let (Some(&ci), Some(&pi)) = (snap.index.get(child), snap.index.get(parent)) else {
            return Ok(false);
        };
        Ok(crate::dag::reachable_idx(pi, snap.ids.len(), &snap.pairs).contains(&ci))
    }

    /// Count of parents that are not COMPLETED.
    fn incomplete_parents(&self, child: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM edges e JOIN tasks t ON t.id = e.parent_id
             WHERE e.child_id = ?1 AND t.state != 'COMPLETED'",
            params![child],
            |r| r.get(0),
        )?)
    }

    /// Recompute BLOCKED/READY for non-terminal tasks (REQ-F-004 rule).
    fn refresh_state(&self, id: &str) -> Result<TaskState> {
        let task = self.require_task_row(id)?;
        if task.state.is_terminal() {
            return Ok(task.state);
        }
        // A task with incomplete parents is BLOCKED; otherwise READY.
        let next = if self.incomplete_parents(id)? > 0 {
            TaskState::Blocked
        } else {
            TaskState::Ready
        };
        self.set_state(id, next)?;
        Ok(next)
    }

    fn set_state(&self, id: &str, state: TaskState) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET state = ?1, updated_at = ?2 WHERE id = ?3",
            params![state.as_str(), now_secs(), id],
        )?;
        Ok(())
    }

    pub fn list_tasks(&self, session: Option<&str>) -> Result<Vec<Task>> {
        let sql = "SELECT id, session_id, title, state, agent, attempts, created_at, updated_at
                 FROM tasks";
        let mut out = Vec::new();
        if let Some(s) = session {
            let mut stmt = self.conn.prepare(&format!(
                "{sql} WHERE session_id = ?1 ORDER BY created_at, id"
            ))?;
            let rows = stmt.query_map(params![s], task_from_row)?;
            for r in rows {
                out.push(r?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare(&format!("{sql} ORDER BY created_at, id"))?;
            let rows = stmt.query_map([], task_from_row)?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    pub fn parents_of(&self, child: &str) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.session_id, t.title, t.state, t.agent, t.attempts, t.created_at, t.updated_at
             FROM tasks t JOIN edges e ON e.parent_id = t.id WHERE e.child_id = ?1",
        )?;
        let rows = stmt.query_map(params![child], task_from_row)?;
        rows.collect::<std::result::Result<Vec<Task>, _>>()
            .map_err(AtlasError::Db)
    }

    pub fn children_of(&self, parent: &str) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.session_id, t.title, t.state, t.agent, t.attempts, t.created_at, t.updated_at
             FROM tasks t JOIN edges e ON e.child_id = t.id WHERE e.parent_id = ?1",
        )?;
        let rows = stmt.query_map(params![parent], task_from_row)?;
        rows.collect::<std::result::Result<Vec<Task>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Every `(child, parent)` edge in the DB (ATLAS-04: feeds `dag` core).
    pub fn all_edges(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT child_id, parent_id FROM edges")?;
        let rows = stmt.query_map([], |r| {
            let child: String = r.get(0)?;
            let parent: String = r.get(1)?;
            Ok((child, parent))
        })?;
        rows.collect::<std::result::Result<Vec<(String, String)>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Transitive ancestor ids of `child`, sorted. Snapshot en memoria +
    /// `dag::reachable_idx` (núcleo puro; SQLite queda como store, ADR-004).
    pub fn ancestors(&self, child: &str) -> Result<Vec<String>> {
        self.require_task_row(child)?;
        let snap = self.snapshot()?;
        let start = *snap
            .index
            .get(child)
            .ok_or_else(|| AtlasError::InvalidState(format!("task {child} vanished mid-query")))?;
        let mut out: Vec<String> = crate::dag::reachable_idx(start, snap.ids.len(), &snap.pairs)
            .iter()
            .map(|&i| snap.ids[i as usize].clone())
            .collect();
        out.sort_unstable();
        Ok(out)
    }

    /// Tasks of one session (or all) in topological order, parents first.
    /// Cycle here is unreachable via the API (edges are cycle-checked on
    /// insert) but surfaces as `InvalidState` for DBs edited by hand.
    pub fn topo_sorted(&self, session: Option<&str>) -> Result<Vec<Task>> {
        let tasks = self.list_tasks(session)?;
        let nodes: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
        let order = crate::dag::topo_order(&nodes, &self.all_edges()?)
            .map_err(|e| AtlasError::InvalidState(e.to_string()))?;
        let by_id: HashMap<&str, &Task> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
        Ok(order
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).map(|t| (*t).clone()))
            .collect())
    }

    /// Count tasks per state for one session (used by `status`).
    pub fn counts_by_state(&self, session_id: &str) -> Result<HashMap<String, i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT state, COUNT(*) FROM tasks WHERE session_id = ?1 GROUP BY state")?;
        let rows = stmt.query_map(params![session_id], |r| {
            let s: String = r.get(0)?;
            let n: i64 = r.get(1)?;
            Ok((s, n))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (s, n) = r?;
            map.insert(s, n);
        }
        Ok(map)
    }

    // -- transitions -------------------------------------------------------

    /// Manually block a task with a reason (REQ-F-005).
    pub fn block_task(&self, id: &str, reason: &str) -> Result<Task> {
        let task = self.require_task_row(id)?;
        if task.state.is_terminal() {
            return Err(AtlasError::InvalidTransition(format!(
                "task {id} is {}",
                task.state
            )));
        }
        self.set_state(id, TaskState::Blocked)?;
        self.record_event(
            "task_blocked",
            &format!("{{\"id\":\"{id}\",\"reason\":\"{reason}\"}}"),
        )?;
        self.get_task(id)
    }

    /// Move a READY task to IN_PROGRESS (REQ-F-009 gate: only READY
    /// work may reach the agent backend). Anything else is rejected.
    pub fn start_task(&self, id: &str) -> Result<Task> {
        let task = self.require_task_row(id)?;
        match task.state {
            TaskState::Ready => self.set_state(id, TaskState::InProgress)?,
            other => {
                return Err(AtlasError::InvalidTransition(format!(
                    "task {id} is {other}, only READY work can start"
                )));
            }
        }
        self.record_event("task_started", &format!("{{\"id\":\"{id}\"}}"))?;
        self.get_task(id)
    }

    /// Strict entry point for `atlas run <id>`: READY -> IN_PROGRESS.
    pub fn run_task(&self, id: &str) -> Result<Task> {
        self.start_task(id)
    }

    /// Verifier-gated promotion to COMPLETED (REQ-F-012).
    /// Agent self-declaration is rejected: the rule layer (DoD fully
    /// checked + at least one evidence) must pass first. The decision
    /// is stored on `verify_decisions`; dependents unlock as before.
    pub fn complete_task(&self, id: &str) -> Result<Task> {
        self.require_task_row(id)?;
        let report = self.verify(id)?;
        if !report.passed {
            let reason = report.failures.join("; ");
            self.record_decision(id, None, false, &reason)?;
            return Err(AtlasError::InvalidTransition(format!(
                "verifier rejected promotion of task {id}: {reason}"
            )));
        }
        self.record_decision(id, None, true, "rules passed")?;
        self.set_state(id, TaskState::Completed)?;
        self.record_event("task_completed", &format!("{{\"id\":\"{id}\"}}"))?;
        for child in self.children_of(id)? {
            let before = child.state;
            let after = self.refresh_state(&child.id)?;
            if before != after && after == TaskState::Ready {
                self.record_event("task_ready", &format!("{{\"id\":\"{}\"}}", child.id))?;
            }
        }
        self.get_task(id)
    }

    pub fn fail_task(&self, id: &str, reason: &str) -> Result<Task> {
        self.require_task_row(id)?;
        self.set_state(id, TaskState::Failed)?;
        self.record_event(
            "task_failed",
            &format!("{{\"id\":\"{id}\",\"reason\":\"{reason}\"}}"),
        )?;
        self.get_task(id)
    }

    // -- verifier: DoD checklist + evidence (REQ-F-012/013) ------------------

    /// Append a DoD checklist item; numbers run 1, 2, ... per task.
    /// Returns the item number for `dod_check`.
    pub fn dod_add(&self, task_id: &str, text: &str) -> Result<i64> {
        self.require_task_row(task_id)?;
        let next: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(n), 0) + 1 FROM dod_items WHERE task_id = ?1",
            params![task_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO dod_items(task_id, n, text, checked) VALUES(?1, ?2, ?3, 0)",
            params![task_id, next, text],
        )?;
        Ok(next)
    }

    /// Mark one DoD item checked (1-based per-task number).
    pub fn dod_check(&self, task_id: &str, n: i64) -> Result<DodItem> {
        self.require_task_row(task_id)?;
        let updated = self.conn.execute(
            "UPDATE dod_items SET checked = 1 WHERE task_id = ?1 AND n = ?2",
            params![task_id, n],
        )?;
        if updated == 0 {
            return Err(AtlasError::NotFound(format!(
                "dod item {n} on task {task_id}"
            )));
        }
        self.dod_get(task_id, n)
    }

    fn dod_get(&self, task_id: &str, n: i64) -> Result<DodItem> {
        self.conn
            .query_row(
                "SELECT task_id, n, text, checked FROM dod_items WHERE task_id = ?1 AND n = ?2",
                params![task_id, n],
                dod_from_row,
            )
            .optional()?
            .ok_or_else(|| AtlasError::NotFound(format!("dod item {n} on task {task_id}")))
    }

    /// Every DoD item of a task, in number order.
    pub fn dod_list(&self, task_id: &str) -> Result<Vec<DodItem>> {
        self.require_task_row(task_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT task_id, n, text, checked FROM dod_items WHERE task_id = ?1 ORDER BY n",
        )?;
        let rows = stmt.query_map(params![task_id], dod_from_row)?;
        rows.collect::<std::result::Result<Vec<DodItem>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Attach one evidence string to a task (REQ-F-012).
    pub fn evidence_add(&self, task_id: &str, text: &str) -> Result<i64> {
        self.require_task_row(task_id)?;
        self.conn.execute(
            "INSERT INTO evidence(task_id, text, created_at) VALUES(?1, ?2, ?3)",
            params![task_id, text, now_secs()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Every evidence string of a task, oldest first.
    pub fn evidence_list(&self, task_id: &str) -> Result<Vec<EvidenceItem>> {
        self.require_task_row(task_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, text, created_at FROM evidence WHERE task_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![task_id], evidence_from_row)?;
        rows.collect::<std::result::Result<Vec<EvidenceItem>, _>>()
            .map_err(AtlasError::Db)
    }

    // -- forge loop (REQ-F-017/018) ------------------------------------------
    // Issues, PRs, and fast-CI evidence live in the SAME SQLite file.
    // Branch existence is validated by the caller (`forge::branch_exists`)
    // before `pr_create`; the store only records the validated name.

    /// Create a forge issue in state OPEN and return its id.
    pub fn issue_create(&self, title: &str, body: &str) -> Result<String> {
        if title.trim().is_empty() {
            return Err(AtlasError::Forge(
                "issue title must not be empty".to_owned(),
            ));
        }
        let id = new_id("iss_");
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO forge_issues(id, title, body, state, created_at, updated_at)
             VALUES(?1, ?2, ?3, 'OPEN', ?4, ?4)",
            params![id, title, body, now],
        )?;
        Ok(id)
    }

    /// Fetch one forge issue by id.
    pub fn issue_get(&self, id: &str) -> Result<ForgeIssue> {
        self.conn
            .query_row(
                "SELECT id, title, body, state, created_at, updated_at
                 FROM forge_issues WHERE id = ?1",
                params![id],
                forge_issue_from_row,
            )
            .optional()?
            .ok_or_else(|| AtlasError::NotFound(format!("issue {id}")))
    }

    /// Every forge issue, oldest first.
    pub fn issue_list(&self) -> Result<Vec<ForgeIssue>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, state, created_at, updated_at
             FROM forge_issues ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([], forge_issue_from_row)?;
        rows.collect::<std::result::Result<Vec<ForgeIssue>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Move an OPEN issue to CLOSED.
    pub fn issue_close(&self, id: &str) -> Result<ForgeIssue> {
        let issue = self.issue_get(id)?;
        if issue.state != IssueState::Open {
            return Err(AtlasError::InvalidTransition(format!(
                "issue {id} is already {}",
                issue.state
            )));
        }
        self.conn.execute(
            "UPDATE forge_issues SET state = 'CLOSED', updated_at = ?1 WHERE id = ?2",
            params![now_secs(), id],
        )?;
        self.issue_get(id)
    }

    /// Record a PR row in state OPEN. The caller must have validated
    /// `branch` with `git rev-parse --verify` (see `forge::branch_exists`).
    pub fn pr_create(&self, title: &str, base: &str, branch: &str) -> Result<i64> {
        if title.trim().is_empty() {
            return Err(AtlasError::Forge("pr title must not be empty".to_owned()));
        }
        if branch.trim().is_empty() {
            return Err(AtlasError::Forge("pr branch must not be empty".to_owned()));
        }
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO forge_prs(title, base, branch, state, created_at, updated_at)
             VALUES(?1, ?2, ?3, 'OPEN', ?4, ?4)",
            params![title, base, branch, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Fetch one PR by id.
    pub fn pr_get(&self, id: i64) -> Result<ForgePr> {
        self.conn
            .query_row(
                "SELECT id, title, base, branch, state, created_at, updated_at
                 FROM forge_prs WHERE id = ?1",
                params![id],
                forge_pr_from_row,
            )
            .optional()?
            .ok_or_else(|| AtlasError::NotFound(format!("pr {id}")))
    }

    /// Every PR, oldest first.
    pub fn pr_list(&self) -> Result<Vec<ForgePr>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, base, branch, state, created_at, updated_at
             FROM forge_prs ORDER BY id",
        )?;
        let rows = stmt.query_map([], forge_pr_from_row)?;
        rows.collect::<std::result::Result<Vec<ForgePr>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Move a PR to MERGED or CLOSED (OPEN only; no reopen).
    pub fn pr_set_state(&self, id: i64, state: PrState) -> Result<ForgePr> {
        let pr = self.pr_get(id)?;
        if pr.state != PrState::Open {
            return Err(AtlasError::InvalidTransition(format!(
                "pr {id} is already {}",
                pr.state
            )));
        }
        if state == PrState::Open {
            return Err(AtlasError::InvalidTransition(
                "pr target state must be MERGED or CLOSED".to_owned(),
            ));
        }
        self.conn.execute(
            "UPDATE forge_prs SET state = ?1, updated_at = ?2 WHERE id = ?3",
            params![state.as_str(), now_secs(), id],
        )?;
        self.pr_get(id)
    }

    /// Record one fast-CI run (PASS/FAIL + evidence) on a PR.
    pub fn ci_record(
        &self,
        pr_id: i64,
        head_sha: &str,
        profile: &str,
        passed: bool,
        evidence: &str,
    ) -> Result<i64> {
        self.pr_get(pr_id)?; // 404 on unknown PRs.
        self.conn.execute(
            "INSERT INTO forge_ci(pr_id, head_sha, profile, passed, evidence, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                pr_id,
                head_sha,
                profile,
                i64::from(passed),
                evidence,
                now_secs()
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Latest CI record for an exact HEAD sha (any PR); the deploy gate
    /// (REQ-F-018) requires this to exist with `passed = true`.
    pub fn ci_latest_for_head(&self, head_sha: &str) -> Result<Option<CiRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, pr_id, head_sha, profile, passed, evidence, created_at
                 FROM forge_ci WHERE head_sha = ?1 ORDER BY id DESC LIMIT 1",
                params![head_sha],
                ci_record_from_row,
            )
            .optional()?)
    }

    /// Every CI record of a PR, oldest first.
    pub fn ci_list_for_pr(&self, pr_id: i64) -> Result<Vec<CiRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pr_id, head_sha, profile, passed, evidence, created_at
             FROM forge_ci WHERE pr_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![pr_id], ci_record_from_row)?;
        rows.collect::<std::result::Result<Vec<CiRecord>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Run the rule layer with a chosen reviewer; the decision is stored.
    pub fn verify_with(
        &self,
        reviewer: &dyn crate::verifier::Reviewer,
        task_id: &str,
    ) -> Result<crate::model::VerifyReport> {
        self.require_task_row(task_id)?;
        let report = crate::verifier::verify_with(self, reviewer, task_id)?;
        let reason = if report.passed {
            "rules passed".to_owned()
        } else {
            report.failures.join("; ")
        };
        self.record_decision(task_id, None, report.passed, &reason)?;
        Ok(report)
    }

    /// Run the rule layer with the bundled stub reviewer (non-blocking).
    pub fn verify(&self, task_id: &str) -> Result<crate::model::VerifyReport> {
        self.verify_with(&crate::verifier::StubReviewer, task_id)
    }

    /// Persist one promotion decision on the task node (REQ-F-012).
    fn record_decision(
        &self,
        task_id: &str,
        event_id: Option<i64>,
        passed: bool,
        reason: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO verify_decisions(task_id, event_id, passed, reason, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![task_id, event_id, i64::from(passed), reason, now_secs()],
        )?;
        Ok(())
    }

    // -- dispatcher (REQ-F-009/010/011) --------------------------------------

    /// Insert an event; with a key the insert is idempotent (`INSERT OR
    /// IGNORE`) and the row id for that key is returned (REQ-F-010).
    pub fn emit_event(&self, kind: &str, payload: &str, key: Option<&str>) -> Result<i64> {
        if let Some(k) = key {
            self.conn.execute(
                "INSERT OR IGNORE INTO events(type, payload, idempotency_key, processed, created_at)
                 VALUES(?1, ?2, ?3, 0, ?4)",
                params![kind, payload, k, now_secs()],
            )?;
            Ok(self.conn.query_row(
                "SELECT id FROM events WHERE idempotency_key = ?1",
                params![k],
                |r| r.get(0),
            )?)
        } else {
            self.conn.execute(
                "INSERT INTO events(type, payload, processed, created_at) VALUES(?1, ?2, 0, ?3)",
                params![kind, payload, now_secs()],
            )?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    /// `atlas complete <id> --ok|--fail`: only IN_PROGRESS tasks are
    /// accepted. Queues a `task_completed`/`task_failed` event under an
    /// idempotency key and snapshots the checkpoint; the state change
    /// itself happens in `tick_once`. Returns the queued event id.
    pub fn finish_task(&self, id: &str, ok: bool, reason: &str) -> Result<i64> {
        let task = self.require_task_row(id)?;
        if task.state != TaskState::InProgress {
            return Err(AtlasError::InvalidTransition(format!(
                "task {id} is {}, only IN_PROGRESS work can complete",
                task.state
            )));
        }
        let (kind, payload, key) = if ok {
            (
                "task_completed",
                format!("{{\"id\":\"{id}\"}}"),
                format!("completed:{id}"),
            )
        } else {
            let safe = reason.replace('\\', "\\\\").replace('"', "\\\"");
            (
                "task_failed",
                format!("{{\"id\":\"{id}\",\"reason\":\"{safe}\"}}"),
                format!("failed:{id}:attempt{}", task.attempts + 1),
            )
        };
        let event_id = self.emit_event(kind, &payload, Some(&key))?;
        self.snapshot_checkpoint()?;
        Ok(event_id)
    }

    /// Queued (unprocessed) events in id order.
    pub fn pending_events(&self) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, payload, idempotency_key, processed
             FROM events WHERE processed = 0 ORDER BY id",
        )?;
        let rows = stmt.query_map([], event_from_row)?;
        rows.collect::<std::result::Result<Vec<Event>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Number of queued (unprocessed) events.
    pub fn pending_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM events WHERE processed = 0", [], |r| {
                r.get(0)
            })?)
    }

    /// Latest events (newest first), capped at `limit`, for the
    /// `atlas serve` history panel (REQ-F-016).
    pub fn recent_events(&self, limit: i64) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, payload, idempotency_key, processed
             FROM events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], event_from_row)?;
        rows.collect::<std::result::Result<Vec<Event>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Current durable checkpoint, if any.
    pub fn checkpoint(&self) -> Result<Option<Checkpoint>> {
        Ok(self
            .conn
            .query_row(
                "SELECT last_event_id, snapshot, updated_at FROM checkpoint WHERE id = 1",
                [],
                |r| {
                    Ok(Checkpoint {
                        last_event_id: r.get(0)?,
                        snapshot: r.get(1)?,
                        updated_at: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Consume every queued event once, in id order (REQ-F-009/010).
    /// Each event is applied, marked processed, and checkpointed inside
    /// one transaction: a crash replays at most the in-flight event, and
    /// the idempotency key plus the processed flag make replays free of
    /// double-execution. Watchdog-only callers run a single pass
    /// (REQ-F-011).
    pub fn tick_once(&self) -> Result<TickSummary> {
        let mut summary = TickSummary {
            last_event_id: self.checkpoint()?.map(|c| c.last_event_id).unwrap_or(0),
            ..Default::default()
        };
        // Events failing the verifier gate stay queued (`processed = 0`)
        // so a later tick can promote them; skip them in-memory to keep
        // this pass moving over later events.
        let mut gated: Vec<i64> = Vec::new();
        loop {
            let mut sql = String::from(
                "SELECT id, type, payload, idempotency_key, processed
                 FROM events WHERE processed = 0",
            );
            if !gated.is_empty() {
                let ids: Vec<String> = gated.iter().map(i64::to_string).collect();
                sql.push_str(&format!(" AND id NOT IN ({})", ids.join(",")));
            }
            sql.push_str(" ORDER BY id LIMIT 1");
            let next: Option<Event> = self.conn.query_row(&sql, [], event_from_row).optional()?;
            let Some(ev) = next else { break };
            self.conn.execute("BEGIN IMMEDIATE", [])?;
            match self.apply_event(&ev) {
                Ok(Applied::Gated(reason)) => {
                    // Leave the event unprocessed with its reason; only the
                    // decision row is committed.
                    self.record_decision(
                        &event_task_id(&ev.payload).unwrap_or_default(),
                        Some(ev.id),
                        false,
                        &reason,
                    )?;
                    self.conn.execute("COMMIT", [])?;
                    summary.skipped += 1;
                    gated.push(ev.id);
                }
                Ok(applied) => {
                    self.conn.execute(
                        "UPDATE events SET processed = 1 WHERE id = ?1",
                        params![ev.id],
                    )?;
                    self.advance_checkpoint(ev.id)?;
                    self.conn.execute("COMMIT", [])?;
                    summary.processed += 1;
                    summary.last_event_id = ev.id;
                    match applied {
                        Applied::Unlocked => summary.unlocked += 1,
                        Applied::Retried => summary.retried += 1,
                        Applied::Escalated => summary.escalated += 1,
                        Applied::Skipped => summary.skipped += 1,
                        Applied::Gated(_) => {}
                    }
                }
                Err(e) => {
                    let _ = self.conn.execute("ROLLBACK", []);
                    return Err(e);
                }
            }
        }
        Ok(summary)
    }

    /// Apply one queued event; called inside the per-event transaction.
    fn apply_event(&self, ev: &Event) -> Result<Applied> {
        match ev.kind.as_str() {
            "task_completed" => {
                let Some(id) = event_task_id(&ev.payload) else {
                    return Ok(Applied::Skipped);
                };
                let task = self.require_task_row(&id);
                let Ok(task) = task else {
                    return Ok(Applied::Skipped);
                };
                match task.state {
                    // Replay or legacy direct-complete: state is already
                    // there, just make sure dependents are unlocked.
                    TaskState::Completed => {
                        self.unlock_dependents(&id, ev.id)?;
                        Ok(Applied::Unlocked)
                    }
                    TaskState::InProgress => {
                        // Promotion gate (REQ-F-012): no COMPLETED without
                        // passing the rule layer. Gated events stay queued
                        // with their reason; a later tick retries them.
                        let report = crate::verifier::verify_with(
                            self,
                            &crate::verifier::StubReviewer,
                            &id,
                        )?;
                        if !report.passed {
                            let reason = report.failures.join("; ");
                            return Ok(Applied::Gated(reason));
                        }
                        self.record_decision(&id, Some(ev.id), true, "rules passed")?;
                        self.set_state(&id, TaskState::Completed)?;
                        self.unlock_dependents(&id, ev.id)?;
                        Ok(Applied::Unlocked)
                    }
                    _ => Ok(Applied::Skipped),
                }
            }
            "task_failed" => {
                let Some(id) = event_task_id(&ev.payload) else {
                    return Ok(Applied::Skipped);
                };
                let task = self.require_task_row(&id);
                let Ok(task) = task else {
                    return Ok(Applied::Skipped);
                };
                if task.state.is_terminal() {
                    // Legacy direct-fail or replay: already settled.
                    return Ok(Applied::Skipped);
                }
                if task.state != TaskState::InProgress {
                    return Ok(Applied::Skipped);
                }
                let attempts = task.attempts + 1;
                self.conn.execute(
                    "UPDATE tasks SET attempts = ?1, updated_at = ?2 WHERE id = ?3",
                    params![attempts, now_secs(), id],
                )?;
                if attempts >= MAX_RETRIES {
                    self.set_state(&id, TaskState::Failed)?;
                    let payload = format!("{{\"id\":\"{id}\",\"attempts\":{attempts}}}");
                    let key = format!("escalated:{}:{id}", ev.id);
                    self.emit_event("task_escalated", &payload, Some(&key))?;
                    Ok(Applied::Escalated)
                } else {
                    self.set_state(&id, TaskState::Ready)?;
                    let payload = format!("{{\"id\":\"{id}\",\"attempt\":{attempts}}}");
                    let key = format!("retried:{}:{id}", ev.id);
                    self.emit_event("task_retried", &payload, Some(&key))?;
                    Ok(Applied::Retried)
                }
            }
            _ => Ok(Applied::Skipped),
        }
    }

    /// Refresh every dependent; newly READY children get a `task_ready`
    /// notice keyed by the consumed event (idempotent on replay).
    fn unlock_dependents(&self, parent: &str, event_id: i64) -> Result<usize> {
        let mut newly = 0;
        for child in self.children_of(parent)? {
            let before = child.state;
            let after = self.refresh_state(&child.id)?;
            if before != after && after == TaskState::Ready {
                newly += 1;
                let key = format!("ready:{event_id}:{}", child.id);
                let payload = format!("{{\"id\":\"{}\"}}", child.id);
                self.emit_event("task_ready", &payload, Some(&key))?;
            }
        }
        Ok(newly)
    }

    /// Full task-state snapshot used by checkpoints (REQ-F-010).
    fn snapshot_json(&self) -> Result<String> {
        let tasks = self.list_tasks(None)?;
        let mut parts = Vec::with_capacity(tasks.len());
        for t in &tasks {
            parts.push(format!(
                "\"{}\":\"{}:{}\"",
                t.id,
                t.state.as_str(),
                t.attempts
            ));
        }
        Ok(format!("{{{}}}", parts.join(",")))
    }

    /// Persist a snapshot checkpoint without moving the event cursor
    /// (used when queueing new work in `finish_task`).
    fn snapshot_checkpoint(&self) -> Result<()> {
        let last = self.checkpoint()?.map(|c| c.last_event_id).unwrap_or(0);
        let snap = self.snapshot_json()?;
        self.conn.execute(
            "INSERT INTO checkpoint(id, last_event_id, snapshot, updated_at)
             VALUES(1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               last_event_id = excluded.last_event_id,
               snapshot = excluded.snapshot,
               updated_at = excluded.updated_at",
            params![last, snap, now_secs()],
        )?;
        Ok(())
    }

    /// Move the checkpoint cursor after consuming one event (called
    /// inside the per-event transaction).
    fn advance_checkpoint(&self, event_id: i64) -> Result<()> {
        let snap = self.snapshot_json()?;
        self.conn.execute(
            "INSERT INTO checkpoint(id, last_event_id, snapshot, updated_at)
             VALUES(1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               last_event_id = excluded.last_event_id,
               snapshot = excluded.snapshot,
               updated_at = excluded.updated_at",
            params![event_id, snap, now_secs()],
        )?;
        Ok(())
    }

    // -- events / metrics --------------------------------------------------

    pub fn record_event(&self, kind: &str, payload: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events(type, payload, created_at) VALUES(?1, ?2, ?3)",
            params![kind, payload, now_secs()],
        )?;
        Ok(())
    }

    pub fn record_metric(
        &self,
        task_id: &str,
        duration_ms: i64,
        prompt_tokens: i64,
        completion_tokens: i64,
        outcome: &str,
    ) -> Result<()> {
        if duration_ms < 0 || prompt_tokens < 0 || completion_tokens < 0 {
            return Err(AtlasError::InvalidState(
                "metric values must be >= 0".to_owned(),
            ));
        }
        self.require_task_row(task_id)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO metrics(task_id, duration_ms, prompt_tokens, completion_tokens, outcome)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![task_id, duration_ms, prompt_tokens, completion_tokens, outcome],
        )?;
        Ok(())
    }

    /// Latest actual for one task, if any (REQ-F-014).
    pub fn get_metric(&self, task_id: &str) -> Result<Option<crate::estimator::MetricActual>> {
        Ok(self
            .conn
            .query_row(
                "SELECT task_id, duration_ms, prompt_tokens, completion_tokens, outcome
                 FROM metrics WHERE task_id = ?1",
                params![task_id],
                |r| {
                    Ok(crate::estimator::MetricActual {
                        task_id: r.get(0)?,
                        duration_ms: r.get(1)?,
                        prompt_tokens: r.get(2)?,
                        completion_tokens: r.get(3)?,
                        outcome: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Actuals of tasks whose title shares `prefix` (first token,
    /// lowercased), optionally excluding one task so its own row never
    /// feeds its estimate. Empty prefix matches nothing.
    pub fn metrics_for_prefix(
        &self,
        prefix: &str,
        exclude: Option<&str>,
    ) -> Result<Vec<crate::estimator::MetricActual>> {
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT m.task_id, m.duration_ms, m.prompt_tokens, m.completion_tokens, m.outcome
             FROM metrics m JOIN tasks t ON t.id = m.task_id
             WHERE lower(substr(t.title, 1, instr(t.title || ' ', ' ') - 1)) = ?1
               AND (?2 IS NULL OR m.task_id != ?2)",
        )?;
        let rows = stmt.query_map(params![prefix, exclude], |r| {
            Ok(crate::estimator::MetricActual {
                task_id: r.get(0)?,
                duration_ms: r.get(1)?,
                prompt_tokens: r.get(2)?,
                completion_tokens: r.get(3)?,
                outcome: r.get(4)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(AtlasError::Db)
    }

    /// Snapshot del grafo, cargado UNA vez y reusado mientras los conteos de
    /// `tasks`/`edges` coincidan. Camino caliente: 2 `COUNT(*)` + cómputo en
    /// memoria, cero scans de aristas.
    fn snapshot(&self) -> Result<std::sync::RwLockReadGuard<'_, GraphSnapshot>> {
        let poisoned = || AtlasError::InvalidState("graph lock poisoned".to_owned());
        let fresh = || -> Result<bool> {
            let g = self.graph.read().map_err(|_| poisoned())?;
            if !g.loaded {
                return Ok(false);
            }
            let tc: i64 = self
                .conn
                .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
            let ec: i64 = self
                .conn
                .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
            Ok(g.tasks == tc && g.edges == ec)
        };
        if !fresh()? {
            self.reload_graph()?;
        }
        self.graph.read().map_err(|_| poisoned())
    }

    /// Recarga el snapshot bajo write lock (una sola pasada por `tasks` y una
    /// por `edges`; los ids del cursor solo viven durante el lookup, cero
    /// `String` por arista). Los conteos se miden POST-scan para que lo
    /// guardado describa exactamente lo leído.
    fn reload_graph(&self) -> Result<()> {
        let mut g = self
            .graph
            .write()
            .map_err(|_| AtlasError::InvalidState("graph lock poisoned".to_owned()))?;
        let tc: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
        let ec: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        if g.loaded && g.tasks == tc && g.edges == ec {
            return Ok(()); // Otro hilo recargó mientras esperábamos el lock.
        }
        let mut tstmt = self.conn.prepare("SELECT id, session_id FROM tasks")?;
        let mut ids: Vec<String> = Vec::with_capacity(tc as usize);
        let mut sessions: Vec<String> = Vec::with_capacity(tc as usize);
        {
            let mut rows = tstmt.query([])?;
            while let Some(r) = rows.next()? {
                ids.push(r.get(0)?);
                sessions.push(r.get(1)?);
            }
        }
        let mut index: HashMap<String, u32> = HashMap::with_capacity(ids.len());
        for (i, id) in ids.iter().enumerate() {
            index.insert(id.clone(), i as u32);
        }
        let mut pairs: Vec<(u32, u32)> = Vec::with_capacity(ec as usize);
        {
            let mut estmt = self.conn.prepare("SELECT child_id, parent_id FROM edges")?;
            let mut rows = estmt.query([])?;
            while let Some(r) = rows.next()? {
                let c: &str = r
                    .get_ref(0)?
                    .as_str()
                    .map_err(|e| AtlasError::Db(e.into()))?;
                let p: &str = r
                    .get_ref(1)?
                    .as_str()
                    .map_err(|e| AtlasError::Db(e.into()))?;
                if let (Some(&ci), Some(&pi)) = (index.get(c), index.get(p)) {
                    pairs.push((ci, pi));
                }
            }
        }
        let tc2: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
        let ec2: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        *g = GraphSnapshot {
            loaded: true,
            tasks: tc2,
            edges: ec2,
            ids,
            sessions,
            index,
            pairs,
        };
        Ok(())
    }

    // -- traversal ---------------------------------------------------------

    /// Topological order of one session's tasks (Kahn's algorithm, REQ-F-006).
    /// Opera sobre el snapshot cacheado (la arista se cargó UNA vez); el
    /// filtro de sesión aplica en memoria, sin JOINs ni scans por query.
    pub fn traverse(&self, session_id: &str) -> Result<Vec<String>> {
        let snap = self.snapshot()?;
        let n = snap.ids.len();
        let mut member = vec![false; n];
        let mut count = 0usize;
        for (i, s) in snap.sessions.iter().enumerate() {
            if s.as_str() == session_id {
                member[i] = true;
                count += 1;
            }
        }
        // Reindexado local denso para Kahn.
        let mut local = vec![u32::MAX; n];
        let mut ids: Vec<String> = Vec::with_capacity(count);
        for (i, m) in member.iter().enumerate() {
            if *m {
                local[i] = ids.len() as u32;
                ids.push(snap.ids[i].clone());
            }
        }
        let m = ids.len();
        let mut fanout = vec![0u32; m];
        let mut indegree = vec![0u32; m];
        for &(gc, gp) in &snap.pairs {
            if member[gc as usize] && member[gp as usize] {
                fanout[local[gp as usize] as usize] += 1;
                indegree[local[gc as usize] as usize] += 1;
            }
        }
        let mut children: Vec<Vec<u32>> = fanout
            .iter()
            .map(|&f| Vec::with_capacity(f as usize))
            .collect();
        for &(gc, gp) in &snap.pairs {
            if member[gc as usize] && member[gp as usize] {
                children[local[gp as usize] as usize].push(local[gc as usize]);
            }
        }
        let mut queue: VecDeque<u32> = indegree
            .iter()
            .enumerate()
            .filter(|(_, d)| **d == 0)
            .map(|(i, _)| i as u32)
            .collect();
        let mut order = Vec::with_capacity(ids.len());
        while let Some(n) = queue.pop_front() {
            order.push(ids[n as usize].clone());
            for &m in &children[n as usize] {
                indegree[m as usize] -= 1;
                if indegree[m as usize] == 0 {
                    queue.push_back(m);
                }
            }
        }
        Ok(order)
    }
}

/// How one queued event was consumed by `tick_once`.
enum Applied {
    Unlocked,
    Retried,
    Escalated,
    Skipped,
    /// Verifier gate failed: event left unprocessed, carries the reason.
    Gated(String),
}

/// Task id carried in a `task_completed` / `task_failed` payload.
fn event_task_id(payload: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    v.get("id")?.as_str().map(str::to_owned)
}

fn event_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let processed: i64 = r.get(4)?;
    Ok(Event {
        id: r.get(0)?,
        kind: r.get(1)?,
        payload: r.get(2)?,
        idempotency_key: r.get(3)?,
        processed: processed != 0,
    })
}

fn dod_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DodItem> {
    let checked: i64 = r.get(3)?;
    Ok(DodItem {
        task_id: r.get(0)?,
        n: r.get(1)?,
        text: r.get(2)?,
        checked: checked != 0,
    })
}

fn evidence_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceItem> {
    Ok(EvidenceItem {
        id: r.get(0)?,
        task_id: r.get(1)?,
        text: r.get(2)?,
        created_at: r.get(3)?,
    })
}

fn task_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let state: String = r.get(3)?;
    Ok(Task {
        id: r.get(0)?,
        session_id: r.get(1)?,
        title: r.get(2)?,
        state: state.parse().unwrap_or(TaskState::Pending),
        agent: r.get(4)?,
        attempts: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

/// Map a `forge_issues` row; unknown states fall back to OPEN.
fn forge_issue_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ForgeIssue> {
    use std::str::FromStr as _;
    let state: String = r.get(3)?;
    Ok(ForgeIssue {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        state: IssueState::from_str(&state).unwrap_or(IssueState::Open),
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

/// Map a `forge_prs` row; unknown states fall back to OPEN.
fn forge_pr_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ForgePr> {
    use std::str::FromStr as _;
    let state: String = r.get(4)?;
    Ok(ForgePr {
        id: r.get(0)?,
        title: r.get(1)?,
        base: r.get(2)?,
        branch: r.get(3)?,
        state: PrState::from_str(&state).unwrap_or(PrState::Open),
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

/// Map a `forge_ci` row.
fn ci_record_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CiRecord> {
    let passed: i64 = r.get(4)?;
    Ok(CiRecord {
        id: r.get(0)?,
        pr_id: r.get(1)?,
        head_sha: r.get(2)?,
        profile: r.get(3)?,
        passed: passed != 0,
        evidence: r.get(5)?,
        created_at: r.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "atlas_{tag}_{}_{}.db",
            std::process::id(),
            now_secs()
        ));
        p
    }

    #[test]
    fn dag_core_matches_db_traversal() {
        // ATLAS-04: `ancestors`/`topo_sorted` run the shared `dag` core;
        // order must respect dependencies (parents before children).
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("dag parity").expect("session");
        let a = store.create_task(&s, "a", None, &[]).expect("a");
        let b = store.create_task(&s, "b", None, &[&a]).expect("b");
        let c = store.create_task(&s, "c", None, &[&b]).expect("c");
        let mut want = vec![a.clone(), b.clone()];
        want.sort();
        assert_eq!(store.ancestors(&c).expect("anc"), want);
        let ids: Vec<String> = store
            .topo_sorted(Some(&s))
            .expect("topo")
            .iter()
            .map(|t| t.id.clone())
            .collect();
        // Parents-first partial order (robust to generated-id collation).
        let pos = |id: &str| ids.iter().position(|x| x == id).expect("present");
        assert!(pos(&a) < pos(&b) && pos(&b) < pos(&c));
    }

    #[test]
    fn migration_up_down_on_temp_db() {
        let path = temp_path("migrate");
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path).expect("open");
        assert_eq!(store.schema_version().expect("version"), SCHEMA_VERSION);
        // Tables exist.
        for table in ["sessions", "tasks", "edges", "events", "metrics"] {
            let n: i64 = store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![table],
                    |r| r.get(0),
                )
                .expect("table check");
            assert_eq!(n, 1, "missing table {table}");
        }
        store.migrate_down().expect("down");
        assert_eq!(store.schema_version().expect("v0"), 0);
        store.migrate().expect("re-up");
        assert_eq!(store.schema_version().expect("v1"), SCHEMA_VERSION);
        drop(store);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn ready_blocked_transitions() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("ship it").expect("session");
        assert_eq!(store.list_sessions().expect("list").len(), 1);
        let t1 = store.create_task(&s, "first", None, &[]).expect("t1");
        assert_eq!(store.get_task(&t1).expect("g").state, TaskState::Ready);
        let t2 = store.create_task(&s, "second", None, &[&t1]).expect("t2");
        assert_eq!(store.get_task(&t2).expect("g").state, TaskState::Blocked);
        // Cannot start blocked work with incomplete parents.
        assert!(store.start_task(&t2).is_err());
        // Promotion needs the verifier gate: DoD checked + evidence.
        store.dod_add(&t1, "done").expect("dod");
        store.dod_check(&t1, 1).expect("check");
        store.evidence_add(&t1, "output").expect("ev");
        store.complete_task(&t1).expect("complete");
        assert_eq!(store.get_task(&t2).expect("g").state, TaskState::Ready);
        store.start_task(&t2).expect("start");
        assert_eq!(store.get_task(&t2).expect("g").state, TaskState::InProgress);
        // Metrics + fail path.
        store
            .record_metric(&t1, 120, 10, 20, "completed")
            .expect("metric");
        let t3 = store.create_task(&s, "third", None, &[]).expect("t3");
        store.fail_task(&t3, "boom").expect("fail");
        assert_eq!(store.get_task(&t3).expect("g").state, TaskState::Failed);
    }

    #[test]
    fn cycle_rejected() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("cycles").expect("session");
        let a = store.create_task(&s, "a", None, &[]).expect("a");
        let b = store.create_task(&s, "b", None, &[&a]).expect("b");
        // b depends on a, so a must not depend on b.
        let err = store.add_dependency(&a, &b).expect_err("cycle");
        assert!(matches!(err, AtlasError::Cycle { .. }), "{err:?}");
        // Self-edge is also a cycle.
        assert!(store.add_dependency(&a, &a).is_err());
        // Longer cycle: c -> b -> a, then a -> c rejected.
        let c = store.create_task(&s, "c", None, &[&b]).expect("c");
        assert!(store.add_dependency(&a, &c).is_err());
    }

    /// Tamaño del fixture pesado compartido por los tests de traversal.
    const BULK_N: usize = 100_000;

    /// Fixture pesado: cadena + skip-one + skip-three (~250k aristas,
    /// acíclico por construcción). Inserta por SQL directo (sin cycle-check
    /// por arista) y devuelve (store, session_id, edge_count).
    fn bulk_100k_fixture(tag: &str) -> (Store, String, i64) {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session(tag).expect("session");
        {
            let mut tstmt = store
                .conn
                .prepare(
                    "INSERT INTO tasks(id, session_id, title, state, agent, created_at, updated_at)
                     VALUES(?1, ?2, 'bulk', 'READY', NULL, 0, 0)",
                )
                .expect("prep task");
            for i in 0..BULK_N {
                tstmt
                    .execute(params![format!("n{i:06}"), s])
                    .expect("insert task");
            }
        }
        {
            let mut estmt = store
                .conn
                .prepare("INSERT OR IGNORE INTO edges(child_id, parent_id) VALUES(?1, ?2)")
                .expect("prep edge");
            for i in 1..BULK_N {
                estmt
                    .execute(params![format!("n{i:06}"), format!("n{:06}", i - 1)])
                    .expect("edge");
            }
            for i in 2..BULK_N {
                estmt
                    .execute(params![format!("n{i:06}"), format!("n{:06}", i - 2)])
                    .expect("edge");
            }
            for i in (3..BULK_N).step_by(2) {
                estmt
                    .execute(params![format!("n{i:06}"), format!("n{:06}", i - 3)])
                    .expect("edge");
            }
        }
        let edge_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .expect("count");
        assert!(edge_count >= 249_990, "edges: {edge_count}");
        (store, s, edge_count)
    }

    #[test]
    fn traversal_100k_nodes() {
        let tx_note = Instant::now();
        let (store, s, edge_count) = bulk_100k_fixture("big");
        eprintln!(
            "bulk insert 100k nodes / {edge_count} edges: {:?}",
            tx_note.elapsed()
        );
        let t0 = Instant::now();
        let order = store.traverse(&s).expect("traverse");
        let dt = t0.elapsed();
        assert_eq!(order.len(), BULK_N);
        // Chain order must be respected.
        let pos: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        assert!(pos["n000000"] < pos["n099999"]);
        eprintln!("traversal 100k nodes / {edge_count} edges: {dt:?}");
        assert!(dt.as_secs() < 5, "traversal budget exceeded: {dt:?}");
    }

    /// Presupuesto DoD: p99 < 100ms en traversal real sobre 100k/250k.
    /// Mide muestras reales de `traverse` + `ancestors` (nodo más profundo,
    /// visita todo el grafo), IMPRIME p50/p99 y falla si p99 >= 100ms.
    #[test]
    fn traversal_p50_p99_100k() {
        const TRAVERSE_SAMPLES: usize = 21;
        const ANCESTOR_SAMPLES: usize = 11;
        let (store, s, edge_count) = bulk_100k_fixture("big-p99");
        // Warmup: pagea SQLite y el allocator antes de medir.
        let warm = store.traverse(&s).expect("warmup");
        assert_eq!(warm.len(), BULK_N);
        let mut samples_ms: Vec<f64> = Vec::with_capacity(TRAVERSE_SAMPLES + ANCESTOR_SAMPLES);
        for _ in 0..TRAVERSE_SAMPLES {
            let t0 = Instant::now();
            let order = store.traverse(&s).expect("traverse");
            samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(order.len(), BULK_N);
        }
        for _ in 0..ANCESTOR_SAMPLES {
            let t0 = Instant::now();
            let anc = store.ancestors("n099999").expect("ancestors");
            samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(anc.len(), BULK_N - 1);
        }
        samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = samples_ms.len();
        let pct = |p: f64| {
            samples_ms[((p * n as f64).ceil() as usize)
                .saturating_sub(1)
                .min(n - 1)]
        };
        let (p50, p99) = (pct(0.50), pct(0.99));
        let (min, max) = (samples_ms[0], samples_ms[n - 1]);
        println!(
            "traversal 100k nodes / {edge_count} edges over {n} samples: min={min:.1}ms p50={p50:.1}ms p99={p99:.1}ms max={max:.1}ms"
        );
        eprintln!(
            "traversal 100k nodes / {edge_count} edges over {n} samples: min={min:.1}ms p50={p50:.1}ms p99={p99:.1}ms max={max:.1}ms"
        );
        assert!(
            p99 < 100.0,
            "p99 budget exceeded: p99={p99:.1}ms (p50={p50:.1}ms, max={max:.1}ms)"
        );
    }

    #[test]
    fn session_stop_resume_roundtrip() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("resume me").expect("session");
        store.create_task(&s, "work", None, &[]).expect("task");
        store.set_session_status(&s, "stopped").expect("stop");
        assert_eq!(store.get_session(&s).expect("g").status, "stopped");
        // History survives the stop: tasks and events intact.
        assert_eq!(store.list_tasks(Some(&s)).expect("t").len(), 1);
        let n: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .expect("events");
        assert!(n >= 3, "events: {n}");
        store.set_session_status(&s, "active").expect("resume");
        assert_eq!(store.get_session(&s).expect("g").status, "active");
    }

    #[test]
    fn dispatcher_unlocks_dependents_via_tick() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("chain").expect("session");
        let l3 = store.create_task(&s, "L3", None, &[]).expect("l3");
        let l4 = store.create_task(&s, "L4", None, &[&l3]).expect("l4");
        assert_eq!(store.get_task(&l4).expect("g").state, TaskState::Blocked);
        // Only READY work may start.
        assert!(store.run_task(&l4).is_err());
        store.run_task(&l3).expect("run l3");
        // Verifier gate must pass before the queued completion applies.
        store.dod_add(&l3, "done").expect("dod l3");
        store.dod_check(&l3, 1).expect("check l3");
        store.evidence_add(&l3, "output l3").expect("ev l3");
        // `complete` queues the event; state moves only on tick.
        store.finish_task(&l3, true, "").expect("finish ok");
        assert_eq!(store.get_task(&l3).expect("g").state, TaskState::InProgress);
        assert_eq!(store.get_task(&l4).expect("g").state, TaskState::Blocked);
        let sum = store.tick_once().expect("tick");
        assert_eq!(sum.unlocked, 1);
        assert_eq!(store.get_task(&l3).expect("g").state, TaskState::Completed);
        assert_eq!(store.get_task(&l4).expect("g").state, TaskState::Ready);
        // Only IN_PROGRESS tasks accept complete.
        assert!(store.finish_task(&l4, true, "").is_err());
        store.run_task(&l4).expect("run l4");
        store.dod_add(&l4, "done").expect("dod l4");
        store.dod_check(&l4, 1).expect("check l4");
        store.evidence_add(&l4, "output l4").expect("ev l4");
        store.finish_task(&l4, true, "").expect("finish l4");
        store.tick_once().expect("tick2");
        assert_eq!(store.get_task(&l4).expect("g").state, TaskState::Completed);
    }

    #[test]
    fn dispatcher_retry_then_escalates() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("flaky").expect("session");
        let t = store.create_task(&s, "flaky work", None, &[]).expect("t");
        for attempt in 1..=MAX_RETRIES {
            store.run_task(&t).expect("run");
            store.finish_task(&t, false, "boom").expect("finish fail");
            let sum = store.tick_once().expect("tick");
            let got = store.get_task(&t).expect("g");
            if attempt < MAX_RETRIES {
                assert_eq!(sum.retried, 1, "attempt {attempt}");
                assert_eq!(got.state, TaskState::Ready);
                assert_eq!(got.attempts, attempt);
            } else {
                assert_eq!(sum.escalated, 1);
                assert_eq!(got.state, TaskState::Failed);
                assert_eq!(got.attempts, MAX_RETRIES);
            }
        }
        // Terminal tasks accept neither run nor complete.
        assert!(store.run_task(&t).is_err());
        assert!(store.finish_task(&t, false, "again").is_err());
    }

    #[test]
    fn dispatcher_resume_has_no_double_run() {
        // File-backed DB so close + reopen simulates `kill -9`.
        let path = temp_path("resume");
        let _ = std::fs::remove_file(&path);
        let s;
        let l3;
        let l4;
        {
            let store = Store::open(&path).expect("open");
            s = store.create_session("crash me").expect("session");
            l3 = store.create_task(&s, "L3", None, &[]).expect("l3");
            l4 = store.create_task(&s, "L4", None, &[&l3]).expect("l4");
            store.run_task(&l3).expect("run");
            store.dod_add(&l3, "done").expect("dod");
            store.dod_check(&l3, 1).expect("check");
            store.evidence_add(&l3, "output").expect("ev");
            store.finish_task(&l3, true, "").expect("finish");
            // Duplicate emission under the same idempotency key is ignored.
            let n_before: i64 = store
                .conn
                .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
                .expect("count");
            store.finish_task(&l3, true, "").expect("dup finish");
            let n_after: i64 = store
                .conn
                .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
                .expect("count");
            assert_eq!(n_before, n_after, "idempotency key must dedupe");
            let sum = store.tick_once().expect("tick");
            assert_eq!(sum.unlocked, 1);
            assert_eq!(store.pending_count().expect("p"), 0);
            // Checkpoint persisted: cursor + full snapshot.
            let cp = store.checkpoint().expect("cp").expect("some");
            assert!(cp.last_event_id > 0);
            assert!(cp.snapshot.contains(&l3) && cp.snapshot.contains(&l4));
        }
        // "kill -9": drop without shutdown, reopen, tick again.
        let before: String;
        let cp_before: i64;
        {
            let store = Store::open(&path).expect("reopen");
            assert_eq!(store.pending_count().expect("p"), 0);
            before = format!(
                "{:?}{:?}",
                store.get_task(&l3).expect("g").state,
                store.get_task(&l4).expect("g").state
            );
            cp_before = store.checkpoint().expect("cp").expect("some").last_event_id;
            let sum = store.tick_once().expect("replay tick");
            assert_eq!(sum.processed, 0, "nothing left to replay");
            let after = format!(
                "{:?}{:?}",
                store.get_task(&l3).expect("g").state,
                store.get_task(&l4).expect("g").state
            );
            assert_eq!(before, after, "replay must not move tasks");
            assert_eq!(
                store.checkpoint().expect("cp").expect("some").last_event_id,
                cp_before,
                "checkpoint cursor must not move on empty replay"
            );
            assert_eq!(store.get_task(&l3).expect("g").attempts, 0);
        }
        let _ = s;
        std::fs::remove_file(&path).ok();
    }
}
