// Minimal agent forge loop: git branch checks, fast CI, main-only deploy.
// REQ-F-017 (issues/PRs API + fast CI profile), REQ-F-018 (deploy gates).
//
// Credentials policy: deploy credentials are env-only at F3. This module
// NEVER reads credentials from env, files, or the DB; `deploy` only prints
// a plan and never executes it.
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use crate::error::{AtlasError, Result};
use crate::model::DeployTarget;
use crate::store::Store;

/// Fast CI profile name recorded on every `atlas ci` run.
pub const FAST_PROFILE: &str = "fast";

/// Outcome of one fast-CI run: `cargo test --offline` + `cargo fmt --check`.
#[derive(Debug, Clone)]
pub struct CiOutcome {
    pub passed: bool,
    pub evidence: String,
}

/// True when `branch` resolves in the repo via `git rev-parse --verify`.
/// Ghost (nonexistent) branches return false; that rejects the PR create.
#[must_use]
pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    if branch.trim().is_empty() {
        return false;
    }
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg("--verify")
        .arg(branch)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Current branch name of the repo (`git rev-parse --abbrev-ref HEAD`).
pub fn current_branch(repo: &Path) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output()
        .map_err(|e| AtlasError::Forge(format!("git rev-parse failed: {e}")))?;
    if !out.status.success() {
        return Err(AtlasError::Forge(format!(
            "not a git repo (or no HEAD): {}",
            repo.display()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Full HEAD sha of the repo (`git rev-parse HEAD`).
pub fn head_sha(repo: &Path) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .map_err(|e| AtlasError::Forge(format!("git rev-parse failed: {e}")))?;
    if !out.status.success() {
        return Err(AtlasError::Forge(format!(
            "cannot resolve HEAD in {}",
            repo.display()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Keep only the tail of a command log so evidence rows stay small.
fn tail(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let skip = lines.len().saturating_sub(max_lines);
    lines[skip..].join("\n")
}

/// Run one command in `repo`, returning (success, combined output).
fn run_cmd(repo: &Path, prog: &str, args: &[&str]) -> (bool, String) {
    let result = Command::new(prog).current_dir(repo).args(args).output();
    match result {
        Ok(o) => {
            let mut combined = String::from_utf8_lossy(&o.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.is_empty() {
                combined.push_str("\n--- stderr ---\n");
                combined.push_str(&stderr);
            }
            (o.status.success(), combined)
        }
        Err(e) => (false, format!("failed to spawn {prog}: {e}")),
    }
}

/// Fast CI profile (REQ-F-017): `cargo test --offline` plus
/// `cargo fmt --check`, both in the repo dir (offline-first).
/// Evidence carries exit codes plus the tail of each log.
pub fn run_fast_ci(repo: &Path) -> CiOutcome {
    let (tests_ok, tests_log) = run_cmd(repo, "cargo", &["test", "--offline"]);
    let (fmt_ok, fmt_log) = run_cmd(repo, "cargo", &["fmt", "--check"]);
    let passed = tests_ok && fmt_ok;
    let verdict = if passed { "PASS" } else { "FAIL" };
    let evidence = format!(
        "fast CI: {verdict}\n\
         cargo test --offline: {}\n{}\n\
         cargo fmt --check: {}\n{}",
        if tests_ok { "ok" } else { "FAILED" },
        tail(&tests_log, 30),
        if fmt_ok { "ok" } else { "FAILED" },
        tail(&fmt_log, 15),
    );
    CiOutcome { passed, evidence }
}

/// Pure deploy gate (REQ-F-018): branch MUST be `main` and fast CI must
/// have passed for HEAD. Returns the printable plan; executes nothing.
pub fn deploy_plan(
    branch: &str,
    head: &str,
    ci_passed_for_head: bool,
    target: DeployTarget,
    repo: &Path,
) -> Result<String> {
    if branch != "main" {
        return Err(AtlasError::Forge(format!(
            "deploy rejected: current branch is '{branch}', must be 'main'"
        )));
    }
    if !ci_passed_for_head {
        return Err(AtlasError::Forge(format!(
            "deploy rejected: no passing fast CI for HEAD {head}"
        )));
    }
    let steps = match target {
        DeployTarget::Vps => {
            "  1. cargo build --offline --release\n  2. rsync binary to VPS\n  3. restart service + health check\n  4. rollback: keep previous binary, restore on failed check"
        }
        DeployTarget::Cloudrun => {
            "  1. cargo build --offline --release\n  2. docker build + push image\n  3. gcloud run deploy (new revision, gradual traffic)\n  4. rollback: route traffic back to previous revision"
        }
    };
    Ok(format!(
        "deploy plan [{target}] (printed only, not executed)\n\
         repo: {} branch: main head: {head}\n\
         gates: branch==main OK; fast CI PASS for HEAD OK\n\
         steps:\n{steps}\n\
         credentials: env-only (F3), none read",
        repo.display()
    ))
}

/// Store-backed deploy gate: resolves branch + HEAD from the repo, checks
/// the latest CI record for HEAD, then prints (returns) the plan.
pub fn deploy_gate(store: &Store, repo: &Path, target: &str) -> Result<String> {
    let want = DeployTarget::from_str(target)?;
    let branch = current_branch(repo)?;
    let head = head_sha(repo)?;
    let ci_ok = store.ci_latest_for_head(&head)?.is_some_and(|r| r.passed);
    deploy_plan(&branch, &head, ci_ok, want, repo)
}

/// Validated PR create: the branch must exist locally
/// (`git rev-parse --verify`), else the ghost branch is rejected.
pub fn pr_create_validated(
    store: &Store,
    repo: &Path,
    title: &str,
    base: &str,
    branch: &str,
) -> Result<i64> {
    if !branch_exists(repo, branch) {
        return Err(AtlasError::Forge(format!(
            "unknown branch '{branch}': git rev-parse --verify failed"
        )));
    }
    store.pr_create(title, base, branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_repo(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atlas_forge_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let run = |args: &[&str]| {
            let st = Command::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .expect("git spawn");
            assert!(st.status.success(), "git {args:?}: {}", {
                String::from_utf8_lossy(&st.stderr).into_owned()
            });
        };
        run(&["init", "-b", "main"]);
        run(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ]);
        dir
    }

    #[test]
    fn issue_lifecycle_open_list_close() {
        let store = Store::open_in_memory().expect("open");
        let id = store.issue_create("bug", "body text").expect("create");
        let issues = store.issue_list().expect("list");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].id, id);
        assert_eq!(issues[0].state, crate::model::IssueState::Open);
        let closed = store.issue_close(&id).expect("close");
        assert_eq!(closed.state, crate::model::IssueState::Closed);
        // Double close is rejected.
        assert!(store.issue_close(&id).is_err());
    }

    #[test]
    fn pr_branch_validation_rejects_ghost_branch() {
        let repo = git_repo("ghost");
        assert!(branch_exists(&repo, "main"));
        assert!(!branch_exists(&repo, "ghost-branch-xyz"));
        let store = Store::open_in_memory().expect("open");
        let err = pr_create_validated(&store, &repo, "t", "main", "ghost-branch-xyz")
            .expect_err("ghost branch must fail");
        assert!(matches!(err, AtlasError::Forge(_)), "{err:?}");
        let id = pr_create_validated(&store, &repo, "t", "main", "main").expect("real branch");
        assert_eq!(store.pr_get(id).expect("get").branch, "main");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn ci_records_pass_fail_evidence() {
        let store = Store::open_in_memory().expect("open");
        let pr = store.pr_create("feat", "main", "main").expect("pr");
        let row = store
            .ci_record(pr, "abc123", FAST_PROFILE, true, "fast CI: PASS")
            .expect("record");
        assert!(row > 0);
        let latest = store.ci_latest_for_head("abc123").expect("latest");
        let rec = latest.expect("present");
        assert!(rec.passed);
        assert_eq!(rec.pr_id, pr);
        assert!(rec.evidence.contains("PASS"));
        assert_eq!(store.ci_list_for_pr(pr).expect("list").len(), 1);
        // Unknown PRs cannot collect CI.
        assert!(
            store
                .ci_record(9999, "abc123", FAST_PROFILE, true, "x")
                .is_err()
        );
    }

    #[test]
    fn deploy_rejects_non_main_and_missing_ci() {
        let repo = std::path::Path::new(".");
        // Non-main branch rejected even with green CI.
        let err = deploy_plan("feature-x", "abc", true, DeployTarget::Vps, repo)
            .expect_err("non-main must fail");
        assert!(err.to_string().contains("must be 'main'"), "{err}");
        // main without passing CI rejected.
        let err = deploy_plan("main", "abc", false, DeployTarget::Vps, repo)
            .expect_err("missing CI must fail");
        assert!(err.to_string().contains("no passing fast CI"), "{err}");
        // main + green CI prints the plan (executes nothing).
        let plan = deploy_plan("main", "abc", true, DeployTarget::Cloudrun, repo).expect("plan");
        assert!(plan.contains("cloudrun"));
        assert!(plan.contains("not executed"));
        // Unknown target rejected.
        assert!("baremetal".parse::<DeployTarget>().is_err());
    }
}
