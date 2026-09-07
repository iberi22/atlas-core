// Atlas core library: SQLite task-DAG store plus record shapes.
// The `atlas` binary (src/main.rs) is a thin CLI over this API (REQ-F-007).
pub mod dag;
mod error;
pub mod estimator;
pub mod forge;
pub mod model;
pub mod serve;
pub mod store;
pub mod verifier;
pub mod xavier;

pub use error::{AtlasError, Result};
pub use model::{
    Checkpoint, CiRecord, DodItem, Event, EvidenceItem, ForgeIssue, ForgePr, Session, Task,
    TaskNode, TaskState, TickSummary, VerifyReport,
};
pub use store::{SCHEMA_VERSION, Store};
pub use verifier::{CommandReviewer, ReviewDecision, Reviewer, StubReviewer};
