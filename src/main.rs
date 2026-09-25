// `atlas` CLI: sessions, task DAG, and tree render (REQ-F-003/005).
use atlas::{Store, model};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::PathBuf;

/// Atlas: autonomous long-horizon task execution, offline-first.
#[derive(Debug, Parser)]
#[command(name = "atlas", version, about = "Task-DAG store + sessions")]
struct Cli {
    /// SQLite file (created on first use).
    #[arg(long, global = true, default_value = "./atlas.db")]
    db: PathBuf,
    /// Machine-readable output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Start a new session for a goal.
    Start {
        #[arg(long)]
        goal: String,
        /// Automatically draft initial tasks grounded in Xavier memory.
        #[arg(long)]
        draft_xavier: bool,
    },
    /// Stop a session (history is kept).
    Stop {
        #[arg(long)]
        session: Option<String>,
    },
    /// Show a session with per-state task counts.
    Status {
        #[arg(long)]
        session: Option<String>,
    },
    /// List tasks, optionally filtered by session.
    List {
        #[arg(long)]
        session: Option<String>,
    },
    /// Render the dependency tree of a session (optionally filtered by project).
    Tree {
        #[arg(long)]
        session: Option<String>,
        /// Filter tree to a specific project (e.g. xavier, gestalt, shelf, atlas-core).
        #[arg(long)]
        project: Option<String>,
    },
    /// Run the Atlas-Xavier project & feature evaluation pipeline.
    Evaluate {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Task operations.
    #[command(subcommand)]
    Task(TaskCmd),
    /// Move a READY task to IN_PROGRESS (only READY work may start).
    Run { id: String },
    /// Complete an IN_PROGRESS task: --ok queues task_completed,
    /// --fail queues task_failed; state moves on `tick --once`.
    Complete {
        id: String,
        #[arg(long)]
        ok: bool,
        #[arg(long)]
        fail: bool,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Consume queued events once (watchdog-only single pass).
    Tick {
        #[arg(long)]
        once: bool,
    },
    /// Run the rule checks on a task (DoD + evidence; external reviewer
    /// when `ATLAS_REVIEW_CMD` is set, stub reviewer otherwise).
    Verify { id: String },
    /// Definition-of-Done checklist of a task.
    Dod {
        id: String,
        #[command(subcommand)]
        action: DodCmd,
    },
    /// Attach one evidence string to a task.
    Evidence { id: String, text: String },
    /// Record per-task actuals: wall time plus token usage (REQ-F-014).
    Record {
        id: String,
        #[arg(long)]
        duration_ms: i64,
        #[arg(long)]
        prompt_toks: i64,
        #[arg(long)]
        completion_toks: i64,
        #[arg(long)]
        outcome: String,
    },
    /// Estimate one task or roll up the whole tree with critical path
    /// and estimate-vs-actual drift (REQ-F-015).
    Estimate {
        id: Option<String>,
        #[arg(long)]
        session: Option<String>,
    },
    /// Serve the local dev dashboard: single-file HTML plus websocket
    /// live updates (REQ-F-016). Loopback only unless `--public`.
    Serve {
        /// TCP port to listen on.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Bind host (default loopback; other hosts need --public).
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Allow non-loopback binds (never the default).
        #[arg(long)]
        public: bool,
    },
    /// Forge issue operations (REQ-F-017, same SQLite DB).
    #[command(subcommand)]
    Issue(IssueCmd),
    /// Forge pull-request operations (REQ-F-017, same SQLite DB).
    #[command(subcommand)]
    Pr(PrCmd),
    /// Run the fast CI profile for a PR and record PASS/FAIL (REQ-F-017).
    Ci {
        /// PR id the run belongs to.
        #[arg(long)]
        pr: i64,
        /// Fast profile: `cargo test --offline` + `cargo fmt --check`.
        #[arg(long)]
        fast: bool,
        /// Repo dir to test (must be a git checkout).
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// Print the deploy plan for a target (REQ-F-018). Main branch plus
    /// passing fast CI for HEAD required; prints only, never executes,
    /// unless `--execute` runs gates plus the real transport against the
    /// destination configured by ATLAS_DEPLOY_DIR (no production default).
    /// Credentials are env-only at F3 and are never read here.
    Deploy {
        /// Deploy target: vps or cloudrun.
        #[arg(long)]
        target: String,
        /// Repo dir to inspect (must be a git checkout).
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Actually execute: gates + real transport (default: print plan only).
        #[arg(long)]
        execute: bool,
        /// Binary file to ship with --execute
        /// (default: <repo>/target/release/atlas).
        #[arg(long)]
        binary: Option<PathBuf>,
    },
    /// Xavier memory adapter (REQ-F-019, ATLAS-09). Optional and
    /// degraded-first: every subcommand exits 0 with Xavier down.
    #[command(subcommand)]
    Xavier(XavierCmd),
    /// Watch the Gestalt Event Bus (:8081) and automatically promote/verify tasks.
    Watch {
        /// Gestalt bus URL (default: http://127.0.0.1:8081).
        #[arg(long, default_value = "http://127.0.0.1:8081")]
        bus: String,
        /// Poll interval in milliseconds.
        #[arg(long, default_value_t = 2000)]
        interval_ms: u64,
        /// Exit after single poll pass.
        #[arg(long)]
        once: bool,
    },
}

/// Xavier subcommands: liveness, task grounding, backlog drafting.
#[derive(Debug, Subcommand)]
enum XavierCmd {
    /// Detect Xavier at runtime and announce capabilities (or degraded mode).
    Status,
    /// Search Xavier memories for a task; prints top hits with path+score.
    /// Unreachable Xavier prints a degraded notice with an empty result.
    Context {
        /// Task id being enriched (informational; not validated).
        #[arg(long)]
        task: String,
        /// Query text sent to Xavier.
        #[arg(long)]
        query: String,
        /// Max hits to show.
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Draft a backlog from CodeGraph/RAG hits for a goal: proposed task
    /// titles with depends-on suggestions (`--json` is machine-readable).
    Draft {
        /// Goal to draft the backlog for.
        #[arg(long)]
        goal: String,
        /// Max grounding hits to fetch.
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
enum DodCmd {
    /// Append a checklist item (numbered per task from 1).
    Add { text: String },
    /// Mark item <n> checked.
    Check { n: i64 },
    /// List every item with its checked flag.
    List,
}

#[derive(Debug, Subcommand)]
enum TaskCmd {
    /// Create a task, optionally depending on other task ids.
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        depends_on: Vec<String>,
        #[arg(long)]
        agent: Option<String>,
    },
    /// Manually block a task with a reason.
    Block {
        id: String,
        #[arg(long)]
        reason: String,
    },
    /// Mark a task completed (unlocks ready dependents).
    Complete { id: String },
    /// Mark a task failed with a reason.
    Fail {
        id: String,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Move a READY task to IN_PROGRESS.
    Start { id: String },
}

/// Forge issue subcommands (REQ-F-017).
#[derive(Debug, Subcommand)]
enum IssueCmd {
    /// Create an issue in state OPEN.
    Create {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        body: String,
    },
    /// List every issue, oldest first.
    List,
    /// Move an OPEN issue to CLOSED.
    Close { id: String },
}

/// Forge pull-request subcommands (REQ-F-017).
#[derive(Debug, Subcommand)]
enum PrCmd {
    /// Record a PR in state OPEN; `branch` must exist locally
    /// (checked with `git rev-parse --verify`).
    Create {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "main")]
        base: String,
        #[arg(long)]
        branch: String,
        /// Repo dir holding the branch (default: current dir).
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// List every PR, oldest first.
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::open(&cli.db)
        .map_err(anyhow::Error::new)
        .with_context(|| format!("cannot open DB {}", cli.db.display()))?;
    run(&store, &cli)
}

fn run(store: &Store, cli: &Cli) -> Result<()> {
    match &cli.cmd {
        Cmd::Start { goal, draft_xavier } => {
            let id = store.create_session(goal).map_err(anyhow::Error::new)?;
            let sess = store.get_session(&id).map_err(anyhow::Error::new)?;
            if *draft_xavier {
                use atlas::xavier::{HttpBackend, XavierBackend, XavierConfig, draft_from_hits};
                let backend = HttpBackend::new(XavierConfig::from_env());
                let hits = backend.search(goal, 5).unwrap_or_default();
                let draft = draft_from_hits(goal, &hits);
                let mut created_ids = Vec::new();
                for item in &draft {
                    let dep_ids: Vec<&str> = item
                        .depends_on
                        .iter()
                        .filter_map(|&idx| created_ids.get(idx).map(|s: &String| s.as_str()))
                        .collect();
                    if let Ok(task_id) = store.create_task(&id, &item.title, None, &dep_ids) {
                        created_ids.push(task_id);
                    }
                }
                if !cli.json {
                    println!("🌱 Auto-drafted {} tasks grounded in Xavier into session {}", created_ids.len(), sess.id);
                }
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&sess)?);
            } else {
                println!("started session {} ({})", sess.id, sess.goal);
            }
            Ok(())
        }
        Cmd::Stop { session } => {
            let s = resolve_session(store, session)?;
            let updated = store
                .set_session_status(&s, "stopped")
                .map_err(anyhow::Error::new)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&updated)?);
            } else {
                println!("stopped session {}", updated.id);
            }
            Ok(())
        }
        Cmd::Status { session } => {
            let s = resolve_session(store, session)?;
            let sess = store.get_session(&s).map_err(anyhow::Error::new)?;
            let counts = store.counts_by_state(&s).map_err(anyhow::Error::new)?;
            let tasks = store.list_tasks(Some(&s)).map_err(anyhow::Error::new)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "session": sess,
                        "counts": counts,
                        "tasks": tasks,
                    }))?
                );
            } else {
                println!("session {} [{}]", sess.id, sess.status);
                println!("goal: {}", sess.goal);
                let mut states: Vec<_> = counts.iter().collect();
                states.sort_by_key(|(k, _)| (*k).clone());
                for (st, n) in states {
                    println!("  {st}: {n}");
                }
                println!("tasks: {}", tasks.len());
            }
            Ok(())
        }
        Cmd::List { session } => {
            let tasks = store
                .list_tasks(session.as_deref())
                .map_err(anyhow::Error::new)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&tasks)?);
            } else {
                for t in &tasks {
                    println!("{} [{}] {}", t.id, t.state, t.title);
                }
            }
            Ok(())
        }
        Cmd::Tree { session, project } => {
            let s = resolve_session(store, session)?;
            let roots = atlas::tree::build_tree(store, &s, project.as_deref()).map_err(anyhow::Error::new)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&roots)?);
            } else {
                if let Some(p) = project {
                    println!("=== [Task Tree: {} | Session: {}] ===", p.to_uppercase(), s);
                }
                for r in &roots {
                    atlas::tree::print_node(r, 0);
                }
            }
            Ok(())
        }
        Cmd::Evaluate { session, project } => {
            let s = resolve_session(store, session)?;
            // Resolve the evaluator from $ATLAS_EVALUATOR or the working
            // directory. Never hardcode a machine-specific absolute path: this
            // repository is public.
            let script = std::env::var("ATLAS_EVALUATOR")
                .unwrap_or_else(|_| "scripts/atlas_xavier_evaluator.py".to_string());
            let mut cmd = std::process::Command::new("python3");
            cmd.arg(&script)
               .arg("--db").arg(&cli.db)
               .arg("--session").arg(&s);
            if let Some(p) = project {
                cmd.arg("--project").arg(p);
            }
            let status = cmd.status().context("Failed to run atlas_xavier_evaluator.py")?;
            if !status.success() {
                bail!("Evaluator script failed with status {}", status);
            }
            Ok(())
        }
        Cmd::Task(sub) => match sub {
            TaskCmd::Create {
                title,
                session,
                depends_on,
                agent,
            } => {
                let s = resolve_session(store, session)?;
                let deps: Vec<&str> = depends_on.iter().map(String::as_str).collect();
                let id = store
                    .create_task(&s, title, agent.as_deref(), &deps)
                    .map_err(anyhow::Error::new)?;
                let task = store.get_task(&id).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&task)?);
                } else {
                    println!("created {} [{}] {}", task.id, task.state, task.title);
                }
                Ok(())
            }
            TaskCmd::Block { id, reason } => {
                let task = store.block_task(id, reason).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&task)?);
                } else {
                    println!("blocked {} ({})", task.id, reason);
                }
                Ok(())
            }
            TaskCmd::Complete { id } => {
                let task = store.complete_task(id).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&task)?);
                } else {
                    println!("completed {}", task.id);
                }
                Ok(())
            }
            TaskCmd::Fail { id, reason } => {
                let task = store.fail_task(id, reason).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&task)?);
                } else {
                    println!("failed {}", task.id);
                }
                Ok(())
            }
            TaskCmd::Start { id } => {
                let task = store.start_task(id).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&task)?);
                } else {
                    println!("started {} [{}]", task.id, task.state);
                }
                Ok(())
            }
        },
        Cmd::Run { id } => {
            let task = store.run_task(id).map_err(anyhow::Error::new)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&task)?);
            } else {
                println!("running {} [{}]", task.id, task.state);
            }
            Ok(())
        }
        Cmd::Complete {
            id,
            ok,
            fail,
            reason,
        } => {
            if *ok == *fail {
                return Err(anyhow!("pass exactly one of --ok or --fail"));
            }
            let why = reason.as_deref().unwrap_or("");
            let event = store
                .finish_task(id, *ok, why)
                .map_err(anyhow::Error::new)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "task": id,
                        "ok": ok,
                        "event": event,
                    }))?
                );
            } else if *ok {
                println!("queued task_completed for {id} (event {event})");
            } else {
                println!("queued task_failed for {id} (event {event})");
            }
            Ok(())
        }
        Cmd::Tick { .. } => {
            let sum = store.tick_once().map_err(anyhow::Error::new)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&sum)?);
            } else {
                println!(
                    "tick: processed={} unlocked={} retried={} escalated={} skipped={} last_event={}",
                    sum.processed,
                    sum.unlocked,
                    sum.retried,
                    sum.escalated,
                    sum.skipped,
                    sum.last_event_id,
                );
            }
            Ok(())
        }
        Cmd::Verify { id } => {
            let report = match atlas::verifier::CommandReviewer::from_env() {
                Some(reviewer) => {
                    let rep = store
                        .verify_with(&reviewer, id)
                        .map_err(anyhow::Error::new)?;
                    store
                        .evidence_add(id, &format!("review: {}", rep.reviewer))
                        .map_err(anyhow::Error::new)?;
                    rep
                }
                None => store.verify(id).map_err(anyhow::Error::new)?,
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if report.passed {
                println!(
                    "verify {id}: PASS (dod {}/{} checked, {} evidence, {})",
                    report.dod_checked, report.dod_total, report.evidence_count, report.reviewer,
                );
            } else {
                println!("verify {id}: FAIL ({})", report.failures.join("; "));
            }
            Ok(())
        }
        Cmd::Dod { id, action } => match action {
            DodCmd::Add { text } => {
                let n = store.dod_add(id, text).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"task": id, "n": n}))?
                    );
                } else {
                    println!("dod {id}#{n}: {text}");
                }
                Ok(())
            }
            DodCmd::Check { n } => {
                let item = store.dod_check(id, *n).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&item)?);
                } else {
                    println!("dod {id}#{n} checked: {}", item.text);
                }
                Ok(())
            }
            DodCmd::List => {
                let items = store.dod_list(id).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else {
                    for it in &items {
                        let mark = if it.checked { "x" } else { " " };
                        println!("[{mark}] {}#{} {}", it.task_id, it.n, it.text);
                    }
                }
                Ok(())
            }
        },
        Cmd::Evidence { id, text } => {
            let row = store.evidence_add(id, text).map_err(anyhow::Error::new)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"task": id, "row": row}))?
                );
            } else {
                println!("evidence {id}#{row}: {text}");
            }
            Ok(())
        }
        Cmd::Record {
            id,
            duration_ms,
            prompt_toks,
            completion_toks,
            outcome,
        } => {
            if outcome != "ok" && outcome != "fail" {
                return Err(anyhow!("outcome must be ok or fail, got '{outcome}'"));
            }
            store
                .record_metric(id, *duration_ms, *prompt_toks, *completion_toks, outcome)
                .map_err(anyhow::Error::new)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "task": id,
                        "duration_ms": duration_ms,
                        "prompt_toks": prompt_toks,
                        "completion_toks": completion_toks,
                        "outcome": outcome,
                    }))?
                );
            } else {
                println!(
                    "recorded {id}: {duration_ms}ms {prompt_toks}+{completion_toks} toks ({outcome})"
                );
            }
            Ok(())
        }
        Cmd::Estimate { id, session } => {
            if cli.json {
                if let Some(task_id) = id {
                    let est = atlas::estimator::estimate_task(store, task_id)
                        .map_err(anyhow::Error::new)?;
                    println!("{}", serde_json::to_string_pretty(&est)?);
                } else {
                    let sess = match session {
                        Some(s) => Some(s.clone()),
                        None => resolve_session(store, &None).ok(),
                    };
                    let roll = atlas::estimator::estimate_session(store, sess.as_deref())
                        .map_err(anyhow::Error::new)?;
                    println!("{}", serde_json::to_string_pretty(&roll)?);
                }
                return Ok(());
            }
            if let Some(task_id) = id {
                let est =
                    atlas::estimator::estimate_task(store, task_id).map_err(anyhow::Error::new)?;
                print_estimate(&est);
            }
            let sess = match session {
                Some(s) => Some(s.clone()),
                None => resolve_session(store, &None).ok(),
            };
            match sess {
                Some(s) => {
                    let roll = atlas::estimator::estimate_session(store, Some(&s))
                        .map_err(anyhow::Error::new)?;
                    print_rollup(&roll);
                }
                None => {
                    if id.is_none() {
                        return Err(anyhow!(
                            "no session yet; run `atlas start --goal \"...\"` first"
                        ));
                    }
                }
            }
            Ok(())
        }
        Cmd::Serve { port, host, public } => {
            let cfg = atlas::serve::ServeConfig {
                host: host.clone(),
                port: *port,
                db: cli.db.clone(),
                public: *public,
            };
            // Refuse non-loopback binds without --public before listening.
            atlas::serve::resolve_bind(&cfg.host, cfg.port, cfg.public)
                .map_err(anyhow::Error::new)?;
            println!("serving dashboard on {}:{}", cfg.host, cfg.port);
            atlas::serve::run_server(&cfg).map_err(anyhow::Error::new)?;
            Ok(())
        }
        Cmd::Issue(sub) => match sub {
            IssueCmd::Create { title, body } => {
                let id = store
                    .issue_create(title, body)
                    .map_err(anyhow::Error::new)?;
                let issue = store.issue_get(&id).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&issue)?);
                } else {
                    println!(
                        "created issue {} [{}] {}",
                        issue.id, issue.state, issue.title
                    );
                }
                Ok(())
            }
            IssueCmd::List => {
                let issues = store.issue_list().map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&issues)?);
                } else {
                    for i in &issues {
                        println!("{} [{}] {}", i.id, i.state, i.title);
                    }
                }
                Ok(())
            }
            IssueCmd::Close { id } => {
                let issue = store.issue_close(id).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&issue)?);
                } else {
                    println!("closed issue {}", issue.id);
                }
                Ok(())
            }
        },
        Cmd::Pr(sub) => match sub {
            PrCmd::Create {
                title,
                base,
                branch,
                repo,
            } => {
                let id = atlas::forge::pr_create_validated(store, repo, title, base, branch)
                    .map_err(anyhow::Error::new)?;
                let pr = store.pr_get(id).map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&pr)?);
                } else {
                    println!(
                        "created pr {} [{}] {} ({} -> {})",
                        pr.id, pr.state, pr.title, pr.branch, pr.base
                    );
                }
                Ok(())
            }
            PrCmd::List => {
                let prs = store.pr_list().map_err(anyhow::Error::new)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&prs)?);
                } else {
                    for p in &prs {
                        println!(
                            "#{} [{}] {} ({} -> {})",
                            p.id, p.state, p.title, p.branch, p.base
                        );
                    }
                }
                Ok(())
            }
        },
        Cmd::Ci { pr, fast, repo } => {
            if !fast {
                return Err(anyhow!("only the fast profile is supported; pass --fast"));
            }
            // 404 early on unknown PRs before spending time on cargo.
            store.pr_get(*pr).map_err(anyhow::Error::new)?;
            let head = atlas::forge::head_sha(repo).map_err(anyhow::Error::new)?;
            let outcome = atlas::forge::run_fast_ci(repo);
            let row = store
                .ci_record(
                    *pr,
                    &head,
                    atlas::forge::FAST_PROFILE,
                    outcome.passed,
                    &outcome.evidence,
                )
                .map_err(anyhow::Error::new)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "pr": pr,
                        "row": row,
                        "head": head,
                        "passed": outcome.passed,
                        "evidence": outcome.evidence,
                    }))?
                );
            } else if outcome.passed {
                println!("ci pr {pr} HEAD {head}: PASS (row {row})");
            } else {
                println!(
                    "ci pr {pr} HEAD {head}: FAIL (row {row})\n{}",
                    outcome.evidence
                );
            }
            Ok(())
        }
        Cmd::Deploy {
            target,
            repo,
            execute,
            binary,
        } => {
            if !execute {
                // Prints the plan only; never executes; reads no credentials.
                let plan =
                    atlas::forge::deploy_gate(store, repo, target).map_err(anyhow::Error::new)?;
                println!("{plan}");
                return Ok(());
            }
            // Real path: same gates, then a real transport against the
            // destination configured by ATLAS_DEPLOY_DIR (never a default).
            let want: model::DeployTarget = target.parse().map_err(anyhow::Error::new)?;
            let transport = atlas::forge::transport_from_env(want).map_err(anyhow::Error::new)?;
            let bin = match binary {
                Some(b) => b.clone(),
                None => repo.join("target/release/atlas"),
            };
            let out = atlas::forge::deploy_execute(store, repo, target, &bin, &transport)
                .map_err(anyhow::Error::new)?;
            println!("{out}");
            Ok(())
        }
        Cmd::Xavier(sub) => run_xavier(sub, cli.json),
        Cmd::Watch { bus, interval_ms, once } => run_watch(store, bus, *interval_ms, *once, cli.json),
    }
}

