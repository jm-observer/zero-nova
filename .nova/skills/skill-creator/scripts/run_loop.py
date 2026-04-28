#!/usr/bin/env python3
"""Run the full skill-improvement loop with promotion, rollback, and convergence."""

from __future__ import annotations

import argparse
import shutil
from pathlib import Path
from typing import Any

from scripts.apply_candidate import apply_candidate_plan
from scripts.generate_report import write_workspace_reports
from scripts.improve_skill import improve_skill
from scripts.run_eval import RUN_MODE_CANDIDATE, RUN_MODE_BASELINE_ORIGINAL
from scripts.run_iteration import ITERATION_NAME_TEMPLATE, run_iteration
from scripts.score_iteration import (
    BEHAVIOR_PASS_THRESHOLD,
    TRIGGER_PASS_THRESHOLD,
    score_iteration_result,
)
from scripts.utils import read_json, session_file_path, update_session_status, write_json

MAX_ITERATIONS = 5
MIN_SCORE_DELTA = 0.01
MAX_CONSECUTIVE_NO_IMPROVEMENT = 2


def ensure_best_skill(session: dict[str, Any], workspace_path: Path) -> Path:
    """Initialize the best-skill directory from snapshot when needed."""
    best_skill_path = workspace_path / "best-skill"
    snapshot_path = Path(session["snapshot_path"])
    if not best_skill_path.exists() or not any(best_skill_path.iterdir()):
        if best_skill_path.exists():
            shutil.rmtree(best_skill_path)
        shutil.copytree(snapshot_path, best_skill_path)
    return best_skill_path


def has_material_improvement(iteration_score: dict[str, Any], best_score: dict[str, Any] | None) -> bool:
    """Decide whether a promoted candidate clears the explicit delta threshold."""
    if best_score is None:
        return True
    overall_delta = float(iteration_score["comparisons"]["candidate_vs_best"]["overall_delta"])
    tie_break_only = bool(iteration_score["decision_hints"]["promoted"]) and abs(overall_delta) < MIN_SCORE_DELTA
    return overall_delta >= MIN_SCORE_DELTA or tie_break_only


def build_iteration_note(
    iteration_number: int,
    plan: dict[str, Any],
    diff_summary: dict[str, Any],
    iteration_score: dict[str, Any],
    promoted: bool,
) -> str:
    """Create a readable per-iteration explanation note."""
    focus = ", ".join(plan.get("focus_areas", [])) or "refresh_description"
    lines = [
        f"# Iteration {iteration_number}",
        "",
        f"- focus: {focus}",
        f"- promoted: {promoted}",
        f"- applied changes: {len(diff_summary.get('applied_changes', []))}",
        "",
        "## Why This Candidate",
        "",
    ]
    for change in diff_summary.get("applied_changes", []):
        lines.append(f"- {change['type']}: {change['reason']}")

    lines.extend(["", "## Failure Clusters", ""])
    candidate_clusters = iteration_score["failure_clusters"][RUN_MODE_CANDIDATE]
    if not candidate_clusters:
        lines.append("- none")
    for cluster_name, cluster in candidate_clusters.items():
        preview = ", ".join(cluster.get("queries", [])[:3])
        lines.append(f"- {cluster_name}: {cluster['count']} ({preview})")

    comparison = iteration_score["comparisons"]["candidate_vs_best"]
    lines.extend(
        [
            "",
            "## Comparison",
            "",
            f"- overall delta vs best: {comparison['overall_delta']:.4f}",
            f"- false trigger delta vs best: {comparison['false_trigger_delta']}",
        ]
    )
    return "\n".join(lines).rstrip() + "\n"


def update_best_skill(best_skill_path: Path, candidate_skill_path: Path) -> None:
    """Promote candidate skill to the durable best-skill directory."""
    if best_skill_path.exists():
        shutil.rmtree(best_skill_path)
    shutil.copytree(candidate_skill_path, best_skill_path)


def upsert_session_iteration(session: dict[str, Any], iteration_record: dict[str, Any]) -> dict[str, Any]:
    """Insert or replace one iteration record in the in-memory session."""
    iterations = [item for item in session.get("iterations", []) if item.get("iteration") != iteration_record["iteration"]]
    iterations.append(iteration_record)
    session["iterations"] = iterations
    return iteration_record


