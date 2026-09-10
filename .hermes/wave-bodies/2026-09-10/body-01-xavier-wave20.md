# [Wave-20.Resto] Xavier remainder issues 2019-2024 — Core logic, WASM, adapters, CLI, benches, protocol docs

> H5-RETRY-BODIES dispatch copy (2026-09-10). Canonical body: `/home/belal/proyectosSWAL/apps/xavier/.hermes/wave-bodies/2026-09-10/body-wave-20-resto.md`.
> Matrix ref: atlas-core `.hermes/wave-bodies/2026-09-10/MATRICES.md` — slots X1, X2, X3, X4, X5, X6.
> Wave-20 remainder — [Core/Infra].
> Labels: `wave-20`, `ola20` (NO `jules` label — add only after pre-dispatch verification, per Platinum Rule).
> Issues covered: 2019, 2020, 2021, 2022, 2023, 2024 (6 micro-issues, one per slot below).

---

## Current State (MEASURABLE)

- Ledger: `.gitcore/features.json` — `total_features=53`, `features_stable=53`, `overall_progress_pct=100.0`, `last_verified=2026-09-05` (verified with `python3 -c "import json;print(json.load(open('.gitcore/features.json'))['metadata'])"`).
- Branch: `fix/mcp-ci-green-20` (verified with `git branch --show-current`); HEAD `3b169776 ci(coverage): report-only until llvm-profparser skew resolved` (verified with `git log --oneline -3`).
- Crates: `crates/xavier-core-logic/` (`Cargo.toml` + `src/`), `crates/xavier-wasm/` (`Cargo.toml` + `src/`) (verified with `ls crates/`).
- `src/` domains (verified with `ls src/`, first 20): `a2a, adapters, agents, api, app, auth2, auto_improvement, billing, bin, checkpoint, chronicle, clavis, cli, codebase, consistency, consolidation, context, coordination, crypto, curation, ...`.
- Dirty state (verified with `git status --porcelain`): `M .gitcore/features.json`, `?? src/adapters/inbound/gestalt/` (untracked gestalt adapter scaffold from a parallel session).
- Scope of this batch: remainder work items 2019-2024 (post-coverage-CI fixes, gestalt adapter completion, WASM/core-logic hardening). None of them may touch the ledger.
- Retry note: this file is the atlas-core H5-RETRY copy; the canonical body above holds the dispatch version. Slot paths are identical.

## Desired State (DELTA)

- **Slot X1 (issue 2019)**: Harden `crates/xavier-core-logic/src/` — finish pending logic, unit tests alongside.
- **Slot X2 (issue 2020)**: Harden `crates/xavier-wasm/src/` — WASM bindings parity + tests.
- **Slot X3 (issue 2021)**: Complete untracked `src/adapters/inbound/gestalt/` scaffold (track files, wire into adapter registry, no other adapter touched).
- **Slot X4 (issue 2022)**: CLI follow-ups in `src/cli/` (subcommands/flags only, no new binary).
- **Slot X5 (issue 2023)**: Criterion benches in `benches/` covering core-logic hot paths (no library code changes).
- **Slot X6 (issue 2024)**: Wave protocol docs in `docs/protocol/` (docs only, zero code).

## 🌐 Web Research Required

**MANDATORY — 4-6 queries. The agent MUST research before implementing.**

1. search: "wasm-bindgen Rust 2026 best practices crate structure"
2. search: "criterion.rs benchmark hot path 2026 setup"
3. search: "tokio spawn_blocking rayon par_iter rule rust"
4. search: "thiserror vs anyhow library binary convention 2026"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the queries above — note current best practices.
2. Read and understand these existing files:
   - `crates/xavier-core-logic/src/lib.rs` — note the lib error-handling pattern (thiserror vs anyhow)
   - `src/cli/` — note the existing subcommand pattern before adding flags
   - `scripts/verify-pipeline.sh` — understand how the feature pipeline judges done
3. Run `ls` on your slot paths FIRST (see Anti-Hallucination Guard).
4. Document your findings in the issue before writing any code."

## Existing Code Patterns (MUST follow these)

- `src/` libs → `thiserror` in libs, `anyhow` in binaries.
- Tokio + Rayon golden rule: never call Rayon `.par_iter()` directly inside a Tokio worker — wrap in `tokio::task::spawn_blocking`.
- `cargo fmt` required; clippy warning-free (`-D warnings`); comments in English, minimal.
- Features: spec `docs/features/specs/FEATURE-*.md` + ledger entry (`status: planned`) — ledger edits happen ONLY at wave reconciliation, never in work branches.

## Acceptance Criteria (VERIFIABLE BY COMMAND)

