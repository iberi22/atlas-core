// Estimator for per-task actuals and whole-tree rollups (REQ-F-014/015).
// Moving averages group metrics rows by title prefix (first whitespace
// token, lowercased); tasks with no history fall back to flat defaults.
// A task's own metric row is excluded from its estimate so drift
// (actual - estimate) stays an honest prediction error.
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::Store;

/// Flat fallback when no same-prefix history exists (REQ-F-014).
pub const DEFAULT_DURATION_MS: i64 = 120_000;
/// Flat fallback for prompt tokens with no history.
pub const DEFAULT_PROMPT_TOKENS: i64 = 2000;
/// Flat fallback for completion tokens with no history.
pub const DEFAULT_COMPLETION_TOKENS: i64 = 1000;

/// One recorded actual for a task (one row of the `metrics` table).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricActual {
    pub task_id: String,
    pub duration_ms: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub outcome: String,
}

/// Predicted cost of one task plus its actual when recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEstimate {
    pub task_id: String,
    pub title: String,
    pub duration_ms: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// History rows averaged; zero when the flat default applied.
    pub samples: usize,
    pub from_history: bool,
    pub actual: Option<MetricActual>,
}

/// Whole-tree rollup: per-task estimates plus dependency-aware totals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rollup {
    pub per_task: Vec<TaskEstimate>,
    /// Flat sum of every per-task estimate (parallel-ignorant total).
    pub total_sum_ms: i64,
    /// Longest dependency path through estimated durations.
    pub critical_path_ms: i64,
    /// Task ids on the critical path, parents first.
    pub critical_path: Vec<String>,
    /// Sum of (actual - estimate) over tasks that have actuals.
    pub drift_ms: i64,
    pub actuals_count: usize,
}

/// Grouping key for history lookup: first token, lowercased.
#[must_use]
pub fn title_prefix(title: &str) -> String {
    title.split_whitespace().next().unwrap_or("").to_lowercase()
}

/// Estimate one task from same-prefix history (own row excluded).
pub fn estimate_task(store: &Store, task_id: &str) -> Result<TaskEstimate> {
    let task = store.get_task(task_id)?;
    let prefix = title_prefix(&task.title);
    let rows = store.metrics_for_prefix(&prefix, Some(task_id))?;
    let (duration_ms, prompt_tokens, completion_tokens, samples, from_history) =
        if rows.is_empty() || prefix.is_empty() {
            (
                DEFAULT_DURATION_MS,
                DEFAULT_PROMPT_TOKENS,
                DEFAULT_COMPLETION_TOKENS,
                0,
                false,
            )
        } else {
            let n = rows.len() as i64;
            let avg = |sum: i64| sum / n;
            (
                avg(rows.iter().map(|r| r.duration_ms).sum()),
                avg(rows.iter().map(|r| r.prompt_tokens).sum()),
                avg(rows.iter().map(|r| r.completion_tokens).sum()),
                rows.len(),
                true,
            )
        };
    Ok(TaskEstimate {
        task_id: task.id.clone(),
        title: task.title.clone(),
        duration_ms,
        prompt_tokens,
        completion_tokens,
        samples,
        from_history,
        actual: store.get_metric(task_id)?,
    })
}

