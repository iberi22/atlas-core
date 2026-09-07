# ADR-003: Boundary — Gestalt executes, Atlas governs

Status: accepted. Date: 2026-09-07.

Context: Centralizing all agent tasks is the goal, but fusing the
backlog into the executor would couple planning to one harness and
break the harness-agnostic requirement.

Decision: Gestalt owns execution (runs, VFS, bus, merge). Atlas owns
governance (DAG, backlog, estimates, verification). Integration is a
library dependency: Atlas consumes the agent event bus and state
(e.g. `gestalt-state` crate API), never a repo merge. No task state
is authoritative inside the executor.

Consequences: either side stays replaceable; the bus contract is the
integration surface and must stay stable and versioned.
