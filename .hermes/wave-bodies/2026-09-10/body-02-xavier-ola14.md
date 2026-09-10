# [Ola14.Seguimiento] Xavier follow-up verification — coverage gaps, gestalt leftovers, CLI polish, bench baselines, protocol changelog

> H5-RETRY-BODIES dispatch copy (2026-09-10). Canonical reference: xavier wave-20-resto body at `/home/belal/proyectosSWAL/apps/xavier/.hermes/wave-bodies/2026-09-10/body-wave-20-resto.md`.
> Matrix ref: atlas-core `.hermes/wave-bodies/2026-09-10/MATRICES.md` — reuses slots X1, X2, X3, X4, X5, X6 as post-wave-20 follow-up islands (batch id `ola14-seguimiento`).
> Ola-14 seguimiento — [Core/Infra follow-up].
> Labels: `ola14`, `wave-14` (NO `jules` label — add only after pre-dispatch verification, per Platinum Rule).
> Issues covered: wave-20 leftovers (verification residuals per slot below; one micro-issue per slot, verify-first).

---

## Current State (MEASURABLE)

- Baseline (last verified 2026-09-05): ledger `.gitcore/features.json` — `total_features=53`, `features_stable=53`, `overall_progress_pct=100.0` (verified with `python3 -c "import json;print(json.load(open('.gitcore/features.json'))['metadata'])"`).
- Branch at baseline: `fix/mcp-ci-green-20` (verified with `git branch --show-current`); HEAD `3b169776 ci(coverage): report-only until llvm-profparser skew resolved` (verified with `git log --oneline -3`).
- Crates present: `crates/xavier-core-logic/` (`Cargo.toml` + `src/`), `crates/xavier-wasm/` (`Cargo.toml` + `src/`) (verified with `ls crates/`).
- `src/` domains include `adapters, cli` among 20+ entries (verified with `ls src/` at baseline; agent MUST re-run `ls src/` at session start and record any drift).
- Known leftover at baseline (verified with `git status --porcelain`): `?? src/adapters/inbound/gestalt/` untracked scaffold — seguimiento slot X3 MUST confirm whether it is still untracked or was wired by wave-20.
- Scope of this batch: verify-first follow-ups ONLY (close residuals, no new features, no ledger edits in work branches).
- Retry note: this file is the atlas-core H5-RETRY copy; slot islands are the same 6 X islands from MATRICES.md.

## Desired State (DELTA)

- **Slot X1 (seguimiento)**: `crates/xavier-core-logic/src/` — close coverage gaps left by wave-20 (llvm-profparser/tarpaulin skew residuals), unit tests for uncovered branches.
- **Slot X2 (seguimiento)**: `crates/xavier-wasm/src/` — bindings parity residuals + wasm-pack build smoke (no new exported API surface).
- **Slot X3 (seguimiento)**: `src/adapters/inbound/gestalt/` — confirm wave-20 wiring state; finish registry wiring leftovers only, touch no other adapter.
- **Slot X4 (seguimiento)**: `src/cli/` — help-text/flag polish + `--help` snapshot assertions (no new binary, no new subcommand tree).
- **Slot X5 (seguimiento)**: `benches/` — commit criterion baselines for core-logic hot paths (measurement only, no library code changes).
- **Slot X6 (seguimiento)**: `docs/protocol/` — wave-20 changelog + updated verification runbook (docs only, zero code).

## 🌐 Web Research Required

**MANDATORY — 4-6 queries. The agent MUST research before implementing.**

1. search: "cargo tarpaulin llvm-profparser coverage skew fix 2026"
2. search: "wasm-pack build smoke test CI Rust 2026"
3. search: "criterion.rs save-baseline compare CI 2026"
4. search: "clap help snapshot testing Rust 2026"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the queries above — note current best practices.
2. Re-verify the baseline FIRST and record drift vs the Current State above:
   - `git branch --show-current && git log --oneline -3 && git status --porcelain`
   - `ls crates/ && ls src/adapters/inbound/ 2>/dev/null || find src/adapters -maxdepth 2 | head -20`
3. Read and understand these existing files:
   - `crates/xavier-core-logic/src/lib.rs` — note the lib error-handling pattern (thiserror vs anyhow)
   - `src/cli/` — note the existing subcommand pattern before polishing help text
   - `scripts/verify-pipeline.sh` — understand how the feature pipeline judges done
4. Run `ls` on your slot paths FIRST (see Anti-Hallucination Guard).
5. Document baseline drift + findings in the issue before writing any code."

## Existing Code Patterns (MUST follow these)

