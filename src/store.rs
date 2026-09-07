// SQLite-backed task-DAG store: versioned schema plus graph operations.
// Single-file DB (WAL), append-only events, cycle-safe edges (REQ-F-002/004).
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AtlasError;
pub use crate::error::Result;
use crate::model::{
    Checkpoint, DodItem, Event, EvidenceItem, Session, Task, TaskState, TickSummary,
};

/// Current schema revision tracked in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i32 = 3;

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
}

impl Store {
    /// Open (creating) the DB file and run pending migrations.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // WAL is a no-op on `:memory:`; failures there are safe to ignore.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// In-memory DB, used by unit tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
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
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// Drop every table and reset the schema version (tests only).
    pub fn migrate_down(&self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS verify_decisions;
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
    fn would_cycle(&self, child: &str, parent: &str) -> Result<bool> {
        if child == parent {
            return Ok(true);
        }
        let hit: Option<i64> = self
            .conn
            .query_row(
                "WITH RECURSIVE anc(id) AS (
                     SELECT parent_id FROM edges WHERE child_id = ?1
                     UNION
                     SELECT e.parent_id FROM edges e JOIN anc a ON e.child_id = a.id
                 )
                 SELECT 1 FROM anc WHERE id = ?2 LIMIT 1",
                params![parent, child],
                |r| r.get(0),
            )
            .optional()?;
        Ok(hit.is_some())
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
        self.require_task_row(task_id)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO metrics(task_id, duration_ms, prompt_tokens, completion_tokens, outcome)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![task_id, duration_ms, prompt_tokens, completion_tokens, outcome],
        )?;
        Ok(())
    }

    // -- traversal ---------------------------------------------------------

    /// Topological order of one session's tasks (Kahn's algorithm, REQ-F-006).
    pub fn traverse(&self, session_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM tasks WHERE session_id = ?1")?;
        let ids: Vec<String> = stmt
            .query_map(params![session_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut index: HashMap<&str, usize> = HashMap::with_capacity(ids.len());
        for (i, id) in ids.iter().enumerate() {
            index.insert(id.as_str(), i);
        }
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
        let mut indegree = vec![0u32; ids.len()];
        let mut estmt = self.conn.prepare(
            "SELECT e.child_id, e.parent_id FROM edges e
             JOIN tasks c ON c.id = e.child_id JOIN tasks p ON p.id = e.parent_id
             WHERE c.session_id = ?1 AND p.session_id = ?1",
        )?;
        let rows = estmt.query_map(params![session_id], |r| {
            let c: String = r.get(0)?;
            let p: String = r.get(1)?;
            Ok((c, p))
        })?;
        for r in rows {
            let (c, p) = r?;
            if let (Some(&ci), Some(&pi)) = (index.get(c.as_str()), index.get(p.as_str())) {
                children[pi].push(ci);
                indegree[ci] += 1;
            }
        }
        let mut queue: VecDeque<usize> = indegree
            .iter()
            .enumerate()
            .filter(|(_, d)| **d == 0)
            .map(|(i, _)| i)
            .collect();
        let mut order = Vec::with_capacity(ids.len());
        while let Some(n) = queue.pop_front() {
            order.push(ids[n].clone());
            for &m in &children[n] {
                indegree[m] -= 1;
                if indegree[m] == 0 {
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

    #[test]
    fn traversal_100k_nodes() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("big").expect("session");
        const N: usize = 100_000;
        // Bulk insert: chain + skip-one + skip-three edges (~250k), acyclic by construction.
        let tx_note = Instant::now();
        {
            let mut tstmt = store
                .conn
                .prepare(
                    "INSERT INTO tasks(id, session_id, title, state, agent, created_at, updated_at)
                     VALUES(?1, ?2, 'bulk', 'READY', NULL, 0, 0)",
                )
                .expect("prep task");
            for i in 0..N {
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
            for i in 1..N {
                estmt
                    .execute(params![format!("n{i:06}"), format!("n{:06}", i - 1)])
                    .expect("edge");
            }
            for i in 2..N {
                estmt
                    .execute(params![format!("n{i:06}"), format!("n{:06}", i - 2)])
                    .expect("edge");
            }
            for i in (3..N).step_by(2) {
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
        eprintln!(
            "bulk insert 100k nodes / {edge_count} edges: {:?}",
            tx_note.elapsed()
        );
        let t0 = Instant::now();
        let order = store.traverse(&s).expect("traverse");
        let dt = t0.elapsed();
        assert_eq!(order.len(), N);
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
