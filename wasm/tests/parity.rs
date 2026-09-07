// ATLAS-04 parity: same fixtures must produce byte-identical JSON from the
// WASM-crate wrappers (which embed `src/dag.rs` verbatim) as from the
// hand-computed contract below. Runs natively via the crate's `rlib`.
use atlas_wasm::{reachable_json, topo_order_json};

const EDGES: &str = r#"[["c","b"],["b","a"],["c","a"],["d","b"],["e","zzz"]]"#;
const NODES: &str = r#"["a","b","c","d","e","zzz"]"#;

#[test]
fn reachable_matches_contract_bytes() {
    // Ancestors of "c": a, b (sorted). Byte-identical expectation.
    assert_eq!(reachable_json("c", EDGES), r#"["a","b"]"#);
    assert_eq!(reachable_json("d", EDGES), r#"["a","b"]"#);
    assert_eq!(reachable_json("e", EDGES), r#"["zzz"]"#);
    assert_eq!(reachable_json("a", EDGES), r#"[]"#);
    assert_eq!(reachable_json("ghost", EDGES), r#"[]"#);
}

#[test]
fn topo_order_matches_contract_bytes() {
    // Kahn with lexicographic tie-break: zzz (indegree 0) precedes e.
    assert_eq!(
        topo_order_json(NODES, EDGES),
        r#"["a","b","c","d","zzz","e"]"#
    );
}

#[test]
fn topo_order_parents_always_first_on_larger_fixture() {
    let nodes = r#"["n0","n1","n2","n3","n4","n5","n6","n7"]"#;
    let edges = r#"[["n1","n0"],["n2","n0"],["n3","n1"],["n3","n2"],
        ["n4","n2"],["n5","n3"],["n6","n3"],["n7","n4"],["n7","n6"]]"#;
    let out = topo_order_json(nodes, edges);
    let order: Vec<String> = serde_json::from_str(&out).expect("array");
    assert_eq!(order.len(), 8);
    let pos = |id: &str| order.iter().position(|x| x == id).expect("present");
    for (child, parent) in [
        ("n1", "n0"),
        ("n2", "n0"),
        ("n3", "n1"),
        ("n3", "n2"),
        ("n4", "n2"),
        ("n5", "n3"),
        ("n6", "n3"),
        ("n7", "n4"),
        ("n7", "n6"),
    ] {
        assert!(pos(parent) < pos(child), "{parent} must precede {child}");
    }
    // Determinism: second call is byte-identical.
    assert_eq!(topo_order_json(nodes, edges), out);
}

#[test]
fn cycles_and_bad_input_report_error_object() {
    let out = topo_order_json(r#"["a","b"]"#, r#"[["a","b"],["b","a"]]"#);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(
        v,
        serde_json::json!({ "error": "cycle detected among: a, b" })
    );

    let out = reachable_json("a", "not-json");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(v.get("error").is_some(), "must be an error object");

    let out = topo_order_json("not-json", EDGES);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(v.get("error").is_some(), "must be an error object");
}
