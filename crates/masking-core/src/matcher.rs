//! literal/regexパターンのマッチング。Ruleが検証済みという不変条件に依存する。

use crate::models::{PatternType, Rule};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchSpan {
    pub start: usize,
    pub end: usize,
    pub matched_text: String,
    pub rule_name: String,
}

pub fn compile_rule_pattern(rule: &Rule) -> regex::Regex {
    match rule.pattern_type() {
        // re::escapeなので特殊文字を含むliteralパターンも常にコンパイルに成功する。
        PatternType::Literal => regex::Regex::new(&regex::escape(rule.pattern()))
            .expect("escaped literal pattern is always a valid regex"),
        // Rule::new/deserialize時点でコンパイル済みであることが保証されている(不変条件)。
        PatternType::Regex => {
            regex::Regex::new(rule.pattern()).expect("validated at Rule construction time")
        }
    }
}

/// `text`中の`rule`にマッチする非重複区間を左から順に返す。
pub fn find_matches(text: &str, rule: &Rule) -> Vec<MatchSpan> {
    if !rule.enabled() {
        return Vec::new();
    }
    let compiled = compile_rule_pattern(rule);
    find_matches_compiled(text, &compiled, rule.name())
}

/// `find_matches`と同じロジックだが、あらかじめコンパイル済みの`Regex`を受け取る。
/// `CompiledProfile`にキャッシュされた正規表現を再コンパイルせずに使い回すための内部API。
pub(crate) fn find_matches_compiled(text: &str, compiled: &regex::Regex, rule_name: &str) -> Vec<MatchSpan> {
    compiled
        .find_iter(text)
        // ゼロ幅マッチ(空のliteralパターン、`a*`等)は「何も保護しない置換」になり、
        // 後続の全ルールを無力化する実害があるため対象から除外する。
        .filter(|m| m.start() != m.end())
        .map(|m| MatchSpan {
            start: m.start(),
            end: m.end(),
            matched_text: m.as_str().to_string(),
            rule_name: rule_name.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Mode;

    fn literal_rule(pattern: &str) -> Rule {
        Rule::new("r", PatternType::Literal, pattern, Mode::Fixed, Some("X".into()), None, true, None).unwrap()
    }

    fn regex_rule(pattern: &str) -> Rule {
        Rule::new("r", PatternType::Regex, pattern, Mode::Fixed, Some("X".into()), None, true, None).unwrap()
    }

    #[test]
    fn literal_matches_regex_special_characters_verbatim() {
        let rule = literal_rule("a.b(c)");
        let matches = find_matches("prefix a.b(c) suffix", &rule);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "a.b(c)");
    }

    #[test]
    fn literal_does_not_match_when_only_the_regex_interpretation_would() {
        // "a.b(c)"をregexとして解釈すればマッチする文字列だが、literalなのでマッチしない否定テスト。
        let rule = literal_rule("a.b(c)");
        let matches = find_matches("aXbc suffix", &rule);
        assert!(matches.is_empty());
    }

    #[test]
    fn disabled_rule_returns_no_matches() {
        let rule = Rule::new("r", PatternType::Literal, "secret", Mode::Fixed, Some("X".into()), None, false, None)
            .unwrap();
        let matches = find_matches("this contains secret data", &rule);
        assert!(matches.is_empty(), "無効化されたルールはマッチしないはず");
    }

    #[test]
    fn regex_returns_non_overlapping_left_to_right_matches() {
        let rule = regex_rule(r"\d+");
        let matches = find_matches("a1 b22 c333", &rule);
        let texts: Vec<_> = matches.iter().map(|m| m.matched_text.as_str()).collect();
        assert_eq!(texts, vec!["1", "22", "333"]);
    }

    #[test]
    fn zero_width_regex_match_is_excluded_instead_of_matching_every_position() {
        // "x*"はxが無いテキストにも各位置でゼロ幅マッチしうるが、そのようなマッチは
        // 何も保護しないまま後続ルールを無力化する実害があるため除外されるべき。
        let rule = regex_rule(r"x*");
        let matches = find_matches("abc", &rule);
        assert!(matches.is_empty(), "ゼロ幅マッチは除外されるはず");
    }

    #[test]
    fn empty_literal_pattern_produces_no_matches() {
        let rule = literal_rule("");
        let matches = find_matches("abc", &rule);
        assert!(matches.is_empty(), "空文字列literalはゼロ幅マッチなので除外されるはず");
    }

    #[test]
    fn zero_width_capable_rule_does_not_disable_a_later_rule() {
        // ゼロ幅マッチ可能なルールが紛れ込んでも、後続ルールの検出対象を破壊しない。
        use crate::masker::{apply_profile, MappingStore};
        use crate::models::{Mode, RuleProfile};

        let decoy = Rule::new("decoy", PatternType::Regex, "a*", Mode::Fixed, Some("X".into()), None, true, None)
            .unwrap();
        let phone = Rule::new(
            "phone",
            PatternType::Regex,
            r"\d{2,4}-\d{2,4}-\d{3,4}",
            Mode::Sequential,
            None,
            Some("__MASK_PHONE_".into()),
            true,
            None,
        )
        .unwrap();
        let profile = RuleProfile::new("p", None, vec![decoy, phone]).unwrap();
        let mut store = MappingStore::new();

        let (masked, counts) = apply_profile("電話番号は090-1234-5678です", &profile, &mut store);

        assert_eq!(masked, "電話番号は__MASK_PHONE_1__です");
        assert_eq!(counts[1].count, 1);
    }

    #[test]
    fn multibyte_japanese_text_uses_consistent_byte_offsets() {
        let rule = regex_rule(r"\d{2,4}-\d{2,4}-\d{3,4}");
        let text = "電話番号は090-1234-5678です";
        let matches = find_matches(text, &rule);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "090-1234-5678");
        assert_eq!(&text[matches[0].start..matches[0].end], "090-1234-5678");
    }
}
