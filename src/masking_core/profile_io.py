from __future__ import annotations

import json
from pathlib import Path

from pydantic import ValidationError

from masking_core.models import RuleProfile


class ProfileLoadError(Exception):
    """Raised when a profile file cannot be read or does not match the schema."""


def load_profile(path: str | Path) -> RuleProfile:
    """Load and validate a RuleProfile from a JSON file.

    Raises ProfileLoadError on: file not found/unreadable, malformed JSON,
    or JSON that doesn't match the RuleProfile schema.
    """
    p = Path(path)
    try:
        raw = p.read_text(encoding="utf-8")
    except OSError as exc:
        raise ProfileLoadError(f"プロファイルファイル '{p}' を読み込めません: {exc}") from exc

    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ProfileLoadError(f"プロファイルファイル '{p}' のJSONが不正です: {exc}") from exc

    try:
        return RuleProfile.model_validate(data)
    except ValidationError as exc:
        raise ProfileLoadError(
            f"プロファイル '{p}' のスキーマ検証に失敗しました:\n{exc}"
        ) from exc


def save_profile(profile: RuleProfile, path: str | Path) -> None:
    """Serialize a RuleProfile to JSON and write it to `path`."""
    p = Path(path)
    try:
        p.write_text(
            profile.model_dump_json(indent=2, exclude_none=True),
            encoding="utf-8",
        )
    except OSError as exc:
        raise ProfileLoadError(f"プロファイルファイル '{p}' を書き込めません: {exc}") from exc
