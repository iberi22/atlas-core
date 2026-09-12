# ADR-004: WASM compute + thin Node wrapper for shared hosting

Status: accepted. Date: 2026-09-07.

Context: CPanel shared servers and edge environments must run Atlas;
Passenger-style Node apps are the blessed shape there, boxes are
low-spec, and no compiler exists on the servers.

Decision: pure-compute Rust (DAG traversal, estimates, checks) compiles
to one `.wasm` (wasm32-unknown-unknown via wasm-bindgen, `--target nodejs`);
a thin `app.js` hosts it under Passenger; persistence lives Node-side
(prebuilt native SQLite binding). The same `.wasm` can run on Cloudflare
Workers and other edge runtimes. Prebuilt artifacts only on servers.

Consequences: rusqlite stays OUT of the WASM module (threading/VFS
friction); WASM/Node boundary is pure data (JSON); memory budget
documented per release (<512MB target).
