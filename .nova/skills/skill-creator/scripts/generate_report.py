#!/usr/bin/env python3
"""Generate per-iteration and final HTML reports for a skill-improvement workspace."""

from __future__ import annotations

import argparse
import html
from pathlib import Path
from typing import Any

from scripts.run_eval import RUN_MODE_BASELINE_ORIGINAL, RUN_MODE_CANDIDATE
from scripts.utils import read_json

REPORT_FILENAME = "report.html"
FINAL_REPORT_FILENAME = "final-report.html"
FEEDBACK_FILENAME = "feedback.json"


def read_optional_json(path: Path) -> dict[str, Any] | None:
    """Read a JSON file when it exists and is valid enough for reporting."""
    if not path.exists():
        return None
    payload = read_json(path)
    if not isinstance(payload, dict):
        raise ValueError(f"Expected JSON object at {path}")
    return payload


def load_feedback_map(workspace_path: Path) -> dict[str, str]:
    """Load manual review feedback keyed by run id."""
    feedback_path = workspace_path / FEEDBACK_FILENAME
    payload = read_optional_json(feedback_path)
    if not payload:
        return {}

    reviews = payload.get("reviews", [])
    if not isinstance(reviews, list):
        return {}
    return {
        str(item.get("run_id", "")): str(item.get("feedback", "")).strip()
        for item in reviews
        if str(item.get("run_id", "")).strip() and str(item.get("feedback", "")).strip()
    }


def read_session(workspace_path: Path) -> dict[str, Any]:
    """Load the improvement session payload from workspace."""
    return read_json(workspace_path / "improvement-session.json")


def iteration_manual_reviews(feedback_map: dict[str, str], iteration_number: int) -> list[str]:
    """Return manual review notes that belong to one iteration."""
    prefix = f"run-{iteration_number}-"
    return [value for key, value in feedback_map.items() if key.startswith(prefix)]


def summarize_candidate_changes(diff_summary: dict[str, Any] | None) -> list[str]:
    """Summarize candidate diff items into short human-readable lines."""
    if not diff_summary:
        return []

    summaries: list[str] = []
    for change in diff_summary.get("applied_changes", []):
        change_type = str(change.get("type", "unknown"))
        reason = str(change.get("reason", "")).strip()
        summaries.append(f"{change_type}: {reason}" if reason else change_type)
    return summaries


def summarize_failures(score_summary: dict[str, Any] | None) -> list[str]:
    """Flatten failure cluster information for reporting."""
    if not score_summary:
        return []

    clusters = score_summary.get("failure_clusters", {}).get(RUN_MODE_CANDIDATE, {})
    summaries: list[str] = []
    for name, cluster in clusters.items():
        count = int(cluster.get("count", 0))
        preview = ", ".join(cluster.get("queries", [])[:3])
        summaries.append(f"{name} ({count}){' - ' + preview if preview else ''}")
    return summaries


def summarize_examples(eval_results: dict[str, Any] | None) -> list[str]:
    """Extract representative failed examples from candidate eval output."""
    if not eval_results:
        return []

    results = eval_results.get(RUN_MODE_CANDIDATE, {}).get("results", [])
    examples: list[str] = []
    for item in results:
        if item.get("pass"):
            continue
        query = str(item.get("query", ""))
        error_type = str(item.get("error_type") or "classification_mismatch")
        trigger_rate = item.get("trigger_rate")
        suffix = f" | trigger_rate={trigger_rate}" if trigger_rate is not None else ""
        examples.append(f"{query} | {error_type}{suffix}")
        if len(examples) >= 5:
            break
    return examples


def iteration_scorecard(iteration_record: dict[str, Any]) -> dict[str, Any]:
    """Load all optional artifacts for one iteration and normalize the view model."""
    score_summary = read_optional_json(Path(iteration_record["score_summary_path"]))
    eval_results = read_optional_json(Path(iteration_record["eval_results_path"]))
    diff_summary = read_optional_json(Path(iteration_record.get("candidate_diff_summary_path", "missing.json")))
    plan = read_optional_json(Path(iteration_record.get("candidate_plan_path", "missing.json")))
    baseline_rate = None
    candidate_rate = None
    delta = None
    if score_summary:
        baseline_rate = score_summary.get("modes", {}).get(RUN_MODE_BASELINE_ORIGINAL, {}).get("pass_rate")
        candidate_rate = score_summary.get("modes", {}).get(RUN_MODE_CANDIDATE, {}).get("pass_rate")
        delta = score_summary.get("comparisons", {}).get("candidate_vs_baseline_original", {}).get("pass_rate_delta")

    return {
        "iteration": iteration_record["iteration"],
        "promoted": bool(iteration_record.get("promoted")),
        "baseline_rate": baseline_rate,
        "candidate_rate": candidate_rate,
        "delta": delta,
        "changes": summarize_candidate_changes(diff_summary),
        "failures": summarize_failures(score_summary),
        "examples": summarize_examples(eval_results),
        "focus_areas": list((plan or {}).get("focus_areas", [])),
    }


