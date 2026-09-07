// Pure DAG traversal core shared by the native `Store` and the
// `atlas-wasm` module (ADR-004, ATLAS-04).
// Constraints for WASM reuse: no I/O, no threads, no time, no rusqlite;
// only allocated collections, so this file is included verbatim by
// `wasm/src/lib.rs` via `#[path]` and must stay dependency-free.
// Edge convention mirrors the `edges` table: `(child, parent)` means
// `child` depends on `parent` (parent runs first).
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;

/// Transitive parents of `start` (child -> parent walk), sorted ascending.
/// Mirrors the `anc` recursive CTE in `Store::would_cycle`: the same edge
/// set yields the same ancestor set. `start` itself is never reported,
/// even when a (rejected-in-DB, possible-in-JSON) cycle leads back to it.
pub fn reachable(start: &str, edges: &[(String, String)]) -> Vec<String> {
    // Hash-based adjacency: O(1) insert/lookup instead of BTree O(log n).
    // Output is sorted at the end, so the result stays deterministic.
    let mut by_child: HashMap<&str, Vec<&str>> = HashMap::with_capacity(edges.len());
    for (child, parent) in edges {
        by_child
            .entry(child.as_str())
            .or_default()
            .push(parent.as_str());
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(1024);
    seen.insert(start);
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<&str> = vec![start];
    while let Some(node) = stack.pop() {
        if let Some(parents) = by_child.get(node) {
            for parent in parents {
                if seen.insert(parent) {
                    out.push((*parent).to_owned());
                    stack.push(parent);
                }
            }
        }
    }
    out.sort();
    out
}

/// Variante indexada de [`reachable`] para snapshots grandes: los nodos son
/// `u32` (internados por el llamador) y no hay ningún `String` por arista.
/// Puro en memoria (apto WASM). Devuelve índices ordenados ascendentes; el
/// llamador mapea a ids y ordena lexicográficamente si lo necesita.
// (`allow`: este archivo se incluye verbatim en `atlas-wasm`, donde esta
// variante indexada aún no se usa.)
#[allow(dead_code)]
pub fn reachable_idx(start: u32, n: usize, edges: &[(u32, u32)]) -> Vec<u32> {
    if (start as usize) >= n {
        return Vec::new();
    }
    let mut by_child: Vec<Vec<u32>> = vec![Vec::new(); n];
    // Reserva por grado para no realocar durante la carga.
    let mut deg = vec![0u32; n];
    for &(c, _) in edges {
        if (c as usize) < n {
            deg[c as usize] += 1;
        }
    }
    for (slot, d) in by_child.iter_mut().zip(deg.iter()) {
        slot.reserve_exact(*d as usize);
    }
    for &(c, p) in edges {
        if (c as usize) < n && (p as usize) < n {
            by_child[c as usize].push(p);
        }
    }
    let mut seen = vec![false; n];
    seen[start as usize] = true;
    let mut out = Vec::new();
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        for &parent in &by_child[node as usize] {
            if !seen[parent as usize] {
                seen[parent as usize] = true;
                out.push(parent);
                stack.push(parent);
            }
        }
    }
    out.sort_unstable();
    out
}

/// Cycle reported by [`topo_order`]: the nodes that could not be ordered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleError {
    /// Leftover nodes (sorted), each on or downstream of a cycle.
    pub nodes: Vec<String>,
}

impl fmt::Display for CycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cycle detected among: {}", self.nodes.join(", "))
    }
}

impl std::error::Error for CycleError {}

/// Topological order (parents before children) via Kahn's algorithm.
/// Ties break lexicographically, so output is fully deterministic for a
/// given input. Ids that appear only in `edges` join the graph as implicit
/// nodes. Returns [`CycleError`] when the edges contain a cycle.
pub fn topo_order(nodes: &[String], edges: &[(String, String)]) -> Result<Vec<String>, CycleError> {
    // Hash-based Kahn: O(1) map ops; the ready frontier is a min-heap so
    // ties still break lexicographically (same order as before, faster).
    let mut children: HashMap<&str, Vec<&str>> = HashMap::with_capacity(edges.len());
    let mut indegree: HashMap<&str, usize> =
        HashMap::with_capacity(nodes.len().saturating_add(edges.len()));
    for node in nodes {
        indegree.entry(node.as_str()).or_insert(0);
    }
    // Deduplicate edges so multi-inserts cannot inflate indegrees.
    let mut seen_edges: HashSet<(&str, &str)> = HashSet::with_capacity(edges.len());
    for (child, parent) in edges {
        let (c, p) = (child.as_str(), parent.as_str());
        if !seen_edges.insert((c, p)) {
            continue;
        }
        indegree.entry(p).or_insert(0);
        *indegree.entry(c).or_insert(0) += 1;
        children.entry(p).or_default().push(c);
    }
    let mut ready: BinaryHeap<Reverse<&str>> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(&n, _)| Reverse(n))
        .collect();
    // Own the output strings; `order` borrows nothing past this call.
    let mut order: Vec<String> = Vec::with_capacity(indegree.len());
    while let Some(Reverse(node)) = ready.pop() {
        order.push(node.to_owned());
        if let Some(kids) = children.get(node) {
            for kid in kids {
                if let Some(d) = indegree.get_mut(kid) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(Reverse(kid));
                    }
                }
            }
        }
    }
    if order.len() == indegree.len() {
        Ok(order)
    } else {
        let emitted: HashSet<&str> = order.iter().map(String::as_str).collect();
        let mut nodes: Vec<String> = indegree
            .keys()
            .filter(|n| !emitted.contains(*n))
            .map(|n| (*n).to_owned())
            .collect();
        nodes.sort();
        Err(CycleError { nodes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(c, p)| ((*c).to_owned(), (*p).to_owned()))
            .collect()
    }

    #[test]
    fn reachable_walks_transitive_parents_sorted() {
        let e = edges(&[("c", "b"), ("b", "a"), ("c", "a"), ("zzz", "a")]);
        assert_eq!(reachable("c", &e), vec!["a", "b"]);
    }

    #[test]
    fn reachable_idx_matches_reachable() {
        // c->b->a, c->a: ancestros de c son {a, b} en ambas variantes.
        let s = &[
            ("c".to_owned(), "b".to_owned()),
            ("b".to_owned(), "a".to_owned()),
        ];
        assert_eq!(reachable("c", s), vec!["a", "b"]);
        let ids = ["a", "b", "c"];
        let idx = [(2u32, 1u32), (1u32, 0u32)];
        let got: Vec<&str> = reachable_idx(2, ids.len(), &idx)
            .iter()
            .map(|&i| ids[i as usize])
            .collect();
        assert_eq!(got, vec!["a", "b"]);
        assert!(reachable_idx(9, ids.len(), &idx).is_empty());
    }

    #[test]
    fn reachable_unknown_start_is_empty() {
        let e = edges(&[("b", "a")]);
        assert!(reachable("ghost", &e).is_empty());
    }

    #[test]
    fn topo_orders_parents_first_deterministically() {
        let nodes = vec!["c".to_owned(), "b".to_owned(), "a".to_owned()];
        let e = edges(&[("c", "b"), ("b", "a")]);
        assert_eq!(
            topo_order(&nodes, &e),
            Ok(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()])
        );
    }

    #[test]
    fn topo_reports_cycle_members() {
        let nodes = vec!["a".to_owned(), "b".to_owned()];
        let e = edges(&[("a", "b"), ("b", "a")]);
        let err = topo_order(&nodes, &e).expect_err("cycle must fail");
        assert_eq!(err.nodes, vec!["a", "b"]);
    }
}
