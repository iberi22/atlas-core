# Atlas Core — User Stories (complete, F1-F2)

Format: As / I want / So that + Acceptance Criteria + Priority.
IDs map to `.gitcore/features.json`.

## C-001: Start orchestration session (feat-dispatcher)

As an AI developer,
I want to start a session with `atlas start --goal "..."`,
So that agents can coordinate on a long-horizon objective.

AC:
- [ ] Session row created in SQLite with unique ID + started_at.
- [ ] `--agent` flag selects CLI backends (default: configured chain).
- [ ] Initial task graph derived from goal (manual seed or Xavier draft).
- [ ] `atlas status` shows session state.

Priority: High.

## C-002: Recursive delegation (feat-dispatcher)

As an orchestrator agent,
I want to delegate subtasks to CLI agents including myself (`atlas delegate`),
So that complex work splits into manageable units.

AC:
- [ ] Delegation creates a child node with parent edge + dependency list.
- [ ] Child receives resolved context (goal slice, relevant history, DoD).
- [ ] Max nesting depth configurable; cycles rejected with clear error.
- [ ] Every delegation emits an event with idempotency key.

Priority: High.

## C-003: Event-driven progress (feat-dispatcher)

As the system,
I want task state to advance on bus events (completed/failed/timeout),
So that blocked tasks unlock immediately without polling.

AC:
- [ ] `task_completed` unlocks dependents whose pre-issues are all done.
- [ ] `task_failed` triggers retry policy (bounded) or escalation.
- [ ] Crash mid-run resumes from last checkpoint; no double-execution
      (verified by idempotency-key replay test).
- [ ] A slow watchdog tick only covers gaps (>N min unprocessed), never
      executes primary work.

Priority: High.

## C-004: Durable SQLite persistence (feat-dag-store)

As a user,
I want sessions, history, DAG, events, and metrics in one SQLite file,
So that work survives restarts and is fully traceable.

AC:
- [ ] Schema: sessions, tasks, edges, events (append-only), metrics.
- [ ] DB auto-created at configured path; WAL mode; migrations versioned.
- [ ] Every agent interaction logged with timestamp + model + backend.

Priority: High.

## C-005: Pre-issue dependencies + blocked states (feat-dag-store)

As a planner,
I want `atlas task create --depends-on <id>` and states
PENDING/BLOCKED/READY/IN_PROGRESS/COMPLETED/FAILED,
So that only READY tasks reach agents.

AC:
- [ ] `atlas tree` renders the DAG with states in terminal.
- [ ] `atlas task block <id>` manual override with reason recorded.
- [ ] Dispatcher assigns agents exclusively from READY queue.
- [ ] 100k-node traversal benchmark <100ms (CI gate).

Priority: High.

## C-006: Time/token estimates (feat-estimate)

As a user,
I want `atlas estimate` before a big run,
So that cost and duration are known upfront.

AC:
- [ ] Per-task actuals (duration, prompt/completion tokens, outcome) recorded.
- [ ] Estimates from moving averages over same-kind history + sane defaults.
- [ ] Whole-tree rollup respecting dependencies (critical path).
- [ ] Estimate-vs-actual drift visible per task.

Priority: Medium.

## C-007: Local dev dashboard (feat-serve-local)

As a user,
I want `atlas serve` with a single-file HTML panel (tasks, chat, history),
So that live state is visible without production infra.

AC:
- [ ] One HTML file, no external assets; borderless single-tone style.
- [ ] Live updates over websocket; no page reload.
- [ ] Dev-only: refuses to bind public interfaces without explicit flag.

Priority: Medium.

## C-008: Main-only deploys (feat-forge-min)

As a DevOps user,
I want `atlas deploy --target vps|cloudrun` from `main` only,
So that delivery is continuous and safe.

AC:
- [ ] Build + fast tests + verifier gate run before any deploy.
- [ ] Non-main branch rejected with clear error.
- [ ] Target credentials from environment only; rollback on failure.

Priority: Medium.

## C-009: Minimal agent forge (feat-forge-min)

As a developer,
I want git hosting + issues + PRs + fast CI tuned for agents,
So that daily agent throughput is not throttled by heavy pipelines.

AC:
- [ ] Issue/PR lifecycle API usable by agents (create, comment, merge).
- [ ] Fast CI profile (<5 min) for agent PRs; heavy profile opt-in.
- [ ] Main-only mirror push to GitHub preserves code history.
- [ ] Explicit docs list what the forge is NOT (see BRD non-goals).

Priority: Medium (F2).

## C-010: Xavier adapter, optional but suggested (feat-xavier-adapter)

As a power user with Xavier,
I want CodeGraph + RAG + clean session history for backlog drafting,
So that plans ground on real code and docs.

AC:
- [ ] Runtime detection; capabilities announced at startup.
- [ ] Draft backlog generation from CodeGraph + docs RAG.
- [ ] Full core suite passes with adapter disabled (degraded mode).

Priority: Medium.

## C-011: Complete CLI (feat-dispatcher)

As a power user,
I want start/stop/status/delegate/list/tree/estimate/deploy/serve
with `--help`, `--verbose`, `--config`, and `--json` output,
So that terminal-only and scripted operation both work.

Priority: Medium.

## C-012: Reusable Rust library (feat-dag-store)

As a Rust developer,
I want Atlas core as a crate with documented API and minimal deps,
So that orchestration embeds in other tools.

AC:
- [ ] Published crate path with examples; semver discipline.
- [ ] No `unsafe` without documented justification; no async blocking.

Priority: Low (F2).

## C-013: Verifier gate (feat-verifier)

As the system,
I want rule checks first and an LLM reviewer for doubtful cases,
So that COMPLETED always means verified.

AC:
- [ ] Rule layer: tests green, artifacts exist, DoD checklist satisfied.
- [ ] Doubtful results routed to LLM reviewer with the task's AC text.
- [ ] Promotion decision + evidence stored on the task node.

Priority: High.

## C-014: WASM+Node deploy unit (feat-wasm-node)

As a user with shared hosting,
I want the compute module as `.wasm` plus a thin Passenger-ready
Node wrapper with Node-side SQLite,
So that Atlas runs on CPanel shared servers and low-spec boxes.

AC:
- [ ] Identical traversal results in Node and in Cloudflare Workers.
- [ ] No compiler needed on the server (prebuilt artifacts only).
- [ ] Memory footprint documented; fits <512MB boxes.

Priority: Medium (F2).
