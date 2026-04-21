"""Shared utilities for skill-creator scripts."""

import json
from pathlib import Path

def parse_skill_md(skill_path: Path) -> tuple[str, str, str]:
    """Parse a SKILL.md file, returning (name, description, full_content)."""
    content = (skill_path / "SKILL.md").read_text(encoding="utf-8")
    lines = content.split("\n")

    if lines[0].strip() != "---":
        raise ValueError("SKILL.md missing frontmatter (no opening ---)")

    end_idx = None
    for i, line in enumerate(lines[1:], start=1):
        if line.strip() == "---":
            end_idx = i
            break

    if end_idx is None:
        raise ValueError("SKILL.md missing frontmatter (no closing ---)")

    name = ""
    description = ""
    frontmatter_lines = lines[1:end_idx]
    i = 0
    while i < len(frontmatter_lines):
        line = frontmatter_lines[i]
        if line.startswith("name:"):
            name = line[len("name:"):].strip().strip('"').strip("'")
        elif line.startswith("description:"):
            value = line[len("description:"):].strip()
            # Handle YAML multiline indicators (>, |, >-, |-)
            if value in (">", "|", ">-", "|-"):
                continuation_lines: list[str] = []
                i += 1
                while i < len(frontmatter_lines) and (frontmatter_lines[i].startswith("  ") or frontmatter_lines[i].startswith("\t")):
                    continuation_lines.append(frontmatter_lines[i].strip())
                    i += 1
                description = " ".join(continuation_lines)
                continue
            else:
                description = value.strip('"').strip("'")
        i += 1

    return name, description, content


def json_dumps(data) -> str:
    """Serialize JSON as UTF-8 friendly, human-readable text."""
    return json.dumps(data, indent=2, ensure_ascii=False)


def extract_assistant_text(data: dict) -> str:
    """Extract assistant text from supported nova/Claude JSON shapes."""
    if isinstance(data.get("text"), str):
        return data["text"]

    messages = data.get("messages", [])
    for msg in reversed(messages):
        if msg.get("role") != "assistant":
            continue
        content = msg.get("content", [])
        if isinstance(content, str):
            return content
        text_parts = []
        for block in content:
            if block.get("type") == "text":
                text_parts.append(block.get("text", ""))
        if text_parts:
            return "".join(text_parts)

    choices = data.get("choices", [])
    if choices:
        message = choices[0].get("message", {})
        content = message.get("content", "")
        if isinstance(content, str):
            return content

    return ""


def has_tool_call(data: dict) -> bool:
    """Return whether supported response shapes contain any tool call."""
    if data.get("tool_calls"):
        return True

    for msg in data.get("messages", []):
        if msg.get("tool_calls"):
            return True
        content = msg.get("content", [])
        if isinstance(content, list):
            for block in content:
                if block.get("type") == "tool_use":
                    return True

    return False


def request_includes_skill(request_path: Path, skill_name: str) -> bool:
    """Check whether a captured nova request contains injected skill instructions."""
    if not request_path.exists():
        return False

    content = request_path.read_text(encoding="utf-8", errors="replace")
    markers = [
        f"## Skill: {skill_name}",
        f"### Instructions for {skill_name}",
        f"Path: ",
    ]
    return markers[0] in content or markers[1] in content
