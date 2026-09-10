# [H4.Batch-1] Hosteler-IA issues 259-272 — SaaS backend, SaaS client, Astro routes/workers/prisma/e2e

> H5-RETRY-BODIES dispatch copy (2026-09-10). Canonical body: `/home/belal/proyectosSWAL/apps/hosteler-ia/.hermes/wave-bodies/2026-09-10/body-h4-batch-1.md`.
> Matrix ref: atlas-core `.hermes/wave-bodies/2026-09-10/MATRICES.md` — slots H1, H2, H3, H4, H5, H6.
> H4 batch-1 — [Biz/SaaS].
> Labels: `h4`, `wave-h4` (NO `jules` label — add only after pre-dispatch verification, per Platinum Rule).
> Issues covered: 259-272 (14 micro-issues mapped to 6 file-island slots below; at most one issue per slot runs in parallel per slot-group — see Merge Order).

---

## Current State (MEASURABLE)

- Ledger: `.gitcore/features.json` — `total_features=36` (verified with `python3 -c "import json;print(len(json.load(open('.gitcore/features.json'))))"` — schema uses non-`done` statuses; counts taken from raw entries).
- Branch: `development` (verified with `git branch --show-current`); HEAD `714ba9168 chore(gitignore): ignore npm lockfiles, repo is pnpm-only` (verified with `git log --oneline -3`).
- `saas-backend/src/` (verified with `ls`): `alerts.ts, index.ts, limits.ts, plans.ts, quotas.ts, usage.ts` + colocated `*.test.ts` (11 files total).
- `saas-client/src/` exists (verified with `ls`); `src-astro/src/` exists with `components, env.d.ts, i18n, layouts, lib, middleware*.ts, pages, stores, tokens, ...` (verified with `ls`).
- `src-astro/` (verified with `ls`): `astro.config.mjs, e2e/, migrations/, prisma/, public/, scripts/, src/, workers/, wrangler.jsonc, wrangler.toml, vitest.config.ts, playwright.config.ts`.
- Stack contract (repo AGENTS.md): TypeScript strict (never `as any`/`@ts-ignore`), `pnpm` only, Biome via `pnpm run check`, Prisma 7, Next.js 16 App Router (`cookies()`/`headers()` are async).
- SaaS variant (VERIFIED 2026-09-06, WAVE-SAAS #289-292): server-side pricing ONLY; client must contain zero price math.
- Retry note: this file is the atlas-core H5-RETRY copy; the canonical body above holds the dispatch version. Slot paths are identical.

## Desired State (DELTA)

- **Slot H1 (issues 259-261)**: `saas-backend/src/` — quotas/limits/plans/usage hardening, fail-closed on infra errors, `[saas-ai mock]` label on mock-AI path.
- **Slot H2 (issues 262-264)**: `saas-client/src/` — PUBLIC-SAFE client (zero deps, zero secrets, deprecation shims, never price math).
- **Slot H3 (issues 265-267)**: `src-astro/src/` (routes/pages/lib) — server-side bridge (`INSTANCE_KEY` via `locals.runtime`, mirror existing routes), no key in client bundle.
- **Slot H4 (issues 268-269)**: `src-astro/workers/` — worker routes, fail-closed 503 on KV/D1 down (mocked-env tests, NO miniflare dep).
- **Slot H5 (issues 270-271)**: `src-astro/prisma/` + `migrations/` — schema follow-ups + `npx prisma generate` parity.
- **Slot H6 (issue 272)**: `src-astro/e2e/` — Playwright e2e for the H4 flows (no source changes).

## 🌐 Web Research Required

**MANDATORY — 4-6 queries. The agent MUST research before implementing.**

1. search: "Cloudflare Workers fail-closed 503 KV D1 down best practice 2026"
2. search: "Astro locals.runtime environment Cloudflare 2026"
3. search: "Prisma 7 generate migrate deploy 2026"
4. search: "Playwright e2e Cloudflare Workers preview 2026"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the queries above.
2. Read and understand:
   - `saas-backend/src/index.ts` + `quotas.ts` — note the fail-closed pattern (or its absence)
   - `saas-client/src/index.ts` — note the PUBLIC-SAFE surface (exports only)
   - One existing `src-astro/src/pages/api/*` route — MIRROR how it reads env via `locals.runtime`; do not invent a new mechanism
3. Run `ls` on your slot paths FIRST.
4. Document your findings in the issue before writing any code."

## Existing Code Patterns (MUST follow these)

- `saas-backend/src/*.ts` → colocated `*.test.ts`, fail-closed (infra error → 503, never grant/consume).
- `saas-client/src/` → shim pattern: `@deprecated use saas-client` with identical exports; NEVER delete a migrated module in the same wave.
- `src-astro/src/pages/api/*` → read `INSTANCE_KEY` server-side via `locals.runtime` (mirror existing routes).
- pnpm only (`pnpm run check`, `pnpm test`); Biome canonical; TS strict.

## Acceptance Criteria (VERIFIABLE BY COMMAND)

- [ ] `ls <slot-path>` — path exists (or nearest match via `find`, recorded in Notes)
- [ ] `pnpm run check` — 0 errors in touched files
- [ ] `pnpm test <slot-test>` — all pass; `pnpm test 2>&1 | tail -3` — no regressions
- [ ] Server-side pricing guard: `grep -rE "\* ?1\.10|\* ?0\.20|HANDLING|TIERS" saas-client/src/ | wc -l` — 0
- [ ] PUBLIC-SAFE audit: `grep -rniE "sk-|secret|password|priceUsd" saas-client/src/ | wc -l` — 0
- [ ] Client bundle leak check (after build): `grep -r "SAAS_INSTANCE_KEY" dist/client/ | wc -l` — 0
- [ ] Mock-AI label: `grep -rn "saas-ai mock" saas-backend/src/ | wc -l` — >= 1 (if mock path touched)
- [ ] Importers listed: `grep -rn "from.*lib/billing" src-astro/src --include="*.ts" | wc -l` — reported in PR (shim wave, no deletes)
- [ ] `git status --porcelain` — ONLY slot-island files
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` — >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| Slot H1 `saas-backend/src/` | 11 files (6 modules + tests) | Harden quotas/limits/plans/usage, fail-closed | MED |
| Slot H2 `saas-client/src/` | Exists | PUBLIC-SAFE surface + shims | MED |
| Slot H3 `src-astro/src/` | Routes/pages/lib exist | Server-side bridge, no key leak | MED |
| Slot H4 `src-astro/workers/` | Exists | Worker routes, fail-closed 503 | MED |
| Slot H5 `src-astro/prisma/` + `src-astro/migrations/` | Exist | Schema follow-ups + generate parity | LOW |
| Slot H6 `src-astro/e2e/` | Exists | Playwright e2e, no source changes | LOW |

## File Islands (explicit, machine-verified)

One `island-micro` block per micro-issue group. The union is verified disjoint by `verify_islands.py` (0 intersections intra-batch and global).

```island-micro
repo: hosteler-ia
batch: h4-batch-1
slot: H1-saas-backend
paths:
  - saas-backend/src/
```

```island-micro
repo: hosteler-ia
batch: h4-batch-1
slot: H2-saas-client
paths:
  - saas-client/src/
```

```island-micro
repo: hosteler-ia
batch: h4-batch-1
slot: H3-astro-src
paths:
  - src-astro/src/
```

```island-micro
repo: hosteler-ia
batch: h4-batch-1
slot: H4-workers
paths:
  - src-astro/workers/
```

```island-micro
repo: hosteler-ia
batch: h4-batch-1
slot: H5-prisma
paths:
  - src-astro/prisma/
  - src-astro/migrations/
```

```island-micro
repo: hosteler-ia
batch: h4-batch-1
slot: H6-e2e
paths:
  - src-astro/e2e/
```

## DO NOT touch (Anti-Regression)

- `.gitcore/features.json` — reconciled at wave end ONLY (never in work branches)
- `.env` / `.env.*` — never read, never commit (secrets scanned in CI)
- `saas-client/src/` from backend slots and vice versa — slot confinement is absolute (H1 never touches H2)
- `pnpm-lock.yaml` unless the slot strictly requires a new dependency (justify in Notes); never introduce npm/yarn lockfiles (repo is pnpm-only per HEAD `714ba9168`)
- `dist/`, `node_modules/`, `.astro/` build outputs — never commit

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read each target file COMPLETELY before modifying it.
2. **LS before path**: run `ls <slot-path>` FIRST; if a subpath does not exist, run `find <parent> -maxdepth 2 | head -20` and record the real path in Notes — never invent files.
3. **Mirror, don't invent**: new API routes MUST mirror an existing `src-astro/src/pages/api/*` route's `locals.runtime` pattern — a new env mechanism is a scope violation.
4. **Slot confinement**: `git status --porcelain` must show ONLY your slot paths; any file outside the island is a scope violation — revert it.
5. **Commit honesty**: the commit message must describe files REALLY changed — verify with `git diff --stat HEAD` before committing. A commit describing changes the diff does not contain is a FALSE POSITIVE.
6. **Belal default rule**: on ambiguity pick the most reasonable default, note it in Notes, keep going; ask ONLY for blocking irreversible decisions.

## PR Delivery Requirements (ANTI-EMPTY-PR)

- [ ] `git status --porcelain` shows new/modified files BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before push
- [ ] `git show HEAD --name-only` lists the SAME source files the PR title describes (config-only PRs → NOT delivered, keep working)
- [ ] IF the work cannot be completed: do NOT open a PR — comment the blocker on the issue

## Verification

```bash
# Slot path reality check
ls saas-backend/src/ saas-client/src/ src-astro/src/ src-astro/workers/ src-astro/prisma/ src-astro/e2e/
# Lint + tests
pnpm run check
pnpm test 2>&1 | tail -3
# PUBLIC-SAFE + pricing guards
grep -rE "\* ?1\.10|\* ?0\.20|HANDLING|TIERS" saas-client/src/ | wc -l
grep -rniE "sk-|secret|password|priceUsd" saas-client/src/ | wc -l
# Scope confinement
git status --porcelain
git diff --stat HEAD
```

## Dependencies & Merge Order

- **Depends on:** none (all 6 slots are independent file islands)
- **Parallel with:** all slots in this batch (disjoint islands); issues sharing one slot run SEQUENTIALLY (259→260→261 inside H1, etc.)
- **Merge order within wave:** H6 (e2e, last — validates all flows) after H1+H2+H3+H4; H5 (prisma) before H3 bridge work that reads new schema
- **Expected effort:** Small-Medium (<1h e2e/prisma; 1-4h backend/client/bridge/workers)

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `pnpm run check` fails | Fix errors, do NOT commit broken code |
| Slot subpath does not exist | `find` nearest match, record in Notes, continue |
| `pnpm test` fails on pre-existing tests | Check if failure exists on base (`git stash`); report, do not mask |
| Worker test needs miniflare | Do NOT add the dep — use mocked-env tests per slot spec |
| PR conflicts with parallel slot | Rebase on development, re-run verification (islands are disjoint so this should not happen) |
| Secret scan flags a file | Remove secret, rotate if real, never push |

## Notes / Assumptions

- Default: `src-astro/migrations/` exists per `ls` baseline; agent MUST `ls` it first — if absent, scope H5 to `src-astro/prisma/` only and record it.
- Default: mocked worker env (KV/D1 down → 503) is tested without miniflare; any new test harness dep needs explicit justification in Notes.
- Retry provenance: H5-RETRY copy written 2026-09-10 in atlas-core; canonical dispatch body lives in the hosteler-ia repo path quoted at the top.
