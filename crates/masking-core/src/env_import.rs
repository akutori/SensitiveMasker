//! .env形式のテキストから、プロファイルルールの取り込み候補を抽出する。
//! ここでの「取り込み対象に含めるか」の既定値はあくまでヒューリスティックによる
//! 初期選択であり、最終判断は呼び出し側(GUI等)でユーザーに委ねる前提。

const MIN_SECRET_VALUE_LEN: usize = 4;
const NON_SECRET_KEY_DENYLIST: &[&str] = &["PORT", "DEBUG", "NODE_ENV", "LOG_LEVEL", "HOST", "TZ", "ENV"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvCandidate {
    pub key: String,
    pub value: String,
    pub included_by_default: bool,
}

pub fn parse_env_candidates(content: &str) -> Vec<EnvCandidate> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: std::collections::HashMap<String, EnvCandidate> = std::collections::HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = strip_export_prefix(key.trim()).to_string();
        if key.is_empty() {
            continue;
        }
        let raw_value = value.trim();
        // 引用符が開いたまま閉じていない場合、値が複数行にまたがっている
        // (例: PEM形式の秘密鍵)可能性がある。この関数は1行ずつしか見ないため、
        // そのようなケースでは実際の値の一部しか取れておらず、生成されるルールの
        // パターンが本物の値と一致しない(=マスクされない)。それに気付かず
        // 「取り込み済み」と誤解しないよう、既定選択から外す。
        let looks_truncated = has_unmatched_leading_quote(raw_value);
        let value = strip_matching_quotes(raw_value);
        let included_by_default = !looks_truncated && should_include_by_default(&key, &value);

        if !by_key.contains_key(&key) {
            order.push(key.clone());
        }
        // 同一キーが複数回現れた場合は.env/shellの一般的な挙動(後勝ち)に合わせる。
        by_key.insert(key.clone(), EnvCandidate { key, value, included_by_default });
    }

    order.into_iter().map(|key| by_key.remove(&key).expect("orderに積んだキーはby_keyに必ず存在する")).collect()
}

// `export FOO=bar`形式(シェルでsourceする前提の.envで使われる)のキーから
// "export "を取り除く。"export"というキー自体(区切りの空白が無い場合)は
// そのまま保持する。
fn strip_export_prefix(key: &str) -> &str {
    key.strip_prefix("export ").map(str::trim_start).unwrap_or(key)
}

fn has_unmatched_leading_quote(raw_value: &str) -> bool {
    match raw_value.as_bytes().first() {
        Some(&first) if first == b'"' || first == b'\'' => {
            let bytes = raw_value.as_bytes();
            !(bytes.len() >= 2 && bytes[bytes.len() - 1] == first)
        }
        _ => false,
    }
}

