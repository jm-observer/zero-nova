#!/usr/bin/env python3
import json
import subprocess
import sys
import argparse
from pathlib import Path

def execute_with_nova(
    prompt: str,
    skill_path: str = None,
    workspace: str = None,
    model: str = "google/gemma-4-26B-A4B-it",
    timeout: int = 300
):
    """
    使用 nova-cli 执行指令，并返回解析后的事件流。
    """
    cmd = [
        "cargo", "run", "--bin", "nova_cli", "--",
        "run",
        "--model", model,
        "--output-format", "stream-json", prompt
    ]

    if workspace:
        cmd.extend(["--workspace", workspace])
        # 确保工作目录存在
        Path(workspace).mkdir(parents=True, exist_ok=True)

    if skill_path:
        # 为了解决隔离问题，主脚本负责读取 Skill 内容并传递
        # 这里预留 --include-skill 参数，nova-cli 需后续实现该参数
        cmd.extend(["--include-skill", skill_path])

    print(f"Executing: {' '.join(cmd)}", file=sys.stderr)

    try:
        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1
        )

        events = []
        for line in process.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
                events.append(event)
                # 实时反馈工具调用
                if event.get("type") == "ToolStart":
                    print(f"  [Tool] Starting: {event['name']}", file=sys.stderr)
            except json.JSONDecodeError:
                # 忽略非 JSON 输出
                continue

        stdout, stderr = process.communicate(timeout=timeout)
        
        if process.returncode != 0:
            print(f"Error: nova-cli exited with {process.returncode}", file=sys.stderr)
            print(stderr, file=sys.stderr)

        return events

    except subprocess.TimeoutExpired:
        process.kill()
        print(f"Error: Task timed out after {timeout} seconds", file=sys.stderr)
        return []

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Helper to execute tasks via nova-cli")
    parser.add_argument("prompt", help="The prompt to execute")
    parser.add_argument("--skill", help="Path to SKILL.md")
    parser.add_argument("--workspace", help="Workspace directory")
    args = parser.parse_args()

    results = execute_with_nova(args.prompt, skill_path=args.skill, workspace=args.workspace)
    print(json.dumps(results, indent=2))
