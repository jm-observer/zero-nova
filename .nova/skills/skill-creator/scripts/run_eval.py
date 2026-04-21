#!/usr/bin/env python3
"""Run trigger evaluation for a skill description.

Tests whether a skill's description causes Claude to trigger (read the skill)
for a set of queries. Outputs results as JSON.
"""

import argparse
import hashlib
import json
import os
import select
import subprocess
import sys
import tempfile
import time
import uuid
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

# Ensure the skill-creator root (parent of scripts/) is on sys.path so that
# `from scripts.xxx import ...` works regardless of cwd or invocation method.
_SKILL_CREATOR_ROOT = str(Path(__file__).resolve().parent.parent)
if _SKILL_CREATOR_ROOT not in sys.path:
    sys.path.insert(0, _SKILL_CREATOR_ROOT)

from scripts.utils import parse_skill_md


def find_project_root() -> Path:
    """Find the project root by walking up from cwd looking for .claude/.

    Mimics how Claude Code discovers its project root, so the command file
    we create ends up where claude -p will look for it.
    """
    current = Path.cwd()
    for parent in [current, *current.parents]:
        if (parent / ".claude").is_dir():
            return parent
    return current


def run_single_query(
    query: str,
    skill_name: str,
    skill_description: str,
    timeout: int,
    project_root: str,
    model: str | None = None,
    output_dir: Path | None = None,
) -> dict:
    """Run a single query and return whether the skill was triggered.

    Creates a command file in .claude/commands/ so it appears in Claude's
    available_skills list, then runs `claude -p` with the raw query.
    Uses --include-partial-messages to detect triggering early from
    stream events (content_block_start) rather than waiting for the
    full assistant message, which only arrives after tool execution.
    """
    unique_id = uuid.uuid4().hex[:8]
    clean_name = f"{skill_name}-skill-{unique_id}"

    try:
        # Note: In zero-nova adaptation, we don't need to write to .claude/commands
        # because the CLI loads skills from .nova/skills automatically.
        
        cmd = [
            "cargo", "run", "--bin", "nova_cli", "--",
            "run", query,
            "--json"
        ]
        if model:
            cmd.extend(["--model", model])

        # Remove CLAUDECODE env var to allow nesting claude -p inside a
        # Claude Code session. The guard is for interactive terminal conflicts;
        # programmatic subprocess usage is safe.
        env = {k: v for k, v in os.environ.items() if k != "CLAUDECODE"}

        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=project_root,
            env=env,
        )

        # nova_cli run --json outputs a single large JSON blob (TurnResult)
        t_start = time.time()
        try:
            stdout, _ = process.communicate(timeout=timeout)
            duration_ms = int((time.time() - t_start) * 1000)
            
            if not stdout:
                return {"triggered": False, "total_tokens": 0, "duration_ms": duration_ms}
                
            try:
                result_data = json.loads(stdout.decode("utf-8"))
                
                # Capture usage data if available
                # Schema assumption: result_data.get("usage", {}) -> {"input_tokens": X, "output_tokens": Y}
                usage = result_data.get("usage", {})
                total_tokens = usage.get("total_tokens", usage.get("input_tokens", 0) + usage.get("output_tokens", 0))

                # Search all messages for ToolUse that matches our target
                triggered = False
                for msg in result_data.get("messages", []):
                    for block in msg.get("content", []):
                        if block.get("type") == "tool_use":
                             triggered = True
                             break
                    if triggered: break
                
                # Optional: save specific output to output_dir if provided
                if output_dir:
                    output_dir.mkdir(parents=True, exist_ok=True)
                    (output_dir / "result.json").write_text(json.dumps(result_data, indent=2))

                return {
                    "triggered": triggered,
                    "total_tokens": total_tokens,
                    "duration_ms": duration_ms
                }
                
            except json.JSONDecodeError:
                return {"triggered": False, "total_tokens": 0, "duration_ms": duration_ms}
        finally:
            # Clean up process on any exit path (return, exception, timeout)
            if process.poll() is None:
                process.kill()
                process.wait()
    finally:
        pass


