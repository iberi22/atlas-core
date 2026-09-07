// Minimal agent forge loop: git branch checks, fast CI, main-only deploy.
// REQ-F-017 (issues/PRs API + fast CI profile), REQ-F-018 (deploy gates +
// real execute via Transport).
//
// Credentials policy: deploy credentials are env-only at F3. This module
// NEVER reads credentials from env, files, or the DB. `deploy` without
// `--execute` only prints a plan and never executes it; `--execute` runs
// gates plus a real Transport against the destination configured by
// ATLAS_DEPLOY_DIR (no production default, ever).
use std::path::{Path, PathBuf};
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

/// Env var selecting the real deploy destination for `--execute`.
/// No default: unset/empty is an error, never a guessed production path.
pub const DEPLOY_DIR_ENV: &str = "ATLAS_DEPLOY_DIR";

/// Real deploy transport (REQ-F-018 execute path).
///
/// Safety: implementations must only touch the configured destination;
/// tests use `LocalDirTransport` against temp fixture dirs, never servers.
pub trait Transport {
    /// Ship `binary` to the destination (keep a backup for rollback).
    fn ship_binary(&self, binary: &Path) -> Result<String>;
    /// Restart the service at the destination.
    fn restart(&self) -> Result<String>;
    /// True when the destination looks healthy after restart.
    fn health_check(&self) -> Result<bool>;
    /// Restore the pre-ship binary. Called on failed health checks.
    fn rollback(&self) -> Result<String>;
}

/// Test/fixture transport: deploys into a plain local directory.
/// Layout: `<dir>/atlas.bin` (shipped binary), `<dir>/atlas.bin.bak`
/// (pre-ship backup), `<dir>/healthy` (restart marker = health signal).
#[derive(Debug, Clone)]
pub struct LocalDirTransport {
    dir: PathBuf,
}

impl LocalDirTransport {
    /// Deploy destination dir (created on ship).
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Destination dir under management.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn binary_path(&self) -> PathBuf {
        self.dir.join("atlas.bin")
    }

    fn backup_path(&self) -> PathBuf {
        self.dir.join("atlas.bin.bak")
    }

    fn marker_path(&self) -> PathBuf {
        self.dir.join("healthy")
    }
}

impl Transport for LocalDirTransport {
    fn ship_binary(&self, binary: &Path) -> Result<String> {
        if !binary.is_file() {
            return Err(AtlasError::Forge(format!(
                "binary not found: {}",
                binary.display()
            )));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            AtlasError::Forge(format!(
                "cannot create deploy dir {}: {e}",
                self.dir.display()
            ))
        })?;
        let dest = self.binary_path();
        if dest.exists() {
            std::fs::copy(&dest, self.backup_path())
                .map_err(|e| AtlasError::Forge(format!("cannot back up previous binary: {e}")))?;
        }
        std::fs::copy(binary, &dest)
            .map_err(|e| AtlasError::Forge(format!("cannot ship binary: {e}")))?;
        Ok(format!(
            "shipped {} -> {}",
            binary.display(),
            dest.display()
        ))
    }

    fn restart(&self) -> Result<String> {
        std::fs::write(self.marker_path(), "restarted\n")
            .map_err(|e| AtlasError::Forge(format!("restart failed: {e}")))?;
        Ok(format!(
            "restarted (marker {})",
            self.marker_path().display()
        ))
    }

    fn health_check(&self) -> Result<bool> {
        Ok(self.marker_path().is_file())
    }

    fn rollback(&self) -> Result<String> {
        let bak = self.backup_path();
        if !bak.is_file() {
            return Err(AtlasError::Forge(
                "rollback: no backup binary to restore".to_owned(),
            ));
        }
        std::fs::copy(&bak, self.binary_path())
            .map_err(|e| AtlasError::Forge(format!("rollback copy failed: {e}")))?;
        std::fs::remove_file(self.marker_path()).ok();
        Ok(format!(
            "rollback: restored {} from backup",
            self.binary_path().display()
        ))
    }
}

/// Honest SSH skeleton: every operation fails with not-implemented.
/// No command is ever spawned; real SSH arrives at F3, not here.
#[derive(Debug, Clone)]
pub struct SshTransport {
    dest: String,
}

impl SshTransport {
    /// Remote destination label (e.g. `user@host`); stored, never dialed.
    #[must_use]
    pub fn new(dest: String) -> Self {
        Self { dest }
    }
}

impl Transport for SshTransport {
    fn ship_binary(&self, _binary: &Path) -> Result<String> {
        Err(AtlasError::Forge(format!(
            "ssh transport to {} not implemented: refusing fake execution",
            self.dest
        )))
    }

