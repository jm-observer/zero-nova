#!/usr/bin/env python3
"""Generate a constrained candidate improvement plan for a skill."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from scripts.improve_description import improve_description
from scripts.utils import parse_skill_md, read_json, write_json

MAX_CHANGE_TOPICS = 2
MANAGED_SECTION_TITLE = "## Improvement Focus"


def classify_failure(result: dict[str, Any], trigger_threshold: float) -> str:
    """Map a failed trigger eval result to a stable failure cluster."""
    has_runtime_error = any(run.get("error_type") for run in result.get("runs_detail", []))
    if has_runtime_error:
        return "runtime_error"

    should_trigger = bool(result.get("should_trigger"))
    trigger_rate = float(result.get("trigger_rate", 0.0))
    if should_trigger and trigger_rate < trigger_threshold:
        return "missed_trigger"
    if not should_trigger and trigger_rate >= trigger_threshold:
        return "false_trigger"
    return "unstable_trigger"


def build_failure_clusters(results: list[dict[str, Any]], trigger_threshold: float) -> dict[str, dict[str, Any]]:
    """Group failed eval samples by explainable cluster."""
    clusters: dict[str, dict[str, Any]] = {}
    for result in results:
        if result.get("pass"):
            continue
        cluster_name = classify_failure(result, trigger_threshold)
        cluster = clusters.setdefault(cluster_name, {"count": 0, "queries": []})
        cluster["count"] += 1
        cluster["queries"].append(result.get("query", ""))
    return clusters


def select_focus_areas(clusters: dict[str, dict[str, Any]]) -> list[str]:
    """Pick the most important improvement themes for this round."""
    ranked = sorted(clusters.items(), key=lambda item: (-item[1]["count"], item[0]))
    return [name for name, _ in ranked[:MAX_CHANGE_TOPICS]]


def build_guidance_lines(focus_areas: list[str], clusters: dict[str, dict[str, Any]]) -> list[str]:
    """Produce compact markdown guidance tied to failure clusters."""
    lines: list[str] = []
    for area in focus_areas:
        queries = ", ".join(clusters.get(area, {}).get("queries", [])[:3])
        if area == "false_trigger":
            lines.append(
                f"- Reduce false triggers: require stronger evidence before activation. Example noisy queries: {queries}."
            )
        elif area == "missed_trigger":
            lines.append(
                f"- Improve recall for legitimate requests: add explicit trigger language and clearer entry conditions. Example misses: {queries}."
            )
        elif area == "runtime_error":
            lines.append(
                f"- Strengthen robustness: clarify fallback steps and required resources before attempting execution. Example failures: {queries}."
            )
        else:
            lines.append(f"- Stabilize triggering behavior for borderline cases. Example queries: {queries}.")
    return lines


def build_description_reason(focus_areas: list[str]) -> str:
    """Explain why the description should change this round."""
    if not focus_areas:
        return "Refresh description wording to improve trigger precision."
    return f"Adjust frontmatter description to address: {', '.join(focus_areas)}."


def build_section_reason(focus_areas: list[str]) -> str:
    """Explain why a managed guidance section should change this round."""
    if not focus_areas:
        return "Add explicit guidance to make optimization choices explainable."
    return f"Record round-specific guidance for: {', '.join(focus_areas)}."


def improve_skill(
    best_skill_path: Path,
    candidate_skill_path: Path,
    failed_results: list[dict[str, Any]],
    score_summary: dict[str, Any],
    history: list[dict[str, Any]],
    model: str | None,
) -> dict[str, Any]:
    """Generate a structured candidate plan limited to 1-2 focused changes."""
    skill_name, current_description, skill_content = parse_skill_md(best_skill_path)
    trigger_threshold = float(score_summary.get("trigger_threshold", 0.5))
    clusters = build_failure_clusters(failed_results, trigger_threshold)
    focus_areas = select_focus_areas(clusters)
    guidance_lines = build_guidance_lines(focus_areas, clusters)

    candidate_description = current_description
    if failed_results:
        eval_results = {
            "description": current_description,
            "results": failed_results,
            "summary": {
                "passed": sum(1 for item in failed_results if item.get("pass")),
                "failed": sum(1 for item in failed_results if not item.get("pass")),
                "total": len(failed_results),
            },
        }
        candidate_description = improve_description(
            skill_name=skill_name,
            skill_content=skill_content,
            current_description=current_description,
            eval_results=eval_results,
            history=history,
            model=model,
        )

    changes = [
        {
            "type": "update_description",
            "path": "SKILL.md",
            "reason": build_description_reason(focus_areas),
            "new_value": candidate_description,
        }
    ]
    if guidance_lines:
        changes.append(
            {
                "type": "upsert_section",
                "path": "SKILL.md",
                "reason": build_section_reason(focus_areas),
                "section_title": MANAGED_SECTION_TITLE,
                "lines": guidance_lines,
            }
        )

    return {
        "best_skill_path": str(best_skill_path.resolve()),
        "candidate_skill_path": str(candidate_skill_path.resolve()),
        "focus_areas": focus_areas,
        "failure_clusters": clusters,
        "change_count": len(changes),
        "changes": changes,
        "history_size": len(history),
        "summary": " / ".join(focus_areas) if focus_areas else "refresh_description",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate a candidate improvement plan for a skill")
    parser.add_argument("--best-skill", required=True)
    parser.add_argument("--candidate-skill", required=True)
    parser.add_argument("--failed-results", required=True)
    parser.add_argument("--score-summary", required=True)
    parser.add_argument("--history", default=None)
    parser.add_argument("--model", default=None)
    parser.add_argument("--output", default=None)
    args = parser.parse_args()

    history = read_json(Path(args.history)) if args.history else []
    plan = improve_skill(
        best_skill_path=Path(args.best_skill),
        candidate_skill_path=Path(args.candidate_skill),
        failed_results=read_json(Path(args.failed_results)),
        score_summary=read_json(Path(args.score_summary)),
        history=history,
        model=args.model,
    )

    if args.output:
        write_json(Path(args.output), plan)
    else:
        print(json.dumps(plan, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
