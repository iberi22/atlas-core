// `atlas` CLI: sessions, task DAG, and tree render (REQ-F-003/005).
use atlas::{Store, model, store};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::collections::HashMap;
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
    Ok(roots
        .into_iter()
        .map(|r| to_node(&meta, &child_ids, &r))
        .collect())
}

fn to_node(
    meta: &HashMap<String, (String, model::TaskState)>,
    child_ids: &HashMap<String, Vec<String>>,
    id: &str,
) -> TaskNode {
    let (title, state) = meta
        .get(id)
        .cloned()
        .unwrap_or_else(|| (String::new(), model::TaskState::Pending));
    let mut kids: Vec<TaskNode> = child_ids
        .get(id)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|cid| to_node(meta, child_ids, cid))
        .collect();
    kids.sort_by(|a, b| a.id.cmp(&b.id));
    TaskNode {
        id: id.to_owned(),
        title,
        state,
        children: kids,
    }
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