/// Watch the Gestalt Event Bus, query new events with cursor pagination,
/// and advance tasks according to SODP status codes and events.
fn run_watch(
    store: &Store,
    bus_url: &str,
    interval_ms: u64,
    once: bool,
    as_json: bool,
) -> Result<()> {
    use std::io::Read;
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let timeout = Duration::from_millis(1500);

    // Helper: fetch events via HTTP GET
    let fetch_events = |after_seq: Option<i64>| -> Result<serde_json::Value> {
        let clean = bus_url.trim_end_matches('/');
        let path = match after_seq {
            Some(seq) => format!("/api/events?after_seq={seq}&limit=50"),
            None => "/api/events?limit=50".to_string(),
        };

        // Extract host and port
        let without_proto = clean
            .strip_prefix("http://")
            .or_else(|| clean.strip_prefix("https://"))
            .unwrap_or(clean);
        let mut parts = without_proto.split(':');
        let host = parts.next().unwrap_or("127.0.0.1");
        let port = parts.next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(8081);

        let addr_str = format!("{host}:{port}");
        let addrs: Vec<std::net::SocketAddr> = addr_str
            .to_socket_addrs()
            .map_err(|e| anyhow!("Address resolution failed: {e}"))?
            .collect();
        let target = addrs
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No address found for {addr_str}"))?;

        let mut stream = TcpStream::connect_timeout(&target, timeout)
            .map_err(|e| anyhow!("Failed to connect to Gestalt bus at {clean}: {e}"))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        let req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        std::io::Write::write_all(&mut stream, req.as_bytes())?;

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf)?;
        let text = String::from_utf8_lossy(&buf).into_owned();

        let body_start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(text.len());
        let body = &text[body_start..];
        let val: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| anyhow!("Failed to parse Gestalt bus events JSON: {e}"))?;
        Ok(val)
    };

    println!("👀 Watching Gestalt Event Bus at {bus_url}...");
    let mut cursor: Option<i64> = None;

    loop {
        match fetch_events(cursor) {
            Ok(data) => {
                if let Some(events) = data.get("events").and_then(|e| e.as_array()) {
                    for ev in events {
                        // Bus events expose `agent_id` at the top level and nest the
                        // producer's original fields inside a JSON-encoded `payload`
                        // string. Read the nested values first, then fall back to the
                        // flat ones, so both shapes resolve instead of yielding
                        // "unknown"/None for every event.
                        let payload: serde_json::Value = ev
                            .get("payload")
                            .and_then(|p| p.as_str())
                            .and_then(|p| serde_json::from_str(p).ok())
                            .unwrap_or(serde_json::Value::Null);

                        let agent = payload
                            .get("agent")
                            .and_then(|a| a.as_str())
                            .or_else(|| ev.get("agent_id").and_then(|a| a.as_str()))
                            .or_else(|| ev.get("agent").and_then(|a| a.as_str()))
                            .unwrap_or("unknown");
                        let ev_type = ev.get("event_type").and_then(|t| t.as_str()).unwrap_or("event");
                        let summary = payload
                            .get("summary")
                            .and_then(|s| s.as_str())
                            .or_else(|| ev.get("summary").and_then(|s| s.as_str()))
                            .unwrap_or("");
                        let state = payload
                            .get("state")
                            .and_then(|s| s.as_str())
                            .or_else(|| ev.get("state").and_then(|s| s.as_str()));

                        if !as_json {
                            println!("📡 [{agent}] {ev_type} {state:?}: {summary}");
                        }

                        // 2026-09-24 fix: `state == Some("Success")` alone used to trigger
                        // tick_once() for ANY bus event with that state — including a plain
                        // Claude Code "Stop" turn or an opencode "session.idle" that has
                        // nothing to do with an Atlas-owned task. That promoted READY tasks
                        // essentially at random whenever any agent finished any turn.
                        //
                        // Now the promotion pass only fires when the event explicitly
                        // correlates to an Atlas task via `atlas_task_id` (checked at the
                        // top level, inside `metadata`, or as a plain `task_id` fallback).
                        // Events without that correlation are logged (above) but never
                        // advance any task — this is what "cambia la fuente a eventos que
                        // traigan atlas_task_id" means in practice: filter at the trigger,
                        // not at the transport.
                        let atlas_task_id = payload
                            .get("atlas_task_id")
                            .or_else(|| payload.get("metadata").and_then(|m| m.get("atlas_task_id")))
                            .or_else(|| payload.get("metadata").and_then(|m| m.get("task_id")))
                            .or_else(|| ev.get("atlas_task_id"))
                            .and_then(|v| v.as_str());

                        if let Some(task_id) = atlas_task_id {
                            if ev_type == "run_finished" || state == Some("Success") {
                                if !as_json {
                                    println!("   ↳ atlas_task_id={task_id}: promoting");
                                }
                                let _ = store.tick_once();
                            }
                        }
                    }
                }
                if let Some(next) = data.get("next_seq").and_then(|s| s.as_i64()) {
                    cursor = Some(next);
                }
            }
            Err(err) => {
                if !as_json {
                    eprintln!("⚠️  Gestalt bus poll error: {err}");
                }
            }
        }

        if once {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval_ms));
    }

    Ok(())
}