def run_eval(
    eval_set: list[dict],
    skill_name: str,
    description: str,
    num_workers: int,
    timeout: int,
    project_root: Path,
    runs_per_query: int = 1,
    trigger_threshold: float = 0.5,
    model: str | None = None,
    iteration: int | None = None,
    output_root: Path | None = None,
) -> dict:
    """Run the full eval set and return results."""
    results = []

    # Path standardization: All tests under a unified temp directory tree.
    # Callers (e.g. run_loop) can pass output_root to keep everything
    # under one skill-level directory; standalone usage falls back to
    # the system temp directory.
    if output_root is None:
        output_root = Path(tempfile.gettempdir()) / "nova_skill_creator" / skill_name
    if iteration is not None:
        output_root = output_root / f"iteration-{iteration}"

    with ProcessPoolExecutor(max_workers=num_workers) as executor:
        future_to_info = {}
        for item in eval_set:
            query_id = hashlib.md5(item["query"].encode()).hexdigest()[:8]
            for run_idx in range(runs_per_query):
                # Unique output dir for each run
                run_output_dir = output_root / f"query-{query_id}" / f"run-{run_idx}"

                future = executor.submit(
                    run_single_query,
                    item["query"],
                    skill_name,
                    description,
                    timeout,
                    str(project_root),
                    model,
                    run_output_dir,
                )
                future_to_info[future] = (item, run_idx)

        query_data: dict[str, list[dict]] = {}
        query_items: dict[str, dict] = {}
        for future in as_completed(future_to_info):
            item, _ = future_to_info[future]
            query = item["query"]
            query_items[query] = item
            if query not in query_data:
                query_data[query] = []
            try:
                query_data[query].append(future.result())
            except Exception as e:
                print(f"Warning: query failed: {e}", file=sys.stderr)
                query_data[query].append({"triggered": False, "total_tokens": 0, "duration_ms": 0})

    total_tokens = 0
    total_duration_ms = 0

    for query, data_list in query_data.items():
        item = query_items[query]
        triggers = [d["triggered"] for d in data_list]
        trigger_rate = sum(triggers) / len(triggers)
        
        # Accumulate metrics
        query_tokens = sum(d["total_tokens"] for d in data_list)
        query_duration = sum(d["duration_ms"] for d in data_list)
        total_tokens += query_tokens
        total_duration_ms += query_duration

        should_trigger = item["should_trigger"]
        if should_trigger:
            did_pass = trigger_rate >= trigger_threshold
        else:
            did_pass = trigger_rate < trigger_threshold
        results.append({
            "query": query,
            "should_trigger": should_trigger,
            "trigger_rate": trigger_rate,
            "triggers": sum(triggers),
            "runs": len(triggers),
            "pass": did_pass,
            "tokens": query_tokens,
            "duration_ms": query_duration,
        })

    passed = sum(1 for r in results if r["pass"])
    total = len(results)

    return {
        "skill_name": skill_name,
        "description": description,
        "results": results,
        "summary": {
            "total": total,
            "passed": passed,
            "failed": total - passed,
            "total_tokens": total_tokens,
            "total_duration_ms": total_duration_ms,
        },
    }


def main():
    parser = argparse.ArgumentParser(description="Run trigger evaluation for a skill description")
    parser.add_argument("--eval-set", required=True, help="Path to eval set JSON file")
    parser.add_argument("--skill-path", required=True, help="Path to skill directory")
    parser.add_argument("--description", default=None, help="Override description to test")
    parser.add_argument("--num-workers", type=int, default=10, help="Number of parallel workers")
    parser.add_argument("--timeout", type=int, default=30, help="Timeout per query in seconds")
    parser.add_argument("--runs-per-query", type=int, default=3, help="Number of runs per query")
    parser.add_argument("--trigger-threshold", type=float, default=0.5, help="Trigger rate threshold")
    parser.add_argument("--model", default=None, help="Model to use for claude -p (default: user's configured model)")
    parser.add_argument("--verbose", action="store_true", help="Print progress to stderr")
    args = parser.parse_args()

    eval_set = json.loads(Path(args.eval_set).read_text())
    skill_path = Path(args.skill_path)

    if not (skill_path / "SKILL.md").exists():
        print(f"Error: No SKILL.md found at {skill_path}", file=sys.stderr)
        sys.exit(1)

    name, original_description, content = parse_skill_md(skill_path)
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
    )

    if args.verbose:
        summary = output["summary"]
        print(f"Results: {summary['passed']}/{summary['total']} passed", file=sys.stderr)
        for r in output["results"]:
            status = "PASS" if r["pass"] else "FAIL"
            rate_str = f"{r['triggers']}/{r['runs']}"
            print(f"  [{status}] rate={rate_str} expected={r['should_trigger']}: {r['query'][:70]}", file=sys.stderr)

    print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
