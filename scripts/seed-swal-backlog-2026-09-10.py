#!/usr/bin/env python3
"""Seed SWAL backlog 2026-09-10 into an Atlas DAG session (offline, local-only).

DB is runtime (*.db gitignored). Durable artifact = this script + printed manifest.
Usage: python3 scripts/seed-swal-backlog-2026-09-10.py [--db ./atlas-swal-backlog.db]
"""
import json
import subprocess
import sys

BIN = "./target/debug/atlas"


def run(*args):
    r = subprocess.run([BIN, *args, "--json"], capture_output=True, text=True)
    if r.returncode != 0:
        print(f"FAIL {args}: {r.stderr.strip()[-500:]}")
        sys.exit(1)
    try:
        return json.loads(r.stdout)
    except Exception:
        print(f"BADJSON {args}: {r.stdout[:300]}")
        sys.exit(1)


def main():
    db = sys.argv[2] if len(sys.argv) > 2 and sys.argv[1] == "--db" else "./atlas-swal-backlog.db"
    s = run("--db", db, "start", "--goal",
            "SWAL backlog 2026-09-10: backlog enfocado + waves activas hasta done verificado")
    sid = s.get("session_id") or s.get("id") or s.get("session")
    print(f"SESSION={sid}")

    tasks = [
        ("XAV-06 bus minimo coordinacion multi-agente", [], "local"),
        ("XAV-02 frescura y carga bajo demanda visibles", [], "local"),
        ("XAV-03 busqueda compacta con procedencia", [], "local"),
        ("XAV-04 PRE/POST recuperable con fallo HTTP", [], "local"),
        ("XAV-05 contrato CLI/skills ejecutable", [], "local"),
        ("SEC-01 separar recovery del sobre cloud sealedPack", [], "local"),
        ("SEC-03 aislar cache KV SQL por instancia", [], "local"),
        ("SEC-04 no confirmar cloud cuando fallo persistencia", [], "local"),
        ("SEC-05 raiz identidad fuera de SQLite recuperable", [], "local"),
        ("PUB-01 claims y metricas con alcance en portal", [], "local"),
        ("PUB-02 traducciones y conteo catalogo portal", [], "local"),
        ("DOC-02 propagar direccion GOAL sin reescribir historia", [], "local"),
        ("ARCH-01 component isolation and local cache validation", [], "local"),
        ("ARCH-02 catalog synchronization and index bounds", [], "local"),
        ("BENCH-01 performance benchmarking suite with test evidence", [], "local"),
        ("SIM-01 decoupled DAG simulation runner", [], "local"),
        ("WAVE xavier-ola14 gate local + integracion PRs 1624-1633", [], "jules"),
        ("WAVE xavier-wave20 watch issues 2019-2024", [], "jules"),
        ("WAVE hosteler wave-close verify pass", [], "jules"),
        ("WAVE veeduria ola1 integracion", [], "jules"),
        ("WAVE gara ola-swal 758-789 e2e PASS", [], "jules"),
        ("WAVE worldexams bundle + portal dual push + GOS API", [], "jules"),
    ]
    ids = {}
    for title, deps, agent in tasks:
        args = ["--db", db, "task", "create", "--title", title,
                "--session", str(sid), "--agent", agent]
        for d in deps:
            args += ["--depends-on", str(ids[d])]
        t = run(*args)
        tid = t.get("task_id") or t.get("id")
        ids[title] = tid
        print(f"TASK {tid} [{agent}] {title}")

    dep_tasks = [
        ("SEC-02 backend acepta ciphertext del cliente", ["SEC-01 separar recovery del sobre cloud sealedPack"], "local"),
        ("BIZ-03 restauracion pagada demostrable dos dispositivos", ["SEC-01 separar recovery del sobre cloud sealedPack"], "local"),
    ]
    for title, deps, agent in dep_tasks:
        args = ["--db", db, "task", "create", "--title", title,
                "--session", str(sid), "--agent", agent]
        for d in deps:
            args += ["--depends-on", str(ids[d])]
        t = run(*args)
        tid = t.get("task_id") or t.get("id")
        ids[title] = tid
        print(f"TASK {tid} [{agent}] {title} deps={deps}")

    lic = run("--db", db, "task", "create", "--title",
              "LIC-01..04 unificar licencias AGPL + decidir BUSL edge-hive",
              "--session", str(sid), "--agent", "belal-decision")
    lic_id = lic.get("task_id") or lic.get("id")
    print(f"TASK {lic_id} [belal-decision] LIC-01..04")
    b = subprocess.run([BIN, "--db", db, "task", "block", str(lic_id),
                        "--reason", "espera SI/NO Belal (push y licencias son ROJO)"],
                       capture_output=True, text=True)
    print("LIC blocked:" , "OK" if b.returncode == 0 else b.stderr.strip()[-200:])
    print(f"TOTAL={len(ids) + 1} SESSION={sid} DB={db}")


if __name__ == "__main__":
    main()
