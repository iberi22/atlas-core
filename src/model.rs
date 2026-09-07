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