    fn restart(&self) -> Result<String> {
        Err(AtlasError::Forge(format!(
            "ssh transport to {} not implemented: refusing fake execution",
            self.dest
        )))
    }

    fn health_check(&self) -> Result<bool> {
        Err(AtlasError::Forge(format!(
            "ssh transport to {} not implemented: refusing fake execution",
            self.dest
        )))
    }

    fn rollback(&self) -> Result<String> {
        Err(AtlasError::Forge(format!(
            "ssh transport to {} not implemented: refusing fake execution",
            self.dest
        )))
    }
}

/// Resolve the real `--execute` destination for `vps` from an explicit
/// dir (`None`/blank is an error). Pure: takes the dir as a value so
/// tests never mutate process env (which is `unsafe` to do).
/// There is no production default to fall back to.
pub fn transport_from_dir(target: DeployTarget, dir: Option<&str>) -> Result<LocalDirTransport> {
    if !matches!(target, DeployTarget::Vps) {
        return Err(AtlasError::Forge(format!(
            "deploy --execute for [{target}]: not implemented (only vps via {DEPLOY_DIR_ENV} today)"
        )));
    }
    let dir = dir.unwrap_or_default();
    if dir.trim().is_empty() {
        return Err(AtlasError::Forge(format!(
            "deploy --execute needs {DEPLOY_DIR_ENV}=<explicit dir>; refusing to guess a production default"
        )));
    }
    Ok(LocalDirTransport::new(PathBuf::from(dir.trim())))
}

/// Resolve the real `--execute` destination for `vps` from the env.
/// Anything else (unset, blank, non-vps target) is an honest error:
/// there is no production default to fall back to.
pub fn transport_from_env(target: DeployTarget) -> Result<LocalDirTransport> {
    let dir = std::env::var(DEPLOY_DIR_ENV).ok();
    transport_from_dir(target, dir.as_deref())
}

