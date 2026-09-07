// `atlas` CLI: sessions, task DAG, and tree render (REQ-F-003/005).
use atlas::{Store, model, store};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use atlas::model::TaskNode;

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
    /// Render the dependency tree of a session.
    Tree {
        #[arg(long)]
        session: Option<String>,
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
    /// Run the rule checks on a task (DoD + evidence, stub reviewer).
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
    /// passing fast CI for HEAD required; prints only, never executes.
    /// Credentials are env-only at F3 and are never read here.
    Deploy {
        /// Deploy target: vps or cloudrun.
        #[arg(long)]
        target: String,
        /// Repo dir to inspect (must be a git checkout).
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// Xavier memory adapter (REQ-F-019, ATLAS-09). Optional and
    /// degraded-first: every subcommand exits 0 with Xavier down.
    #[command(subcommand)]
    Xavier(XavierCmd),
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
        Cmd::Start { goal } => {
            let id = store.create_session(goal).map_err(anyhow::Error::new)?;
            let sess = store.get_session(&id).map_err(anyhow::Error::new)?;
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
        Cmd::Tree { session } => {
            let s = resolve_session(store, session)?;
            let roots = build_tree(store, &s).map_err(anyhow::Error::new)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&roots)?);
            } else {
                for r in &roots {
                    print_node(r, 0);
                }
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
            let report = store.verify(id).map_err(anyhow::Error::new)?;
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
        Cmd::Deploy { target, repo } => {
            // Prints the plan only; never executes; reads no credentials.
            let plan =
                atlas::forge::deploy_gate(store, repo, target).map_err(anyhow::Error::new)?;
            println!("{plan}");
            Ok(())
        }
        Cmd::Xavier(sub) => run_xavier(sub, cli.json),
    }
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

/// Forest of task trees for a session (roots = tasks without parents).
fn build_tree(store: &Store, session: &str) -> store::Result<Vec<TaskNode>> {
    let tasks = store.list_tasks(Some(session))?;
    let mut meta: HashMap<String, (String, model::TaskState)> = HashMap::with_capacity(tasks.len());
    for t in &tasks {
        meta.insert(t.id.clone(), (t.title.clone(), t.state));
    }
    let mut child_ids: HashMap<String, Vec<String>> = HashMap::new();
    let mut roots: Vec<String> = Vec::new();
    for t in &tasks {
        let parents = store.parents_of(&t.id)?;
        if parents.is_empty() {
            roots.push(t.id.clone());
        }
        for par in parents {
            if meta.contains_key(&par.id) {
                child_ids.entry(par.id).or_default().push(t.id.clone());
            }
        }
    }
    for kids in child_ids.values_mut() {
        kids.sort();
    }
    roots.sort();
    let mut visited: HashSet<String> = HashSet::with_capacity(tasks.len());
    Ok(roots
        .into_iter()
        .filter_map(|r| to_node(&meta, &child_ids, &r, &mut visited))
        .collect())
}

fn to_node(
    meta: &HashMap<String, (String, model::TaskState)>,
    child_ids: &HashMap<String, Vec<String>>,
    id: &str,
    visited: &mut HashSet<String>,
) -> Option<TaskNode> {
    if !visited.insert(id.to_owned()) {
        return None;
    }
    let (title, state) = meta
        .get(id)
        .cloned()
        .unwrap_or_else(|| (String::new(), model::TaskState::Pending));
    let mut kids: Vec<TaskNode> = child_ids
        .get(id)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|cid| to_node(meta, child_ids, cid, visited))
        .collect();
    kids.sort_by(|a, b| a.id.cmp(&b.id));
    Some(TaskNode {
        id: id.to_owned(),
        title,
        state,
        children: kids,
    })
}

fn print_node(n: &TaskNode, depth: usize) {
    println!("{}{} [{}] {}", "  ".repeat(depth), n.id, n.state, n.title);
    for c in &n.children {
        print_node(c, depth + 1);
    }
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

        let roots = build_tree(&store, &session).expect("tree");
        assert_eq!(
            count_occurrences(&roots, &d),
            1,
            "D con 2 padres sale 1 vez"
        );
        assert_eq!(count_occurrences(&roots, &a), 1);
        assert_eq!(count_occurrences(&roots, &b), 1);
        assert_eq!(count_occurrences(&roots, &c), 1);
    }
}
