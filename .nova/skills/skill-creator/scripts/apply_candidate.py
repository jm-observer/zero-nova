#!/usr/bin/env python3
"""Apply a structured candidate plan to a candidate skill copy."""

from __future__ import annotations

import argparse
import json
import re
import shutil
from pathlib import Path
from typing import Any

from scripts.quick_validate import validate_skill
from scripts.utils import parse_skill_md, write_json

ALLOWED_FILES = {"SKILL.md"}
MANAGED_SECTION_MARKER = "<!-- skill-creator:managed-section -->"


def replace_description(content: str, new_description: str) -> str:
    """Replace frontmatter description while preserving the rest of the file."""
    pattern = re.compile(r"^(description:\s*)(.*)$", re.MULTILINE)
    if not pattern.search(content):
        raise ValueError("Could not find frontmatter description field in SKILL.md")
    return pattern.sub(rf"\1{new_description}", content, count=1)


def render_managed_section(title: str, lines: list[str]) -> str:
    """Build the managed markdown section payload."""
    body = "\n".join(lines).strip()
    return f"{MANAGED_SECTION_MARKER}\n{title}\n\n{body}\n"


def upsert_section(content: str, title: str, lines: list[str]) -> str:
    """Insert or replace the managed guidance section."""
    section = render_managed_section(title, lines)
    pattern = re.compile(
        rf"\n?{re.escape(MANAGED_SECTION_MARKER)}\n{re.escape(title)}\n(?:.|\n)*?(?=\n## |\Z)",
        re.MULTILINE,
    )
    if pattern.search(content):
        updated = pattern.sub("\n" + section, content, count=1)
        return updated.rstrip() + "\n"
    return content.rstrip() + "\n\n" + section


def ensure_candidate_copy(base_skill_path: Path, candidate_skill_path: Path) -> None:
    """Recreate candidate skill from the current best skill."""
    if candidate_skill_path.exists():
        shutil.rmtree(candidate_skill_path)
    shutil.copytree(base_skill_path, candidate_skill_path)


def apply_candidate_plan(base_skill_path: Path, candidate_skill_path: Path, plan: dict[str, Any]) -> dict[str, Any]:
    """Apply allowed changes and write a diff summary artifact."""
    ensure_candidate_copy(base_skill_path, candidate_skill_path)
    skill_file = candidate_skill_path / "SKILL.md"
    content = skill_file.read_text(encoding="utf-8")
    applied_changes: list[dict[str, Any]] = []

    for change in plan.get("changes", []):
        relative_path = change.get("path")
        if relative_path not in ALLOWED_FILES:
            raise ValueError(f"Change targets forbidden file: {relative_path}")

        change_type = change.get("type")
        if change_type == "update_description":
            content = replace_description(content, str(change.get("new_value", "")))
        elif change_type == "upsert_section":
            content = upsert_section(content, str(change.get("section_title", "## Improvement Focus")), list(change.get("lines", [])))
        else:
            raise ValueError(f"Unsupported change type: {change_type}")

        applied_changes.append(
            {
                "type": change_type,
                "path": relative_path,
                "reason": change.get("reason", ""),
            }
        )

    skill_file.write_text(content, encoding="utf-8")
    parse_skill_md(candidate_skill_path)
    valid, message = validate_skill(candidate_skill_path, require_iteration_ready=True)
    if not valid:
        raise ValueError(message)

    diff_summary = {
        "base_skill_path": str(base_skill_path.resolve()),
        "candidate_skill_path": str(candidate_skill_path.resolve()),
        "applied_changes": applied_changes,
        "allowed_files": sorted(ALLOWED_FILES),
        "validation": {"ok": True, "message": message},
    }
    write_json(candidate_skill_path.parent / "candidate-diff-summary.json", diff_summary)
    return diff_summary


def main() -> None:
    parser = argparse.ArgumentParser(description="Apply a candidate improvement plan")
    parser.add_argument("--base-skill", required=True)
    parser.add_argument("--candidate-skill", required=True)
    parser.add_argument("--plan", required=True)
    parser.add_argument("--output", default=None)
    args = parser.parse_args()

    result = apply_candidate_plan(
        Path(args.base_skill),
        Path(args.candidate_skill),
        json.loads(Path(args.plan).read_text(encoding="utf-8")),
    )
    if args.output:
        write_json(Path(args.output), result)
    else:
        print(json.dumps(result, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
