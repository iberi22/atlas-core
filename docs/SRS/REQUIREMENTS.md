# Atlas Core — Software Requirements Specification (IEEE 830 reduced)

Version: F0. Status: approved for F1 build. Main feature: autonomous
long-horizon task execution (backlog → verified done, no babysitting).

## 1. Functional requirements

### L1 Foundation
- REQ-F-001: The system SHALL create a session (`atlas start --goal`)
  with unique ID + timestamp persisted in SQLite.
- REQ-F-002: The system SHALL persist sessions, tasks, edges, events
  (append-only), and metrics in ONE SQLite file (WAL, versioned migrations).
- REQ-F-003: The CLI SHALL provide start/stop/status/delegate/list/tree/
  estimate/deploy/serve with `--help`, `--verbose`, `--config`, `--json`.

### L2 Task graph
- REQ-F-004: The system SHALL model tasks as a DAG with pre-issue edges
  and states PENDING/BLOCKED/READY/IN_PROGRESS/COMPLETED/FAILED.
- REQ-F-005: The system SHALL expose `task create --depends-on`,
  `task block <id>` (with reason), and `tree` render.
- REQ-F-006: Traversal over 100k nodes + 250k edges SHALL complete p99
  <100ms (CI benchmark gate).
- REQ-F-007: The core SHALL be usable as a Rust library with documented
  API, minimal deps, no `unsafe` without justification.

### L3 Execution drive
- REQ-F-008: The system SHALL support recursive delegation to CLI agents
  (including self), with bounded depth and cycle rejection.
- REQ-F-009: `task_completed` events SHALL unlock dependents whose
  pre-issues are all done; `task_failed` SHALL follow bounded retry policy.
- REQ-F-010: After a crash, the system SHALL resume from the last
  checkpoint without double-execution (idempotency keys).
- REQ-F-011: Cron/polling SHALL be watchdog-only (stale-gap coverage),
  never the primary execution loop.

### L4 Trust
- REQ-F-012: No task SHALL reach COMPLETED by agent declaration alone;
  only the verifier promotes, storing decision + evidence on the node.
- REQ-F-013: The verifier SHALL run rule checks first (tests, artifacts,
  DoD) and route doubtful cases to an LLM reviewer.

### L5 Foresight
- REQ-F-014: The system SHALL record per-task actuals (duration, tokens,
  outcome) and estimate via moving averages with sane defaults.
- REQ-F-015: `atlas estimate` SHALL roll up the whole tree respecting
  dependencies (critical path) and show estimate-vs-actual drift.

### L6 Visibility
- REQ-F-016: `atlas serve` SHALL render a single-file HTML dashboard
  (tasks, history) with websocket live updates, dev-only bind by default.

### L7 Forge
- REQ-F-017: The forge SHALL offer git hosting + issue/PR API + fast CI
  profile for agent PRs, and mirror `main` only to GitHub.
- REQ-F-018: `atlas deploy --target vps|cloudrun` SHALL run build +
  fast tests + verifier gate first, reject non-main branches, and
  support rollback. Credentials from environment only.

### L8 Intelligence (optional)
- REQ-F-019: The system SHALL detect Xavier at runtime, use CodeGraph +
  RAG + session history for backlog drafting when present, and pass the
  full core suite with the adapter disabled (degraded mode).

### L9 Distribution
- REQ-F-020: Pure-compute logic SHALL compile to `.wasm`
  (wasm32-unknown-unknown, nodejs target) with byte-identical traversal
  results vs native; rusqlite stays out of the WASM boundary (JSON I/O).
- REQ-F-021: A thin Node wrapper SHALL run the module under Passenger-style
  shared hosting with Node-side SQLite; no compiler needed on servers;
  memory budget documented (<512MB target).

## 2. Non-functional requirements

- REQ-NF-001: Offline-first; core never requires network except remote sync.
- REQ-NF-002: Harness-agnostic; no hardcoded agent names, paths, or ports.
- REQ-NF-003: License Apache-2.0; no BSL/GPL deps in core dependency tree.
- REQ-NF-004: `cargo fmt` clean; `clippy --all-targets -- -D warnings` clean.
- REQ-NF-005: One PR = one feature id; features.json promoted by green runs only.

## 3. Traceability matrix (REQ → story → feature → issue → branch)

| REQ | Story | Feature | Issue | Branch |
|-----|-------|---------|-------|--------|
| F-001..003 | C-001 C-004 C-011 | feat-dag-store | ATLAS-05 | mod/foundation |
| F-004..007 | C-005 C-012 | feat-dag-store | ATLAS-01 | mod/dag |
| F-008..011 | C-002 C-003 | feat-dispatcher | ATLAS-02 | mod/dispatcher |
| F-012..013 | C-013 | feat-verifier | ATLAS-03 | mod/verifier |
| F-014..015 | C-006 | feat-estimate | ATLAS-06 | mod/estimate |
| F-016 | C-007 | feat-serve-local | ATLAS-07 | mod/serve |
| F-017..018 | C-008 C-009 | feat-forge-min | ATLAS-08 | mod/forge |
| F-019 | C-010 | feat-xavier-adapter | ATLAS-09 | mod/xavier-adapter |
| F-020..021 | C-014 | feat-wasm-node | ATLAS-04 | mod/wasm-node |
