// Atlas core library: SQLite task-DAG store plus record shapes.
// The `atlas` binary (src/main.rs) is a thin CLI over this API (REQ-F-007).
mod error;
pub mod model;
pub mod store;

pub use error::{AtlasError, Result};
pub use model::{Session, Task, TaskNode, TaskState};
pub use store::{SCHEMA_VERSION, Store};
