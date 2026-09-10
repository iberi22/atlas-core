#!/usr/bin/env python3
"""H5-BODIES island verifier (2026-09-10).

Parses ```island-micro blocks from the 6 canonical wave-batch bodies and proves
0 intersections intra-batch AND globally (repo-prefixed).

Usage:
    python3 verify_islands.py
Exit 0 + OK line on success; exit 1 with conflict details otherwise.
"""
import re
import sys
from pathlib import Path

BODIES = {
    "xavier wave-20-resto": Path(
        "/home/belal/proyectosSWAL/apps/xavier/.hermes/wave-bodies/2026-09-10/body-wave-20-resto.md"
    ),
    "hosteler-ia h4-batch-1": Path(
        "/home/belal/proyectosSWAL/apps/hosteler-ia/.hermes/wave-bodies/2026-09-10/body-h4-batch-1.md"
    ),
    "veeduria ola1-batch-1": Path(
        "/home/belal/proyectosSWAL/apps/veedur-IA.co/.hermes/wave-bodies/2026-09-10/body-ola1-batch-1.md"
    ),
    "gara-g ola-swal-batch-1": Path(
        "/home/belal/proyectosSWAL/apps/gara-g/.hermes/wave-bodies/2026-09-10/body-ola-swal-batch-1.md"
    ),
    "worldexams bundle": Path(
        "/home/belal/proyectosSWAL/apps/worldexams/.hermes/wave-bodies/2026-09-10/body-bundle.md"
    ),
    "portal pub-01-02": Path(
        "/home/belal/proyectosSWAL/apps/swal-portal/.hermes/wave-bodies/2026-09-10/body-pub-01-02.md"
    ),
}

BLOCK_RE = re.compile(r"```island-micro\s*\n(.*?)```", re.DOTALL)


def parse_block(text):
    repo = batch = slot = None
    paths = []
    in_paths = False
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("repo:"):
            repo = s.split(":", 1)[1].strip()
            in_paths = False
        elif s.startswith("batch:"):
            batch = s.split(":", 1)[1].strip()
            in_paths = False
        elif s.startswith("slot:"):
            slot = s.split(":", 1)[1].strip()
            in_paths = False
        elif s.startswith("paths:"):
            in_paths = True
        elif in_paths and s.startswith("-"):
            paths.append(s[1:].strip().rstrip("/"))
    return repo, batch, slot, paths


def overlap(a, b):
    return a == b or a.startswith(b + "/") or b.startswith(a + "/")


def main():
    slots = []  # (repo, batch, slot, [paths])
    errors = []
    for name, path in BODIES.items():
        if not path.is_file():
            errors.append(f"MISSING body file: {name} -> {path}")
            continue
        blocks = BLOCK_RE.findall(path.read_text(encoding="utf-8"))
        if not blocks:
            errors.append(f"NO island-micro blocks in {path}")
        for b in blocks:
            repo, batch, slot, paths = parse_block(b)
            if not (repo and batch and slot and paths):
                errors.append(f"MALFORMED block in {path}:\n{b[:200]}")
                continue
            slots.append((repo, batch, slot, paths))

    # Intra-batch check
    intra = 0
    by_batch = {}
    for repo, batch, slot, paths in slots:
        by_batch.setdefault((repo, batch), []).append((slot, paths))
    for (repo, batch), items in by_batch.items():
        for i in range(len(items)):
            for j in range(i + 1, len(items)):
                (s1, p1), (s2, p2) = items[i], items[j]
                hits = [(a, c) for a in p1 for c in p2 if overlap(a, c)]
                if hits:
                    intra += 1
                    errors.append(
                        f"INTRA-BATCH CONFLICT {repo}/{batch}: {s1} x {s2} share {hits}"
                    )

    # Global check (repo-prefixed)
    glob = 0
    flat = [
        (f"{repo}/{p}", repo, slot) for repo, _b, slot, paths in slots for p in paths
    ]
    for i in range(len(flat)):
        for j in range(i + 1, len(flat)):
            (a, ra, sa), (c, rc, sc) = flat[i], flat[j]
            if overlap(a, c):
                glob += 1
                errors.append(f"GLOBAL CONFLICT: {ra}/{sa} ({a}) x {rc}/{sc} ({c})")

    total_paths = sum(len(p) for _, _, _, p in slots)
    if errors:
        print(f"FAIL — {len(slots)} slots, {intra} intra + {glob} global conflicts:")
        for e in errors:
            print(f"  - {e}")
        return 1
    print(
        f"OK — {len(slots)} slots, {total_paths} paths, "
        f"{intra} intra-batch + {glob} global intersections (expected 0+0). "
        f"100% Disjoint File Islands Verified!"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
