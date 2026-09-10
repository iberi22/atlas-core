# [Ola-SWAL.Batch-1] Gara-g — dispatch copy

> H5 dispatch copy (2026-09-10). Canonical body: `/home/belal/proyectosSWAL/apps/gara-g/.hermes/wave-bodies/2026-09-10/body-ola-swal-batch-1.md`.
> Matrix ref: atlas-core `.hermes/wave-bodies/2026-09-10/MATRICES.md` — slots G1, G2, G3, G4, G5, G6.
> Ola-SWAL batch-1 (issues 758-789 range; issues sharing a slot run SEQUENTIALLY) — [Core/Biz].
> Labels: `ola-swal`, `wave-swal` (NO `jules` label — add only after pre-dispatch verification, per Platinum Rule).
> Wave rule: e2e PASS required (memory order); fonts via fontsource locales.

## Slots (repo: gara-g)

- G1-shared: `packages/shared/src/`
- G2-backend: `packages/backend/src/`
- G3-pwa-src: `packages/app-pwa/src/`
- G4-pwa-tests: `packages/app-pwa/tests/`
- G5-pwa-workers: `packages/app-pwa/workers/`
- G6-chain: `packages/gara-chain/`

## Pre-dispatch checklist

1. Read the canonical body fully before creating any issue.
2. Create issues WITHOUT the `jules` label (one micro-issue per slot max in parallel).
3. Re-read each body via `gh issue view <N> --json body` and fix gaps.
4. Run `verify_islands.py` (expect OK, 0 intersections) before dispatch.
5. Dispatch: `gh issue edit <N> --add-label jules` in one loop.
6. Monitor PRs first (`gh pr list`), then Jules sessions; merge sequential with local gate + e2e.
7. Never touch `features.json` in work branches — reconciled at wave end only.
