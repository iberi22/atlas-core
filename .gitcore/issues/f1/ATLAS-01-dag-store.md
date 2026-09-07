# ATLAS-01: DAG store schema + 100k traversal benchmark

Goal: `feat-dag-store` skeleton — tables sessions/tasks/edges/events/metrics
with versioned migration, WAL mode, and a benchmark gate.

Stories: C-004, C-005, C-012 (library shape).

AC:
- [ ] `cargo test` passes: migrate up/down on temp DB.
- [ ] Benchmark: 100k-node + 250k-edge traversal p99 <100ms (CI gate).
- [ ] No `unsafe`, no network, single-file DB only.

DoD: benchmark runs in CI and fails the build over budget.
