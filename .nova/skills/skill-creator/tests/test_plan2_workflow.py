import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts.prepare_improvement import prepare_improvement_workspace
from scripts.quick_validate import validate_skill
from scripts.run_eval import RUN_MODE_BASELINE_NONE, RUN_MODE_CANDIDATE, analyze_cli_output, run_eval
from scripts.run_iteration import run_iteration
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


class Plan2WorkflowTests(unittest.TestCase):
    def test_validate_skill_checks_referenced_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            skill_dir = Path(temp_dir) / "demo-skill"
            write_skill(skill_dir)

            valid, message = validate_skill(skill_dir, require_iteration_ready=True)
            self.assertFalse(valid)
            self.assertIn("references/guide.md", message)

            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")

            valid, message = validate_skill(skill_dir, require_iteration_ready=True)
            self.assertTrue(valid)
            self.assertIn("iteration-ready", message)

    def test_prepare_improvement_workspace_creates_snapshot_and_placeholders(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            write_skill(skill_dir)
            references_dir = skill_dir / "references"
            references_dir.mkdir()
            (references_dir / "guide.md").write_text("guide", encoding="utf-8")

            session = prepare_improvement_workspace(skill_dir)
            workspace = Path(session["workspace_path"])
            self.assertTrue((workspace / "target-skill-snapshot" / "SKILL.md").exists())
            self.assertTrue((workspace / "evals" / "trigger-evals.json").exists())
            self.assertTrue((workspace / "evals" / "behavior-evals.json").exists())
            persisted = read_json(workspace / "improvement-session.json")
            self.assertEqual(persisted["status"], "evaluating")

    def test_analyze_cli_output_recognizes_skill_events(self) -> None:
        output = "\n".join(
            [
                '{"ToolStart":{"id":"1","name":"Skill","input":{"skill":"demo"}}}',
                '{"SkillLoaded":{"skill_name":"demo-skill"}}',
            ]
        )
        result = analyze_cli_output(output)
        self.assertTrue(result["triggered"])
        self.assertIsNone(result["error_type"])
        self.assertIn("demo-skill", result["loaded_skills"])

    @patch("scripts.run_eval.run_single_query")
    def test_run_eval_supports_baseline_none(self, mock_run_single_query) -> None:
        mock_run_single_query.return_value = {
            "query": "hello",
            "mode": RUN_MODE_BASELINE_NONE,
            "triggered": False,
            "timed_out": False,
            "error_type": None,
            "error_message": None,
            "loaded_skills": [],
            "stdout": "",
            "stderr": "",
            "return_code": 0,
        }

        output = run_eval(
            [{"query": "hello", "should_trigger": False}],
            skill_path=None,
            num_workers=1,
            timeout=5,
            project_root=Path.cwd(),
            runs_per_query=1,
            mode=RUN_MODE_BASELINE_NONE,
        )
        self.assertEqual(output["summary"]["passed"], 1)
        self.assertEqual(output["mode"], RUN_MODE_BASELINE_NONE)

    @patch("scripts.run_iteration.run_eval")
    def test_run_iteration_writes_artifacts(self, mock_run_eval) -> None:
        baseline_none = {
            "summary": {"passed": 1, "failed": 0, "total": 1, "pass_rate": 1.0, "error_counts": {}},
            "results": [{"query": "q", "pass": True}],
        }
        baseline_original = {
            "summary": {"passed": 1, "failed": 0, "total": 1, "pass_rate": 1.0, "error_counts": {}},
            "results": [{"query": "q", "pass": True}],
        }
        candidate = {
            "summary": {"passed": 0, "failed": 1, "total": 1, "pass_rate": 0.0, "error_counts": {"load_failed": 1}},
            "results": [{"query": "q", "pass": False}],
        }
        mock_run_eval.side_effect = [baseline_none, baseline_original, candidate]

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

            output = run_iteration(
                workspace,
                trigger_eval_path,
                iteration_number=1,
                num_workers=1,
                timeout=5,
                runs_per_query=1,
                trigger_threshold=0.5,
                model=None,
            )

            iteration_dir = Path(output["iteration_path"])
            self.assertTrue((iteration_dir / "candidate-skill" / "SKILL.md").exists())
            self.assertTrue((iteration_dir / "eval-results.json").exists())
            self.assertTrue((iteration_dir / "score-summary.json").exists())
            self.assertTrue((iteration_dir / "notes.md").exists())
            persisted = read_json(workspace / "improvement-session.json")
            self.assertEqual(persisted["status"], "optimizing")
            self.assertEqual(len(persisted["iterations"]), 1)
            self.assertEqual(persisted["iterations"][0]["iteration"], 1)


if __name__ == "__main__":
    unittest.main()
