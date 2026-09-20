//! Rule/RuleProfile: 検証済み状態でしか構築できないマスキングルール定義。

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroize;

/// 正規表現1件あたりのコンパイル後サイズ上限。regexクレートの既定(10MiB)より厳しくし、
/// 大量のルールを持つプロファイルでのコンパイルコスト積み上げを抑える。
const REGEX_SIZE_LIMIT_BYTES: usize = 1 << 20; // 1MiB

/// プロファイル名・ルール名・タグ名に共通の長さ上限(文字数)。
pub const MAX_DISPLAY_NAME_LENGTH: usize = 100;

/// 制御文字(改行・タブ等)およびUnicode双方向書式文字(RLO等)を拒否する。後者は
/// 確認画面上でテキストの表示順を偽装でき、インポート確認ダイアログのような
/// 「内容を見て判断する」UIの前提を崩すため(インポートしたプロファイル名・ルール名・
/// タグ名を対象にした監査指摘への対応)。
fn find_disallowed_char(value: &str) -> Option<char> {
    value.chars().find(|c| {
        c.is_control()
            || matches!(c,
                '\u{061C}' // ALM
                | '\u{200E}' | '\u{200F}' // LRM, RLM
                | '\u{202A}'..='\u{202E}' // LRE, RLE, PDF, LRO, RLO
                | '\u{2066}'..='\u{2069}' // LRI, RLI, FSI, PDI
            )
    })
}

/// プロファイル名・ルール名・タグ名に共通の検証(呼び出し側で各エラー型に包む)。
pub fn validate_display_name(value: &str) -> Result<(), String> {
    let len = value.chars().count();
    if len > MAX_DISPLAY_NAME_LENGTH {
        return Err(format!("長すぎます({len}文字、上限{MAX_DISPLAY_NAME_LENGTH}文字)"));
    }
    if let Some(c) = find_disallowed_char(value) {
        return Err(format!("使用できない文字が含まれています(U+{:04X})", c as u32));
    }
    Ok(())
}

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
    #[error("ルール名 '{name}' が不正です: {reason}")]
    InvalidName { name: String, reason: String },
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

/// ルールの文字列(名前・パターン・固定値・接頭辞・説明)を、メモリ上で消去する。パターンや固定値には、マスク対象の
/// 実際の値(環境変数の値など)が入りうるため、保留していた内容を捨てるときに使う。消去した後のルールは、名前が空で、
/// 検証を通らない状態なので、使わずに捨てる。
impl Zeroize for Rule {
    fn zeroize(&mut self) {
        self.name.zeroize();
        self.pattern.zeroize();
        self.fixed_value.zeroize();
        self.prefix.zeroize();
        self.description.zeroize();
    }
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

