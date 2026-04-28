#!/usr/bin/env python3
"""Quick validation helpers for skills."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import yaml

ALLOWED_PROPERTIES = {"name", "description", "license", "allowed-tools", "metadata", "compatibility"}
REFERENCE_PATTERNS = (
    re.compile(r"(?P<path>(?:references|scripts|assets|evals)/[^\s`\)\]\"']+)"),
    re.compile(r"\((?P<path>\.?\.?/(?:[^\)]+))\)"),
)
IGNORED_PATH_SEGMENTS = {"http:", "https:", "mailto:"}


def load_skill_frontmatter(skill_path: Path) -> tuple[dict, str]:
    """Load and validate the frontmatter block from SKILL.md."""
    skill_md = skill_path / "SKILL.md"
    if not skill_md.exists():
        raise ValueError("SKILL.md not found")

    content = skill_md.read_text(encoding="utf-8")
    if not content.startswith("---"):
        raise ValueError("No YAML frontmatter found")

    match = re.match(r"^---\n(.*?)\n---", content, re.DOTALL)
    if not match:
        raise ValueError("Invalid frontmatter format")

    frontmatter_text = match.group(1)
    try:
        frontmatter = yaml.safe_load(frontmatter_text)
    except yaml.YAMLError as exc:
        raise ValueError(f"Invalid YAML in frontmatter: {exc}") from exc

    if not isinstance(frontmatter, dict):
        raise ValueError("Frontmatter must be a YAML dictionary")

    return frontmatter, content


def validate_frontmatter(frontmatter: dict) -> None:
    """Validate frontmatter fields and value constraints."""
    unexpected_keys = set(frontmatter.keys()) - ALLOWED_PROPERTIES
    if unexpected_keys:
        allowed = ", ".join(sorted(ALLOWED_PROPERTIES))
        keys = ", ".join(sorted(unexpected_keys))
        raise ValueError(f"Unexpected key(s) in SKILL.md frontmatter: {keys}. Allowed properties are: {allowed}")

    if "name" not in frontmatter:
        raise ValueError("Missing 'name' in frontmatter")
    if "description" not in frontmatter:
        raise ValueError("Missing 'description' in frontmatter")

    name = frontmatter["name"]
    if not isinstance(name, str):
        raise ValueError(f"Name must be a string, got {type(name).__name__}")
    name = name.strip()
    if name:
        if not re.match(r"^[a-z0-9-]+$", name):
            raise ValueError(f"Name '{name}' should be kebab-case (lowercase letters, digits, and hyphens only)")
        if name.startswith("-") or name.endswith("-") or "--" in name:
            raise ValueError(f"Name '{name}' cannot start/end with hyphen or contain consecutive hyphens")
        if len(name) > 64:
            raise ValueError(f"Name is too long ({len(name)} characters). Maximum is 64 characters.")

    description = frontmatter["description"]
    if not isinstance(description, str):
        raise ValueError(f"Description must be a string, got {type(description).__name__}")
    description = description.strip()
    if description:
        if "<" in description or ">" in description:
            raise ValueError("Description cannot contain angle brackets (< or >)")
        if len(description) > 1024:
            raise ValueError(f"Description is too long ({len(description)} characters). Maximum is 1024 characters.")

    compatibility = frontmatter.get("compatibility")
    if compatibility is not None:
        if not isinstance(compatibility, str):
            raise ValueError(f"Compatibility must be a string, got {type(compatibility).__name__}")
        if len(compatibility) > 500:
            raise ValueError(f"Compatibility is too long ({len(compatibility)} characters). Maximum is 500 characters.")


def referenced_relative_paths(content: str) -> set[Path]:
    """Return relative file paths referenced by the skill content."""
    found: set[Path] = set()
    for pattern in REFERENCE_PATTERNS:
        for match in pattern.finditer(content):
            raw_path = match.group("path").strip()
            normalized = raw_path.replace("\\", "/")
            if any(normalized.startswith(prefix) for prefix in IGNORED_PATH_SEGMENTS):
                continue
            path = Path(normalized)
            if path.is_absolute():
                continue
            found.add(path)
    return found


def validate_iteration_structure(skill_path: Path, content: str) -> None:
    """Validate the minimal directory structure needed for improvement runs."""
    for rel_path in sorted(referenced_relative_paths(content)):
        if not (skill_path / rel_path).exists():
            raise ValueError(f"Referenced path not found: {rel_path.as_posix()}")

    evals_dir = skill_path / "evals"
    if evals_dir.exists() and not evals_dir.is_dir():
        raise ValueError("evals exists but is not a directory")


def validate_skill(skill_path: str | Path, *, require_iteration_ready: bool = False) -> tuple[bool, str]:
    """Validate a skill and return a `(success, message)` tuple."""
    path = Path(skill_path)
    try:
        frontmatter, content = load_skill_frontmatter(path)
        validate_frontmatter(frontmatter)
        if require_iteration_ready:
            validate_iteration_structure(path, content)
    except ValueError as exc:
        return False, str(exc)

    if require_iteration_ready:
        return True, "Skill is valid and iteration-ready!"
    return True, "Skill is valid!"


def main() -> None:
    parser = argparse.ArgumentParser(description="Quick validation for a skill directory")
    parser.add_argument("skill_directory", help="Path to the skill directory")
    parser.add_argument(
        "--require-iteration-ready",
        action="store_true",
        help="Also validate referenced resources and minimal improvement-time structure",
    )
    args = parser.parse_args()

    valid, message = validate_skill(args.skill_directory, require_iteration_ready=args.require_iteration_ready)
    print(message)
    sys.exit(0 if valid else 1)


if __name__ == "__main__":
    main()
