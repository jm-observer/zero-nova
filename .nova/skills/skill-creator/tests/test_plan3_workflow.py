import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts.apply_candidate import apply_candidate_plan
from scripts.prepare_improvement import prepare_improvement_workspace
from scripts.run_eval import RUN_MODE_BASELINE_NONE, RUN_MODE_BASELINE_ORIGINAL, RUN_MODE_CANDIDATE
from scripts.run_loop import run_loop
from scripts.score_iteration import score_iteration_result
from scripts.utils import read_json


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


def make_mode_result(pass_count: int, total: int, *, should_trigger: bool = True) -> dict:
    results = []
    for index in range(total):
        passed = index < pass_count
        trigger_rate = 1.0 if passed and should_trigger else 0.0
        if not should_trigger:
            trigger_rate = 0.0 if passed else 1.0
        results.append(
            {
                "query": f"q{index}",
                "should_trigger": should_trigger,
                "trigger_rate": trigger_rate,
                "triggers": 1 if trigger_rate else 0,
                "runs": 1,
                "pass": passed,
                "runs_detail": [
                    {
                        "error_type": None,
                        "triggered": bool(trigger_rate),
                    }
                ],
            }
        )
    return {
        "summary": {
            "passed": pass_count,
            "failed": total - pass_count,
            "total": total,
            "pass_rate": pass_count / total,
            "error_counts": {},
            "trigger_threshold": 0.5,
        },
        "results": results,
    }


class Plan3WorkflowTests(unittest.TestCase):
    def test_apply_candidate_writes_diff_summary(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            base_skill = root / "best-skill"
            write_skill(base_skill)
            references_dir = base_skill / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")

            candidate_skill = root / "iteration-001" / "candidate-skill"
            plan = {
                "changes": [
                    {
                        "type": "update_description",
                        "path": "SKILL.md",
                        "reason": "tighten trigger wording",
                        "new_value": "Better description",
                    },
                    {
                        "type": "upsert_section",
                        "path": "SKILL.md",
                        "reason": "make focus explicit",
                        "section_title": "## Improvement Focus",
                        "lines": ["- reduce false triggers"],
                    },
                ]
            }

            output = apply_candidate_plan(base_skill, candidate_skill, plan)
            content = (candidate_skill / "SKILL.md").read_text(encoding="utf-8")
            self.assertIn("description: Better description", content)
            self.assertIn("## Improvement Focus", content)
            self.assertEqual(len(output["applied_changes"]), 2)
            self.assertTrue((candidate_skill.parent / "candidate-diff-summary.json").exists())

    def test_score_iteration_reports_failure_clusters(self) -> None:
        results_by_mode = {
            RUN_MODE_BASELINE_ORIGINAL: make_mode_result(2, 2),
            RUN_MODE_CANDIDATE: make_mode_result(1, 2),
        }
        payload = score_iteration_result(results_by_mode)
        self.assertIn("missed_trigger", payload["failure_clusters"][RUN_MODE_CANDIDATE])
        self.assertIn("candidate_vs_best", payload["comparisons"])

    @patch("scripts.run_loop.improve_skill")
    @patch("scripts.run_loop.apply_candidate_plan")
    @patch("scripts.run_loop.run_iteration")
    def test_run_loop_promotes_better_candidate(self, mock_run_iteration, mock_apply_candidate, mock_improve_skill) -> None:
        mock_improve_skill.return_value = {"changes": [], "focus_areas": ["missed_trigger"]}
        mock_apply_candidate.return_value = {"applied_changes": [{"type": "update_description", "reason": "x"}]}
        mock_run_iteration.return_value = {
            "iteration": 1,
            "iteration_path": "unused",
            "results": {
                RUN_MODE_BASELINE_NONE: make_mode_result(0, 2),
                RUN_MODE_BASELINE_ORIGINAL: make_mode_result(1, 2),
                RUN_MODE_CANDIDATE: make_mode_result(2, 2),
            },
            "summary": {},
        }

        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir)
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")
            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            trigger_eval_path = workspace / "evals" / "trigger-evals.json"
            trigger_eval_path.write_text('[{"query":"q","should_trigger":true}]\n', encoding="utf-8")

            output = run_loop(
                workspace,
                trigger_eval_path,
                num_workers=1,
                timeout=5,
                max_iterations=1,
                runs_per_query=1,
                trigger_threshold=0.5,
                model=None,
            )

            persisted = read_json(workspace / "improvement-session.json")
            self.assertEqual(output["best_iteration"], 1)
            self.assertEqual(persisted["best_iteration"], 1)
            self.assertEqual(persisted["status"], "completed")

    @patch("scripts.run_loop.improve_skill")
    @patch("scripts.run_loop.apply_candidate_plan")
    @patch("scripts.run_loop.run_iteration")
    def test_run_loop_rolls_back_on_no_improvement(self, mock_run_iteration, mock_apply_candidate, mock_improve_skill) -> None:
        mock_improve_skill.return_value = {"changes": [], "focus_areas": ["false_trigger"]}
        mock_apply_candidate.return_value = {"applied_changes": [{"type": "update_description", "reason": "x"}]}
        mock_run_iteration.side_effect = [
            {
                "iteration": 1,
                "iteration_path": "unused",
                "results": {
                    RUN_MODE_BASELINE_NONE: make_mode_result(0, 2),
                    RUN_MODE_BASELINE_ORIGINAL: make_mode_result(1, 2),
                    RUN_MODE_CANDIDATE: make_mode_result(1, 2),
                },
                "summary": {},
            },
            {
                "iteration": 2,
                "iteration_path": "unused",
                "results": {
                    RUN_MODE_BASELINE_NONE: make_mode_result(0, 2),
                    RUN_MODE_BASELINE_ORIGINAL: make_mode_result(1, 2),
                    RUN_MODE_CANDIDATE: make_mode_result(1, 2),
                },
                "summary": {},
            },
        ]

        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir)
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")
            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            trigger_eval_path = workspace / "evals" / "trigger-evals.json"
            trigger_eval_path.write_text('[{"query":"q","should_trigger":true}]\n', encoding="utf-8")

            output = run_loop(
                workspace,
                trigger_eval_path,
                num_workers=1,
                timeout=5,
                max_iterations=3,
                runs_per_query=1,
                trigger_threshold=0.5,
                model=None,
            )

            persisted = read_json(workspace / "improvement-session.json")
            self.assertEqual(output["exit_reason"], "converged")
            self.assertIsNone(persisted["best_iteration"])

    @patch("scripts.run_loop.improve_skill")
    @patch("scripts.run_loop.apply_candidate_plan")
    def test_run_loop_pauses_on_apply_failure(self, mock_apply_candidate, mock_improve_skill) -> None:
        mock_improve_skill.return_value = {"changes": [], "focus_areas": []}
        mock_apply_candidate.side_effect = ValueError("broken plan")

        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir)
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")
            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            trigger_eval_path = workspace / "evals" / "trigger-evals.json"
            trigger_eval_path.write_text('[{"query":"q","should_trigger":true}]\n', encoding="utf-8")

            output = run_loop(
                workspace,
                trigger_eval_path,
                num_workers=1,
                timeout=5,
                max_iterations=1,
                runs_per_query=1,
                trigger_threshold=0.5,
                model=None,
            )

            persisted = read_json(workspace / "improvement-session.json")
            self.assertEqual(output["status"], "paused")
            self.assertEqual(persisted["status"], "paused")
            self.assertIn("apply_candidate_failed", persisted["last_error"])


if __name__ == "__main__":
    unittest.main()