def is_iteration_complete(iteration_record: dict[str, Any]) -> bool:
    """Return whether an iteration has all durable artifacts required for resume."""
    required_paths = [
        iteration_record.get("candidate_skill_path"),
        iteration_record.get("eval_results_path"),
        iteration_record.get("score_summary_path"),
        iteration_record.get("notes_path"),
        iteration_record.get("candidate_plan_path"),
        iteration_record.get("candidate_diff_summary_path"),
    ]
    return all(path and Path(path).exists() for path in required_paths)


def determine_resume_point(session: dict[str, Any]) -> tuple[int, str]:
    """Return the next iteration number and the reason for that choice."""
    iterations = sorted(session.get("iterations", []), key=lambda item: item.get("iteration", 0))
    if not iterations:
        return 1, "fresh_start"

    last_iteration = iterations[-1]
    if is_iteration_complete(last_iteration):
        return int(last_iteration["iteration"]) + 1, "next_iteration"
    return int(last_iteration["iteration"]), "rerun_iteration"


def enter_optimizing(session: dict[str, Any]) -> None:
    """Move a resumable session into optimizing state."""
    status = session["status"]
    if status in {"initialized", "paused"}:
        update_session_status(session, "evaluating")
        status = session["status"]
    if status == "evaluating":
        update_session_status(session, "optimizing")
        return
    if status not in {"optimizing", "completed"}:
        raise ValueError(f"Session cannot enter optimizing from status: {status}")


