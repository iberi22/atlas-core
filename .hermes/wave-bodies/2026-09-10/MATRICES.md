# H5-BODIES — Disjoint file-island matrix (2026-09-10)

Central index for the 6 canonical wave-batch bodies. Each body lives in its own repo
(`.hermes/wave-bodies/2026-09-10/`); this matrix + `verify_islands.py` prove 0 intersections
(intra-batch AND global, repo-prefixed). Run the verifier before ANY dispatch.

Bodies (absolute paths):

| # | Batch | Body file |
|---|-------|-----------|
| 1 | xavier wave-20 resto (issues 2019-2024) | `/home/belal/proyectosSWAL/apps/xavier/.hermes/wave-bodies/2026-09-10/body-wave-20-resto.md` |
| 2 | hosteler-ia H4 batch-1 (issues 259-272) | `/home/belal/proyectosSWAL/apps/hosteler-ia/.hermes/wave-bodies/2026-09-10/body-h4-batch-1.md` |
| 3 | veeduria ola1 batch-1 | `/home/belal/proyectosSWAL/apps/veedur-IA.co/.hermes/wave-bodies/2026-09-10/body-ola1-batch-1.md` |
| 4 | gara-g ola-swal batch-1 (758-789) | `/home/belal/proyectosSWAL/apps/gara-g/.hermes/wave-bodies/2026-09-10/body-ola-swal-batch-1.md` |
| 5 | worldexams bundle | `/home/belal/proyectosSWAL/apps/worldexams/.hermes/wave-bodies/2026-09-10/body-bundle.md` |
| 6 | portal PUB-01/02 | `/home/belal/proyectosSWAL/apps/swal-portal/.hermes/wave-bodies/2026-09-10/body-pub-01-02.md` |

## Island matrix (slot → paths, repo-relative)

### xavier / wave-20-resto

| Slot | Paths |
|------|-------|
| X1-core-logic | `crates/xavier-core-logic/src/` |
| X2-wasm | `crates/xavier-wasm/src/` |
| X3-gestalt-adapter | `src/adapters/inbound/gestalt/` |
| X4-cli | `src/cli/` |
| X5-benches | `benches/` |
| X6-protocol-docs | `docs/protocol/` |

### hosteler-ia / h4-batch-1

| Slot | Paths |
|------|-------|
| H1-saas-backend | `saas-backend/src/` |
| H2-saas-client | `saas-client/src/` |
| H3-astro-src | `src-astro/src/` |
| H4-workers | `src-astro/workers/` |
| H5-prisma | `src-astro/prisma/`, `src-astro/migrations/` |
| H6-e2e | `src-astro/e2e/` |

### veeduria / ola1-batch-1

| Slot | Paths |
|------|-------|
| V1-socrata-sdk | `backend/crates/socrata-sdk/` |
| V2-domain | `backend/crates/domain/` |
| V3-api | `backend/src/api/` |
| V4-backend-tests | `backend/tests/` |
| V5-frontend-pages | `frontend/src/pages/` |
| V6-frontend-e2e | `frontend/tests/e2e/` |

### gara-g / ola-swal-batch-1

| Slot | Paths |
|------|-------|
| G1-shared | `packages/shared/src/` |
| G2-backend | `packages/backend/src/` |
| G3-pwa-src | `packages/app-pwa/src/` |
| G4-pwa-tests | `packages/app-pwa/tests/` |
| G5-pwa-workers | `packages/app-pwa/workers/` |
| G6-chain | `packages/gara-chain/` |

### worldexams / bundle

| Slot | Paths |
|------|-------|
| W1-colombia | `questions_data/colombia/` |
| W2-mexico | `questions_data/mexico/` |
| W3-brasil | `questions_data/brasil/` |
| W4-argentina | `questions_data/argentina/` |
| W5-pack-scripts | `saberparatodos/scripts/` |
| W6-packs | `apps/worldexams-api/public/v1/packs/` |

### portal / pub-01-02

| Slot | Paths |
|------|-------|
| P1-components | `src/components/` |
| P2-pages | `src/pages/` |
| P3-i18n | `src/i18n/` |
| P4-data | `src/data/` |
| P5-layouts | `src/layouts/` |
| P6-public | `public/` |

## Disjointness argument

- Intra-batch: every slot path within one batch is a distinct directory prefix; no path is a prefix of (or equal to) another in the same batch. Machine-checked.
- Global: every path is namespaced `repo/...` (different repos per batch), so cross-batch intersections are impossible by construction. Machine-checked with repo prefix.
- Prefix rule: `a/` covers `a/b` — overlap is tested by normalization (trailing slash, `a == b`, `a startswith b`, `b startswith a`).

## Verify

```bash
python3 /home/belal/proyectosSWAL/apps/atlas-core/.hermes/wave-bodies/2026-09-10/verify_islands.py
# expected: OK — 36 slots, 0 intersections (intra-batch + global)
```

Rules honored: NO `gh issue create/edit`, NO `git push`, NO `.env` reads, `features.json` never touched by work slots.
