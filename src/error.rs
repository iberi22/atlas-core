// Error type for the Atlas DAG store and CLI.
use thiserror::Error;

/// All recoverable failures produced by the store layer.
#[derive(Debug, Error)]
pub enum AtlasError {
    /// Underlying SQLite failure.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    /// A session, task, or edge target does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// Adding the edge would close a dependency cycle.
    #[error("cycle rejected: task {child} already reaches {parent}")]
    Cycle { child: String, parent: String },
    /// Unknown or unexpected task state value.
    #[error("invalid state: {0}")]
    InvalidState(String),
    /// The requested state transition is not allowed.
    #[error("invalid transition: {0}")]
    InvalidTransition(String),
}

/// Store-level result alias.
pub type Result<T> = std::result::Result<T, AtlasError>;
