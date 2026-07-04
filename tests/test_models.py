import pytest
from pydantic import ValidationError

from masking_core.models import Rule, RuleProfile


def test_rule_fixed_mode_requires_fixed_value():
    with pytest.raises(ValidationError):
        Rule(name="r1", pattern_type="literal", pattern="x", mode="fixed")


def test_rule_fixed_mode_with_fixed_value_succeeds():
    rule = Rule(
        name="r1",
        pattern_type="literal",
        pattern="x",
        mode="fixed",
        fixed_value="__MASK__",
    )
    assert rule.fixed_value == "__MASK__"
    assert rule.prefix is None


def test_rule_random_mode_requires_prefix():
    with pytest.raises(ValidationError):
        Rule(name="r1", pattern_type="regex", pattern=r"\d+", mode="random")


def test_rule_random_mode_with_prefix_succeeds():
    rule = Rule(
        name="r1",
        pattern_type="regex",
        pattern=r"\d+",
        mode="random",
        prefix="__MASK_X_",
    )
    assert rule.prefix == "__MASK_X_"
    assert rule.fixed_value is None


def test_rule_fixed_mode_rejects_empty_string_fixed_value():
    with pytest.raises(ValidationError):
        Rule(name="r1", pattern_type="literal", pattern="x", mode="fixed", fixed_value="")


def test_rule_random_mode_rejects_empty_string_prefix():
    with pytest.raises(ValidationError):
        Rule(name="r1", pattern_type="regex", pattern=r"\d+", mode="random", prefix="")


def test_rule_invalid_mode_literal_rejected():
    with pytest.raises(ValidationError):
        Rule(
            name="r1",
            pattern_type="literal",
            pattern="x",
            mode="bogus",
            fixed_value="v",
        )


def test_rule_invalid_pattern_type_rejected():
    with pytest.raises(ValidationError):
        Rule(
            name="r1",
            pattern_type="fuzzy",
            pattern="x",
            mode="fixed",
            fixed_value="v",
        )


def test_rule_enabled_defaults_true():
    rule = Rule(
        name="r1",
        pattern_type="literal",
        pattern="x",
        mode="fixed",
        fixed_value="v",
    )
    assert rule.enabled is True


def test_rule_enabled_can_be_set_false():
    rule = Rule(
        name="r1",
        pattern_type="literal",
        pattern="x",
        mode="fixed",
        fixed_value="v",
        enabled=False,
    )
    assert rule.enabled is False


def test_rule_profile_holds_multiple_rules_in_order():
    rule_a = Rule(name="a", pattern_type="literal", pattern="x", mode="fixed", fixed_value="v")
    rule_b = Rule(name="b", pattern_type="regex", pattern=r"\d+", mode="random", prefix="__P_")
    profile = RuleProfile(profile_name="test", rules=[rule_a, rule_b])
    assert [r.name for r in profile.rules] == ["a", "b"]


def test_rule_profile_defaults_to_empty_rules_list():
    profile = RuleProfile(profile_name="empty")
    assert profile.rules == []
