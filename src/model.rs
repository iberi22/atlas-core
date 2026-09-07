// Record shapes and task states for the Atlas DAG store.
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::{AtlasError, Result};

/// Lifecycle states of a task node (REQ-F-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Pending,
    Blocked,
    Ready,
    InProgress,
    Completed,
    Failed,
}

impl TaskState {
    /// Canonical uppercase name stored in SQLite.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Blocked => "BLOCKED",
            Self::Ready => "READY",
            Self::InProgress => "IN_PROGRESS",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
        }
    }

    /// True once the task needs no further work.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskState {
    type Err = AtlasError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "PENDING" => Ok(Self::Pending),
            "BLOCKED" => Ok(Self::Blocked),
            "READY" => Ok(Self::Ready),
            "IN_PROGRESS" => Ok(Self::InProgress),
            "COMPLETED" => Ok(Self::Completed),
            "FAILED" => Ok(Self::Failed),
            other => Err(AtlasError::InvalidState(other.to_owned())),
        }
    }
}

/// A durable unit of work identity (REQ-F-001).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub goal: String,
    pub started_at: i64,
    pub status: String,
}

/// A single DAG node (REQ-F-004).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub state: TaskState,
    pub agent: Option<String>,
    /// Consecutive failure count driving bounded retry (REQ-F-009).
    pub attempts: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A task node with its rendered children, used by `tree --json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub title: String,
    pub state: TaskState,
    pub children: Vec<TaskNode>,
}

/// One row of the append-only event log (REQ-F-002/009).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub kind: String,
    pub payload: String,
    pub idempotency_key: Option<String>,
    pub processed: bool,
}

/// Durable dispatcher checkpoint: last consumed event plus a full
/// task-state snapshot, resumable after `kill -9` (REQ-F-010).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub last_event_id: i64,
    pub snapshot: String,
    pub updated_at: i64,
}

/// Outcome of one `tick --once` pass over queued events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TickSummary {
    pub processed: usize,
    pub unlocked: usize,
    pub retried: usize,
    pub escalated: usize,
    pub skipped: usize,
    pub last_event_id: i64,
}

/// One Definition-of-Done checklist row owned by a task (REQ-F-013).
/// Items are numbered per task starting at 1; promotion requires every
/// item to exist-checked, i.e. no unchecked rows remain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DodItem {
    pub task_id: String,
    pub n: i64,
    pub text: String,
    pub checked: bool,
}

/// One free-form evidence string attached to a task (REQ-F-012).
/// Promotion requires at least one row per task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub id: i64,
    pub task_id: String,
    pub text: String,
    pub created_at: i64,
}

/// Rule-check outcome for one task (REQ-F-012/013).
/// `passed` depends ONLY on the rule layer; the reviewer verdict is
/// advisory and recorded here without blocking promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    pub task_id: String,
    pub passed: bool,
    pub failures: Vec<String>,
    pub dod_total: i64,
    pub dod_checked: i64,
    pub evidence_count: i64,
    pub reviewer: String,
}

/// Lifecycle of a forge issue (REQ-F-017): OPEN until closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueState {
    Open,
    Closed,
}

impl IssueState {
    /// Canonical uppercase name stored in SQLite.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Closed => "CLOSED",
        }
    }
}

impl fmt::Display for IssueState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IssueState {
    type Err = AtlasError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "OPEN" => Ok(Self::Open),
            "CLOSED" => Ok(Self::Closed),
            other => Err(AtlasError::InvalidState(other.to_owned())),
        }
    }
}

/// Lifecycle of a forge pull request (REQ-F-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

impl PrState {
    /// Canonical uppercase name stored in SQLite.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Merged => "MERGED",
            Self::Closed => "CLOSED",
        }
    }
}

impl fmt::Display for PrState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PrState {
    type Err = AtlasError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "OPEN" => Ok(Self::Open),
            "MERGED" => Ok(Self::Merged),
            "CLOSED" => Ok(Self::Closed),
            other => Err(AtlasError::InvalidState(other.to_owned())),
        }
    }
}

/// One forge issue row (REQ-F-017), stored in the same SQLite DB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeIssue {
    pub id: String,
    pub title: String,
    pub body: String,
    pub state: IssueState,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One forge pull request row (REQ-F-017): `branch` must name a real
/// local git branch (validated with `git rev-parse --verify` at create).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgePr {
    pub id: i64,
    pub title: String,
    pub base: String,
    pub branch: String,
    pub state: PrState,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Fast-CI evidence attached to a PR (REQ-F-017): profile is `fast`
/// (`cargo test --offline` + `cargo fmt --check`); `passed` gates deploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiRecord {
    pub id: i64,
    pub pr_id: i64,
    pub head_sha: String,
    pub profile: String,
    pub passed: bool,
    pub evidence: String,
    pub created_at: i64,
}

/// Deploy target for `atlas deploy` (REQ-F-018).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeployTarget {
    Vps,
    Cloudrun,
}

impl fmt::Display for DeployTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vps => f.write_str("vps"),
            Self::Cloudrun => f.write_str("cloudrun"),
        }
    }
}

impl FromStr for DeployTarget {
    type Err = AtlasError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "vps" => Ok(Self::Vps),
            "cloudrun" => Ok(Self::Cloudrun),
            other => Err(AtlasError::Forge(format!(
                "unknown deploy target '{other}' (want vps|cloudrun)"
            ))),
        }
    }
}
