// Pure DAG traversal core shared by the native `Store` and the
// `atlas-wasm` module (ADR-004, ATLAS-04).
// Constraints for WASM reuse: no I/O, no threads, no time, no rusqlite;
// only allocated collections, so this file is included verbatim by
// `wasm/src/lib.rs` via `#[path]` and must stay dependency-free.
// Edge convention mirrors the `edges` table: `(child, parent)` means
// `child` depends on `parent` (parent runs first).
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Transitive parents of `start` (child -> parent walk), sorted ascending.
/// Mirrors the `anc` recursive CTE in `Store::would_cycle`: the same edge
/// set yields the same ancestor set. `start` itself is never reported,
/// even when a (rejected-in-DB, possible-in-JSON) cycle leads back to it.
pub fn reachable(start: &str, edges: &[(String, String)]) -> Vec<String> {
    let mut by_child: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (child, parent) in edges {
        by_child
            .entry(child.as_str())
            .or_default()
            .push(parent.as_str());
    }
    let mut seen: BTreeSet<&str> = BTreeSet::from([start]);
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<&str> = vec![start];
    while let Some(node) = stack.pop() {
        if let Some(parents) = by_child.get(node) {
            for parent in parents {
                if seen.insert(parent) {
                    out.insert((*parent).to_owned());
                    stack.push(parent);
                }
            }
        }
    }
    out.into_iter().collect()
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
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut indegree: BTreeMap<&str, usize> = BTreeMap::new();
    for node in nodes {
        indegree.entry(node.as_str()).or_insert(0);
    }
    // Deduplicate edges so multi-inserts cannot inflate indegrees.
    let mut seen_edges: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (child, parent) in edges {
        let (c, p) = (child.as_str(), parent.as_str());
        if !seen_edges.insert((c, p)) {
            continue;
        }
        indegree.entry(p).or_insert(0);
        *indegree.entry(c).or_insert(0) += 1;
        children.entry(p).or_default().push(c);
    }
    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(&n, _)| n)
        .collect();
    // Own the output strings; `order` borrows nothing past this call.
    let mut order: Vec<String> = Vec::with_capacity(indegree.len());
    while let Some(&node) = ready.iter().next() {
        ready.remove(node);
        order.push(node.to_owned());
        if let Some(kids) = children.get(node) {
            for kid in kids {
                if let Some(d) = indegree.get_mut(kid) {
                    *d -= 1;
                    if *d == 0 {
                        ready.insert(kid);
                    }
                }
            }
        }
    }
    if order.len() == indegree.len() {
        Ok(order)
    } else {
        let emitted: BTreeSet<&str> = order.iter().map(String::as_str).collect();
        let nodes = indegree
            .keys()
            .filter(|n| !emitted.contains(*n))
            .map(|n| (*n).to_owned())
            .collect();
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
