#!/usr/bin/env python3
"""
Atlas-Xavier Evaluator & Task Tree Synthesizer
=============================================
1. Scans SWAL projects (apps/* and cores/*).
2. Parses features.json / .gitcore/features.json / user stories.
3. Evaluates tests and code symbols via Xavier CodeGraph DB (code_graph.db) and Memory.
4. Identifies missing gaps and test coverage.
5. Populates / updates tasks and builds the dependency DAG (edges table) in atlas.db.
6. Generates token-compact project context packs for LLMs.
"""

import os
import sys
import json
import os
import sqlite3
import re
import time
import argparse
from pathlib import Path
from typing import Dict, List, Any, Optional, Tuple

# Workspace root: overridable, otherwise derived from this file's location
# (<root>/apps/atlas-core/scripts/atlas_xavier_evaluator.py). Never hardcode a
# machine-specific path: this repository is public.
SWAL_ROOT = Path(os.environ.get("SWAL_ROOT") or Path(__file__).resolve().parents[3])
ATLAS_DB = SWAL_ROOT / "apps/atlas-core/atlas.db"
XAVIER_CODEGRAPH_DB = SWAL_ROOT / "apps/xavier/data/code_graph.db"
XAVIER_VECSTORE_DB = SWAL_ROOT / "apps/xavier/data/vec-store.sqlite3"

