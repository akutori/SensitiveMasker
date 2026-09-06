//! Rule/RuleProfile: 検証済み状態でしか構築できないマスキングルール定義。

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternType {
    Literal,
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Fixed,
    Sequential,
}

#[derive(Debug, Error)]
pub enum RuleError {
    #[error("ルール '{name}': mode='fixed' の場合は 'fixed_value' が必須です")]
    MissingFixedValue { name: String },
    #[error("ルール '{name}': mode='sequential' の場合は 'prefix' が必須です")]
    MissingPrefix { name: String },
    #[error("ルール '{name}': 'pattern' が正しい正規表現ではありません: {source}")]
    InvalidRegex {
        name: String,
        #[source]
        source: regex::Error,
    },
}

// deserialize時もRule::newを必ず経由させ、二重チェックを呼び出し側に持たせないための橋渡し用DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleDto {
    name: String,
    pattern_type: PatternType,
    pattern: String,
    mode: Mode,
    #[serde(default)]
    fixed_value: Option<String>,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    description: Option<String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RuleDto", into = "RuleDto")]
pub struct Rule {
    name: String,
    pattern_type: PatternType,
    pattern: String,
    mode: Mode,
    fixed_value: Option<String>,
    prefix: Option<String>,
    enabled: bool,
    description: Option<String>,
}

