#!/usr/bin/env python3
"""Prepare an improvement workspace for an existing skill."""

from __future__ import annotations

import argparse
import shutil
from pathlib import Path

from scripts.quick_validate import validate_skill
from scripts.utils import (
    load_or_init_improvement_session,
    session_file_path,
    update_session_status,
    write_json,
)

TRIGGER_EVALS_FILENAME = "trigger-evals.json"
BEHAVIOR_EVALS_FILENAME = "behavior-evals.json"


def ensure_placeholder_json(path: Path) -> None:
    """Create an empty JSON array file when the target does not exist."""
    if not path.exists():
        path.write_text("[]\n", encoding="utf-8")


def prepare_improvement_workspace(target: str | Path, workspace_path: Path | None = None) -> dict:
    """Create workspace directories, snapshot the target skill, and persist the session."""
    session = load_or_init_improvement_session(target, workspace_path=workspace_path)
    target_skill_path = Path(session["target_skill_path"])
    effective_workspace = Path(session["workspace_path"])

    valid, message = validate_skill(target_skill_path, require_iteration_ready=True)
    if not valid:
        raise ValueError(message)

    snapshot_path = effective_workspace / "target-skill-snapshot"
    evals_dir = effective_workspace / "evals"
    iterations_dir = effective_workspace / "iterations"
    best_skill_dir = effective_workspace / "best-skill"

    effective_workspace.mkdir(parents=True, exist_ok=True)
    evals_dir.mkdir(parents=True, exist_ok=True)
    iterations_dir.mkdir(parents=True, exist_ok=True)
    best_skill_dir.mkdir(parents=True, exist_ok=True)

    if snapshot_path.exists():
        shutil.rmtree(snapshot_path)
    shutil.copytree(target_skill_path, snapshot_path)

    ensure_placeholder_json(evals_dir / TRIGGER_EVALS_FILENAME)
    ensure_placeholder_json(evals_dir / BEHAVIOR_EVALS_FILENAME)

    session["snapshot_path"] = str(snapshot_path.resolve())
    session["baseline_result_path"] = str((effective_workspace / "baseline-result.json").resolve())
    update_session_status(session, "evaluating")
    write_json(session_file_path(effective_workspace), session)
    return session


def main() -> None:
    parser = argparse.ArgumentParser(description="Prepare an improvement workspace for a skill")
    parser.add_argument("target_skill", help="Skill path or skill id")
    parser.add_argument("--workspace", default=None, help="Optional workspace output directory")
    args = parser.parse_args()

    workspace = Path(args.workspace).resolve() if args.workspace else None
    session = prepare_improvement_workspace(args.target_skill, workspace_path=workspace)
    print(session_file_path(Path(session["workspace_path"])))


if __name__ == "__main__":
    main()