/// Xavier adapter commands (REQ-F-019). Degraded-first: unreachable
/// Xavier always yields exit 0 with an explicit degraded notice.
fn run_xavier(sub: &XavierCmd, as_json: bool) -> Result<()> {
    use atlas::xavier::{HttpBackend, XavierBackend, XavierConfig, draft_from_hits};
    let backend = HttpBackend::new(XavierConfig::from_env());
    match sub {
        XavierCmd::Status => {
            let status = backend.check_status();
            if as_json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else if status.reachable {
                println!("xavier: reachable at {}", status.base_url);
                println!("capabilities: {}", status.capabilities.join(", "));
            } else {
                println!("xavier: degraded mode (unreachable at {})", status.base_url);
                println!("detail: {}", status.detail);
                println!("capabilities: none (core suite runs without xavier)");
            }
            Ok(())
        }
        XavierCmd::Context { task, query, limit } => {
            match backend.search(query, *limit) {
                Ok(hits) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "task": task,
                                "query": query,
                                "degraded": false,
                                "hits": hits,
                            }))?
                        );
                    } else if hits.is_empty() {
                        println!("context {task}: no hits for '{query}'");
                    } else {
                        println!("context {task}: top {} hit(s) for '{query}'", hits.len());
                        for (i, h) in hits.iter().enumerate() {
                            println!("  {}. [{}] {:.3} {}", i + 1, h.id, h.score, h.path);
                            if !h.snippet.is_empty() {
                                println!("     {}", h.snippet);
                            }
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    // Degraded path: empty result, exit 0, say so loudly.
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "task": task,
                                "query": query,
                                "degraded": true,
                                "hits": Vec::<serde_json::Value>::new(),
                            }))?
                        );
                    } else {
                        println!("xavier: degraded mode ({e})");
                        println!("context {task}: empty result (no xavier, exit 0)");
                    }
                    Ok(())
                }
            }
        }
        XavierCmd::Draft { goal, limit } => {
            // Grounding hits are best-effort: unreachable Xavier drafts
            // from the goal alone (scope task only).
            let hits = backend.search(goal, *limit).unwrap_or_default();
            let degraded = backend.check_status().degraded && hits.is_empty();
            let draft = draft_from_hits(goal, &hits);
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "goal": goal,
                        "degraded": degraded,
                        "hits": hits.len(),
                        "draft": draft,
                    }))?
                );
            } else {
                if degraded {
                    println!("xavier: degraded mode (drafting from goal alone)");
                }
                println!("draft for '{goal}': {} proposed task(s)", draft.len());
                for (i, d) in draft.iter().enumerate() {
                    if d.depends_on.is_empty() {
                        println!("  {i}. {}", d.title);
                    } else {
                        let deps = d
                            .depends_on
                            .iter()
                            .map(|n| n.to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        println!("  {i}. {} (depends-on: {deps})", d.title);
                    }
                }
            }
            Ok(())
        }
    }
}