/// Roll up every task of a session (or all tasks when `session` is None).
/// Totals respect dependencies: the critical path is the longest
/// parents-first path through per-task estimated durations.
pub fn estimate_session(store: &Store, session: Option<&str>) -> Result<Rollup> {
    let tasks = store.list_tasks(session)?;
    let mut per_task = Vec::with_capacity(tasks.len());
    for t in &tasks {
        per_task.push(estimate_task(store, &t.id)?);
    }
    let est: std::collections::HashMap<&str, i64> = per_task
        .iter()
        .map(|e| (e.task_id.as_str(), e.duration_ms))
        .collect();
    // Parents-first order; fall back to creation order on hand-edited cycles.
    let order: Vec<String> = store
        .topo_sorted(session)
        .map(|ts| ts.iter().map(|t| t.id.clone()).collect())
        .unwrap_or_else(|_| tasks.iter().map(|t| t.id.clone()).collect());
    // Longest-path DP over (child -> parents) edges.
    let mut dist: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut prev: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for id in &order {
        let own = est.get(id.as_str()).copied().unwrap_or(DEFAULT_DURATION_MS);
        let mut best_parent: Option<String> = None;
        let mut best_dist: i64 = 0;
        if let Ok(parents) = store.parents_of(id) {
            for p in &parents {
                if session.is_some_and(|s| p.session_id != s) {
                    continue;
                }
                match dist.get(&p.id) {
                    Some(&d) if d > best_dist => {
                        best_dist = d;
                        best_parent = Some(p.id.clone());
                    }
                    _ => {}
                }
            }
        }
        dist.insert(id.clone(), best_dist + own);
        prev.insert(id.clone(), best_parent);
    }
    let (critical_end, critical_path_ms) = dist
        .iter()
        .max_by_key(|(_, d)| **d)
        .map(|(id, d)| (id.clone(), *d))
        .unwrap_or_default();
    let mut critical_path = Vec::new();
    let mut cursor: Option<String> = if critical_path_ms > 0 {
        Some(critical_end)
    } else {
        None
    };
    while let Some(id) = cursor {
        critical_path.push(id.clone());
        cursor = prev.get(&id).cloned().flatten();
    }
    critical_path.reverse();
    let total_sum_ms = per_task.iter().map(|e| e.duration_ms).sum();
    let mut drift_ms: i64 = 0;
    let mut actuals_count: usize = 0;
    for e in &per_task {
        if let Some(a) = &e.actual {
            drift_ms += a.duration_ms - e.duration_ms;
            actuals_count += 1;
        }
    }
    Ok(Rollup {
        per_task,
        total_sum_ms,
        critical_path_ms,
        critical_path,
        drift_ms,
        actuals_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_roundtrip() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("est").expect("session");
        let t = store.create_task(&s, "build api", None, &[]).expect("task");
        store
            .record_metric(&t, 1500, 100, 50, "ok")
            .expect("record");
        let got = store.get_metric(&t).expect("get").expect("present");
        assert_eq!(got.task_id, t);
        assert_eq!(got.duration_ms, 1500);
        assert_eq!(got.prompt_tokens, 100);
        assert_eq!(got.completion_tokens, 50);
        assert_eq!(got.outcome, "ok");
        // Recording on a missing task is rejected.
        assert!(store.record_metric("t_missing", 1, 1, 1, "ok").is_err());
    }

    #[test]
    fn average_math() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("avg").expect("session");
        let a = store.create_task(&s, "build api", None, &[]).expect("a");
        let b = store.create_task(&s, "build web", None, &[]).expect("b");
        store.record_metric(&a, 1000, 100, 50, "ok").expect("m");
        store.record_metric(&b, 3000, 300, 150, "ok").expect("m");
        let c = store.create_task(&s, "build other", None, &[]).expect("c");
        let est = estimate_task(&store, &c).expect("estimate");
        assert!(est.from_history, "expected history average");
        assert_eq!(est.samples, 2);
        assert_eq!(est.duration_ms, 2000);
        assert_eq!(est.prompt_tokens, 200);
        assert_eq!(est.completion_tokens, 100);
        // Own row excluded: estimate for `a` averages only `b`.
        let est_a = estimate_task(&store, &a).expect("estimate a");
        assert_eq!(est_a.samples, 1);
        assert_eq!(est_a.duration_ms, 3000);
        // Unknown prefix falls back to flat defaults.
        let other = store.create_task(&s, "zzz-lonely", None, &[]).expect("o");
        let def = estimate_task(&store, &other).expect("default");
        assert!(!def.from_history);
        assert_eq!(def.duration_ms, DEFAULT_DURATION_MS);
        assert_eq!(def.prompt_tokens, DEFAULT_PROMPT_TOKENS);
        assert_eq!(def.completion_tokens, DEFAULT_COMPLETION_TOKENS);
    }

    #[test]
    fn critical_path_rollup() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("cp").expect("session");
        // Unique prefixes -> every task estimates the flat default.
        let a = store.create_task(&s, "aaa first", None, &[]).expect("a");
        let b = store.create_task(&s, "bbb second", None, &[&a]).expect("b");
        let _c = store.create_task(&s, "ccc side", None, &[]).expect("c");
        let roll = estimate_session(&store, Some(&s)).expect("rollup");
        assert_eq!(roll.per_task.len(), 3);
        assert_eq!(roll.total_sum_ms, 3 * DEFAULT_DURATION_MS);
        assert_eq!(roll.critical_path_ms, 2 * DEFAULT_DURATION_MS);
        assert_eq!(roll.critical_path, vec![a, b]);
    }

    #[test]
    fn drift_report() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("drift").expect("session");
        let a = store.create_task(&s, "solo-a work", None, &[]).expect("a");
        let b = store.create_task(&s, "solo-b work", None, &[]).expect("b");
        // `a` runs faster than the default prediction.
        store.record_metric(&a, 60_000, 100, 50, "ok").expect("m");
        let roll = estimate_session(&store, Some(&s)).expect("rollup");
        assert_eq!(roll.actuals_count, 1);
        // Own row excluded -> estimate stays the default, drift is honest.
        assert_eq!(roll.drift_ms, 60_000 - DEFAULT_DURATION_MS);
        let est_a = roll.per_task.iter().find(|e| e.task_id == a).expect("a");
        assert!(est_a.actual.is_some());
        assert_eq!(est_a.duration_ms, DEFAULT_DURATION_MS);
        let _ = b;
    }
}