/// Gated real deploy: branch==main + passing fast CI for HEAD (same gates
/// as `deploy_plan`), then ship + restart + health check over `transport`.
/// A negative (or erroring) health check rolls back and fails loudly.
pub fn deploy_execute(
    store: &Store,
    repo: &Path,
    target: &str,
    binary: &Path,
    transport: &dyn Transport,
) -> Result<String> {
    let plan = deploy_gate(store, repo, target)?;
    let shipped = transport.ship_binary(binary)?;
    let restarted = transport.restart()?;
    let healthy = match transport.health_check() {
        Ok(v) => v,
        Err(e) => {
            let rolled = transport
                .rollback()
                .unwrap_or_else(|r| format!("rollback failed: {r}"));
            return Err(AtlasError::Forge(format!(
                "deploy failed: health check errored ({e}); {rolled}"
            )));
        }
    };
    if !healthy {
        let rolled = transport
            .rollback()
            .unwrap_or_else(|r| format!("rollback failed: {r}"));
        return Err(AtlasError::Forge(format!(
            "deploy failed: health check negative; {rolled}"
        )));
    }
    Ok(format!(
        "{plan}\ndeploy executed: {shipped}; {restarted}; health OK"
    ))
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

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atlas_deploy_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// Repo on main with passing fast CI for HEAD: deploy gates open.
    fn gated_repo(tag: &str) -> (std::path::PathBuf, Store, String) {
        let repo = git_repo(tag);
        let head = head_sha(&repo).expect("head");
        let store = Store::open_in_memory().expect("open");
        let pr = store.pr_create("feat", "main", "main").expect("pr");
        store
            .ci_record(pr, &head, FAST_PROFILE, true, "fast CI: PASS")
            .expect("ci");
        (repo, store, head)
    }

    #[test]
    fn deploy_execute_against_fixture() {
        let (repo, store, _head) = gated_repo("exec");
        let bin = tmp_dir("exec_bin").join("fake-atlas");
        std::fs::write(&bin, "fake-binary-v1").expect("write fixture binary");
        let dest = tmp_dir("exec_dest");
        let t = LocalDirTransport::new(dest.clone());
        let out = deploy_execute(&store, &repo, "vps", &bin, &t).expect("execute");
        assert!(out.contains("health OK"), "{out}");
        assert_eq!(
            std::fs::read_to_string(dest.join("atlas.bin")).expect("shipped"),
            "fake-binary-v1"
        );
        assert!(dest.join("healthy").is_file());
        assert!(t.health_check().expect("health"));
        std::fs::remove_dir_all(&repo).ok();
        std::fs::remove_dir_all(&dest).ok();
        std::fs::remove_dir_all(bin.parent().expect("parent")).ok();
    }

    #[test]
    fn deploy_execute_keeps_gates() {
        let repo = git_repo("execgates");
        let store = Store::open_in_memory().expect("open");
        let bin = tmp_dir("execgates_bin").join("fake-atlas");
        std::fs::write(&bin, "x").expect("write");
        let t = LocalDirTransport::new(tmp_dir("execgates_dest"));
        // No CI record for HEAD: gates stay shut even with --execute.
        let err = deploy_execute(&store, &repo, "vps", &bin, &t).expect_err("gates");
        assert!(err.to_string().contains("no passing fast CI"), "{err}");
        assert!(!t.dir().join("atlas.bin").exists());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn deploy_rollback_on_failed_health() {
        let t = LocalDirTransport::new(tmp_dir("rollback_dest"));
        let v1 = tmp_dir("rollback_v1").join("a");
        let v2 = tmp_dir("rollback_v2").join("a");
        std::fs::write(&v1, "binary-v1").expect("v1");
        std::fs::write(&v2, "binary-v2").expect("v2");
        // v1 ships healthy; v2 overwrites it (backup keeps v1).
        t.ship_binary(&v1).expect("ship v1");
        t.restart().expect("restart");
        assert!(t.health_check().expect("healthy"));
        t.ship_binary(&v2).expect("ship v2");
        // Service never came back: marker gone, health negative.
        std::fs::remove_file(t.marker_path()).expect("sabotage marker");
        assert!(!t.health_check().expect("unhealthy"));
        let msg = t.rollback().expect("rollback");
        assert!(msg.contains("restored"), "{msg}");
        assert_eq!(
            std::fs::read_to_string(t.binary_path()).expect("restored"),
            "binary-v1"
        );
    }

    /// Wrapper that ships fine but never becomes healthy: exercises the
    /// `deploy_execute` rollback path end to end.
    struct NeverHealthy {
        inner: LocalDirTransport,
    }

    impl Transport for NeverHealthy {
        fn ship_binary(&self, binary: &Path) -> Result<String> {
            self.inner.ship_binary(binary)
        }
        fn restart(&self) -> Result<String> {
            // Restart "succeeds" but leaves no health marker.
            Ok("restarted (unhealthy fixture)".to_owned())
        }
        fn health_check(&self) -> Result<bool> {
            Ok(false)
        }
        fn rollback(&self) -> Result<String> {
            self.inner.rollback()
        }
    }

    #[test]
    fn deploy_execute_rolls_back_on_failed_health() {
        let (repo, store, _head) = gated_repo("execrb");
        let v1 = tmp_dir("execrb_v1").join("a");
        let v2 = tmp_dir("execrb_v2").join("a");
        std::fs::write(&v1, "binary-v1").expect("v1");
        std::fs::write(&v2, "binary-v2").expect("v2");
        let inner = LocalDirTransport::new(tmp_dir("execrb_dest"));
        inner.ship_binary(&v1).expect("baseline v1");
        let t = NeverHealthy { inner };
        let err = deploy_execute(&store, &repo, "vps", &v2, &t).expect_err("must fail");
        assert!(err.to_string().contains("health check negative"), "{err}");
        assert!(err.to_string().contains("restored"), "{err}");
        assert_eq!(
            std::fs::read_to_string(t.inner.binary_path()).expect("restored"),
            "binary-v1"
        );
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn deploy_dir_env_required_and_ssh_honest() {
        // No production default: missing/blank dir is an error naming the var.
        // Pure constructor: no process-env mutation (which is unsafe to do).
        let err = transport_from_dir(DeployTarget::Vps, None).expect_err("missing dir");
        assert!(err.to_string().contains(DEPLOY_DIR_ENV), "{err}");
        assert!(transport_from_dir(DeployTarget::Vps, Some("   ")).is_err());
        // Explicit dir resolves.
        let t = transport_from_dir(DeployTarget::Vps, Some("/tmp/atlas-fixture-dest"))
            .expect("explicit dir");
        assert_eq!(t.dir(), Path::new("/tmp/atlas-fixture-dest"));
        // cloudrun has no execute path yet: honest error, not fake success.
        assert!(transport_from_dir(DeployTarget::Cloudrun, Some("/tmp/x")).is_err());
        // SSH skeleton: every op fails without spawning anything.
        let ssh = SshTransport::new("user@prod.example".to_owned());
        let fake = Path::new("/tmp/atlas-nope");
        assert!(ssh.ship_binary(fake).is_err());
        assert!(ssh.restart().is_err());
        assert!(ssh.health_check().is_err());
        assert!(ssh.rollback().is_err());
    }
}
