// SQLite-backed task-DAG store: versioned schema plus graph operations.
// Single-file DB (WAL), append-only events, cycle-safe edges (REQ-F-002/004).
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AtlasError;
pub use crate::error::Result;
use crate::model::{Session, Task, TaskState};

/// Current schema revision tracked in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i32 = 1;

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
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS metrics(
    task_id TEXT PRIMARY KEY REFERENCES tasks(id),
    duration_ms INTEGER NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    outcome TEXT NOT NULL
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
        if version < SCHEMA_VERSION {
            self.conn.execute_batch(SCHEMA_UP)?;
            self.conn
                .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    /// Drop every table and reset the schema version (tests only).
    pub fn migrate_down(&self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS metrics;
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
                "SELECT id, session_id, title, state, agent, created_at, updated_at
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
                        created_at: r.get(5)?,
                        updated_at: r.get(6)?,
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
        let sql = "SELECT id, session_id, title, state, agent, created_at, updated_at
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
            "SELECT t.id, t.session_id, t.title, t.state, t.agent, t.created_at, t.updated_at
             FROM tasks t JOIN edges e ON e.parent_id = t.id WHERE e.child_id = ?1",
        )?;
        let rows = stmt.query_map(params![child], task_from_row)?;
        rows.collect::<std::result::Result<Vec<Task>, _>>()
            .map_err(AtlasError::Db)
    }

    pub fn children_of(&self, parent: &str) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.session_id, t.title, t.state, t.agent, t.created_at, t.updated_at
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

    /// Move READY (or BLOCKED with complete parents) to IN_PROGRESS.
    pub fn start_task(&self, id: &str) -> Result<Task> {
        let task = self.require_task_row(id)?;
        match task.state {
            TaskState::Ready | TaskState::Pending => self.set_state(id, TaskState::InProgress)?,
            TaskState::Blocked if self.incomplete_parents(id)? == 0 => {
                self.set_state(id, TaskState::InProgress)?;
            }
            other => {
                return Err(AtlasError::InvalidTransition(format!(
                    "task {id} is {other}, only READY work can start"
                )));
            }
        }
        self.record_event("task_started", &format!("{{\"id\":\"{id}\"}}"))?;
        self.get_task(id)
    }

    /// Mark COMPLETED and unlock dependents whose parents are all done.
    pub fn complete_task(&self, id: &str) -> Result<Task> {
        self.require_task_row(id)?;
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

fn task_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let state: String = r.get(3)?;
    Ok(Task {
        id: r.get(0)?,
        session_id: r.get(1)?,
        title: r.get(2)?,
        state: state.parse().unwrap_or(TaskState::Pending),
        agent: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
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
}
