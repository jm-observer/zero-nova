#!/usr/bin/env python3
"""Export or write back the best skill from an improvement workspace."""

from __future__ import annotations

import argparse
import difflib
import shutil
from pathlib import Path
from typing import Any

from scripts.generate_report import write_workspace_reports
from scripts.utils import parse_skill_md, read_json, session_file_path, write_json

FINALIZE_DIRNAME = "finalization"
BACKUP_DIRNAME = "original-backup"


def load_session(workspace_path: Path) -> dict[str, Any]:
    """Load the persisted session for finalization."""
    return read_json(session_file_path(workspace_path))


def best_skill_path(workspace_path: Path) -> Path:
    """Return the workspace best-skill directory."""
    return workspace_path / "best-skill"


def copy_tree(src: Path, dst: Path) -> None:
    """Replace destination with a full copy of source."""
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst)


def build_skill_diff(original_skill: Path, candidate_skill: Path) -> str:
    """Create a unified diff for SKILL.md, which is the only managed file today."""
    original_lines = (original_skill / "SKILL.md").read_text(encoding="utf-8").splitlines(keepends=True)
    candidate_lines = (candidate_skill / "SKILL.md").read_text(encoding="utf-8").splitlines(keepends=True)
    return "".join(
        difflib.unified_diff(
            original_lines,
            candidate_lines,
            fromfile=str((original_skill / "SKILL.md").resolve()),
            tofile=str((candidate_skill / "SKILL.md").resolve()),
        )
    )


def finalize_improvement(
    workspace_path: Path,
    *,
    export_dir: Path | None,
    writeback: bool,
) -> dict[str, Any]:
    """Generate finalization artifacts and optionally write the best skill back."""
    session = load_session(workspace_path)
    source_skill = Path(session["target_skill_path"])
    candidate_skill = best_skill_path(workspace_path)
    if not source_skill.exists():
        raise ValueError(f"Original skill path does not exist: {source_skill}")
    if not candidate_skill.exists() or not (candidate_skill / "SKILL.md").exists():
        raise ValueError(f"Best skill directory does not exist: {candidate_skill}")

    parse_skill_md(source_skill)
    parse_skill_md(candidate_skill)
    report_paths = write_workspace_reports(workspace_path)

    finalize_dir = workspace_path / FINALIZE_DIRNAME
    finalize_dir.mkdir(parents=True, exist_ok=True)
    diff_path = finalize_dir / "best-skill.diff"
    summary_path = finalize_dir / "finalization-summary.json"
    diff_text = build_skill_diff(source_skill, candidate_skill)
    diff_path.write_text(diff_text, encoding="utf-8")

    exported_path = None
    if export_dir is not None:
        exported_path = export_dir.resolve()
        copy_tree(candidate_skill, exported_path)

    backup_path = None
    if writeback:
        backup_path = (workspace_path / BACKUP_DIRNAME).resolve()
        copy_tree(source_skill, backup_path)
        copy_tree(candidate_skill, source_skill)

    summary = {
        "workspace_path": str(workspace_path.resolve()),
        "target_skill_path": str(source_skill.resolve()),
        "best_skill_path": str(candidate_skill.resolve()),
        "best_iteration": session.get("best_iteration"),
        "best_score": session.get("best_score"),
        "diff_path": str(diff_path.resolve()),
        "final_report_path": report_paths["final"],
        "exported_path": str(exported_path) if exported_path else None,
        "backup_path": str(backup_path) if backup_path else None,
        "writeback": writeback,
    }
    write_json(summary_path, summary)
    return summary | {"summary_path": str(summary_path.resolve())}


def main() -> None:
    parser = argparse.ArgumentParser(description="Finalize a skill-improvement workspace")
    parser.add_argument("workspace", help="Path to the improvement workspace")
    parser.add_argument("--export-dir", default=None, help="Optional directory to export best-skill into")
    parser.add_argument("--writeback", action="store_true", help="Overwrite the original skill after creating a backup")
    args = parser.parse_args()

    export_dir = Path(args.export_dir).resolve() if args.export_dir else None
    output = finalize_improvement(Path(args.workspace).resolve(), export_dir=export_dir, writeback=args.writeback)
    import json

    print(json.dumps(output, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
