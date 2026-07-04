import json

import pytest

from masking_core.models import Rule, RuleProfile
from masking_core.profile_io import ProfileLoadError, load_profile, save_profile


def test_load_profile_valid_json_returns_rule_profile(tmp_path):
    data = {
        "profile_name": "test",
        "description": "a test profile",
        "rules": [
            {
                "name": "phone",
                "pattern_type": "regex",
                "pattern": r"\d{3}-\d{4}",
                "mode": "random",
                "prefix": "__MASK_PHONE_",
            }
        ],
    }
    path = tmp_path / "profile.json"
    path.write_text(json.dumps(data), encoding="utf-8")

    profile = load_profile(path)

    assert isinstance(profile, RuleProfile)
    assert profile.profile_name == "test"
    assert len(profile.rules) == 1
    assert profile.rules[0].name == "phone"


def test_load_profile_missing_file_raises_profile_load_error(tmp_path):
    with pytest.raises(ProfileLoadError):
        load_profile(tmp_path / "does_not_exist.json")


def test_load_profile_malformed_json_raises_profile_load_error(tmp_path):
    path = tmp_path / "bad.json"
    path.write_text("{not valid json", encoding="utf-8")
    with pytest.raises(ProfileLoadError, match="Invalid JSON"):
        load_profile(path)


def test_load_profile_schema_violation_raises_profile_load_error(tmp_path):
    data = {
        "profile_name": "test",
        "rules": [
            {
                "name": "phone",
                "pattern_type": "regex",
                "pattern": r"\d{3}-\d{4}",
                "mode": "fixed",
                # missing fixed_value -- violates Rule's model_validator
            }
        ],
    }
    path = tmp_path / "invalid_schema.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    with pytest.raises(ProfileLoadError):
        load_profile(path)


def test_save_profile_round_trip(tmp_path):
    rule = Rule(
        name="phone",
        pattern_type="regex",
        pattern=r"\d{3}-\d{4}",
        mode="random",
        prefix="__MASK_PHONE_",
    )
    profile = RuleProfile(profile_name="roundtrip", rules=[rule])
    path = tmp_path / "out.json"

    save_profile(profile, path)
    loaded = load_profile(path)

    assert loaded == profile


def test_save_profile_excludes_none_fields(tmp_path):
    rule = Rule(
        name="phone",
        pattern_type="regex",
        pattern=r"\d{3}-\d{4}",
        mode="random",
        prefix="__MASK_PHONE_",
    )
    profile = RuleProfile(profile_name="p", rules=[rule])
    path = tmp_path / "out.json"

    save_profile(profile, path)
    raw = json.loads(path.read_text(encoding="utf-8"))

    assert "fixed_value" not in raw["rules"][0]


def test_load_and_save_real_shipped_profiles():
    # Regression test: the actual profiles shipped in rules/ must always
    # be loadable through the same schema used elsewhere.
    from pathlib import Path

    rules_dir = Path(__file__).resolve().parents[1] / "rules"
    general = load_profile(rules_dir / "general.json")
    sip = load_profile(rules_dir / "sip.json")

    assert general.profile_name == "general"
    assert sip.profile_name == "sip"
    assert len(general.rules) > 0
    assert len(sip.rules) > 0
