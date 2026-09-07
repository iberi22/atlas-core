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
- Private SaaS plane (managed validation, QA, deploy service): separate repo.

## Layout

- `src/` — Rust core (future: dag, dispatcher, verifier, estimate, serve)
- `wasm/` — WASM compute module (future: DAG traversal for Node + Workers)
- `node/` — Node wrapper for shared hosting (future: Passenger-ready app.js)
- `docs/` — BRD, user stories, SRS requirements, ADRs
- `.gitcore/` — GitCore ledger (features, manifest, issues)
