#!/usr/bin/env python3
"""Run trigger evaluations through the real `nova_cli` skill loading path."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path
from typing import Any

import yaml

from scripts.utils import parse_skill_md

DEFAULT_TRIGGER_THRESHOLD = 0.5
RUN_MODE_BASELINE_NONE = "baseline_none"
RUN_MODE_BASELINE_ORIGINAL = "baseline_original"
RUN_MODE_CANDIDATE = "candidate"
RUN_MODE_INLINE_DESCRIPTION = "inline_description"
SUPPORTED_RUN_MODES = (
    RUN_MODE_BASELINE_NONE,
    RUN_MODE_BASELINE_ORIGINAL,
    RUN_MODE_CANDIDATE,
    RUN_MODE_INLINE_DESCRIPTION,
)
CLI_ENV_VAR = "NOVA_SKILL_EVAL_CLI"
DEFAULT_CLI_COMMAND = ["cargo", "run", "--quiet", "--bin", "nova_cli", "--"]
ERROR_TIMEOUT = "timeout"
ERROR_LOAD_FAILED = "load_failed"
ERROR_PROCESS_FAILED = "process_failed"
ERROR_OUTPUT_INVALID = "output_invalid"


def find_project_root() -> Path:
    """Find the project root by walking up from cwd looking for `.nova/`."""
    current = Path.cwd()
    for parent in [current, *current.parents]:
        if (parent / ".nova").is_dir():
            return parent
    return current


def load_eval_set(path: str | Path) -> list[dict[str, Any]]:
    """Load an eval set from JSON."""
    payload = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(payload, list):
        raise ValueError("Eval set must be a JSON array")
    return payload


def parse_cli_command() -> list[str]:
    """Return the CLI command prefix used to launch `nova_cli`."""
    configured = os.environ.get(CLI_ENV_VAR, "").strip()
    if configured:
        return configured.split()
    return list(DEFAULT_CLI_COMMAND)


def extract_event(line: str) -> tuple[str | None, Any]:
    """Parse one stream-json line into a `(event_name, payload)` tuple."""
    try:
        parsed = json.loads(line)
    except json.JSONDecodeError:
        return None, None

    if isinstance(parsed, dict) and len(parsed) == 1:
        name, payload = next(iter(parsed.items()))
        return name, payload

    if isinstance(parsed, dict) and isinstance(parsed.get("type"), str):
        return parsed["type"], parsed

    return None, parsed


def analyze_cli_output(stdout: str) -> dict[str, Any]:
    """Inspect `nova_cli` JSONL output and determine whether the skill triggered."""
    triggered = False
    parsed_lines = 0
    load_errors: list[str] = []
    loaded_skills: list[str] = []

    for raw_line in stdout.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        event_name, payload = extract_event(line)
        if event_name is None:
            continue

        parsed_lines += 1
        if event_name in {"SkillLoaded", "SkillActivated", "SkillInvocation"}:
            triggered = True
            if isinstance(payload, dict):
                skill_name = payload.get("skill_name")
                if isinstance(skill_name, str) and skill_name:
                    loaded_skills.append(skill_name)

        if event_name == "ToolStart" and isinstance(payload, dict) and payload.get("name") == "Skill":
            triggered = True

        if event_name == "ToolEnd" and isinstance(payload, dict) and payload.get("name") == "Skill":
            if payload.get("is_error"):
                load_errors.append(str(payload.get("output", "Skill tool failed")))

        if event_name == "Error":
            message = payload.get("message") if isinstance(payload, dict) else payload
            if message:
                load_errors.append(str(message))

    error_type = None
    error_message = None
    if load_errors:
        error_type = ERROR_LOAD_FAILED
        error_message = "; ".join(load_errors)
    elif parsed_lines == 0:
        error_type = ERROR_OUTPUT_INVALID
        error_message = "No JSON events were emitted by nova_cli"

    return {
        "triggered": triggered,
        "parsed_lines": parsed_lines,
        "loaded_skills": loaded_skills,
        "error_type": error_type,
        "error_message": error_message,
    }


def override_skill_description(skill_copy_path: Path, description: str) -> None:
    """Rewrite the copied SKILL.md frontmatter description for description-only evals."""
    skill_md = skill_copy_path / "SKILL.md"
    content = skill_md.read_text(encoding="utf-8")
    parts = content.split("---", 2)
    if len(parts) < 3:
        raise ValueError(f"Invalid SKILL.md frontmatter in {skill_md}")

    frontmatter = yaml.safe_load(parts[1])
    if not isinstance(frontmatter, dict):
        raise ValueError(f"SKILL.md frontmatter must be a mapping: {skill_md}")

    frontmatter["description"] = description
    rendered_frontmatter = yaml.safe_dump(frontmatter, sort_keys=False, allow_unicode=True).strip()
    rewritten = f"---\n{rendered_frontmatter}\n---{parts[2]}"
    skill_md.write_text(rewritten, encoding="utf-8")


def prepare_eval_skill_path(skill_path: Path, description_override: str | None) -> Path:
    """Return the skill path to evaluate, applying description overrides if needed."""
    if description_override is None:
        return skill_path

    temp_root = Path(tempfile.mkdtemp(prefix="skill-eval-"))
    copied_skill = temp_root / skill_path.name
    shutil.copytree(skill_path, copied_skill)
    override_skill_description(copied_skill, description_override)
    return copied_skill


def cleanup_temp_skill_path(path: Path, original_skill_path: Path) -> None:
    """Delete a temporary skill copy when one was created."""
    if path == original_skill_path:
        return
    temp_root = path.parent
    shutil.rmtree(temp_root, ignore_errors=True)


def build_eval_command(
    query: str,
    project_root: Path,
    skill_path: Path | None,
    model: str | None,
) -> list[str]:
    """Build the `nova_cli` invocation for one query."""
    command = parse_cli_command()
    if skill_path is not None:
        command.extend(["--include-skill", str(skill_path)])
    if model:
        command.extend(["--model", model])
    command.extend(["run", query, "--output-format", "stream-json"])
    return command


def run_single_query(
    query: str,
    timeout: int,
    project_root: str,
    mode: str,
    skill_path: str | None = None,
    model: str | None = None,
) -> dict[str, Any]:
    """Run a single query through `nova_cli` and inspect whether the skill triggered."""
    resolved_skill_path = Path(skill_path) if skill_path is not None else None
    command = build_eval_command(query, Path(project_root), resolved_skill_path, model)
    env = dict(os.environ)
    env.pop("CLAUDECODE", None)

    try:
        completed = subprocess.run(
            command,
            cwd=project_root,
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        return {
            "query": query,
            "mode": mode,
            "triggered": False,
            "timed_out": True,
            "error_type": ERROR_TIMEOUT,
            "error_message": f"Command timed out after {timeout} seconds",
            "loaded_skills": [],
            "stdout": (exc.stdout or ""),
            "stderr": (exc.stderr or ""),
            "return_code": None,
        }

    analysis = analyze_cli_output(completed.stdout)
    error_type = analysis["error_type"]
    error_message = analysis["error_message"]

    if completed.returncode != 0 and error_type is None:
        error_type = ERROR_PROCESS_FAILED
        stderr = completed.stderr.strip()
        if stderr:
            error_message = stderr
        else:
            error_message = f"nova_cli exited with code {completed.returncode}"

    return {
        "query": query,
        "mode": mode,
        "triggered": analysis["triggered"],
        "timed_out": False,
        "error_type": error_type,
        "error_message": error_message,
        "loaded_skills": analysis["loaded_skills"],
        "stdout": completed.stdout,
        "stderr": completed.stderr,
        "return_code": completed.returncode,
    }


def summarize_results(results: list[dict[str, Any]], trigger_threshold: float) -> dict[str, Any]:
    """Convert raw per-run query data into eval summaries."""
    passed = sum(1 for result in results if result["pass"])
    error_counts: dict[str, int] = {}
    for result in results:
        for run in result["runs_detail"]:
            error_type = run.get("error_type")
            if error_type:
                error_counts[error_type] = error_counts.get(error_type, 0) + 1

    total = len(results)
    return {
        "total": total,
        "passed": passed,
        "failed": total - passed,
        "pass_rate": passed / total if total else 0.0,
        "trigger_threshold": trigger_threshold,
        "error_counts": error_counts,
    }


def run_eval(
    eval_set: list[dict[str, Any]],
    *,
    skill_path: Path | None,
    num_workers: int,
    timeout: int,
    project_root: Path,
    runs_per_query: int = 1,
    trigger_threshold: float = DEFAULT_TRIGGER_THRESHOLD,
    model: str | None = None,
    mode: str = RUN_MODE_CANDIDATE,
    description_override: str | None = None,
) -> dict[str, Any]:
    """Run the full eval set and return aggregated results."""
    if mode not in SUPPORTED_RUN_MODES:
        raise ValueError(f"Unsupported run mode: {mode}")
    if mode != RUN_MODE_BASELINE_NONE and skill_path is None:
        raise ValueError(f"Run mode '{mode}' requires a skill path")

    effective_skill_path = None if skill_path is None else prepare_eval_skill_path(skill_path, description_override)
    skill_name = "no-skill"
    description = ""
    if effective_skill_path is not None:
        skill_name, description, _ = parse_skill_md(effective_skill_path)

    try:
        future_to_info: dict[Any, tuple[dict[str, Any], int]] = {}
        query_runs: dict[str, list[dict[str, Any]]] = {}
        query_items: dict[str, dict[str, Any]] = {}

        with ProcessPoolExecutor(max_workers=num_workers) as executor:
            for item in eval_set:
                query = item["query"]
                query_items[query] = item
                query_runs.setdefault(query, [])
                for run_index in range(runs_per_query):
                    future = executor.submit(
                        run_single_query,
                        query,
                        timeout,
                        str(project_root),
                        mode,
                        str(effective_skill_path) if effective_skill_path is not None else None,
                        model,
                    )
                    future_to_info[future] = (item, run_index)

            for future in as_completed(future_to_info):
                item, _ = future_to_info[future]
                query = item["query"]
                try:
                    query_runs[query].append(future.result())
                except Exception as exc:
                    query_runs[query].append(
                        {
                            "query": query,
                            "mode": mode,
                            "triggered": False,
                            "timed_out": False,
                            "error_type": ERROR_PROCESS_FAILED,
                            "error_message": str(exc),
                            "loaded_skills": [],
                            "stdout": "",
                            "stderr": "",
                            "return_code": None,
                        }
                    )

        results: list[dict[str, Any]] = []
        for query, runs in query_runs.items():
            item = query_items[query]
            trigger_count = sum(1 for run in runs if run["triggered"])
            trigger_rate = trigger_count / len(runs) if runs else 0.0
            should_trigger = bool(item["should_trigger"])
            did_pass = trigger_rate >= trigger_threshold if should_trigger else trigger_rate < trigger_threshold
            results.append(
                {
                    "query": query,
                    "should_trigger": should_trigger,
                    "trigger_rate": trigger_rate,
                    "triggers": trigger_count,
                    "runs": len(runs),
                    "pass": did_pass,
                    "runs_detail": runs,
                }
            )

        results.sort(key=lambda item: item["query"])
        summary = summarize_results(results, trigger_threshold)
        return {
            "mode": mode,
            "skill_name": skill_name,
            "skill_path": str(effective_skill_path) if effective_skill_path is not None else None,
            "description": description_override or description,
            "results": results,
            "summary": summary,
        }
    finally:
        if skill_path is not None and effective_skill_path is not None:
            cleanup_temp_skill_path(effective_skill_path, skill_path)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run trigger evaluation through nova_cli")
    parser.add_argument("--eval-set", required=True, help="Path to eval set JSON file")
    parser.add_argument("--mode", choices=SUPPORTED_RUN_MODES[:3], default=RUN_MODE_CANDIDATE)
    parser.add_argument("--skill-path", help="Path to the skill directory")
    parser.add_argument("--description", default=None, help="Override description to test against a copied skill")
    parser.add_argument("--num-workers", type=int, default=10, help="Number of parallel workers")
    parser.add_argument("--timeout", type=int, default=30, help="Timeout per query in seconds")
    parser.add_argument("--runs-per-query", type=int, default=3, help="Number of runs per query")
    parser.add_argument("--trigger-threshold", type=float, default=DEFAULT_TRIGGER_THRESHOLD)
    parser.add_argument("--model", default=None, help="Optional model override for nova_cli")
    parser.add_argument("--verbose", action="store_true", help="Print progress to stderr")
    args = parser.parse_args()

    eval_set = load_eval_set(args.eval_set)
    skill_path = Path(args.skill_path) if args.skill_path else None
    if args.mode != RUN_MODE_BASELINE_NONE and skill_path is None:
        print(f"Error: --skill-path is required for mode {args.mode}", file=sys.stderr)
        sys.exit(1)

    project_root = find_project_root()
    output = run_eval(
        eval_set,
        skill_path=skill_path,
        num_workers=args.num_workers,
        timeout=args.timeout,
        project_root=project_root,
        runs_per_query=args.runs_per_query,
        trigger_threshold=args.trigger_threshold,
        model=args.model,
        mode=args.mode,
        description_override=args.description,
    )

    if args.verbose:
        summary = output["summary"]
        print(
            f"{args.mode}: {summary['passed']}/{summary['total']} passed, errors={summary['error_counts']}",
            file=sys.stderr,
        )

    print(json.dumps(output, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
