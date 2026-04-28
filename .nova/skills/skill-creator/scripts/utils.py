"""Shared utilities for skill-creator scripts."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

from scripts.session_schema import (
    SESSION_FILENAME,
    assert_transition,
    load_session_template,
    utc_now_iso,
    validate_session_payload,
)

DEFAULT_SKILL_SEARCH_ROOTS = (
    Path(".nova/skills"),
    Path.home() / ".codex" / "skills",
)
WORKSPACE_SUFFIX = "-improvement-workspace"


def parse_skill_md(skill_path: Path) -> tuple[str, str, str]:
    """Parse a SKILL.md file, returning (name, description, full_content)."""
    skill_md_path = skill_path / "SKILL.md"
    if not skill_md_path.exists():
        raise ValueError(f"Missing SKILL.md in skill directory: {skill_path}")

    content = skill_md_path.read_text(encoding="utf-8")
    lines = content.split("\n")

    if not lines or lines[0].strip() != "---":
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
            name = line[len("name:") :].strip().strip('"').strip("'")
        elif line.startswith("description:"):
            value = line[len("description:") :].strip()
            if value in (">", "|", ">-", "|-"):
                continuation_lines: list[str] = []
                i += 1
                while i < len(frontmatter_lines) and (
                    frontmatter_lines[i].startswith("  ") or frontmatter_lines[i].startswith("\t")
                ):
                    continuation_lines.append(frontmatter_lines[i].strip())
                    i += 1
                description = " ".join(continuation_lines)
                continue
            description = value.strip('"').strip("'")
        i += 1

    if not name:
        raise ValueError(f"SKILL.md missing required frontmatter field 'name': {skill_md_path}")
    if not description:
        raise ValueError(f"SKILL.md missing required frontmatter field 'description': {skill_md_path}")

    return name, description, content


def slugify_skill_name(skill_name: str) -> str:
    """Convert a skill name into a stable workspace-safe slug."""
    slug = re.sub(r"[^a-z0-9]+", "-", skill_name.strip().lower())
    slug = slug.strip("-")
    if not slug:
        raise ValueError("Skill name cannot be empty when generating workspace name")
    return slug


def stable_workspace_name(skill_name: str) -> str:
    """Return the canonical improvement workspace directory name."""
    return f"{slugify_skill_name(skill_name)}{WORKSPACE_SUFFIX}"


def read_json(path: Path) -> Any:
    """Read JSON from path using UTF-8."""
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, payload: Any) -> None:
    """Write JSON to path using UTF-8 and stable formatting."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def default_skill_search_roots(search_roots: list[Path] | None = None) -> list[Path]:
    """Return the ordered list of skill roots used for id-only resolution."""
    roots = list(DEFAULT_SKILL_SEARCH_ROOTS if search_roots is None else search_roots)
    normalized: list[Path] = []
    for root in roots:
        normalized.append(root if root.is_absolute() else Path.cwd() / root)
    return normalized


def resolve_target_skill_path(target: str | Path, search_roots: list[Path] | None = None) -> Path:
    """Resolve a skill path from either an explicit path or a skill id."""
    candidate = Path(target).expanduser()
    if candidate.exists():
        return candidate.resolve()

    if candidate.is_absolute():
        raise ValueError(f"Target skill path does not exist: {candidate}")

    checked_paths: list[str] = []
    for root in default_skill_search_roots(search_roots):
        resolved = (root / str(target)).resolve()
        checked_paths.append(str(resolved))
        if resolved.exists():
            return resolved

    raise ValueError(f"Could not resolve skill '{target}' in standard skill roots: {', '.join(checked_paths)}")


def parse_target_skill(target: str | Path, search_roots: list[Path] | None = None) -> dict[str, str]:
    """Resolve and parse a target skill into canonical metadata."""
    skill_path = resolve_target_skill_path(target, search_roots=search_roots)
    skill_name, description, _ = parse_skill_md(skill_path)
    return {
        "target_skill_path": str(skill_path),
        "target_skill_name": skill_name,
        "target_skill_description": description,
        "target_skill_id": skill_path.name,
    }


def default_workspace_path(skill_path: Path, skill_name: str) -> Path:
    """Return the canonical workspace path for a target skill."""
    return skill_path.parent / stable_workspace_name(skill_name)


def session_file_path(workspace_path: Path) -> Path:
    """Return the improvement session file location."""
    return workspace_path / SESSION_FILENAME


def build_improvement_session(
    target: str | Path,
    workspace_path: Path | None = None,
    search_roots: list[Path] | None = None,
) -> dict[str, Any]:
    """Create a new improvement session payload from target skill metadata."""
    skill_info = parse_target_skill(target, search_roots=search_roots)
    skill_path = Path(skill_info["target_skill_path"])
    effective_workspace = workspace_path or default_workspace_path(skill_path, skill_info["target_skill_name"])
    effective_workspace = effective_workspace.resolve()
    timestamp = utc_now_iso()

    session = load_session_template()
    session.update(
        {
            "session_id": f"{skill_info['target_skill_id']}-{timestamp.replace(':', '').replace('-', '')}",
            "target_skill_path": str(skill_path),
            "target_skill_name": skill_info["target_skill_name"],
            "workspace_path": str(effective_workspace),
            "snapshot_path": str((effective_workspace / "skill-snapshot").resolve()),
            "baseline_result_path": str((effective_workspace / "baseline-result.json").resolve()),
            "created_at": timestamp,
            "updated_at": timestamp,
        }
    )
    validate_session_payload(session)
    return session


def load_or_init_improvement_session(
    target: str | Path,
    workspace_path: Path | None = None,
    search_roots: list[Path] | None = None,
) -> dict[str, Any]:
    """Load an existing improvement session or create a new one."""
    skill_info = parse_target_skill(target, search_roots=search_roots)
    skill_path = Path(skill_info["target_skill_path"])
    effective_workspace = workspace_path or default_workspace_path(skill_path, skill_info["target_skill_name"])
    effective_workspace = effective_workspace.resolve()
    session_path = session_file_path(effective_workspace)

    if session_path.exists():
        session = read_json(session_path)
        validate_session_payload(session)
        return session

    session = build_improvement_session(skill_path, workspace_path=effective_workspace, search_roots=search_roots)
    write_json(session_path, session)
    return session


def update_session_status(session: dict[str, Any], next_status: str) -> dict[str, Any]:
    """Update session status with transition validation and timestamp refresh."""
    assert_transition(session["status"], next_status)
    session["status"] = next_status
    session["updated_at"] = utc_now_iso()
    return session
