# [Ola1.Batch-1] Veeduria — dispatch copy

> H5 dispatch copy (2026-09-10). Canonical body: `/home/belal/proyectosSWAL/apps/veedur-IA.co/.hermes/wave-bodies/2026-09-10/body-ola1-batch-1.md`.
> Matrix ref: atlas-core `.hermes/wave-bodies/2026-09-10/MATRICES.md` — slots V1, V2, V3, V4, V5, V6.
> Ola 1 batch-1 — [Foundation/Core].
> Labels: `ola1`, `wave-1` (NO `jules` label — add only after pre-dispatch verification, per Platinum Rule).

## Slots (repo: veedur-IA.co)

- V1-socrata-sdk: `backend/crates/socrata-sdk/`
- V2-domain: `backend/crates/domain/`
- V3-api: `backend/src/api/`
- V4-backend-tests: `backend/tests/`
- V5-frontend-pages: `frontend/src/pages/`
- V6-frontend-e2e: `frontend/tests/e2e/`

## Pre-dispatch checklist

1. Read the canonical body fully before creating any issue.
2. Create issues WITHOUT the `jules` label (one micro-issue per slot max in parallel).
3. Re-read each body via `gh issue view <N> --json body` and fix gaps.
4. Run `verify_islands.py` (expect OK, 0 intersections) before dispatch.
5. Dispatch: `gh issue edit <N> --add-label jules` in one loop.
6. Monitor PRs first (`gh pr list`), then Jules sessions; merge sequential with local gate.
7. Never touch `features.json` in work branches — reconciled at wave end only.