def run_loop(
    workspace_path: Path,
    trigger_eval_path: Path,
    *,
    num_workers: int,
    timeout: int,
    max_iterations: int,
    runs_per_query: int,
    trigger_threshold: float,
    model: str | None,
) -> dict[str, Any]:
    """Run session-driven candidate generation, scoring, rollback, and convergence."""
    session_path = session_file_path(workspace_path)
    session = read_json(session_path)
    enter_optimizing(session)
    write_json(session_path, session)

    best_skill_path = ensure_best_skill(session, workspace_path)
    best_score = session.get("best_score")
    best_iteration = session.get("best_iteration")
    no_improvement_count = 0
    exit_reason = "max_iterations"
    start_iteration, resume_reason = determine_resume_point(session)

    for iteration_number in range(start_iteration, max_iterations + 1):
        iteration_dir = workspace_path / "iterations" / ITERATION_NAME_TEMPLATE.format(number=iteration_number)
        candidate_skill_path = iteration_dir / "candidate-skill"
        iteration_dir.mkdir(parents=True, exist_ok=True)

        failed_results: list[dict[str, Any]] = []
        if session.get("iterations"):
            last_iteration = session["iterations"][-1]
            score_path = Path(last_iteration["score_summary_path"])
            if score_path.exists():
                previous_score = read_json(score_path)
                previous_eval_path = Path(last_iteration["eval_results_path"])
                previous_eval = read_json(previous_eval_path)
                failed_results = [
                    item for item in previous_eval[RUN_MODE_CANDIDATE]["results"] if not item.get("pass")
                ]
                best_score = best_score or previous_score.get("score")

        plan = improve_skill(
            best_skill_path=best_skill_path,
            candidate_skill_path=candidate_skill_path,
            failed_results=failed_results,
            score_summary={"trigger_threshold": trigger_threshold},
            history=session.get("iterations", []),
            model=model,
        )
        write_json(iteration_dir / "candidate-plan.json", plan)

        try:
            diff_summary = apply_candidate_plan(best_skill_path, candidate_skill_path, plan)
        except Exception as exc:
            update_session_status(session, "paused")
            session["last_error"] = f"apply_candidate_failed: {exc}"
            write_json(session_path, session)
            return {
                "status": "paused",
                "exit_reason": "apply_candidate_failed",
                "error": str(exc),
            }
        if not candidate_skill_path.exists():
            shutil.copytree(best_skill_path, candidate_skill_path)

        iteration_output = run_iteration(
            workspace_path,
            trigger_eval_path,
            iteration_number=iteration_number,
            num_workers=num_workers,
            timeout=timeout,
            runs_per_query=runs_per_query,
            trigger_threshold=trigger_threshold,
            model=model,
        )
        session = read_json(session_path)
        existing_iteration = next(
            (item for item in session.get("iterations", []) if item.get("iteration") == iteration_number),
            None,
        )
        if existing_iteration is None:
            existing_iteration = upsert_session_iteration(
                session,
                {
                    "iteration": iteration_number,
                    "iteration_path": str(iteration_dir.resolve()),
                    "candidate_skill_path": str(candidate_skill_path.resolve()),
                    "eval_results_path": str((iteration_dir / "eval-results.json").resolve()),
                    "score_summary_path": str((iteration_dir / "score-summary.json").resolve()),
                    "notes_path": str((iteration_dir / "notes.md").resolve()),
                    "candidate_pass_rate": iteration_output["results"][RUN_MODE_CANDIDATE]["summary"]["pass_rate"],
                },
            )
        iteration_score = score_iteration_result(
            iteration_output["results"],
            best_score=best_score,
            trigger_pass_threshold=TRIGGER_PASS_THRESHOLD,
            behavior_pass_threshold=BEHAVIOR_PASS_THRESHOLD,
        )

        write_json(iteration_dir / "eval-results.json", iteration_output["results"])
        write_json(iteration_dir / "candidate-diff-summary.json", diff_summary)
        score_path = iteration_dir / "score-summary.json"
        notes_path = iteration_dir / "notes.md"
        write_json(score_path, iteration_score)

        promoted = bool(iteration_score["decision_hints"]["promoted"]) and has_material_improvement(iteration_score, best_score)
        notes_path.write_text(
            build_iteration_note(iteration_number, plan, diff_summary, iteration_score, promoted),
            encoding="utf-8",
        )

        session_iteration = next(item for item in session["iterations"] if item["iteration"] == iteration_number)
        session_iteration["candidate_plan_path"] = str((iteration_dir / "candidate-plan.json").resolve())
        session_iteration["candidate_diff_summary_path"] = str((iteration_dir / "candidate-diff-summary.json").resolve())
        session_iteration["score_summary_path"] = str(score_path.resolve())
        session_iteration["notes_path"] = str(notes_path.resolve())
        session_iteration["promoted"] = promoted
        session_iteration["report_path"] = str((iteration_dir / "report.html").resolve())

        if promoted:
            update_best_skill(best_skill_path, candidate_skill_path)
            best_score = iteration_score["score"] | {
                "false_trigger_count": iteration_score["decision_hints"]["false_trigger_count"]
            }
            best_iteration = iteration_number
            session["best_iteration"] = best_iteration
            session["best_score"] = best_score
            no_improvement_count = 0
            if best_score["trigger_pass_rate"] >= TRIGGER_PASS_THRESHOLD:
                exit_reason = "threshold_reached"
                break
        else:
            no_improvement_count += 1
            if iteration_score["decision_hints"]["severe_regression"]:
                exit_reason = "severe_regression"
                break
            if no_improvement_count >= MAX_CONSECUTIVE_NO_IMPROVEMENT:
                exit_reason = "converged"
                break

        write_json(session_path, session)
        write_workspace_reports(workspace_path)

    if exit_reason in {"threshold_reached", "converged", "max_iterations", "severe_regression"}:
        update_session_status(session, "completed")
    session["best_iteration"] = best_iteration
    session["best_score"] = best_score
    session["exit_reason"] = exit_reason
    session["resume_reason"] = resume_reason
    write_json(session_path, session)
    write_workspace_reports(workspace_path)
    return {
        "status": session["status"],
        "exit_reason": exit_reason,
        "resume_reason": resume_reason,
        "best_iteration": best_iteration,
        "best_score": best_score,
        "iterations_run": len(session.get("iterations", [])),
        "workspace_path": str(workspace_path.resolve()),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the full skill improvement loop")
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--trigger-evals", default=None)
    parser.add_argument("--num-workers", type=int, default=10)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--max-iterations", type=int, default=MAX_ITERATIONS)
    parser.add_argument("--runs-per-query", type=int, default=3)
    parser.add_argument("--trigger-threshold", type=float, default=0.5)
    parser.add_argument("--model", default=None)
    args = parser.parse_args()

    workspace = Path(args.workspace).resolve()
    trigger_eval_path = Path(args.trigger_evals).resolve() if args.trigger_evals else workspace / "evals" / "trigger-evals.json"
    output = run_loop(
        workspace,
        trigger_eval_path,
        num_workers=args.num_workers,
        timeout=args.timeout,
        max_iterations=args.max_iterations,
        runs_per_query=args.runs_per_query,
        trigger_threshold=args.trigger_threshold,
        model=args.model,
    )
    write_json(workspace / "latest-loop-result.json", output)


if __name__ == "__main__":
    main()
