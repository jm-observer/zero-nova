#!/usr/bin/env python3
"""Run trigger evaluation for a skill description.

Tests whether a skill's description causes Claude to trigger (read the skill)
for a set of queries. Outputs results as JSON.
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

# Ensure the skill-creator root (parent of scripts/) is on sys.path.
_SKILL_CREATOR_ROOT = str(Path(__file__).resolve().parent.parent)
if _SKILL_CREATOR_ROOT not in sys.path:
    sys.path.insert(0, _SKILL_CREATOR_ROOT)

from scripts.utils import (
    cleanup_residual_processes,
    has_tool_call,
    json_dumps,
    parse_skill_md,
    request_includes_skill,
    subprocess_group_kwargs,
    terminate_process_tree,
)


def find_project_root() -> Path:
    """Find the project root by walking up from cwd looking for .nova/."""
    current = Path.cwd()
    for parent in [current, *current.parents]:
        if (parent / ".nova").is_dir():
            return parent
    return current


def _replace_frontmatter_fields(content: str, name: str, description: str) -> str:
    """Replace name/description in SKILL.md frontmatter."""
    lines = content.split("\n")
    if not lines or lines[0].strip() != "---":
        raise ValueError("SKILL.md missing frontmatter")

    new_lines = [lines[0]]
    in_frontmatter = True
    skip_continuation = False
    saw_name = False
    saw_description = False

    for line in lines[1:]:
        if line.strip() == "---" and in_frontmatter:
            in_frontmatter = False
            if not saw_name:
                new_lines.insert(1, f"name: {name}")
            if not saw_description:
                new_lines.insert(1, f"description: {description}")
            new_lines.append(line)
            continue

        if in_frontmatter:
            if skip_continuation and (line.startswith("  ") or line.startswith("\t")):
                continue
            skip_continuation = False

            if line.startswith("name:"):
                new_lines.append(f"name: {name}")
                saw_name = True
                continue
            if line.startswith("description:"):
                new_lines.append(f"description: {description}")
                saw_description = True
                value = line[len("description:"):].strip()
                if value in (">", "|", ">-", "|-"):
                    skip_continuation = True
                continue

        new_lines.append(line)

    return "\n".join(new_lines)


def _install_eval_skill(
    source_skill_path: Path,
    project_root: Path,
    eval_skill_name: str,
    description: str,
) -> Path:
    """Copy skill to workspace .nova/skills/ and patch its name/description."""
    dest_path = project_root / ".nova" / "skills" / eval_skill_name
    if dest_path.exists():
        shutil.rmtree(dest_path)
    shutil.copytree(source_skill_path, dest_path)

    skill_md_path = dest_path / "SKILL.md"
    content = skill_md_path.read_text(encoding="utf-8")
    patched = _replace_frontmatter_fields(content, eval_skill_name, description)
    skill_md_path.write_text(patched, encoding="utf-8")

    return dest_path


def _run_single_query(
    query: str,
    eval_skill_name: str,
    project_root: Path,
    timeout_secs: int,
    model: str | None = None,
) -> dict:
    """Run a single query through nova_cli and check if skill was triggered."""
    cmd = ["cargo", "run", "--bin", "nova_cli", "--", "run", query, "--json"]
    if model:
        cmd.extend(["--model", model])

    env = os.environ.copy()
    # Ensure current project root is used for skill lookup
    env["NOVA_PROJECT_ROOT"] = str(project_root)

    t0 = time.time()
    try:
        # We don't want to use shell=True if we can avoid it.
        # Use subprocess_group_kwargs to ensure we can kill the whole tree later.
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=str(project_root),
            env=env,
            encoding="utf-8",
            **subprocess_group_kwargs(),
        )
        try:
            stdout, stderr = proc.communicate(timeout=timeout_secs)
            duration_ms = int((time.time() - t0) * 1000)
        except subprocess.TimeoutExpired:
            terminate_process_tree(proc.pid)
            stdout, stderr = proc.communicate()
            return {
                "triggered": False,
                "error": "timeout",
                "duration_ms": int((time.time() - t0) * 1000),
                "stdout": stdout,
                "stderr": stderr,
            }

        if proc.returncode != 0:
            return {
                "triggered": False,
                "error": f"exit_code_{proc.returncode}",
                "duration_ms": duration_ms,
                "stdout": stdout,
                "stderr": stderr,
            }

        try:
            # The last line of stdout should be our JSON results if using --json
            # But sometimes there's noise (compilation logs etc). Try to find the JSON block.
            lines = stdout.strip().split("\n")
            result_json = None
            for line in reversed(lines):
                if line.strip().startswith("{") and line.strip().endswith("}"):
                    try:
                        result_json = json.loads(line)
                        break
                    except json.JSONDecodeError:
                        continue
            
            if not result_json:
                return {"triggered": False, "error": "no_json_output", "stdout": stdout, "stderr": stderr}

            # Check if our specific eval skill was requested/triggered
            triggered = request_includes_skill(result_json, eval_skill_name) or has_tool_call(result_json, eval_skill_name)
            
            return {
                "triggered": triggered,
                "tokens": result_json.get("usage", {}).get("total_tokens", 0),
                "duration_ms": duration_ms,
            }
        except Exception as e:
            return {"triggered": False, "error": str(e), "stdout": stdout, "stderr": stderr}

    except Exception as e:
        return {"triggered": False, "error": str(e), "duration_ms": int((time.time() - t0) * 1000)}


def run_eval(
    eval_set: list[dict],
    skill_name: str,
    description: str,
    num_workers: int,
    timeout: int,
    project_root: Path,
    runs_per_query: int = 3,
    trigger_threshold: float = 0.5,
    model: str | None = None,
    iteration: int = 0,
    output_root: Path | None = None,
    source_skill_path: Path | None = None,
) -> dict:
    """Run evaluation for a specific description."""
    unique_id = uuid.uuid4().hex[:8]
    eval_skill_name = f"{skill_name}-eval-{unique_id}"
    
    # Pre-flight cleanup
    cleanup_residual_processes()

    installed_skill_path = None
    try:
        if source_skill_path:
            installed_skill_path = _install_eval_skill(
                source_skill_path,
                project_root,
                eval_skill_name,
                description,
            )

        results = []
        with ProcessPoolExecutor(max_workers=num_workers) as executor:
            future_to_query = {}
            for item in eval_set:
                query = item["query"]
                for r in range(runs_per_query):
                    f = executor.submit(
                        _run_single_query,
                        query,
                        eval_skill_name,
                        project_root,
                        timeout,
                        model,
                    )
                    future_to_query[f] = (item, r)

            # Collect results
            raw_results = {} # query -> list of results
            for future in as_completed(future_to_query):
                item, run_idx = future_to_query[future]
                query = item["query"]
                if query not in raw_results:
                    raw_results[query] = []
                raw_results[query].append(future.result())

        # Aggregate per query
        for item in eval_set:
            query = item["query"]
            query_results = raw_results.get(query, [])
            triggers = sum(1 for r in query_results if r.get("triggered"))
            trigger_rate = triggers / len(query_results) if query_results else 0
            
            # A query passes if its trigger rate matches the 'should_trigger' expectation
            should_trigger = item.get("should_trigger", True)
            passed = (trigger_rate >= trigger_threshold) if should_trigger else (trigger_rate < (1 - trigger_threshold))
            
            results.append({
                "query": query,
                "should_trigger": should_trigger,
                "trigger_rate": trigger_rate,
                "triggers": triggers,
                "runs": len(query_results),
                "pass": passed,
                "tokens": sum(r.get("tokens", 0) for r in query_results),
                "duration_ms": sum(r.get("duration_ms", 0) for r in query_results),
                "details": query_results,
            })

        summary = {
            "total": len(results),
            "passed": sum(1 for r in results if r["pass"]),
            "failed": sum(1 for r in results if not r["pass"]),
            "total_tokens": sum(r["tokens"] for r in results),
            "total_duration_ms": sum(r["duration_ms"] for r in results),
        }

        return {"results": results, "summary": summary}

    finally:
        if installed_skill_path and installed_skill_path.exists():
            shutil.rmtree(installed_skill_path)


def main():
    parser = argparse.ArgumentParser(description="Run trigger evaluation for a skill description")
    parser.add_argument("--eval-set", required=True, help="Path to eval set JSON file")
    parser.add_argument("--skill-path", required=True, help="Path to skill directory")
    parser.add_argument("--description", default=None, help="Override description to test")
    parser.add_argument("--num-workers", type=int, default=10, help="Number of parallel workers")
    parser.add_argument("--timeout", type=int, default=30, help="Timeout per query in seconds")
    parser.add_argument("--runs-per-query", type=int, default=3, help="Number of runs per query")
    parser.add_argument("--trigger-threshold", type=float, default=0.5, help="Trigger rate threshold")
    parser.add_argument("--model", default=None, help="Model to use")
    parser.add_argument("--verbose", action="store_true", help="Print progress to stderr")
    args = parser.parse_args()

    content = Path(args.eval_set).read_text(encoding="utf-8")
    eval_data = json.loads(content)
    
    # Robustly handle evals.json structure
    if isinstance(eval_data, dict):
        if "evals" in eval_data:
            eval_set = eval_data["evals"]
        else:
            print("Error: JSON dict missing 'evals' key", file=sys.stderr)
            sys.exit(1)
    else:
        eval_set = eval_data
        
    # Map 'prompt' -> 'query'
    for i, item in enumerate(eval_set):
        if "query" not in item and "prompt" in item:
            item["query"] = item.pop("prompt")
        if "query" not in item:
            print(f"Error: item {i} missing 'query' or 'prompt'", file=sys.stderr)
            sys.exit(1)

    skill_path = Path(args.skill_path)
    name, original_description, _ = parse_skill_md(skill_path)
    description = args.description or original_description
    project_root = find_project_root()

    if args.verbose:
        print(f"Evaluating: {description}", file=sys.stderr)

    output = run_eval(
        eval_set=eval_set,
        skill_name=name,
        description=description,
        num_workers=args.num_workers,
        timeout=args.timeout,
        project_root=project_root,
        runs_per_query=args.runs_per_query,
        trigger_threshold=args.trigger_threshold,
        model=args.model,
        source_skill_path=skill_path,
    )

    if args.verbose:
        summary = output["summary"]
        print(f"Results: {summary['passed']}/{summary['total']} passed", file=sys.stderr)

    print(json_dumps(output))


if __name__ == "__main__":
    main()
