# [Bundle+Pub] WorldExams + Portal — dispatch copy

> H5 dispatch copy (2026-09-10). Canonical bodies:
> - `/home/belal/proyectosSWAL/apps/worldexams/.hermes/wave-bodies/2026-09-10/body-bundle.md`
> - `/home/belal/proyectosSWAL/apps/swal-portal/.hermes/wave-bodies/2026-09-10/body-pub-01-02.md`
> Matrix ref: atlas-core `.hermes/wave-bodies/2026-09-10/MATRICES.md` — slots W1..W6 (worldexams) + P1..P6 (portal).
> Content bundle wave + PUB-01/02 public release — [Content/Biz + Polish/Public].
> Labels: `bundle`, `wave-content` / `pub`, `wave-pub` (NO `jules` label — add only after pre-dispatch verification, per Platinum Rule).
> Portal rule: dual remotes origin + github-io, BOTH pushed on public edits; GOS business NEVER public (API only).

## Slots (repo: worldexams)

- W1-colombia: `questions_data/colombia/`
- W2-mexico: `questions_data/mexico/`
- W3-brasil: `questions_data/brasil/`
- W4-argentina: `questions_data/argentina/`
- W5-pack-scripts: `saberparatodos/scripts/`
- W6-packs: `apps/worldexams-api/public/v1/packs/`

## Slots (repo: swal-portal)

- P1-components: `src/components/`
- P2-pages: `src/pages/`
- P3-i18n: `src/i18n/`
- P4-data: `src/data/`
- P5-layouts: `src/layouts/`
- P6-public: `public/`

## Pre-dispatch checklist

1. Read both canonical bodies fully before creating any issue.
2. Create issues WITHOUT the `jules` label (one micro-issue per slot max in parallel).
3. Re-read each body via `gh issue view <N> --json body` and fix gaps.
4. Run `verify_islands.py` (expect OK, 0 intersections) before dispatch.
5. Dispatch: `gh issue edit <N> --add-label jules` in one loop.
6. Web rule: NO web delivery without Playwright desktop+mobile captures + 0-error console + vision review.
7. Never touch `features.json` in work branches — reconciled at wave end only.