class AtlasXavierEvaluator:
    def __init__(self, atlas_db_path: Path, codegraph_db_path: Path, session_id: Optional[str] = None):
        self.atlas_db_path = atlas_db_path
        self.codegraph_db_path = codegraph_db_path
        self.session_id = session_id or self._get_active_session()
        self.graph_conn = None
        if self.codegraph_db_path.exists():
            try:
                self.graph_conn = sqlite3.connect(str(self.codegraph_db_path))
            except Exception as e:
                print(f"[warn] Could not connect to Xavier CodeGraph: {e}")

    def _get_active_session(self) -> str:
        con = sqlite3.connect(str(self.atlas_db_path))
        cur = con.cursor()
        row = cur.execute("SELECT id FROM sessions ORDER BY started_at DESC LIMIT 1").fetchone()
        con.close()
        return row[0] if row else "s_active"

    def scan_all_projects(self) -> Dict[str, Dict[str, Any]]:
        """Scans apps/ and returns structured project metadata and features."""
        projects = {}
        apps_dir = SWAL_ROOT / "apps"
        
        for p in sorted(apps_dir.iterdir()):
            if not p.is_dir() or p.name.startswith("."):
                continue
            
            proj_name = p.name
            features_file = None
            
            # Look for features.json
            for candidate in [
                p / ".gitcore/features.json",
                p / "features.json",
                p / "docs/legacy/features.json"
            ]:
                if candidate.exists():
                    features_file = candidate
                    break
            
            features = []
            metadata = {}
            if features_file:
                try:
                    with open(features_file, "r", encoding="utf-8") as f:
                        data = json.load(f)
                        metadata = data.get("metadata", {})
                        raw_features = data.get("features", [])
                        if isinstance(raw_features, dict):
                            for fid, fval in raw_features.items():
                                if isinstance(fval, dict):
                                    fval["id"] = fval.get("id", fid)
                                    features.append(fval)
                        elif isinstance(raw_features, list):
                            features = raw_features
                except Exception as e:
                    print(f"[warn] Failed to parse {features_file}: {e}")

            # Check test suites
            test_info = self._analyze_tests(p)

            projects[proj_name] = {
                "name": proj_name,
                "path": str(p),
                "features_file": str(features_file) if features_file else None,
                "features": features,
                "metadata": metadata,
                "tests": test_info
            }
        return projects

    def _analyze_tests(self, proj_path: Path) -> Dict[str, Any]:
        """Detects test files and test framework in project."""
        test_files = []
        framework = "unknown"
        
        if (proj_path / "Cargo.toml").exists():
            framework = "cargo"
            for t in proj_path.glob("tests/**/*.rs"):
                test_files.append(str(t.relative_to(proj_path)))
            for t in proj_path.glob("src/**/tests.rs"):
                test_files.append(str(t.relative_to(proj_path)))
        elif (proj_path / "package.json").exists():
            framework = "npm/vitest/playwright"
            for ext in ["*.spec.ts", "*.test.ts", "*.spec.tsx", "*.test.tsx", "*.cy.ts"]:
                for t in proj_path.glob(f"**/{ext}"):
                    if "node_modules" not in t.parts:
                        test_files.append(str(t.relative_to(proj_path)))
        elif (proj_path / "pubspec.yaml").exists():
            framework = "flutter"
            for t in proj_path.glob("test/**/*.dart"):
                test_files.append(str(t.relative_to(proj_path)))

        return {
            "framework": framework,
            "test_files_count": len(test_files),
            "test_files_sample": test_files[:5]
        }

    def evaluate_with_codegraph(self, project_name: str, features: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
        """Evaluates each feature against Xavier CodeGraph symbols and file existence."""
        if not self.graph_conn:
            return features

        cur = self.graph_conn.cursor()
        evaluated = []
        for feat in features:
            f_copy = dict(feat)
            f_id = feat.get("id", "")
            f_name = feat.get("name", "")
            evidence = feat.get("evidence", [])
            
            # Check symbols matching name keywords
            keywords = [w for w in re.findall(r'[a-zA-Z_]{4,}', f"{f_id} {f_name}") if w.lower() not in ["feat", "feature", "layer", "test", "with"]]
            symbol_matches = 0
            for kw in keywords[:3]:
                try:
                    count = cur.execute("SELECT count(*) FROM symbols WHERE name LIKE ?", (f"%{kw}%",)).fetchone()[0]
                    symbol_matches += count
                except Exception:
                    pass

            # Check evidence files
            evidence_found = 0
            if isinstance(evidence, list):
                for ev in evidence:
                    if isinstance(ev, str):
                        full_ev = SWAL_ROOT / "apps" / project_name / ev
                        if full_ev.exists():
                            evidence_found += 1

            f_copy["codegraph_symbols"] = symbol_matches
            f_copy["evidence_files_found"] = evidence_found
            
            # Status check
            progress = feat.get("progress_pct", feat.get("implementation_percentage", 0))
            is_passing = feat.get("passes", True if progress >= 80 else False)
            
            if progress >= 90 and is_passing:
                f_copy["evaluated_state"] = "Completed"
            elif progress > 0 or symbol_matches > 0:
                f_copy["evaluated_state"] = "InProgress"
            else:
                f_copy["evaluated_state"] = "Ready"

            evaluated.append(f_copy)
        return evaluated

    def sync_to_atlas_dag(self, projects_evaluated: Dict[str, Any]) -> Tuple[int, int]:
        """Creates or updates tasks and connects dependencies (edges) in atlas.db."""
        con = sqlite3.connect(str(self.atlas_db_path))
        cur = con.cursor()

        tasks_created = 0
        edges_created = 0
        now_ts = int(time.time())

        # Load existing tasks to map by project and title/ID
        existing_tasks = {}
        for row in cur.execute("SELECT id, title, state FROM tasks WHERE session_id = ?", (self.session_id,)).fetchall():
            tid, title, state = row
            existing_tasks[tid] = {"title": title, "state": state}

        # Clear existing edges for fresh coherent DAG linking
        cur.execute("DELETE FROM edges WHERE child_id IN (SELECT id FROM tasks WHERE session_id = ?)", (self.session_id,))

        for proj_name, pdata in projects_evaluated.items():
            features = pdata.get("features", [])
            
            # 1. Create or identify Root Epic Task for the project
            root_task_id = f"{proj_name}-root-epic"
            root_title = f"[{proj_name.upper()}] Epic: {pdata.get('metadata', {}).get('project_display', proj_name.capitalize())} Full Implementation"
            
            root_state = "InProgress"
            all_comp = all(f.get("evaluated_state") == "Completed" for f in features) if features else False
            if all_comp:
                root_state = "Completed"

            if root_task_id not in existing_tasks:
                cur.execute(
                    "INSERT INTO tasks (id, session_id, title, state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
                    (root_task_id, self.session_id, root_title, root_state, now_ts, now_ts)
                )
                existing_tasks[root_task_id] = {"title": root_title, "state": root_state}
                tasks_created += 1
            else:
                cur.execute(
                    "UPDATE tasks SET title = ?, state = ?, updated_at = ? WHERE id = ?",
                    (root_title, root_state, now_ts, root_task_id)
                )

            # Map of feature_id -> task_id
            feat_task_map = {}

            # 2. Synchronize Feature Tasks
            for f in features:
                fid = f.get("id", "")
                fname = f.get("name", fid)
                task_id = f"{proj_name}-{fid.lower().replace('_', '-')}"
                task_title = f"[{proj_name.upper()}] {fid}: {fname}"
                task_state = f.get("evaluated_state", "Ready")
                
                feat_task_map[fid] = task_id

                if task_id not in existing_tasks:
                    cur.execute(
                        "INSERT INTO tasks (id, session_id, title, state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
                        (task_id, self.session_id, task_title, task_state, now_ts, now_ts)
                    )
                    existing_tasks[task_id] = {"title": task_title, "state": task_state}
                    tasks_created += 1
                else:
                    cur.execute(
                        "UPDATE tasks SET title = ?, state = ?, updated_at = ? WHERE id = ?",
                        (task_title, task_state, now_ts, task_id)
                    )

                # Link root_task (parent) -> feature_task (child)
                try:
                    cur.execute(
                        "INSERT OR IGNORE INTO edges (child_id, parent_id) VALUES (?, ?)",
                        (task_id, root_task_id)
                    )
                    edges_created += 1
                except Exception:
                    pass

            # 3. Synchronize Inter-Feature Dependencies
            for f in features:
                fid = f.get("id", "")
                child_task_id = feat_task_map.get(fid)
                if not child_task_id:
                    continue

                deps = f.get("dependencies", [])
                if isinstance(deps, list):
                    for dep_fid in deps:
                        parent_task_id = feat_task_map.get(dep_fid)
                        if parent_task_id and parent_task_id != child_task_id:
                            try:
                                cur.execute(
                                    "INSERT OR IGNORE INTO edges (child_id, parent_id) VALUES (?, ?)",
                                    (child_task_id, parent_task_id)
                                )
                                edges_created += 1
                            except Exception:
                                pass

            # 4. Link existing Hermes board tasks for this project to root or feature tasks
            proj_tag = f"[{proj_name.upper()}]"
            proj_tag_lower = f"[{proj_name.lower()}]"
            for tid, tinfo in existing_tasks.items():
                if tid == root_task_id or tid in feat_task_map.values():
                    continue
                ttitle = tinfo.get("title", "")
                if ttitle.startswith(proj_tag) or ttitle.startswith(proj_tag_lower) or f"[{proj_name}]" in ttitle or tid.startswith(f"{proj_name}-"):
                    # Link to root epic as child
                    try:
                        cur.execute(
                            "INSERT OR IGNORE INTO edges (child_id, parent_id) VALUES (?, ?)",
                            (tid, root_task_id)
                        )
                        edges_created += 1
                    except Exception:
                        pass

        con.commit()
        con.close()
        return tasks_created, edges_created

    def generate_llm_project_pack(self, project_name: str) -> Dict[str, Any]:
        """Generates a compact, token-efficient summary for LLMs."""
        con = sqlite3.connect(str(self.atlas_db_path))
        cur = con.cursor()

        tasks = cur.execute(
            "SELECT id, title, state FROM tasks WHERE session_id = ? AND (title LIKE ? OR id LIKE ?) ORDER BY state, title",
            (self.session_id, f"[{project_name.upper()}]%", f"{project_name}%")
        ).fetchall()

        edges = cur.execute("""
            SELECT e.parent_id, e.child_id 
            FROM edges e
            JOIN tasks tp ON e.parent_id = tp.id
            JOIN tasks tc ON e.child_id = tc.id
            WHERE tp.session_id = ? AND (tp.title LIKE ? OR tc.title LIKE ?)
        """, (self.session_id, f"[{project_name.upper()}]%", f"[{project_name.upper()}]%")).fetchall()

        con.close()

        summary = {
            "project": project_name.upper(),
            "session": self.session_id,
            "total_tasks": len(tasks),
            "dependencies_count": len(edges),
            "counts": {
                "Completed": sum(1 for t in tasks if t[2] in ["Completed", "Done"]),
                "InProgress": sum(1 for t in tasks if t[2] in ["InProgress", "Running"]),
                "Ready": sum(1 for t in tasks if t[2] in ["Ready"]),
                "Pending_or_Blocked": sum(1 for t in tasks if t[2] in ["Pending", "Blocked"])
            },
            "tasks": [{"id": t[0], "title": t[1], "state": t[2]} for t in tasks],
            "edges": [{"parent_id": e[0], "child_id": e[1]} for e in edges]
        }
        return summary


def main():
    parser = argparse.ArgumentParser(description="Atlas-Xavier Evaluator & Task Tree Synthesizer")
    parser.add_argument("--db", type=Path, default=ATLAS_DB, help="Path to atlas.db")
    parser.add_argument("--codegraph", type=Path, default=XAVIER_CODEGRAPH_DB, help="Path to code_graph.db")
    parser.add_argument("--session", type=str, default=None, help="Session ID")
    parser.add_argument("--project", type=str, default=None, help="Export LLM pack for specific project")
    parser.add_argument("--json", action="store_true", help="Output JSON")
    args = parser.parse_args()

    evaluator = AtlasXavierEvaluator(args.db, args.codegraph, args.session)

    if args.project:
        pack = evaluator.generate_llm_project_pack(args.project)
        print(json.dumps(pack, indent=2))
        return

    print("=" * 60)
    print("🚀 Running Atlas-Xavier Evaluation Pipeline...")
    print(f"Atlas DB:     {args.db}")
    print(f"CodeGraph DB: {args.codegraph}")
    print(f"Session ID:   {evaluator.session_id}")
    print("=" * 60)

    # 1. Scan projects
    projects = evaluator.scan_all_projects()
    print(f"📦 Discovered {len(projects)} projects in SWAL apps/")

    # 2. Evaluate each project against CodeGraph
    evaluated_projects = {}
    for pname, pdata in projects.items():
        eval_feats = evaluator.evaluate_with_codegraph(pname, pdata.get("features", []))
        pdata["features"] = eval_feats
        evaluated_projects[pname] = pdata
        print(f"  - {pname:24}: {len(eval_feats):2} features | {pdata['tests']['test_files_count']:2} test files ({pdata['tests']['framework']})")

    # 3. Synchronize to Atlas DAG
    tasks_created, edges_created = evaluator.sync_to_atlas_dag(evaluated_projects)
    print("=" * 60)
    print(f"✅ Sync complete! Tasks created/updated: {tasks_created}, Edges linked: {edges_created}")
    print("=" * 60)


if __name__ == "__main__":
    main()
