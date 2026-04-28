#!/usr/bin/env python3
"""Score one iteration with explainable failure clusters and promotion hints."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from scripts.run_eval import RUN_MODE_BASELINE_ORIGINAL, RUN_MODE_CANDIDATE
from scripts.utils import read_json, write_json

TRIGGER_PASS_THRESHOLD = 0.95
BEHAVIOR_PASS_THRESHOLD = 0.95
SEVERE_REGRESSION_DELTA = -0.25


def classify_failure(result: dict[str, Any], trigger_threshold: float) -> str:
    """Assign a stable failure category for trigger eval output."""
    if any(run.get("error_type") for run in result.get("runs_detail", [])):
        return "runtime_error"
    should_trigger = bool(result.get("should_trigger"))
    trigger_rate = float(result.get("trigger_rate", 0.0))
    if should_trigger and trigger_rate < trigger_threshold:
        return "missed_trigger"
    if not should_trigger and trigger_rate >= trigger_threshold:
        return "false_trigger"
    return "unstable_trigger"


def cluster_failures(results: list[dict[str, Any]], trigger_threshold: float) -> dict[str, dict[str, Any]]:
    """Group failures for one mode into explainable buckets."""
    clusters: dict[str, dict[str, Any]] = {}
    for result in results:
        if result.get("pass"):
            continue
        name = classify_failure(result, trigger_threshold)
        cluster = clusters.setdefault(name, {"count": 0, "queries": []})
        cluster["count"] += 1
        cluster["queries"].append(result.get("query", ""))
    return clusters


def false_trigger_count(clusters: dict[str, dict[str, Any]]) -> int:
    """Return false-trigger cluster size for tie breaking."""
    return int(clusters.get("false_trigger", {}).get("count", 0))


def summarize_mode(result: dict[str, Any]) -> dict[str, Any]:
    """Normalize a mode summary payload."""
    summary = result["summary"]
    return {
        "passed": summary["passed"],
        "failed": summary["failed"],
        "total": summary["total"],
        "pass_rate": summary["pass_rate"],
        "error_counts": summary.get("error_counts", {}),
    }


def score_iteration_result(
    results_by_mode: dict[str, dict[str, Any]],
    best_score: dict[str, Any] | None = None,
    trigger_pass_threshold: float = TRIGGER_PASS_THRESHOLD,
    behavior_pass_threshold: float = BEHAVIOR_PASS_THRESHOLD,
) -> dict[str, Any]:
    """Produce explainable scoring and promotion hints for one iteration."""
    candidate_result = results_by_mode[RUN_MODE_CANDIDATE]
    baseline_result = results_by_mode[RUN_MODE_BASELINE_ORIGINAL]
    trigger_threshold = float(candidate_result["summary"].get("trigger_threshold", 0.5))

    candidate_clusters = cluster_failures(candidate_result["results"], trigger_threshold)
    baseline_clusters = cluster_failures(baseline_result["results"], trigger_threshold)
    candidate_summary = summarize_mode(candidate_result)
    baseline_summary = summarize_mode(baseline_result)
    overall_score = float(candidate_summary["pass_rate"])

    score = {
        "overall": overall_score,
        "trigger_pass_rate": candidate_summary["pass_rate"],
        "behavior_pass_rate": None,
        "meets_trigger_threshold": candidate_summary["pass_rate"] >= trigger_pass_threshold,
        "meets_behavior_threshold": behavior_pass_threshold <= 0.0,
        "trigger_threshold": trigger_threshold,
    }

    best_overall = float(best_score["overall"]) if best_score else float(baseline_summary["pass_rate"])
    best_false_trigger = int(best_score.get("false_trigger_count", 0)) if best_score else false_trigger_count(baseline_clusters)
    overall_delta = overall_score - best_overall
    candidate_false_trigger = false_trigger_count(candidate_clusters)
    promoted = overall_delta > 0 or (overall_delta == 0 and candidate_false_trigger < best_false_trigger)

    return {
        "modes": {
            RUN_MODE_BASELINE_ORIGINAL: baseline_summary,
            RUN_MODE_CANDIDATE: candidate_summary,
        },
        "comparisons": {
            "candidate_vs_baseline_original": {
                "pass_rate_delta": candidate_summary["pass_rate"] - baseline_summary["pass_rate"],
                "passed_delta": candidate_summary["passed"] - baseline_summary["passed"],
            },
            "candidate_vs_best": {
                "overall_delta": overall_delta,
                "false_trigger_delta": candidate_false_trigger - best_false_trigger,
            },
        },
        "failure_clusters": {
            RUN_MODE_BASELINE_ORIGINAL: baseline_clusters,
            RUN_MODE_CANDIDATE: candidate_clusters,
        },
        "score": score,
        "decision_hints": {
            "promoted": promoted,
            "severe_regression": overall_delta <= SEVERE_REGRESSION_DELTA,
            "false_trigger_count": candidate_false_trigger,
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Score an iteration with explainable clusters")
    parser.add_argument("--eval-results", required=True)
    parser.add_argument("--best-score", default=None)
    parser.add_argument("--output", default=None)
    args = parser.parse_args()

    payload = score_iteration_result(
        read_json(Path(args.eval_results)),
        best_score=read_json(Path(args.best_score)) if args.best_score else None,
    )
    if args.output:
        write_json(Path(args.output), payload)
    else:
        print(json.dumps(payload, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
