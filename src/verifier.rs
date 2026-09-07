// Rule verifier: promotion gate for COMPLETED (REQ-F-012/013).
// The rule layer (DoD checklist + required evidence) decides; the LLM
// reviewer behind `Reviewer` is advisory only and never blocks.
use crate::model::VerifyReport;
use crate::store::Store;
use std::io::Write;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

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

/// Env var holding the external reviewer command (a shell snippet run
/// via `sh -c`). When unset or blank, callers fall back to `StubReviewer`.
pub const REVIEW_CMD_ENV: &str = "ATLAS_REVIEW_CMD";
/// Env var overriding the reviewer wait in milliseconds.
pub const REVIEW_TIMEOUT_ENV: &str = "ATLAS_REVIEW_TIMEOUT_MS";
/// Default wait for the reviewer command before deferring.
pub const DEFAULT_REVIEW_TIMEOUT_MS: u64 = 60_000;

/// External reviewer (REQ-F-013 real gap): runs the `ATLAS_REVIEW_CMD`
/// shell command, feeds task + rule criteria as JSON on stdin, and reads
/// a verdict line `{"verdict":"approve"|"reject","notes":"..."}` from
/// stdout. Any failure (spawn, timeout, bad JSON, nonzero exit) degrades
/// to `Defer`, so the reviewer stays advisory and never blocks promotion.
#[derive(Debug, Clone)]
pub struct CommandReviewer {
    cmd: String,
    timeout_ms: u64,
}

impl CommandReviewer {
    /// Build from an explicit command and timeout (used by tests and CLI).
    #[must_use]
    pub fn new(cmd: impl Into<String>, timeout_ms: u64) -> Self {
        Self {
            cmd: cmd.into(),
            timeout_ms,
        }
    }

    /// Build from `ATLAS_REVIEW_CMD` / `ATLAS_REVIEW_TIMEOUT_MS`.
    /// Returns `None` when no command is configured, so the caller can
    /// keep the `StubReviewer` behaviour intact.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let cmd = std::env::var(REVIEW_CMD_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        Some(Self {
            cmd,
            timeout_ms: resolve_timeout_ms(),
        })
    }

    /// The configured shell snippet.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.cmd
    }

    /// Max wait for the reviewer command.
    #[must_use]
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
}

/// Parse `ATLAS_REVIEW_TIMEOUT_MS`; missing, unparsable, or zero values
/// fall back to the 60s default.
fn resolve_timeout_ms() -> u64 {
    std::env::var(REVIEW_TIMEOUT_ENV)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_REVIEW_TIMEOUT_MS)
}

impl Reviewer for CommandReviewer {
    fn name(&self) -> &'static str {
        "cmd"
    }

    fn review(&self, task_id: &str, report: &VerifyReport) -> ReviewDecision {
        let input = serde_json::json!({
            "task_id": task_id,
            "passed": report.passed,
            "failures": report.failures,
            "dod_total": report.dod_total,
            "dod_checked": report.dod_checked,
            "evidence_count": report.evidence_count,
        });
        run_review_cmd(&self.cmd, input.to_string().as_bytes(), self.timeout_ms)
    }
}

/// Spawn `cmd` via `sh -c`, hand it `stdin_bytes`, and wait up to
/// `timeout_ms` for a verdict. Never fails hard: every error path defers.
fn run_review_cmd(cmd: &str, stdin_bytes: &[u8], timeout_ms: u64) -> ReviewDecision {
    let mut child = match std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return ReviewDecision::Defer(format!("review spawn failed: {e}")),
    };
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(stdin_bytes)
    {
        let _ = child.kill();
        let _ = child.wait();
        return ReviewDecision::Defer(format!("review stdin failed: {e}"));
    }
    // `stdin` drops here: the pipe closes so the child sees EOF.
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => match child.wait_with_output() {
                Ok(out) => return parse_review_output(out.status, &out.stdout, &out.stderr),
                Err(e) => return ReviewDecision::Defer(format!("review output failed: {e}")),
            },
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ReviewDecision::Defer(format!("review timeout after {timeout_ms}ms"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                let _ = child.kill();
                return ReviewDecision::Defer(format!("review wait failed: {e}"));
            }
        }
    }
}