fn strip_matching_quotes(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn should_include_by_default(key: &str, value: &str) -> bool {
    if value.chars().count() < MIN_SECRET_VALUE_LEN {
        return false;
    }
    !NON_SECRET_KEY_DENYLIST.iter().any(|denied| denied.eq_ignore_ascii_case(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_key_value_line() {
        let result = parse_env_candidates("FOO=bar1");
        assert_eq!(result, vec![EnvCandidate { key: "FOO".into(), value: "bar1".into(), included_by_default: true }]);
    }

    #[test]
    fn skips_a_comment_line() {
        let result = parse_env_candidates("# comment\nFOO=bar1");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "FOO");
    }

    #[test]
    fn skips_a_blank_line() {
        let result = parse_env_candidates("\n\nFOO=bar1\n\n");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn skips_a_line_without_an_equals_sign() {
        let result = parse_env_candidates("JUSTAKEY\nFOO=bar1");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "FOO");
    }

    #[test]
    fn skips_a_line_with_an_empty_key() {
        let result = parse_env_candidates("=noKey\nFOO=bar1");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "FOO");
    }

    #[test]
    fn strips_surrounding_double_quotes() {
        let result = parse_env_candidates(r#"FOO="bar1""#);
        assert_eq!(result[0].value, "bar1");
    }

    #[test]
    fn strips_surrounding_single_quotes() {
        let result = parse_env_candidates("FOO='bar1'");
        assert_eq!(result[0].value, "bar1");
    }

    #[test]
    fn does_not_strip_a_quote_that_only_appears_on_one_side() {
        let result = parse_env_candidates(r#"FOO="bar1"#);
        assert_eq!(result[0].value, "\"bar1");
    }

    #[test]
    fn splits_only_on_the_first_equals_sign() {
        let result = parse_env_candidates("DATABASE_URL=postgres://x?sslmode=require");
        assert_eq!(result[0].value, "postgres://x?sslmode=require");
    }

    #[test]
    fn trims_whitespace_around_the_equals_sign() {
        let result = parse_env_candidates("FOO = bar1");
        assert_eq!(result[0].key, "FOO");
        assert_eq!(result[0].value, "bar1");
    }

    #[test]
    fn a_later_duplicate_key_overrides_the_earlier_value_but_keeps_its_original_position() {
        let result = parse_env_candidates("FOO=first111\nBAR=second11\nFOO=third111");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].key, "FOO");
        assert_eq!(result[0].value, "third111");
        assert_eq!(result[1].key, "BAR");
    }

    #[test]
    fn a_value_shorter_than_the_minimum_length_is_excluded_by_default() {
        let result = parse_env_candidates("SHORT=abc");
        assert!(!result[0].included_by_default);
    }

    #[test]
    fn a_value_at_the_minimum_length_is_included_by_default() {
        let result = parse_env_candidates("OK=abcd");
        assert!(result[0].included_by_default);
    }

    #[test]
    fn an_empty_value_is_excluded_by_default() {
        let result = parse_env_candidates("EMPTY=");
        assert!(!result[0].included_by_default);
    }

    #[test]
    fn a_denylisted_key_is_excluded_by_default_even_with_a_long_value() {
        let result = parse_env_candidates("PORT=1234567890");
        assert!(!result[0].included_by_default);
    }

    #[test]
    fn a_key_not_in_the_denylist_with_a_long_value_is_included_by_default() {
        let result = parse_env_candidates("API_KEY=sk_live_1234567890");
        assert!(result[0].included_by_default);
    }

    #[test]
    fn a_denylisted_key_is_matched_case_insensitively() {
        let result = parse_env_candidates("port=1234567890");
        assert!(!result[0].included_by_default);
    }

    #[test]
    fn strips_a_leading_export_prefix_from_the_key() {
        let result = parse_env_candidates("export FOO=bar1");
        assert_eq!(result[0].key, "FOO");
    }

    #[test]
    fn does_not_strip_export_when_it_is_the_entire_key_name() {
        let result = parse_env_candidates("export=bar1");
        assert_eq!(result[0].key, "export");
    }

    #[test]
    fn does_not_strip_a_key_that_merely_starts_with_the_word_export() {
        let result = parse_env_candidates("EXPORT_PATH=bar1");
        assert_eq!(result[0].key, "EXPORT_PATH");
    }

    // PEM形式の秘密鍵等、値が複数行にまたがる.envは1行ずつのパースでは正しく
    // 取り込めない。取り込めたつもりで実際にはマスクされない、という事態を防ぐため
    // 既定選択から外す(パースを諦めて除外するのではなく、除外した上でキー自体は
    // 一覧に残す。手動で選んで気付けるようにするため)。
    #[test]
    fn a_value_with_an_unmatched_leading_quote_is_excluded_by_default_even_when_long_enough() {
        let result = parse_env_candidates(r#"FOO="0123456789"#);
        assert!(!result[0].included_by_default);
    }

    #[test]
    fn a_value_with_a_properly_matched_quote_is_still_included_by_default() {
        let result = parse_env_candidates(r#"FOO="0123456789""#);
        assert!(result[0].included_by_default);
    }
}
