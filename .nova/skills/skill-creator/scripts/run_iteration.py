#!/usr/bin/env python3
"""Run one improvement iteration against snapshot and candidate skill copies."""

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any

from scripts.run_eval import (
    RUN_MODE_BASELINE_NONE,
    RUN_MODE_BASELINE_ORIGINAL,
    RUN_MODE_CANDIDATE,
    find_project_root,
    load_eval_set,
    run_eval,
)
from scripts.utils import read_json, session_file_path, update_session_status, write_json

ITERATION_NAME_TEMPLATE = "iteration-{number:03d}"


def next_iteration_number(iterations_dir: Path) -> int:
    """Return the next numeric iteration index."""
    existing_numbers: list[int] = []
    for child in iterations_dir.iterdir() if iterations_dir.exists() else []:
        if not child.is_dir() or not child.name.startswith("iteration-"):
            continue
        suffix = child.name.split("iteration-", 1)[1]
        if suffix.isdigit():
            existing_numbers.append(int(suffix))
    if not existing_numbers:
        return 1
    return max(existing_numbers) + 1


def compute_score_summary(results_by_mode: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Compute a compact comparison summary across modes."""
    summary: dict[str, Any] = {"modes": {}, "comparisons": {}}
    for mode, result in results_by_mode.items():
        mode_summary = result["summary"]
        summary["modes"][mode] = {
            "passed": mode_summary["passed"],
            "failed": mode_summary["failed"],
            "total": mode_summary["total"],
            "pass_rate": mode_summary["pass_rate"],
            "error_counts": mode_summary["error_counts"],
        }

    baseline_original = summary["modes"].get(RUN_MODE_BASELINE_ORIGINAL)
    candidate = summary["modes"].get(RUN_MODE_CANDIDATE)
    baseline_none = summary["modes"].get(RUN_MODE_BASELINE_NONE)

    if baseline_original and candidate:
        summary["comparisons"]["candidate_vs_baseline_original"] = {
            "pass_rate_delta": candidate["pass_rate"] - baseline_original["pass_rate"],
            "passed_delta": candidate["passed"] - baseline_original["passed"],
        }
    if baseline_none and baseline_original:
        summary["comparisons"]["baseline_original_vs_none"] = {
            "pass_rate_delta": baseline_original["pass_rate"] - baseline_none["pass_rate"],
            "passed_delta": baseline_original["passed"] - baseline_none["passed"],
        }
    return summary


def build_iteration_notes(results_by_mode: dict[str, dict[str, Any]]) -> str:
    """Render a markdown note for the current iteration."""
    lines = ["# Iteration Notes", ""]
    for mode, result in results_by_mode.items():
        summary = result["summary"]
        lines.append(f"## {mode}")
        lines.append(f"- pass: {summary['passed']}/{summary['total']}")
        lines.append(f"- errors: {summary['error_counts']}")
        failures = [item for item in result["results"] if not item["pass"]]
        if failures:
            lines.append("- failed queries:")
            for failure in failures:
                lines.append(f"  - {failure['query']}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def run_iteration(
    workspace_path: Path,
    trigger_eval_path: Path,
    *,
    iteration_number: int | None,
    num_workers: int,
    timeout: int,
    runs_per_query: int,
    trigger_threshold: float,
    model: str | None,
) -> dict[str, Any]:
    """Prepare candidate workspace and run baseline/candidate evals."""
    session = read_json(session_file_path(workspace_path))
    snapshot_path = Path(session["snapshot_path"])
    iterations_dir = workspace_path / "iterations"
    iterations_dir.mkdir(parents=True, exist_ok=True)

    current_iteration = iteration_number or next_iteration_number(iterations_dir)
    iteration_dir = iterations_dir / ITERATION_NAME_TEMPLATE.format(number=current_iteration)
    candidate_skill_dir = iteration_dir / "candidate-skill"
    if not candidate_skill_dir.exists():
        import shutil

        shutil.copytree(snapshot_path, candidate_skill_dir)

    eval_set = load_eval_set(trigger_eval_path)
    project_root = find_project_root()
    results_by_mode = {
        RUN_MODE_BASELINE_NONE: run_eval(
            eval_set,
            skill_path=None,
            num_workers=num_workers,
            timeout=timeout,
            project_root=project_root,
            runs_per_query=runs_per_query,
            trigger_threshold=trigger_threshold,
            model=model,
            mode=RUN_MODE_BASELINE_NONE,
        ),
        RUN_MODE_BASELINE_ORIGINAL: run_eval(
            eval_set,
            skill_path=snapshot_path,
            num_workers=num_workers,
            timeout=timeout,
            project_root=project_root,
            runs_per_query=runs_per_query,
            trigger_threshold=trigger_threshold,
            model=model,
            mode=RUN_MODE_BASELINE_ORIGINAL,
        ),
        RUN_MODE_CANDIDATE: run_eval(
            eval_set,
            skill_path=candidate_skill_dir,
            num_workers=num_workers,
            timeout=timeout,
            project_root=project_root,
            runs_per_query=runs_per_query,
            trigger_threshold=trigger_threshold,
            model=model,
            mode=RUN_MODE_CANDIDATE,
        ),
    }

    score_summary = compute_score_summary(results_by_mode)
    eval_results_path = iteration_dir / "eval-results.json"
    score_summary_path = iteration_dir / "score-summary.json"
    notes_path = iteration_dir / "notes.md"
    write_json(eval_results_path, results_by_mode)
    write_json(score_summary_path, score_summary)
    notes_path.write_text(build_iteration_notes(results_by_mode), encoding="utf-8")

    session_iteration = {
        "iteration": current_iteration,
        "iteration_path": str(iteration_dir.resolve()),
        "candidate_skill_path": str(candidate_skill_dir.resolve()),
        "eval_results_path": str(eval_results_path.resolve()),
        "score_summary_path": str(score_summary_path.resolve()),
        "notes_path": str(notes_path.resolve()),
        "candidate_pass_rate": score_summary["modes"][RUN_MODE_CANDIDATE]["pass_rate"],
    }
    session["iterations"] = [item for item in session["iterations"] if item.get("iteration") != current_iteration]
    session["iterations"].append(session_iteration)
    update_session_status(session, "optimizing")
    write_json(session_file_path(workspace_path), session)

    return {
        "iteration": current_iteration,
        "iteration_path": str(iteration_dir.resolve()),
        "results": results_by_mode,
        "summary": score_summary,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Run one skill-improvement iteration")
    parser.add_argument("--workspace", required=True, help="Prepared improvement workspace path")
    parser.add_argument("--trigger-evals", default=None, help="Trigger eval set JSON path")
    parser.add_argument("--iteration", type=int, default=None, help="Explicit iteration number")
    parser.add_argument("--num-workers", type=int, default=10)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--runs-per-query", type=int, default=3)
    parser.add_argument("--trigger-threshold", type=float, default=0.5)
    parser.add_argument("--model", default=None)
    args = parser.parse_args()

    workspace = Path(args.workspace).resolve()
    trigger_eval_path = Path(args.trigger_evals).resolve() if args.trigger_evals else workspace / "evals" / "trigger-evals.json"
    output = run_iteration(
        workspace,
        trigger_eval_path,
        iteration_number=args.iteration,
        num_workers=args.num_workers,
        timeout=args.timeout,
        runs_per_query=args.runs_per_query,
        trigger_threshold=args.trigger_threshold,
        model=args.model,
    )
    write_json(workspace / "latest-iteration.json", output)


if __name__ == "__main__":
    main()
