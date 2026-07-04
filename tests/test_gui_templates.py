"""Tests for gui/settings.py's PROFILE_TEMPLATES.

Only imports gui.settings (pure data, no tkinter dependency) plus
masking_core -- no Tk widgets are constructed, so this runs fine
without a display, unlike gui/app.py itself (which CLAUDE.md excludes
from automated testing).
"""

from gui.settings import PROFILE_TEMPLATES

from masking_core.masker import MappingStore, apply_profile
from masking_core.models import Rule, RuleProfile

from tests.fixtures.synthetic_logs import (
    FAKE_IP_1,
    FAKE_IP_2,
    FAKE_PHONE_1,
    FAKE_PHONE_2,
    FAKE_SIP_URI,
    SAMPLE_GENERAL_LOG,
    SAMPLE_SIP_LOG,
)


def _build_profile(template_key: str) -> RuleProfile:
    template = PROFILE_TEMPLATES[template_key]
    return RuleProfile(
        profile_name=template["profile_name"],
        description=template["description"],
        rules=[Rule(**rule_kwargs) for rule_kwargs in template["rules"]],
    )


def test_all_profile_templates_build_valid_rule_profiles():
    for key in PROFILE_TEMPLATES:
        profile = _build_profile(key)
        assert len(profile.rules) > 0


def test_general_template_masks_synthetic_log():
    profile = _build_profile("汎用 (general)")
    masked, _ = apply_profile(SAMPLE_GENERAL_LOG, profile, MappingStore())

    assert FAKE_PHONE_1 not in masked
    assert FAKE_IP_1 not in masked
    assert "hunter2_FAKE" not in masked
    assert "__MASK_PHONE_" in masked
    assert "__MASK_IP_" in masked
    assert "password=__MASK_REDACTED__" in masked
    # Unrelated log scaffolding must survive untouched.
    assert "INFO connection established" in masked


def test_sip_template_masks_synthetic_log():
    profile = _build_profile("SIP")
    masked, _ = apply_profile(SAMPLE_SIP_LOG, profile, MappingStore())

    assert FAKE_SIP_URI not in masked
    assert FAKE_PHONE_2 not in masked
    assert FAKE_IP_2 not in masked
    assert "__MASK_SIPURI_" in masked
    assert "Authorization: Digest __MASK_REDACTED__" in masked
    assert "__MASK_SIPHDR_" in masked
