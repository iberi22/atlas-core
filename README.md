# Atlas Core

Harness-agnostic long-horizon task orchestration for AI coding agents.
Apache-2.0 open core of the Atlas system.

Atlas governs the backlog (task DAG with pre-issue dependencies), dispatches
work to any CLI agent (Hermes, Claude Code, opencode, others), verifies
completion with teeth (rule-based checks + LLM review), and learns time/token
costs from history. Executors run the work (e.g. Gestalt); Atlas decides what
is ready, what is blocked, and what counts as done.

- Status: F0 scaffolding. See `docs/BRD.md`, `docs/USER_STORIES.md`, `docs/adr/`.
- Source of truth for progress: `.gitcore/features.json` (promoted only by
  green verification runs, never by hand).

## Current state (live `atlas.db`, measured 2026-09-24)

Row counts queried directly from the production SQLite store
(`PRAGMA user_version = 5`):

- tasks: 900 (READY 736, COMPLETED 105, IN_PROGRESS 51, BLOCKED 7, FAILED 1)
- edges: 765
- dod_items: 2943
- evidence: 1005
- verify_decisions: 302

(Counts move as the live DB evolves; re-query with
`SELECT COUNT(*) FROM <table>` to refresh.)

## Task-state normalization migration (schema v5 -> v6)

`atlas.db` historically held two spellings for the same state (`"Ready"`
next to `"READY"`, `"InProgress"` next to `"IN_PROGRESS"`) because no
constraint ever enforced `TaskState::as_str()`'s canonical vocabulary.
Branch `fix/normalize-task-state-schema` (`src/store.rs`,
`SCHEMA_VERSION 5 -> 6`) fixes this:

1. Normalizes existing rows (`Ready` -> `READY`, `InProgress` ->
   `IN_PROGRESS`).
2. Rebuilds `tasks` (SQLite has no `ALTER TABLE ... ADD CONSTRAINT`) with
   `CHECK(state IN ('BLOCKED','READY','IN_PROGRESS','COMPLETED','FAILED'))`
   so the duplicate spellings cannot recur.

Validated against a copy of production `atlas.db` (row counts match
pre/post migration; the CHECK rejects invalid values). The branch is not
merged yet, so the live DB is still `user_version = 5` with the data
normalization applied out of band (all states now canonical uppercase).

## Layout

- `src/` — Rust core (future: dag, dispatcher, verifier, estimate, serve)
- `wasm/` — WASM compute module (future: DAG traversal for Node + Workers)
- `node/` — Node wrapper for shared hosting (future: Passenger-ready app.js)
- `docs/` — BRD, user stories, SRS requirements, ADRs
- `.gitcore/` — GitCore ledger (features, manifest, issues)
