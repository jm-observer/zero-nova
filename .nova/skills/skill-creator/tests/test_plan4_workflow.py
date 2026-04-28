import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.finalize_improvement import finalize_improvement
from scripts.generate_report import write_workspace_reports
from scripts.prepare_improvement import prepare_improvement_workspace
from scripts.resume_improvement import resume_improvement
from scripts.run_eval import RUN_MODE_BASELINE_NONE, RUN_MODE_BASELINE_ORIGINAL, RUN_MODE_CANDIDATE
from scripts.utils import read_json, write_json


def write_skill(skill_dir: Path, description: str = "Demo description") -> None:
    skill_dir.mkdir(parents=True, exist_ok=True)
    (skill_dir / "SKILL.md").write_text(
        "---\n"
        "name: demo-skill\n"
        f"description: {description}\n"
        "---\n"
        "Use `references/guide.md` when needed.\n",
        encoding="utf-8",
    )


def make_eval_results() -> dict:
    return {
        RUN_MODE_BASELINE_NONE: {"summary": {"passed": 0, "failed": 1, "total": 1, "pass_rate": 0.0}, "results": []},
        RUN_MODE_BASELINE_ORIGINAL: {
            "summary": {"passed": 0, "failed": 1, "total": 1, "pass_rate": 0.0},
            "results": [{"query": "baseline", "pass": False, "trigger_rate": 0.0, "runs_detail": [{"error_type": None}]}],
        },
        RUN_MODE_CANDIDATE: {
            "summary": {"passed": 1, "failed": 0, "total": 1, "pass_rate": 1.0},
            "results": [{"query": "candidate", "pass": True, "trigger_rate": 1.0, "runs_detail": [{"error_type": None}]}],
        },
    }


def make_score_summary() -> dict:
    return {
        "modes": {
            RUN_MODE_BASELINE_ORIGINAL: {"pass_rate": 0.0, "passed": 0, "failed": 1, "total": 1, "error_counts": {}},
            RUN_MODE_CANDIDATE: {"pass_rate": 1.0, "passed": 1, "failed": 0, "total": 1, "error_counts": {}},
        },
        "comparisons": {
            "candidate_vs_baseline_original": {"pass_rate_delta": 1.0, "passed_delta": 1},
            "candidate_vs_best": {"overall_delta": 1.0, "false_trigger_delta": 0},
        },
        "failure_clusters": {
            RUN_MODE_BASELINE_ORIGINAL: {"missed_trigger": {"count": 1, "queries": ["baseline"]}},
            RUN_MODE_CANDIDATE: {},
        },
        "score": {"overall": 1.0, "trigger_pass_rate": 1.0, "behavior_pass_rate": None},
        "decision_hints": {"promoted": True, "severe_regression": False, "false_trigger_count": 0},
    }


def seed_iteration_artifacts(workspace: Path) -> None:
    iteration_dir = workspace / "iterations" / "iteration-001"
    candidate_skill = iteration_dir / "candidate-skill"
    write_skill(candidate_skill, description="Improved description")
    write_json(iteration_dir / "eval-results.json", make_eval_results())
    write_json(iteration_dir / "score-summary.json", make_score_summary())
    write_json(
        iteration_dir / "candidate-diff-summary.json",
        {"applied_changes": [{"type": "update_description", "reason": "improve trigger wording"}]},
    )
    write_json(
        iteration_dir / "candidate-plan.json",
        {"focus_areas": ["missed_trigger"], "changes": [{"type": "update_description"}]},
    )
    (iteration_dir / "notes.md").write_text("# Iteration 1\n", encoding="utf-8")

    session = read_json(workspace / "improvement-session.json")
    session["iterations"] = [
        {
            "iteration": 1,
            "iteration_path": str(iteration_dir.resolve()),
            "candidate_skill_path": str(candidate_skill.resolve()),
            "eval_results_path": str((iteration_dir / "eval-results.json").resolve()),
            "score_summary_path": str((iteration_dir / "score-summary.json").resolve()),
            "notes_path": str((iteration_dir / "notes.md").resolve()),
            "candidate_plan_path": str((iteration_dir / "candidate-plan.json").resolve()),
            "candidate_diff_summary_path": str((iteration_dir / "candidate-diff-summary.json").resolve()),
            "promoted": True,
        }
    ]
    session["best_iteration"] = 1
    session["best_score"] = {"overall": 1.0, "trigger_pass_rate": 1.0, "false_trigger_count": 0}
    session["status"] = "completed"
    write_json(workspace / "improvement-session.json", session)

    best_skill = workspace / "best-skill"
    write_skill(best_skill, description="Improved description")
    write_json(workspace / "feedback.json", {"reviews": [{"run_id": "run-1-0", "feedback": "Looks better."}]})


class Plan4WorkflowTests(unittest.TestCase):
    def test_generate_reports_writes_iteration_and_final_pages(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir)
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")

            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            seed_iteration_artifacts(workspace)

            outputs = write_workspace_reports(workspace)
            self.assertTrue(Path(outputs["iteration-1"]).exists())
            self.assertTrue(Path(outputs["final"]).exists())
            final_html = Path(outputs["final"]).read_text(encoding="utf-8")
            self.assertIn("Final Report", final_html)
            self.assertIn("Looks better.", final_html)

    def test_resume_improvement_detects_rerun_for_incomplete_iteration(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir)
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")

            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            iteration_dir = workspace / "iterations" / "iteration-001"
            candidate_skill = iteration_dir / "candidate-skill"
            write_skill(candidate_skill, description="Half done")

            session_payload = read_json(workspace / "improvement-session.json")
            session_payload["status"] = "paused"
            session_payload["iterations"] = [
                {
                    "iteration": 1,
                    "iteration_path": str(iteration_dir.resolve()),
                    "candidate_skill_path": str(candidate_skill.resolve()),
                    "eval_results_path": str((iteration_dir / "eval-results.json").resolve()),
                    "score_summary_path": str((iteration_dir / "score-summary.json").resolve()),
                    "notes_path": str((iteration_dir / "notes.md").resolve()),
                }
            ]
            write_json(workspace / "improvement-session.json", session_payload)

            result = resume_improvement(
                workspace,
                num_workers=1,
                timeout=5,
                max_iterations=1,
                runs_per_query=1,
                trigger_threshold=0.5,
                model=None,
                dry_run=True,
            )
            self.assertEqual(result["resume_iteration"], 1)
            self.assertEqual(result["resume_reason"], "rerun_iteration")

    def test_finalize_improvement_exports_and_writebacks_with_backup(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir, description="Original description")
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")

            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            seed_iteration_artifacts(workspace)

            export_dir = root / "exported-best-skill"
            output = finalize_improvement(workspace, export_dir=export_dir, writeback=True)

            self.assertTrue(export_dir.exists())
            self.assertTrue(Path(output["backup_path"]).exists())
            self.assertTrue(Path(output["diff_path"]).exists())
            self.assertIn("Improved description", (skill_dir / "SKILL.md").read_text(encoding="utf-8"))

    def test_finalize_improvement_rejects_missing_best_skill(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir)
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")

            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            best_skill = workspace / "best-skill"
            if best_skill.exists():
                for child in best_skill.iterdir():
                    if child.is_dir():
                        shutil.rmtree(child)
                    else:
                        child.unlink()

            with self.assertRaisesRegex(ValueError, "Best skill directory"):
                finalize_improvement(workspace, export_dir=None, writeback=False)


if __name__ == "__main__":
    unittest.main()