- [ ] `ls <slot-path>` — path exists (or nearest match found via `find`, recorded in Notes)
- [ ] `cargo fmt --check` — 0 diffs in touched files
- [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings in touched crates
- [ ] `cargo test -p xavier-core-logic` (X1) / workspace subset per slot — 0 failures
- [ ] `git status --porcelain` — shows ONLY files inside the slot island
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` — >= 1 (PR contains files)
- [ ] `git show HEAD --name-only | grep -cE "src/|crates/|benches/"` — >= 1 (real source, not only manifests)
- [ ] Retry parity: `diff` of slot paths vs canonical body shows no island drift (same 6 X slots).

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| Slot X1 `crates/xavier-core-logic/src/` | Crate exists (`Cargo.toml`+`src/`) | Harden logic + unit tests | MED |
| Slot X2 `crates/xavier-wasm/src/` | Crate exists (`Cargo.toml`+`src/`) | Bindings parity + tests | MED |
| Slot X3 `src/adapters/inbound/gestalt/` | Untracked scaffold (`??` in git status) | Complete + wire into registry | MED |
| Slot X4 `src/cli/` | Exists (listed in `ls src/`) | Subcommand/flag follow-ups | LOW |
| Slot X5 `benches/` | Exists (repo root) | Add criterion benches | LOW |
| Slot X6 `docs/protocol/` | `docs/` exists; subpath verify-first | Protocol docs only | LOW |

## File Islands (explicit, machine-verified)

One `island-micro` block per micro-issue. The union is verified disjoint by `verify_islands.py` (0 intersections intra-batch and global).

```island-micro
repo: xavier
batch: wave-20-resto
slot: X1-core-logic
paths:
  - crates/xavier-core-logic/src/
```

```island-micro
repo: xavier
batch: wave-20-resto
slot: X2-wasm
paths:
  - crates/xavier-wasm/src/
```

```island-micro
repo: xavier
batch: wave-20-resto
slot: X3-gestalt-adapter
paths:
  - src/adapters/inbound/gestalt/
```

```island-micro
repo: xavier
batch: wave-20-resto
slot: X4-cli
paths:
  - src/cli/
```

```island-micro
repo: xavier
batch: wave-20-resto
slot: X5-benches
paths:
  - benches/
```

```island-micro
repo: xavier
batch: wave-20-resto
slot: X6-protocol-docs
paths:
  - docs/protocol/
```

## DO NOT touch (Anti-Regression)

- `.gitcore/features.json` — reconciled at wave end ONLY (never in work branches)
- `.env` / `.env.example` — never read, never commit (12-factor; secrets scanned by `scripts/check-secrets.sh`)
- `src/` domains outside your slot (e.g. `src/api/`, `src/billing/`, `src/crypto/`) unless listed above
- `Cargo.lock` unless the slot strictly requires a new dependency (justify in Notes)
- `target/`, `*.db`, `*.sqlite*`, `.xavier/` runtime caches — never commit

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read each target file COMPLETELY before modifying it.
2. **LS before path**: run `ls <slot-path>` FIRST; if a subpath does not exist, run `find <parent> -maxdepth 2 | head -20` and record the real path in Notes — never invent files.
3. **No invented deps**: verify every crate exists in `Cargo.toml` / `Cargo.lock` before importing.
4. **Slot confinement**: `git status --porcelain` must show ONLY your slot paths; any file outside the island is a scope violation — revert it.
5. **Commit honesty**: the commit message must describe files REALLY changed — verify with `git diff --stat HEAD` before committing. A commit describing changes the diff does not contain is a FALSE POSITIVE.
6. **Belal default rule**: on ambiguity pick the most reasonable default, note it in Notes, keep going; ask ONLY for blocking irreversible decisions.

## PR Delivery Requirements (ANTI-EMPTY-PR)

- [ ] `git status --porcelain` shows new/modified files BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before push
- [ ] `git show HEAD --name-only` lists the SAME source files the PR title describes (if only `Cargo.toml`/lock → NOT delivered, keep working)
- [ ] IF the work cannot be completed: do NOT open a PR — comment the blocker on the issue

## Verification

```bash
# Slot path reality check
ls crates/xavier-core-logic/src/ crates/xavier-wasm/src/ src/adapters/inbound/gestalt/ src/cli/ benches/ docs/protocol/
# Style + lints
cargo fmt --check
cargo clippy --all-targets -- -D warnings
# Tests (slot subset; full suite at reconciliation)
cargo test -p xavier-core-logic
# Scope confinement
git status --porcelain
git diff --stat HEAD
```

## Dependencies & Merge Order

- **Depends on:** none (all 6 slots are independent file islands)
- **Parallel with:** all slots in this batch (disjoint islands)
- **Merge order within wave:** X6 (docs) → X5 (benches) → X4 (CLI) → X1/X2 (crates) → X3 (adapter wiring, last: touches registry)
- **Expected effort:** Small-Medium (<1h docs/benches; 1-4h crates/adapter)

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo clippy -D warnings` fails | Fix warnings, do NOT commit broken code |
| Slot subpath does not exist | `find` nearest match, record in Notes, continue |
| `cargo test` fails on pre-existing tests | Check if failure exists on base (`git stash`); report, do not mask |
| PR conflicts with parallel slot | Rebase on main, re-run verification (islands are disjoint so this should not happen) |
| Secret scan flags a file | Remove secret, rotate if real, never push |

## Notes / Assumptions

- Default: `docs/protocol/` exists per repo AGENTS.md reference; agent MUST `ls` it first and fall back to `docs/` root file if missing.
- Default: coverage-CI follow-ups (slot context 2019-2024) target `llvm-profparser`/`tarpaulin` skew noted in recent commits `3b169776/cb96f813/6f046bb9`.
- Retry provenance: H5-RETRY copy written 2026-09-10 in atlas-core; canonical dispatch body lives in the xavier repo path quoted at the top.
