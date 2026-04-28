#!/usr/bin/env python3
"""Resume a paused skill-improvement workspace."""

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any

from scripts.run_loop import MAX_ITERATIONS, determine_resume_point, run_loop
from scripts.session_schema import validate_session_payload
from scripts.utils import read_json, session_file_path

REQUIRED_WORKSPACE_PATHS = (
    "target_skill_path",
    "workspace_path",
    "snapshot_path",
)


def load_session(workspace_path: Path) -> dict[str, Any]:
    """Load and validate the persisted improvement session."""
    session = read_json(session_file_path(workspace_path))
    validate_session_payload(session)
    return session


def validate_workspace(session: dict[str, Any], workspace_path: Path) -> None:
    """Ensure the resume target still has the minimum required files."""
    if Path(session["workspace_path"]).resolve() != workspace_path.resolve():
        raise ValueError("Workspace path does not match session metadata")

    for key in REQUIRED_WORKSPACE_PATHS:
        path = Path(session[key])
        if not path.exists():
            raise ValueError(f"Workspace is incomplete, missing required path: {path}")

    trigger_eval_path = workspace_path / "evals" / "trigger-evals.json"
    if not trigger_eval_path.exists():
        raise ValueError(f"Workspace is incomplete, missing trigger evals: {trigger_eval_path}")


def resume_improvement(
    workspace_path: Path,
    *,
    num_workers: int,
    timeout: int,
    max_iterations: int,
    runs_per_query: int,
    trigger_threshold: float,
    model: str | None,
    dry_run: bool,
) -> dict[str, Any]:
    """Resume a paused session or report what would happen next."""
    session = load_session(workspace_path)
    validate_workspace(session, workspace_path)
    resume_iteration, resume_reason = determine_resume_point(session)
    trigger_eval_path = workspace_path / "evals" / "trigger-evals.json"
    payload = {
        "workspace_path": str(workspace_path.resolve()),
        "status": session["status"],
        "resume_iteration": resume_iteration,
        "resume_reason": resume_reason,
        "best_iteration": session.get("best_iteration"),
    }
    if dry_run or session["status"] == "completed":
        payload["action"] = "noop" if session["status"] == "completed" else "resume"
        return payload

    output = run_loop(
        workspace_path,
        trigger_eval_path,
        num_workers=num_workers,
        timeout=timeout,
        max_iterations=max_iterations,
        runs_per_query=runs_per_query,
        trigger_threshold=trigger_threshold,
        model=model,
    )
    return payload | {"action": "resumed", "result": output}


def main() -> None:
    parser = argparse.ArgumentParser(description="Resume a paused skill-improvement session")
    parser.add_argument("workspace", help="Path to the improvement workspace")
    parser.add_argument("--num-workers", type=int, default=10)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--max-iterations", type=int, default=MAX_ITERATIONS)
    parser.add_argument("--runs-per-query", type=int, default=3)
    parser.add_argument("--trigger-threshold", type=float, default=0.5)
    parser.add_argument("--model", default=None)
    parser.add_argument("--dry-run", action="store_true", help="Only inspect the session without resuming it")
    args = parser.parse_args()

    output = resume_improvement(
        Path(args.workspace).resolve(),
        num_workers=args.num_workers,
        timeout=args.timeout,
        max_iterations=args.max_iterations,
        runs_per_query=args.runs_per_query,
        trigger_threshold=args.trigger_threshold,
        model=args.model,
        dry_run=args.dry_run,
    )
    import json

    print(json.dumps(output, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