def format_rate(value: Any) -> str:
    """Format a score-like numeric value consistently."""
    if value is None:
        return "n/a"
    return f"{float(value):.1%}"


def html_list(items: list[str], empty_text: str) -> str:
    """Render a bullet list fragment."""
    if not items:
        return f"<p class=\"muted\">{html.escape(empty_text)}</p>"
    rendered = "".join(f"<li>{html.escape(item)}</li>" for item in items)
    return f"<ul>{rendered}</ul>"


def page_shell(title: str, body: str) -> str:
    """Wrap report content into a compact shared HTML shell."""
    escaped_title = html.escape(title)
    return f"""<!DOCTYPE html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\">
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
  <title>{escaped_title}</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 0; background: #f7f7f5; color: #1c1c1a; }}
    main {{ max-width: 980px; margin: 0 auto; padding: 24px; }}
    h1, h2 {{ margin: 0 0 12px; }}
    .card {{ background: #fff; border: 1px solid #e5e3db; border-radius: 8px; padding: 16px 18px; margin-bottom: 16px; }}
    .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; }}
    .metric {{ background: #faf9f5; border-radius: 6px; padding: 12px; }}
    .metric-label {{ color: #6b6b67; font-size: 12px; text-transform: uppercase; letter-spacing: .04em; }}
    .metric-value {{ margin-top: 4px; font-size: 20px; font-weight: 600; }}
    .muted {{ color: #6b6b67; }}
    .tag {{ display: inline-block; padding: 2px 8px; border-radius: 999px; font-size: 12px; margin-right: 6px; background: #ece9df; }}
    .tag.promoted {{ background: #e6f1df; color: #46632d; }}
    .tag.not-promoted {{ background: #f8e3e3; color: #9f3434; }}
    table {{ width: 100%; border-collapse: collapse; }}
    th, td {{ border: 1px solid #e5e3db; padding: 10px; text-align: left; vertical-align: top; }}
    th {{ background: #141413; color: #fff; }}
    code {{ background: #f4f1e8; padding: 2px 6px; border-radius: 4px; }}
    ul {{ margin: 8px 0 0 18px; }}
  </style>
</head>
<body>
  <main>
    {body}
  </main>
</body>
</html>
"""


def build_iteration_report_html(
    session: dict[str, Any],
    iteration_record: dict[str, Any],
    feedback_map: dict[str, str],
) -> str:
    """Build the HTML for one iteration report."""
    scorecard = iteration_scorecard(iteration_record)
    manual_reviews = iteration_manual_reviews(feedback_map, int(iteration_record["iteration"]))
    tags = "<span class=\"tag promoted\">promoted</span>" if scorecard["promoted"] else "<span class=\"tag not-promoted\">not promoted</span>"
    body = f"""
    <div class=\"card\">
      <h1>{html.escape(session['target_skill_name'])} - Iteration {scorecard['iteration']}</h1>
      <p class=\"muted\">{tags} Session: <code>{html.escape(session['session_id'])}</code></p>
    </div>
    <div class=\"card\">
      <h2>Score Comparison</h2>
      <div class=\"grid\">
        <div class=\"metric\"><div class=\"metric-label\">Baseline</div><div class=\"metric-value\">{format_rate(scorecard['baseline_rate'])}</div></div>
        <div class=\"metric\"><div class=\"metric-label\">Candidate</div><div class=\"metric-value\">{format_rate(scorecard['candidate_rate'])}</div></div>
        <div class=\"metric\"><div class=\"metric-label\">Delta</div><div class=\"metric-value\">{format_rate(scorecard['delta'])}</div></div>
      </div>
    </div>
    <div class=\"card\"><h2>Candidate Focus</h2>{html_list(scorecard['focus_areas'], 'No explicit focus areas.')}</div>
    <div class=\"card\"><h2>Candidate Diff Summary</h2>{html_list(scorecard['changes'], 'No candidate diff summary.')}</div>
    <div class=\"card\"><h2>Failure Clusters</h2>{html_list(scorecard['failures'], 'No failed clusters.')}</div>
    <div class=\"card\"><h2>Representative Failed Outputs</h2>{html_list(scorecard['examples'], 'No failed outputs captured.')}</div>
    <div class=\"card\"><h2>Manual Review</h2>{html_list(manual_reviews, 'No manual review conclusion yet.')}</div>
    """
    return page_shell(f"{session['target_skill_name']} Iteration {scorecard['iteration']} Report", body)