        if let Err(reason) = validate_display_name(&name) {
            return Err(RuleError::InvalidName { name, reason });
        }
        if mode == Mode::Fixed && fixed_value.as_deref().unwrap_or("").is_empty() {
            return Err(RuleError::MissingFixedValue { name });
        }
        if mode == Mode::Sequential && prefix.as_deref().unwrap_or("").is_empty() {
            return Err(RuleError::MissingPrefix { name });
        }
        if pattern_type == PatternType::Regex {
            // 既定のsize_limit(10MiB)のままだと、大量のルールを持つプロファイルを
            // インポート/作成された場合にコンパイルコストが積み上がりうる。
            // 実用上のマスクルール(電話番号・IP等)は数KB程度で収まるため実害は無い。
            if let Err(source) = regex::RegexBuilder::new(&pattern).size_limit(REGEX_SIZE_LIMIT_BYTES).build() {
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
    #[error("プロファイル名が不正です: {reason}")]
    InvalidProfileName { reason: String },
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

/// プロファイルの文字列(名前・説明)と、全てのルールの内容を、メモリ上で消去する。消去した後のプロファイルは、
/// 名前が空で、検証を通らない状態なので、使わずに捨てる。
impl Zeroize for RuleProfile {
    fn zeroize(&mut self) {
        self.profile_name.zeroize();
        self.description.zeroize();
        self.rules.zeroize();
    }
}

impl RuleProfile {
    pub fn new(
        profile_name: impl Into<String>,
        description: Option<String>,
        rules: Vec<Rule>,
    ) -> Result<Self, RuleProfileError> {
        let profile_name = profile_name.into();
        if let Err(reason) = validate_display_name(&profile_name) {
            return Err(RuleProfileError::InvalidProfileName { reason });
        }

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

    #[test]
    fn zeroize_clears_every_text_field_of_a_fixed_rule() {
        let mut rule = Rule::new(
            "secret_rule_name",
            PatternType::Literal,
            "secret-literal-value",
            Mode::Fixed,
            Some("secret-fixed-value".to_string()),
            None,
            true,
            Some("secret description".to_string()),
        )
        .unwrap();

        rule.zeroize();

        assert_eq!(rule.name(), "");
        assert_eq!(rule.pattern(), "");
        assert_eq!(rule.fixed_value(), None);
        assert_eq!(rule.description(), None);
    }

    #[test]
    fn zeroize_clears_the_prefix_of_a_sequential_rule() {
        let mut rule = Rule::new(
            "secret_rule_name",
            PatternType::Literal,
            "secret-literal-value",
            Mode::Sequential,
            None,
            Some("__SECRET_PREFIX_".to_string()),
            true,
            None,
        )
        .unwrap();

        rule.zeroize();

        assert_eq!(rule.prefix(), None);
        assert_eq!(rule.pattern(), "");
    }

    #[test]
    fn zeroize_clears_a_profile_and_every_rule_in_it() {
        let mut profile = RuleProfile::new(
            "secret-profile-name",
            Some("secret description".to_string()),
            vec![valid_fixed_rule().unwrap(), valid_sequential_rule().unwrap()],
        )
        .unwrap();

        profile.zeroize();

        assert_eq!(profile.profile_name(), "");
        assert_eq!(profile.description(), None);
        assert!(profile.rules().is_empty());
    }

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
    fn rule_name_exceeding_the_length_limit_is_rejected() {
        let too_long = "a".repeat(MAX_DISPLAY_NAME_LENGTH + 1);
        let err = Rule::new(too_long, PatternType::Literal, "x", Mode::Fixed, Some("x".to_string()), None, true, None)
            .expect_err("長さ上限超過は拒否されるはず");
        assert!(matches!(err, RuleError::InvalidName { .. }));
    }

    #[test]
    fn rule_name_at_the_length_limit_is_accepted() {
        let at_limit = "a".repeat(MAX_DISPLAY_NAME_LENGTH);
        let rule =
            Rule::new(at_limit, PatternType::Literal, "x", Mode::Fixed, Some("x".to_string()), None, true, None);
        assert!(rule.is_ok());
    }

    #[test]
    fn rule_name_containing_a_control_character_is_rejected() {
        let err = Rule::new(
            "r1\u{0007}",
            PatternType::Literal,
            "x",
            Mode::Fixed,
            Some("x".to_string()),
            None,
            true,
            None,
        )
        .expect_err("制御文字を含む名前は拒否されるはず");
        assert!(matches!(err, RuleError::InvalidName { .. }));
    }

    #[test]
    fn rule_name_containing_a_bidi_override_character_is_rejected() {
        // U+202E (RIGHT-TO-LEFT OVERRIDE): 確認画面での表示順を偽装しうる。
        let err = Rule::new(
            "r1\u{202E}",
            PatternType::Literal,
            "x",
            Mode::Fixed,
            Some("x".to_string()),
            None,
            true,
            None,
        )
        .expect_err("双方向書式文字を含む名前は拒否されるはず");
        assert!(matches!(err, RuleError::InvalidName { .. }));
    }

    #[test]
    fn regex_pattern_type_rejects_patterns_that_compile_to_an_excessive_size() {
        // ネストした繰り返しでコンパイル後サイズを膨張させる。"(?:a{200}){200}"は
        // regexクレートの既定size_limit(10MiB)なら受理されるが、このプロジェクトが
        // 課す1MiB制限では拒否されることを確認済みのパターン。
        let err = Rule::new(
            "r1",
            PatternType::Regex,
            "(?:a{200}){200}",
            Mode::Fixed,
            Some("x".to_string()),
            None,
            true,
            None,
        )
        .expect_err("既定より厳しいsize_limitにより拒否されるはず");
        assert!(matches!(err, RuleError::InvalidRegex { .. }));
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

    #[test]
    fn profile_name_exceeding_the_length_limit_is_rejected() {
        let too_long = "a".repeat(MAX_DISPLAY_NAME_LENGTH + 1);
        let err = RuleProfile::new(too_long, None, vec![]).expect_err("長さ上限超過は拒否されるはず");
        assert!(matches!(err, RuleProfileError::InvalidProfileName { .. }));
    }

    #[test]
    fn profile_name_containing_a_bidi_override_character_is_rejected() {
        let err = RuleProfile::new("p\u{202E}", None, vec![]).expect_err("双方向書式文字を含む名前は拒否されるはず");
        assert!(matches!(err, RuleProfileError::InvalidProfileName { .. }));
    }
}
