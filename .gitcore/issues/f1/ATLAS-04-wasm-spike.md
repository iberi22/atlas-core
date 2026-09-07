# ATLAS-04: WASM compute spike (traversal parity)

Goal: `feat-wasm-node` spike — DAG traversal compiled to `.wasm` and
executed under Node with byte-identical results vs native.

Stories: C-014.

AC:
- [ ] `wasm32-unknown-unknown` build via wasm-bindgen `--target nodejs`.
- [ ] Parity test: same fixture, same output, native vs WASM vs Workers-shape.
- [ ] rusqlite excluded from WASM boundary (pure-data JSON I/O only).

DoD: parity test green; memory budget measured and documented.