def build_final_report_html(session: dict[str, Any], feedback_map: dict[str, str]) -> str:
    """Build the HTML for the workspace-level final report."""
    iterations = sorted(session.get("iterations", []), key=lambda item: item.get("iteration", 0))
    scorecards = [iteration_scorecard(record) for record in iterations]
    baseline_rate = scorecards[0]["baseline_rate"] if scorecards else None
    best_score = (session.get("best_score") or {}).get("overall")
    overall_delta = None
    if baseline_rate is not None and best_score is not None:
        overall_delta = float(best_score) - float(baseline_rate)
    unresolved = [failure for scorecard in scorecards for failure in scorecard["failures"] if not scorecard["promoted"]]
    recommendation = "建议写回" if session.get("best_iteration") is not None and overall_delta is not None and overall_delta >= 0 else "暂不建议写回"
    rows = []
    for scorecard in scorecards:
        promoted = "Yes" if scorecard["promoted"] else "No"
        rows.append(
            f"<tr><td>{scorecard['iteration']}</td><td>{promoted}</td><td>{format_rate(scorecard['baseline_rate'])}</td>"
            f"<td>{format_rate(scorecard['candidate_rate'])}</td><td>{format_rate(scorecard['delta'])}</td></tr>"
        )
    review_notes = list(feedback_map.values())
    body = f"""
    <div class=\"card\">
      <h1>{html.escape(session['target_skill_name'])} - Final Report</h1>
      <p class=\"muted\">Session: <code>{html.escape(session['session_id'])}</code></p>
    </div>
    <div class=\"card\">
      <h2>Summary</h2>
      <div class=\"grid\">
        <div class=\"metric\"><div class=\"metric-label\">Best Iteration</div><div class=\"metric-value\">{session.get('best_iteration', 'n/a')}</div></div>
        <div class=\"metric\"><div class=\"metric-label\">Baseline</div><div class=\"metric-value\">{format_rate(baseline_rate)}</div></div>
        <div class=\"metric\"><div class=\"metric-label\">Best Score</div><div class=\"metric-value\">{format_rate(best_score)}</div></div>
        <div class=\"metric\"><div class=\"metric-label\">Overall Delta</div><div class=\"metric-value\">{format_rate(overall_delta)}</div></div>
      </div>
      <p><strong>{html.escape(recommendation)}</strong></p>
    </div>
    <div class=\"card\">
      <h2>Iteration Timeline</h2>
      <table>
        <thead><tr><th>Iteration</th><th>Promoted</th><th>Baseline</th><th>Candidate</th><th>Delta</th></tr></thead>
        <tbody>{''.join(rows) or '<tr><td colspan="5">No iterations yet.</td></tr>'}</tbody>
      </table>
    </div>
    <div class=\"card\"><h2>Outstanding Issues</h2>{html_list(unresolved, 'No outstanding issue captured.')}</div>
    <div class=\"card\"><h2>Manual Review Conclusions</h2>{html_list(review_notes, 'No manual review conclusion yet.')}</div>
    """
    return page_shell(f"{session['target_skill_name']} Final Report", body)


def write_workspace_reports(workspace_path: Path) -> dict[str, str]:
    """Write iteration and final HTML reports for a workspace."""
    session = read_session(workspace_path)
    feedback_map = load_feedback_map(workspace_path)
    outputs: dict[str, str] = {}
    for iteration_record in session.get("iterations", []):
        iteration_dir = Path(iteration_record["iteration_path"])
        report_path = iteration_dir / REPORT_FILENAME
        report_path.write_text(build_iteration_report_html(session, iteration_record, feedback_map), encoding="utf-8")
        outputs[f"iteration-{iteration_record['iteration']}"] = str(report_path.resolve())
    final_report_path = workspace_path / FINAL_REPORT_FILENAME
    final_report_path.write_text(build_final_report_html(session, feedback_map), encoding="utf-8")
    outputs["final"] = str(final_report_path.resolve())
    return outputs


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate improvement reports for a workspace")
    parser.add_argument("workspace", help="Path to the improvement workspace")
    args = parser.parse_args()

    outputs = write_workspace_reports(Path(args.workspace).resolve())
    for name, output in outputs.items():
        print(f"{name}: {output}")


if __name__ == "__main__":
    main()
