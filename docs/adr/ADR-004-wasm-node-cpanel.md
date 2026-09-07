# ADR-004: WASM compute + thin Node wrapper for shared hosting

Status: accepted. Date: 2026-09-07.

Context: Two CPanel shared servers must run Atlas as the free tier;
Passenger-style Node apps are the blessed shape there, boxes are
low-spec, and no compiler exists on the servers.

Decision: pure-compute Rust (DAG traversal, estimates, checks) compiles
to one `.wasm` (wasm32-unknown-unknown via wasm-bindgen, `--target nodejs`);
a thin `app.js` hosts it under Passenger; persistence lives Node-side
(prebuilt native SQLite binding). The same `.wasm` runs on Cloudflare
Workers for the SaaS side. Prebuilt artifacts only on servers.

Consequences: rusqlite stays OUT of the WASM module (threading/VFS
friction); WASM/Node boundary is pure data (JSON); memory budget
documented per release (<512MB target).
