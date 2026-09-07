# Atlas — Module tree (goals, scopes, branches)

Main feature (top): AUTONOMOUS LONG-HORIZON TASK EXECUTION.
Goal: a defined backlog runs to verified done without human babysitting,
using every united resource (agents, bus, memory, codegraph, CI, deploys).
Everything below exists only to serve this feature.

Layer order = dependency order (lower layers gate upper layers).

## L1 Foundation — sessions + persistence
Goal: durable identity of work across crashes and days.
Scope: session lifecycle, one-file SQLite (sessions/tasks/edges/events/
metrics), full CLI surface.
Stories: C-001, C-004, C-011. Feature: feat-dag-store (store half).
Branch: `mod/foundation`. Issue: ATLAS-05.

## L2 Task graph — DAG + dependencies
Goal: the project as a queryable graph; only READY work reaches agents.
Scope: pre-issue edges, six states, tree render, manual block,
100k-node traversal budget, reusable crate API.
Stories: C-005, C-012. Feature: feat-dag-store.
Branch: `mod/dag`. Issue: ATLAS-01.

## L3 Execution drive — dispatcher + delegation
Goal: events move the graph; agents are invoked, never poll for work.
Scope: recursive delegation, event reaction (completed/failed/timeout),
durable checkpoints, idempotency keys, watchdog-only cron.
Stories: C-002, C-003. Feature: feat-dispatcher.
Branch: `mod/dispatcher`. Issue: ATLAS-02.

## L4 Trust — verifier gate
Goal: COMPLETED always means verified, never declared.
Scope: rule layer (tests, artifacts, DoD), LLM reviewer for doubt,
decision + evidence stored per task.
Stories: C-013. Feature: feat-verifier.
Branch: `mod/verifier`. Issue: ATLAS-03.

## L5 Foresight — estimator
Goal: cost and duration known before big runs; drift visible after.
Scope: per-task actuals, moving averages, whole-tree rollup with
critical path, `atlas estimate`.
Stories: C-006. Feature: feat-estimate.
Branch: `mod/estimate`. Issue: ATLAS-06.

## L6 Visibility — local dashboard
Goal: live state without production infra (dev only).
Scope: `atlas serve`, single-file HTML, websocket updates.
Stories: C-007. Feature: feat-serve-local.
Branch: `mod/serve`. Issue: ATLAS-07.

## L7 Forge — minimal agent forge + main-only deploy
Goal: daily agent throughput unthrottled; GitHub receives main only.
Scope: git hosting, issues/PRs API, fast CI profile, main-only mirror,
`atlas deploy --target vps|cloudrun` with pre-gates + rollback.
Stories: C-008, C-009. Feature: feat-forge-min.
Branch: `mod/forge`. Issue: ATLAS-08.

## L8 Intelligence — Xavier adapter (optional)
Goal: plans grounded on real code, docs, and history.
Scope: runtime detection, backlog drafting via CodeGraph+RAG,
explicit degraded mode without Xavier.
Stories: C-010. Feature: feat-xavier-adapter.
Branch: `mod/xavier-adapter`. Issue: ATLAS-09.

## L9 Distribution — WASM+Node unit
Goal: Atlas runs on shared/CPanel and low-spec boxes as the free tier.
Scope: WASM compute module, thin Node wrapper, Node-side SQLite,
parity native/WASM/Workers, memory budget.
Stories: C-014. Feature: feat-wasm-node.
Branch: `mod/wasm-node`. Issue: ATLAS-04.

## L10 SaaS plane (private repo)
Goal: managed validation/QA/deploy for customers.
Scope: auth+CLI link, remote validation, ephemeral envs, assisted QA,
external feedback, enterprise dashboard. F3 only.
Stories: S-001..S-005. Features: feat-saas-*.
Branches: `saas/auth`, `saas/validate`, `saas/qa`, `saas/feedback`,
`saas/dashboard`. Issues: SAAS-01 (auth+link), SAAS-02 (validate pipe).

## Branching rule

One module = one branch from `main`, named as above; one issue per
module; merge only through verifier gate + green tests. Submodules
branch off their module branch (`mod/dag/storage`, never off main).
