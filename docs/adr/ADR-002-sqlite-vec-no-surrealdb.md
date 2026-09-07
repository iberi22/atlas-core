# ADR-002: SQLite + sqlite-vec, no SurrealDB

Status: accepted. Date: 2026-09-07.

Context: Task DAG traversals are bounded (dependencies, unlock paths,
blast radius); vectors serve memory recall. SurrealDB present in the
ecosystem is BSL-licensed (conflicts with the Apache-2.0 goal) and heavy
(RocksDB, large binary) for a local-first CLI on metered links.

Decision: one embedded SQLite file (WAL, versioned migrations) +
recursive CTEs for graph traversal + sqlite-vec for vectors.
Single-file DB, zero services.

Consequences: 100k-node traversal must hold <100ms (CI benchmark gate);
distributed/multi-model needs are out of scope until proven otherwise.
