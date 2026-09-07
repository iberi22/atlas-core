// `atlas-wasm`: pure DAG compute for Node/Passenger and Cloudflare
// Workers (ADR-004, ATLAS-04). rusqlite stays OUT of this module; the
// WASM/Node boundary is pure JSON data only.
// The traversal core is the verbatim `src/dag.rs` file (no-std-compatible,
// dependency-free), so native and WASM results share one implementation.
#[path = "../../src/dag.rs"]
mod dag;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Parse `edges_json` (`[["child","parent"],...]`) or describe the failure.
fn parse_edges(edges_json: &str) -> Result<Vec<(String, String)>, String> {
    serde_json::from_str(edges_json).map_err(|e| format!("invalid edges_json: {e}"))
}

/// Parse `nodes_json` (`["id",...]`) or describe the failure.
fn parse_nodes(nodes_json: &str) -> Result<Vec<String>, String> {
    serde_json::from_str(nodes_json).map_err(|e| format!("invalid nodes_json: {e}"))
}

/// Render an error as a stable single-key JSON object.
fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// Ancestor ids of `start` as a JSON array (sorted); errors as
/// `{"error":"..."}`. Pure function: runs natively and under WASM.
pub fn reachable_json(start: &str, edges_json: &str) -> String {
    match parse_edges(edges_json) {
        Ok(edges) => serde_json::to_string(&dag::reachable(start, &edges))
            .unwrap_or_else(|e| err_json(&e.to_string())),
        Err(e) => err_json(&e),
    }
}

/// Topological order of `nodes_json` as a JSON array (parents first);
/// cycles and bad input surface as `{"error":"..."}`.
pub fn topo_order_json(nodes_json: &str, edges_json: &str) -> String {
    let (nodes, edges) = match (parse_nodes(nodes_json), parse_edges(edges_json)) {
        (Ok(n), Ok(e)) => (n, e),
        (Err(e), _) | (_, Err(e)) => return err_json(&e),
    };
    match dag::topo_order(&nodes, &edges) {
        Ok(order) => serde_json::to_string(&order).unwrap_or_else(|e| err_json(&e.to_string())),
        Err(e) => err_json(&e.to_string()),
    }
}

/// WASM boundary: `reachable(start, edges_json) -> Json` (Node/Workers
/// call `JSON.parse` only if they want text; here they get a JS value).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn reachable(start: &str, edges_json: &str) -> JsValue {
    js_sys::JSON::parse(&reachable_json(start, edges_json)).unwrap_or(JsValue::NULL)
}

/// WASM boundary: `topo_order(nodes_json, edges_json) -> Json`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn topo_order(nodes_json: &str, edges_json: &str) -> JsValue {
    js_sys::JSON::parse(&topo_order_json(nodes_json, edges_json)).unwrap_or(JsValue::NULL)
}
