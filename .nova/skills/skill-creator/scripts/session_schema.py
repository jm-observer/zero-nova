"""Schema helpers for skill improvement sessions."""

from __future__ import annotations

from copy import deepcopy
from datetime import datetime, timezone
from pathlib import Path

SESSION_FILENAME = "improvement-session.json"
SESSION_TEMPLATE_FILENAME = "improvement_session_template.json"
SESSION_STATUSES = (
    "initialized",
    "evaluating",
    "optimizing",
    "paused",
    "completed",
    "failed",
)

ALLOWED_STATUS_TRANSITIONS: dict[str, tuple[str, ...]] = {
    "initialized": ("evaluating", "failed"),
    "evaluating": ("optimizing", "paused", "failed"),
    "optimizing": ("optimizing", "paused", "completed", "failed"),
    "paused": ("evaluating", "failed"),
    "completed": (),
    "failed": (),
}

REQUIRED_SESSION_FIELDS = (
    "session_id",
    "target_skill_path",
    "target_skill_name",
    "workspace_path",
    "snapshot_path",
    "status",
    "baseline_result_path",
    "best_iteration",
    "best_score",
    "iterations",
    "created_at",
    "updated_at",
)


def utc_now_iso() -> str:
    """Return the current UTC timestamp in a stable ISO 8601 format."""
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def session_template_path() -> Path:
    """Return the bundled session template path."""
    return Path(__file__).resolve().parent.parent / "assets" / SESSION_TEMPLATE_FILENAME


def load_session_template() -> dict:
    """Load the bundled session template JSON."""
    import json

    template = json.loads(session_template_path().read_text(encoding="utf-8"))
    return deepcopy(template)


def is_valid_status(status: str) -> bool:
    """Return whether the given status is recognized."""
    return status in SESSION_STATUSES


def can_transition(current_status: str, next_status: str) -> bool:
    """Return whether a session may move from current_status to next_status."""
    if current_status == next_status:
        return True
    return next_status in ALLOWED_STATUS_TRANSITIONS.get(current_status, ())


def validate_session_payload(session: dict) -> None:
    """Validate a session payload and raise ValueError on schema issues."""
    missing = [field for field in REQUIRED_SESSION_FIELDS if field not in session]
    if missing:
        raise ValueError(f"Session missing required fields: {', '.join(missing)}")

    status = session["status"]
    if not is_valid_status(status):
        raise ValueError(f"Invalid session status: {status}")

    if not isinstance(session["iterations"], list):
        raise ValueError("Session field 'iterations' must be a list")


def assert_transition(current_status: str, next_status: str) -> None:
    """Raise ValueError when a transition is not allowed."""
    if not can_transition(current_status, next_status):
        raise ValueError(f"Invalid session transition: {current_status} -> {next_status}")