- `src/` libs → `thiserror` in libs, `anyhow` in binaries.
- Tokio + Rayon golden rule: never call Rayon `.par_iter()` directly inside a Tokio worker — wrap in `tokio::task::spawn_blocking`.
- `cargo fmt` required; clippy warning-free (`-D warnings`); comments in English, minimal.
- Seguimiento rule: if a residual is already fixed on main, report `ALREADY-CLOSED` with the fixing commit hash — do NOT manufacture a diff.

## Acceptance Criteria (VERIFIABLE BY COMMAND)

- [ ] `ls <slot-path>` — path exists (or nearest match found via `find`, recorded in Notes)
- [ ] Baseline re-verified: `git log --oneline -3` hash recorded in the issue (drift vs Current State documented or `NO-DRIFT`)
- [ ] `cargo fmt --check` — 0 diffs in touched files
- [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings in touched crates
- [ ] `cargo test -p xavier-core-logic` (X1) / workspace subset per slot — 0 failures, no regressions vs baseline
- [ ] `git status --porcelain` — shows ONLY files inside the slot island
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` — >= 1 (PR contains files), OR `ALREADY-CLOSED` with proof comment (no PR needed)
- [ ] `git show HEAD --name-only | grep -cE "src/|crates/|benches/"` — >= 1 (real source, not only manifests)
- [ ] Retry parity: slot islands identical to MATRICES.md X1-X6 (same 6 paths, no drift).

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| Slot X1 `crates/xavier-core-logic/src/` | Crate exists; coverage skew residual | Coverage-gap tests | MED |
| Slot X2 `crates/xavier-wasm/src/` | Crate exists; parity residual | Parity residuals + build smoke | MED |
| Slot X3 `src/adapters/inbound/gestalt/` | Verify-first (was `??` untracked) | Finish wiring leftovers only | MED |
| Slot X4 `src/cli/` | Exists | Help/flag polish + snapshots | LOW |
| Slot X5 `benches/` | Exists (repo root) | Commit criterion baselines | LOW |
| Slot X6 `docs/protocol/` | `docs/` exists; subpath verify-first | Wave-20 changelog + runbook | LOW |

## File Islands (explicit, machine-verified)

One `island-micro` block per micro-issue. Islands reuse the MATRICES.md X slots (batch id `ola14-seguimiento`); the canonical verifier covers batch `wave-20-resto`, so these blocks are documentation-identical and conflict-free.

```island-micro
repo: xavier
batch: ola14-seguimiento
slot: X1-core-logic
paths:
  - crates/xavier-core-logic/src/
```

```island-micro
repo: xavier
batch: ola14-seguimiento
slot: X2-wasm
paths:
  - crates/xavier-wasm/src/
```

```island-micro
repo: xavier
batch: ola14-seguimiento
slot: X3-gestalt-adapter
paths:
  - src/adapters/inbound/gestalt/
```

```island-micro
repo: xavier
batch: ola14-seguimiento
slot: X4-cli
paths:
  - src/cli/
```

```island-micro
repo: xavier
batch: ola14-seguimiento
slot: X5-benches
paths:
  - benches/
```

```island-micro
repo: xavier
batch: ola14-seguimiento
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
- [ ] IF the residual is already fixed OR cannot be completed: do NOT open a PR — comment the proof/blocker on the issue

## Verification

```bash
# Baseline re-verification (record hashes in the issue)
git branch --show-current && git log --oneline -3 && git status --porcelain
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

- **Depends on:** wave-20-resto merged state (verify-first; record drift before coding)
- **Parallel with:** all slots in this batch (disjoint islands)
- **Merge order within wave:** X6 (docs) → X5 (baselines) → X4 (CLI) → X1/X2 (crates) → X3 (adapter leftovers, last: touches registry)
- **Expected effort:** Small (<1h docs/baselines/CLI; 1-3h crates/adapter residuals)

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| Baseline drifted from Current State | Record new hashes, re-scope the slot, continue — do NOT assume old paths |
| Residual already fixed on main | Comment `ALREADY-CLOSED` + fixing hash, close without PR |
| `cargo clippy -D warnings` fails | Fix warnings, do NOT commit broken code |
| Slot subpath does not exist | `find` nearest match, record in Notes, continue |
| `cargo test` fails on pre-existing tests | Check if failure exists on base (`git stash`); report, do not mask |
| Secret scan flags a file | Remove secret, rotate if real, never push |

## Notes / Assumptions

- Default: `docs/protocol/` exists per repo AGENTS.md reference; agent MUST `ls` it first and fall back to `docs/` root file if missing.
- Default: if wave-20 already wired the gestalt scaffold, slot X3 becomes a registry-verification + test task, not a scaffolding task.
- Retry provenance: H5-RETRY copy written 2026-09-10 in atlas-core; canonical dispatch bodies live in the xavier repo path quoted at the top.
