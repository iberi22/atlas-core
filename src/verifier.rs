// Rule verifier: promotion gate for COMPLETED (REQ-F-012/013).
// The rule layer (DoD checklist + required evidence) decides; the LLM
// reviewer behind `Reviewer` is advisory only and never blocks.
use crate::model::VerifyReport;
use crate::store::Store;

/// Advisory verdict of an LLM reviewer; recorded, never blocking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    Approve,
    Defer(String),
    Reject(String),
}

impl ReviewDecision {
    /// Short tag stored on the verify report.
    #[must_use]
    pub fn tag(&self) -> String {
        match self {
            Self::Approve => "approve".to_owned(),
            Self::Defer(r) => format!("deferred:{r}"),
            Self::Reject(r) => format!("rejected:{r}"),
        }
    }
}

/// Pluggable LLM reviewer. Real implementations call a model; the
/// bundled `StubReviewer` always defers so offline runs stay green.
pub trait Reviewer {
    /// Stable reviewer name recorded on every report.
    fn name(&self) -> &'static str;
    /// Advisory verdict for a task given its rule report.
    fn review(&self, task_id: &str, report: &VerifyReport) -> ReviewDecision;
}

/// Non-blocking placeholder reviewer: always defers (REQ-F-013 stub).
pub struct StubReviewer;

impl Reviewer for StubReviewer {
    fn name(&self) -> &'static str {
        "stub"
    }

    fn review(&self, _task_id: &str, _report: &VerifyReport) -> ReviewDecision {
        ReviewDecision::Defer("no LLM configured".to_owned())
    }
}

/// Run the rule layer plus one reviewer over a task.
/// Fails only when rules fail; a deferring/rejecting reviewer is
/// recorded on the report without changing the verdict.
pub fn verify_with(
    store: &Store,
    reviewer: &dyn Reviewer,
    task_id: &str,
) -> crate::Result<VerifyReport> {
    let items = store.dod_list(task_id)?;
    let evidence = store.evidence_list(task_id)?;
    let total = items.len() as i64;
    let checked = items.iter().filter(|i| i.checked).count() as i64;
    let mut failures = Vec::new();
    if total == 0 {
        failures.push("dod: no checklist items".to_owned());
    } else if checked < total {
        failures.push(format!("dod: {checked}/{total} checked"));
    }
    if evidence.is_empty() {
        failures.push("evidence: none attached".to_owned());
    }
    let passed = failures.is_empty();
    let draft = VerifyReport {
        task_id: task_id.to_owned(),
        passed,
        failures,
        dod_total: total,
        dod_checked: checked,
        evidence_count: evidence.len() as i64,
        reviewer: String::new(),
    };
    let decision = reviewer.review(task_id, &draft);
    let mut report = draft;
    report.reviewer = format!("{}:{}", reviewer.name(), decision.tag());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(task: &str) -> VerifyReport {
        VerifyReport {
            task_id: task.to_owned(),
            passed: true,
            failures: Vec::new(),
            dod_total: 1,
            dod_checked: 1,
            evidence_count: 1,
            reviewer: String::new(),
        }
    }

    #[test]
    fn stub_defers() {
        let stub = StubReviewer;
        assert_eq!(stub.name(), "stub");
        let decision = stub.review("t_x", &report("t_x"));
        assert!(
            matches!(decision, ReviewDecision::Defer(_)),
            "stub must always defer, got {decision:?}"
        );
    }

    #[test]
    fn promote_blocked_without_dod() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("gate me").expect("session");
        let t = store.create_task(&s, "work", None, &[]).expect("task");
        store.run_task(&t).expect("run");
        // No DoD items and no evidence: the report fails ...
        let rep = store.verify(&t).expect("verify");
        assert!(!rep.passed, "must fail without dod/evidence");
        assert_eq!(rep.dod_total, 0);
        assert_eq!(rep.evidence_count, 0);
        // ... and the self-declaration path is rejected.
        let err = store
            .complete_task(&t)
            .expect_err("agent must not self-promote");
        assert!(
            err.to_string().contains("verifier"),
            "unexpected error: {err}"
        );
        assert_eq!(
            store.get_task(&t).expect("get").state,
            crate::model::TaskState::InProgress
        );
    }

    #[test]
    fn promote_ok_with_dod_plus_evidence() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("ship it").expect("session");
        let t = store.create_task(&s, "work", None, &[]).expect("task");
        store.run_task(&t).expect("run");
        store.dod_add(&t, "tests green").expect("dod1");
        store.dod_add(&t, "artifacts stored").expect("dod2");
        store.dod_check(&t, 1).expect("check1");
        // One unchecked item still blocks.
        assert!(store.complete_task(&t).is_err());
        store.dod_check(&t, 2).expect("check2");
        // DoD done but no evidence still blocks.
        assert!(store.complete_task(&t).is_err());
        store.evidence_add(&t, "log: all tests passed").expect("ev");
        // Stub reviewer defers, yet promotion succeeds (non-blocking).
        let rep = store.verify(&t).expect("verify");
        assert!(rep.passed, "failures: {:?}", rep.failures);
        assert!(
            rep.reviewer.starts_with("stub:deferred"),
            "{}",
            rep.reviewer
        );
        let done = store.complete_task(&t).expect("promote");
        assert_eq!(done.state, crate::model::TaskState::Completed);
    }

    #[test]
    fn tick_skips_unverified() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("chain").expect("session");
        let t = store.create_task(&s, "work", None, &[]).expect("task");
        store.run_task(&t).expect("run");
        store.finish_task(&t, true, "").expect("finish");
        // Gate fails: completion event stays queued, tick counts a skip.
        // (Informational events like session_started also skip, hence >= 1.)
        let sum = store.tick_once().expect("tick");
        assert!(sum.skipped >= 1, "unverified completion must skip");
        assert_eq!(sum.unlocked, 0);
        assert_eq!(store.pending_count().expect("pending"), 1);
        assert_eq!(
            store.get_task(&t).expect("get").state,
            crate::model::TaskState::InProgress
        );
        // Satisfy the gate; the SAME queued event now promotes.
        store.dod_add(&t, "done").expect("dod");
        store.dod_check(&t, 1).expect("check");
        store.evidence_add(&t, "output attached").expect("ev");
        let sum2 = store.tick_once().expect("tick2");
        assert_eq!(sum2.unlocked, 1);
        assert_eq!(
            store.get_task(&t).expect("get").state,
            crate::model::TaskState::Completed
        );
        assert_eq!(store.pending_count().expect("pending"), 0);
    }
}