/// Explicit `--session` or the most recent one.
fn resolve_session(store: &Store, explicit: &Option<String>) -> Result<String> {
    if let Some(s) = explicit {
        return Ok(s.clone());
    }
    store
        .latest_session()
        .map_err(anyhow::Error::new)?
        .map(|s| s.id)
        .ok_or_else(|| anyhow!("no session yet; run `atlas start --goal \"...\"` first"))
}

/// One-line per-task estimate with its history source and actual, if any.
fn print_estimate(e: &atlas::estimator::TaskEstimate) {
    let src = if e.from_history {
        format!("avg({} samples)", e.samples)
    } else {
        "default".to_owned()
    };
    println!(
        "estimate {} '{}': {}ms {}+{} toks [{src}]",
        e.task_id, e.title, e.duration_ms, e.prompt_tokens, e.completion_tokens
    );
    if let Some(a) = &e.actual {
        let drift = a.duration_ms - e.duration_ms;
        let sign = if drift >= 0 { "+" } else { "" };
        println!(
            "  actual: {}ms {}+{} toks ({}) drift {sign}{drift}ms",
            a.duration_ms, a.prompt_tokens, a.completion_tokens, a.outcome
        );
    }
}

/// Whole-tree rollup: per-task rows, critical path, totals, drift.
fn print_rollup(r: &atlas::estimator::Rollup) {
    for e in &r.per_task {
        print_estimate(e);
    }
    println!(
        "total: {}ms sum / {}ms critical path [{}]",
        r.total_sum_ms,
        r.critical_path_ms,
        r.critical_path.join(" -> ")
    );
    if r.actuals_count > 0 {
        let sign = if r.drift_ms >= 0 { "+" } else { "" };
        println!(
            "drift: {sign}{}ms over {} actual(s)",
            r.drift_ms, r.actuals_count
        );
    } else {
        println!("drift: no actuals recorded yet");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas::model::TaskNode;
    use atlas::tree::build_tree;

    fn count_occurrences(nodes: &[TaskNode], id: &str) -> usize {
        nodes
            .iter()
            .map(|n| usize::from(n.id == id) + count_occurrences(&n.children, id))
            .sum()
    }

    /// Diamante A -> {B, C} -> D: D tiene dos padres y debe aparecer una sola vez.
    /// Sin el set de visitados, D se expande bajo B y bajo C (2 ocurrencias).
    #[test]
    fn tree_renders_multi_parent_node_once() {
        let store = Store::open_in_memory().expect("in-memory store");
        let session = store.create_session("diamante").expect("session");
        let a = store.create_task(&session, "A", None, &[]).expect("task A");
        let b = store
            .create_task(&session, "B", None, &[&a])
            .expect("task B");
        let c = store
            .create_task(&session, "C", None, &[&a])
            .expect("task C");
        let d = store
            .create_task(&session, "D", None, &[&b])
            .expect("task D");
        store.add_dependency(&d, &c).expect("D depende de C");

        let roots = build_tree(&store, &session, None).expect("tree");
        assert_eq!(
            count_occurrences(&roots, &d),
            1,
            "D con 2 padres sale 1 vez"
        );
        assert_eq!(count_occurrences(&roots, &a), 1);
        assert_eq!(count_occurrences(&roots, &b), 1);
        assert_eq!(count_occurrences(&roots, &c), 1);
    }

    #[test]
    fn test_watch_promotes_on_run_finished() {
        let store = Store::open_in_memory().expect("in-memory store");
        let session = store.create_session("watch test").expect("session");
        let a = store.create_task(&session, "A", None, &[]).expect("task A");
        assert_eq!(store.get_task(&a).unwrap().state, model::TaskState::Ready);

        // Advance task A to in_progress
        store.run_task(&a).expect("run task A");
        assert_eq!(store.get_task(&a).unwrap().state, model::TaskState::InProgress);

        // Verifier gate: DoD + Evidence
        store.dod_add(&a, "done").expect("dod");
        store.dod_check(&a, 1).expect("check");
        store.evidence_add(&a, "proof").expect("ev");

        // Queue completion
        store.finish_task(&a, true, "done").expect("finish task");

        // Simulating run_watch event trigger: executing tick_once
        let sum = store.tick_once().expect("tick once");
        assert_eq!(sum.processed, 4);
        assert_eq!(store.get_task(&a).unwrap().state, model::TaskState::Completed);
    }
}
