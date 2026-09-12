# Atlas — Business Requirements (BRD)

## 1. Executive summary

Atlas is a harness-agnostic governance plane for long-horizon AI-agent work.
Story: any CLI agent (Hermes or others) can call itself and sibling agents
recursively as subagents, with durable sessions, a dependency-aware task DAG
(pre-issues), time/token estimates from history, and completion that is
verified, never declared. A dev-only local dashboard (single HTML file over
websocket) shows live state. An optional Xavier adapter enriches planning
with CodeGraph, RAG, and session history. Packaged as a reusable Rust
core (+ WASM compute module + thin Node wrapper for shared hosting).

## 2. Scope

IN: agent-native minimal forge (git hosting, issues, PRs, fast agent CI,
main-only mirror to GitHub); task DAG to 100k nodes; event-driven
dispatcher with checkpoints and idempotency; two-layer verifier;
moving-average estimator; local dev dashboard; WASM+Node deploy unit
for low-spec/CPanel shared hosting; optional Xavier adapter.

OUT (explicit non-goals): full GitHub replacement (no Actions clone, no
Packages, no Pages, no social); native GUI; non-git VCS; ML-based
estimation before F2; managed hosting infrastructure.

## 3. Phases

- F0: repos, docs, backlog, governance (this commit).
- F1: DAG store, dispatcher, rule verifier, estimate basics, serve local.
- F2: history learning, LLM reviewer, event-driven over agent bus,
  forge-min loop, WASM+Node unit on CPanel.
- F3+: multi-agent swarm governance, distributed DAG traversal and federated execution.

## 4. License

Atlas Core is free and open source under the Apache-2.0 license
(permissive with express patent grant).