/// Turn a finished reviewer process into an advisory decision.
fn parse_review_output(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> ReviewDecision {
    if !status.success() {
        let tail = truncate_lossy(stderr, 200);
        return ReviewDecision::Defer(format!("review command failed ({status}): {tail}"));
    }
    let value: serde_json::Value = match serde_json::from_slice(stdout) {
        Ok(value) => value,
        Err(e) => return ReviewDecision::Defer(format!("review bad JSON: {e}")),
    };
    let verdict = value
        .get("verdict")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let notes = value
        .get("notes")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    match verdict {
        "approve" => ReviewDecision::Approve,
        "reject" => {
            if notes.is_empty() {
                ReviewDecision::Reject("rejected by reviewer".to_owned())
            } else {
                ReviewDecision::Reject(notes)
            }
        }
        other => ReviewDecision::Defer(format!("review: unknown verdict '{other}'")),
    }
}

/// Lossy stderr snippet capped at `max` chars for defer messages.
fn truncate_lossy(bytes: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= max {
        text.into_owned()
    } else {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &text[..end])
    }
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

    // --- CommandReviewer (/bin/sh stubs, REQ-F-013 real gap) ---

    use std::sync::atomic::{AtomicU64, Ordering};

    static STUB_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Write an executable `/bin/sh` stub that first drains stdin (the
    /// task+criteria JSON) and then runs `body`.
    fn stub_script(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atlas-review-{}-{name}-{}",
            std::process::id(),
            STUB_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("stub tmpdir");
        let path = dir.join("review.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("stub write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("stub chmod");
        }
        path
    }

    fn review_with_stub(body: &str, timeout_ms: u64) -> ReviewDecision {
        let path = stub_script("case", body);
        CommandReviewer::new(path.to_str().expect("tmp path").to_owned(), timeout_ms)
            .review("t_x", &report("t_x"))
    }

    #[test]
    fn command_approves() {
        let decision = review_with_stub(
            "cat >/dev/null\necho '{\"verdict\":\"approve\",\"notes\":\"lgtm\"}'",
            5_000,
        );
        assert_eq!(decision, ReviewDecision::Approve);
    }

    #[test]
    fn command_rejects_with_notes() {
        let decision = review_with_stub(
            "cat >/dev/null\necho '{\"verdict\":\"reject\",\"notes\":\"tests missing\"}'",
            5_000,
        );
        assert_eq!(decision, ReviewDecision::Reject("tests missing".to_owned()));
    }

    #[test]
    fn command_timeout_defers() {
        let decision = review_with_stub("cat >/dev/null\nsleep 5", 150);
        assert!(
            matches!(&decision, ReviewDecision::Defer(m) if m.contains("timeout")),
            "slow reviewer must defer, got {decision:?}"
        );
    }

    #[test]
    fn command_bad_json_defers() {
        let decision = review_with_stub("cat >/dev/null\necho 'not json'", 5_000);
        assert!(
            matches!(&decision, ReviewDecision::Defer(m) if m.contains("bad JSON")),
            "garbage output must defer, got {decision:?}"
        );
    }

    #[test]
    fn command_failing_exit_defers() {
        let decision = review_with_stub("cat >/dev/null\necho boom >&2\nexit 3", 5_000);
        assert!(
            matches!(&decision, ReviewDecision::Defer(m) if m.contains("failed")),
            "nonzero exit must defer, got {decision:?}"
        );
    }

    #[test]
    fn verify_with_command_reviewer_records_verdict() {
        let store = Store::open_in_memory().expect("open");
        let s = store.create_session("reviewed").expect("session");
        let t = store.create_task(&s, "work", None, &[]).expect("task");
        store.run_task(&t).expect("run");
        store.dod_add(&t, "tests green").expect("dod");
        store.dod_check(&t, 1).expect("check");
        store.evidence_add(&t, "log: all tests passed").expect("ev");
        let path = stub_script(
            "approve",
            "cat >/dev/null\necho '{\"verdict\":\"approve\",\"notes\":\"ok\"}'",
        );
        let reviewer = CommandReviewer::new(path.to_str().expect("tmp path").to_owned(), 5_000);
        let rep = store.verify_with(&reviewer, &t).expect("verify");
        assert!(rep.passed, "failures: {:?}", rep.failures);
        assert!(
            rep.reviewer.starts_with("cmd:approve"),
            "unexpected reviewer tag: {}",
            rep.reviewer
        );
    }

    #[test]
    fn review_env_config() {
        // Sole test touching the process env (other tests use `new`
        // directly), so it cannot race with parallel tests.
        unsafe {
            std::env::remove_var(REVIEW_CMD_ENV);
            std::env::remove_var(REVIEW_TIMEOUT_ENV);
        }
        assert!(
            CommandReviewer::from_env().is_none(),
            "unset cmd must defer to stub"
        );
        unsafe {
            std::env::set_var(REVIEW_CMD_ENV, "true");
            std::env::set_var(REVIEW_TIMEOUT_ENV, "bogus");
        }
        let reviewer = CommandReviewer::from_env().expect("configured reviewer");
        assert_eq!(reviewer.command(), "true");
        assert_eq!(reviewer.timeout_ms(), DEFAULT_REVIEW_TIMEOUT_MS);
        unsafe {
            std::env::set_var(REVIEW_TIMEOUT_ENV, "1500");
        }
        assert_eq!(CommandReviewer::from_env().expect("cfg").timeout_ms(), 1500);
        unsafe {
            std::env::set_var(REVIEW_CMD_ENV, "   ");
        }
        assert!(
            CommandReviewer::from_env().is_none(),
            "blank cmd must defer to stub"
        );
        unsafe {
            std::env::remove_var(REVIEW_CMD_ENV);
            std::env::remove_var(REVIEW_TIMEOUT_ENV);
        }
    }
}
