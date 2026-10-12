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

## Current state (Database)

The repository provides scaffolding for the Atlas task store. It does not include a live database.

When running Atlas locally, an embedded SQLite database (`atlas.db`) will be created or used to store the task DAG, execution logs, and evaluation metrics.

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

## Evaluating a session

`atlas evaluate` runs an external evaluator script and fails if it fails. There is no default: set
`ATLAS_EVALUATOR` to your evaluator (it receives `--db`, `--session` and optionally `--project`).
`scripts/synthetic_evaluator.py` is only a test stub that always succeeds.

```bash
ATLAS_EVALUATOR=path/to/evaluator.py atlas evaluate --session <id>
```

## Layout

- `src/` — Rust core (future: dag, dispatcher, verifier, estimate, serve)
- `wasm/` — WASM compute module (future: DAG traversal for Node + Workers)
- `node/` — Node wrapper for shared hosting (future: Passenger-ready app.js)
- `docs/` — BRD, user stories, SRS requirements, ADRs
- `.gitcore/` — GitCore ledger (features, manifest, issues)