impl Rule {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        pattern_type: PatternType,
        pattern: impl Into<String>,
        mode: Mode,
        fixed_value: Option<String>,
        prefix: Option<String>,
        enabled: bool,
        description: Option<String>,
    ) -> Result<Self, RuleError> {
        let name = name.into();
        let pattern = pattern.into();

        if mode == Mode::Fixed && fixed_value.as_deref().unwrap_or("").is_empty() {
            return Err(RuleError::MissingFixedValue { name });
        }
        if mode == Mode::Sequential && prefix.as_deref().unwrap_or("").is_empty() {
            return Err(RuleError::MissingPrefix { name });
        }
        if pattern_type == PatternType::Regex {
            if let Err(source) = regex::Regex::new(&pattern) {
                return Err(RuleError::InvalidRegex { name, source });
            }
        }

        Ok(Self {
            name,
            pattern_type,
            pattern,
            mode,
            fixed_value,
            prefix,
            enabled,
            description,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn pattern_type(&self) -> PatternType {
        self.pattern_type
    }
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn fixed_value(&self) -> Option<&str> {
        self.fixed_value.as_deref()
    }
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

impl TryFrom<RuleDto> for Rule {
    type Error = RuleError;

    fn try_from(dto: RuleDto) -> Result<Self, Self::Error> {
        Rule::new(
            dto.name,
            dto.pattern_type,
            dto.pattern,
            dto.mode,
            dto.fixed_value,
            dto.prefix,
            dto.enabled,
            dto.description,
        )
    }
}

impl From<Rule> for RuleDto {
    fn from(rule: Rule) -> Self {
        RuleDto {
            name: rule.name,
            pattern_type: rule.pattern_type,
            pattern: rule.pattern,
            mode: rule.mode,
            fixed_value: rule.fixed_value,
            prefix: rule.prefix,
            enabled: rule.enabled,
            description: rule.description,
        }
    }
}

#[derive(Debug, Error)]
pub enum RuleProfileError {
    // MappingStoreはルール名をキーの一部に使うため、名前が重複すると異なるルールの
    // 置換値が誤って混線する。よって構築時点で重複を拒否する。
    #[error("プロファイル内でルール名が重複しています: '{name}'")]
    DuplicateRuleName { name: String },
}

// RuleProfile::newの検証(ルール名の重複拒否)をdeserialize経由でも必ず通すための橋渡し用DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleProfileDto {
    profile_name: String,
    description: Option<String>,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RuleProfileDto", into = "RuleProfileDto")]
pub struct RuleProfile {
    profile_name: String,
    description: Option<String>,
    rules: Vec<Rule>,
}

impl RuleProfile {
    pub fn new(
        profile_name: impl Into<String>,
        description: Option<String>,
        rules: Vec<Rule>,
    ) -> Result<Self, RuleProfileError> {
        let mut seen_names = std::collections::HashSet::new();
        for rule in &rules {
            if !seen_names.insert(rule.name()) {
                return Err(RuleProfileError::DuplicateRuleName {
                    name: rule.name().to_string(),
                });
            }
        }
        Ok(Self {
            profile_name: profile_name.into(),
            description,
            rules,
        })
    }

    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}

impl TryFrom<RuleProfileDto> for RuleProfile {
    type Error = RuleProfileError;

    fn try_from(dto: RuleProfileDto) -> Result<Self, Self::Error> {
        RuleProfile::new(dto.profile_name, dto.description, dto.rules)
    }
}

impl From<RuleProfile> for RuleProfileDto {
    fn from(profile: RuleProfile) -> Self {
        RuleProfileDto {
            profile_name: profile.profile_name,
            description: profile.description,
            rules: profile.rules,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_fixed_rule() -> Result<Rule, RuleError> {
        Rule::new(
            "password_kv",
            PatternType::Regex,
            r"(?i)password=\S+",
            Mode::Fixed,
            Some("password=__MASK_REDACTED__".to_string()),
            None,
            true,
            None,
        )
    }

    fn valid_sequential_rule() -> Result<Rule, RuleError> {
        Rule::new(
            "ipv4",
            PatternType::Regex,
            r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
            Mode::Sequential,
            None,
            Some("__MASK_IP_".to_string()),
            true,
            None,
        )
    }

    #[test]
    fn fixed_mode_without_fixed_value_is_rejected() {
        let err = Rule::new("r1", PatternType::Literal, "x", Mode::Fixed, None, None, true, None)
            .expect_err("fixed_value必須のはず");
        assert!(matches!(err, RuleError::MissingFixedValue { .. }));
    }

    #[test]
    fn fixed_mode_with_fixed_value_succeeds() {
        assert!(valid_fixed_rule().is_ok());
    }

    #[test]
    fn sequential_mode_without_prefix_is_rejected() {
        let err = Rule::new("r1", PatternType::Literal, "x", Mode::Sequential, None, None, true, None)
            .expect_err("prefix必須のはず");
        assert!(matches!(err, RuleError::MissingPrefix { .. }));
    }

    #[test]
    fn sequential_mode_with_prefix_succeeds() {
        assert!(valid_sequential_rule().is_ok());
    }

    #[test]
    fn regex_pattern_type_rejects_invalid_regex_syntax() {
        // Rustのregexクレートはlookaheadを未サポート。Pythonのreとの既知の差分。
        let err = Rule::new(
            "r1",
            PatternType::Regex,
            r"(?=lookahead_not_supported)",
            Mode::Fixed,
            Some("x".to_string()),
            None,
            true,
            None,
        )
        .expect_err("lookaheadは非対応のはず");
        assert!(matches!(err, RuleError::InvalidRegex { .. }));
    }

    #[test]
    fn literal_pattern_type_does_not_validate_pattern_as_regex() {
        // literalは正規表現として解釈しないので、不正な正規表現構文でも受理される。
        let rule = Rule::new(
            "r1",
            PatternType::Literal,
            "a(b",
            Mode::Fixed,
            Some("x".to_string()),
            None,
            true,
            None,
        );
        assert!(rule.is_ok());
    }

    #[test]
    fn deserialize_enforces_same_validation_as_constructor() {
        // 検証をすり抜けてデシリアライズだけで無効なRuleが作れないことを保証する回帰テスト。
        let json = r#"{"name":"r1","pattern_type":"literal","pattern":"x","mode":"fixed"}"#;
        let result: Result<Rule, _> = serde_json::from_str(json);
        assert!(result.is_err(), "fixed_value無しのfixedモードはdeserializeでも拒否されるべき");
    }

    #[test]
    fn deserialize_valid_json_round_trips() {
        let rule = valid_sequential_rule().unwrap();
        let json = serde_json::to_string(&rule).unwrap();
        let restored: Rule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, restored);
    }

    #[test]
    fn rule_profile_holds_rules_in_order() {
        let profile = RuleProfile::new(
            "general",
            Some("desc".to_string()),
            vec![valid_fixed_rule().unwrap(), valid_sequential_rule().unwrap()],
        )
        .unwrap();
        assert_eq!(profile.rules().len(), 2);
        assert_eq!(profile.rules()[0].name(), "password_kv");
        assert_eq!(profile.rules()[1].name(), "ipv4");
    }

    #[test]
    fn rule_profile_deserialize_enforces_unique_rule_names() {
        let json = r#"{
            "profile_name": "p",
            "rules": [
                {"name": "dup", "pattern_type": "literal", "pattern": "a", "mode": "fixed", "fixed_value": "X"},
                {"name": "dup", "pattern_type": "literal", "pattern": "b", "mode": "fixed", "fixed_value": "Y"}
            ]
        }"#;
        let result: Result<RuleProfile, _> = serde_json::from_str(json);
        assert!(result.is_err(), "ルール名の重複はdeserializeでも拒否されるべき");
    }
}
