import json
import tempfile
import unittest
from pathlib import Path

from scripts.session_schema import validate_session_payload
from scripts.utils import (
    load_or_init_improvement_session,
    parse_skill_md,
    resolve_target_skill_path,
    stable_workspace_name,
    update_session_status,
)


class SessionUtilsTests(unittest.TestCase):
    def test_parse_skill_md_requires_frontmatter_fields(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            skill_dir = Path(temp_dir) / "broken-skill"
            skill_dir.mkdir()
            (skill_dir / "SKILL.md").write_text("---\nname: demo\n---\n", encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "description"):
                parse_skill_md(skill_dir)

    def test_resolve_target_skill_path_supports_skill_id(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            skill_dir.mkdir()
            (skill_dir / "SKILL.md").write_text(
                "---\nname: Demo Skill\ndescription: Demo description\n---\n",
                encoding="utf-8",
            )

            resolved = resolve_target_skill_path("demo-skill", search_roots=[root])
            self.assertEqual(resolved, skill_dir.resolve())

    def test_load_or_init_improvement_session_restores_existing_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            skill_dir.mkdir()
            (skill_dir / "SKILL.md").write_text(
                "---\nname: Demo Skill\ndescription: Demo description\n---\n",
                encoding="utf-8",
            )

            session = load_or_init_improvement_session(skill_dir)
            session["best_iteration"] = 2
            session["best_score"] = 0.95
            session_path = Path(session["workspace_path"]) / "improvement-session.json"
            session_path.write_text(json.dumps(session), encoding="utf-8")

            restored = load_or_init_improvement_session(skill_dir)
            self.assertEqual(restored["best_iteration"], 2)
            self.assertEqual(restored["best_score"], 0.95)

    def test_update_session_status_validates_transition(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            skill_dir = root / "demo-skill"
            skill_dir.mkdir()
            (skill_dir / "SKILL.md").write_text(
                "---\nname: Demo Skill\ndescription: Demo description\n---\n",
                encoding="utf-8",
            )

            session = load_or_init_improvement_session(skill_dir)
            validate_session_payload(session)
            update_session_status(session, "evaluating")
            self.assertEqual(session["status"], "evaluating")

            with self.assertRaisesRegex(ValueError, "Invalid session transition"):
                update_session_status(session, "initialized")

    def test_stable_workspace_name_slugifies_skill_name(self) -> None:
        self.assertEqual(stable_workspace_name("Demo Skill"), "demo-skill-improvement-workspace")


if __name__ == "__main__":
    unittest.main()
