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

Companion private repo `atlas-saas`: managed validation/QA/deploy plane
(Astro + Svelte on Cloudflare Workers). Open core Apache-2.0; SaaS modules
under a source-available license (FSL/ELv2, pending legal review).

## 2. Scope (renamed — no "GitHub replica")

IN: agent-native minimal forge (git hosting, issues, PRs, fast agent CI,
main-only mirror to GitHub); task DAG to 100k nodes; event-driven
dispatcher with checkpoints and idempotency; two-layer verifier;
moving-average estimator; local dev dashboard; WASM+Node deploy unit
for low-spec/CPanel shared hosting; optional Xavier adapter.

OUT (explicit non-goals): full GitHub replacement (no Actions clone, no
Packages, no Pages, no social); native GUI; non-git VCS; SaaS features
in core; ML-based estimation before F2; ephemeral QA before F3.

## 3. Phases

- F0: repos, docs, backlog, governance (this commit).
- F1: DAG store, dispatcher, rule verifier, estimate basics, serve local.
- F2: history learning, LLM reviewer, event-driven over agent bus,
  forge-min loop, WASM+Node unit on CPanel.
- F3+: SaaS validation/QA/deploy (private repo).

## 4. Business model

Core free forever (Apache-2.0). Revenue from managed SaaS (hosting,
ephemeral QA infra, support, SLA). Protection: trademark + separate
SaaS repo under FSL/ELv2 + operated infrastructure as moat
(Supabase-style). Final license text pending lawyer review.
