# ATLAS-02: Event dispatcher skeleton + checkpoint resume

Goal: `feat-dispatcher` minimal loop over an event queue with durable
checkpoints and idempotency keys (bus adapter stubbed for tests).

Stories: C-001, C-002, C-003, C-011 (commands), C-013 (gate hooks).

AC:
- [ ] `task_completed` unlocks dependents; `task_failed` retries bounded.
- [ ] Kill -9 mid-run resumes without double-execution (replay test).
- [ ] Only READY tasks reach the (stubbed) agent backend.

DoD: crash-recovery test green; watchdog tick covers only stale gaps.
