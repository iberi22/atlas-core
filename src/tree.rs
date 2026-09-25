// Tree building, project filtering, and LLM context synthesis (REQ-F-003/005).
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::model::{TaskNode, TaskState};
use crate::store::{self, Store};

/// Summary of tasks belonging to a specific project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub name: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub ready_tasks: usize,
    pub in_progress_tasks: usize,
    pub pending_or_blocked_tasks: usize,
    pub progress_pct: f64,
}

/// Helper to check if a task matches a project filter.
#[must_use]
pub fn matches_project(title: &str, id: &str, proj: &str) -> bool {
    let p_lower = proj.to_lowercase();
    let tag = format!("[{}]", p_lower);
    title.to_lowercase().starts_with(&tag)
        || title.to_lowercase().contains(&tag)
        || id.to_lowercase().starts_with(&format!("{}-", p_lower))
        || id.to_lowercase().contains(&p_lower)
}

/// Forest of task trees for a session (roots = tasks without parents within the view).
/// If `project_filter` is given, only tasks for that project are included and
/// root tasks of that project become roots.
pub fn build_tree(
    store: &Store,
    session: &str,
    project_filter: Option<&str>,
) -> store::Result<Vec<TaskNode>> {
    let all_tasks = store.list_tasks(Some(session))?;

    let tasks: Vec<_> = if let Some(proj) = project_filter {
        all_tasks
            .into_iter()
            .filter(|t| matches_project(&t.title, &t.id, proj))
            .collect()
    } else {
        all_tasks
    };

    let mut meta: HashMap<String, (String, TaskState)> = HashMap::with_capacity(tasks.len());
    for t in &tasks {
        meta.insert(t.id.clone(), (t.title.clone(), t.state));
    }

    let mut child_ids: HashMap<String, Vec<String>> = HashMap::new();
    let mut roots: Vec<String> = Vec::new();

    for t in &tasks {
        let parents = store.parents_of(&t.id)?;
        // A task is a root in this view if it has no parents, or none of its parents are in `meta`
        let local_parents: Vec<_> = parents.into_iter().filter(|p| meta.contains_key(&p.id)).collect();
        if local_parents.is_empty() {
            roots.push(t.id.clone());
        } else {
            for par in local_parents {
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

pub fn to_node(
    meta: &HashMap<String, (String, TaskState)>,
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
        .unwrap_or_else(|| (String::new(), TaskState::Pending));
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

pub fn print_node(n: &TaskNode, depth: usize) {
    println!("{}{} [{}] {}", "  ".repeat(depth), n.id, n.state, n.title);
    for c in &n.children {
        print_node(c, depth + 1);
    }
}

/// Scans tasks to compute project-level rollups and progress.
pub fn get_projects_summary(store: &Store, session: Option<&str>) -> store::Result<Vec<ProjectSummary>> {
    let tasks = store.list_tasks(session)?;
    let mut map: HashMap<String, Vec<&crate::model::Task>> = HashMap::new();

    for t in &tasks {
        let mut proj = "GENERAL".to_string();
        if let Some(start) = t.title.find('[') {
            if let Some(end) = t.title[start..].find(']') {
                let tag = &t.title[start + 1..start + end];
                if !tag.is_empty() && !tag.starts_with("Ola") && !tag.starts_with("JULES") {
                    proj = tag.to_uppercase();
                }
            }
        }
        if proj == "GENERAL" {
            if let Some(dash) = t.id.find('-') {
                proj = t.id[..dash].to_uppercase();
            }
        }
        map.entry(proj).or_default().push(t);
    }

    let mut out = Vec::with_capacity(map.len());
    for (name, pro_tasks) in map {
        let total = pro_tasks.len();
        let completed = pro_tasks.iter().filter(|t| t.state == TaskState::Completed).count();
        let ready = pro_tasks.iter().filter(|t| t.state == TaskState::Ready).count();
        let in_prog = pro_tasks.iter().filter(|t| t.state == TaskState::InProgress).count();
        let blocked = total.saturating_sub(completed + ready + in_prog);
        let progress_pct = if total > 0 {
            (completed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        out.push(ProjectSummary {
            name,
            total_tasks: total,
            completed_tasks: completed,
            ready_tasks: ready,
            in_progress_tasks: in_prog,
            pending_or_blocked_tasks: blocked,
            progress_pct,
        });
    }

    out.sort_by(|a, b| b.total_tasks.cmp(&a.total_tasks));
    Ok(out)
}
