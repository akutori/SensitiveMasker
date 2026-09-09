use std::collections::HashMap;
use std::sync::Mutex;

use masking_core::{apply_profile, MappingStore, RuleProfile};
use serde::Serialize;

#[derive(Default)]
pub struct MaskingState(Mutex<HashMap<String, MappingStore>>);

#[derive(Serialize, Debug, PartialEq)]
pub struct MatchCountDto {
    rule_name: String,
    count: usize,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct MaskTextResult {
    text: String,
    match_counts: Vec<MatchCountDto>,
}

// MappingStoreはプロファイルごとに呼び出しをまたいで保持する(連番モードの採番を
// プロファイル単位で一貫させるため)。プロファイル切り替え時はprofile_idが変わり
// 自動的に別のMappingStoreになる。tauri::Stateに依存しない形にして単体テスト可能にする。
fn mask_text_with_stores(
    stores: &mut HashMap<String, MappingStore>,
    profile_id: String,
    profile: &RuleProfile,
    text: &str,
) -> MaskTextResult {
    let store = stores.entry(profile_id).or_default();
    let (masked_text, match_counts) = apply_profile(text, profile, store);
    MaskTextResult {
        text: masked_text,
        match_counts: match_counts
            .into_iter()
            .map(|m| MatchCountDto {
                rule_name: m.rule_name,
                count: m.count,
            })
            .collect(),
    }
}

// 大きなテキストの処理でUIスレッドをブロックしないよう非同期コマンドにする
// (Tauriは非asyncコマンドをメインスレッドで実行するため)。
#[tauri::command]
pub async fn mask_text(
    state: tauri::State<'_, MaskingState>,
    profile_id: String,
    profile: RuleProfile,
    text: String,
) -> Result<MaskTextResult, ()> {
    // poison時も後続の呼び出しを永久に失敗させないよう、中身を取り出して継続する
    // (release buildのpanic=abortではpanic自体がプロセスごと終了するため、この回復が
    // 意味を持つのはdebug build時のみ)。
    let mut stores = state.0.lock().unwrap_or_else(|e| e.into_inner());
    Ok(mask_text_with_stores(&mut stores, profile_id, &profile, &text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use masking_core::{Mode, PatternType, Rule};

    fn sequential_rule(name: &str, pattern: &str, prefix: &str) -> Rule {
        Rule::new(
            name,
            PatternType::Regex,
            pattern,
            Mode::Sequential,
            None,
            Some(prefix.to_string()),
            true,
            None,
        )
        .unwrap()
    }

    #[test]
    fn masks_text_and_reports_match_counts() {
        let mut stores = HashMap::new();
        let rule = sequential_rule("ip", r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}", "__MASK_IP_");
        let profile = RuleProfile::new("test", None, vec![rule]).unwrap();

        let result = mask_text_with_stores(
            &mut stores,
            "profile-1".to_string(),
            &profile,
            "from 10.0.0.1 to 10.0.0.2",
        );

        assert_eq!(result.text, "from __MASK_IP_1__ to __MASK_IP_2__");
        assert_eq!(
            result.match_counts,
            vec![MatchCountDto {
                rule_name: "ip".to_string(),
                count: 2,
            }]
        );
    }

    #[test]
    fn same_profile_id_keeps_numbering_consistent_across_calls() {
        let mut stores = HashMap::new();
        let rule = sequential_rule("ip", r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}", "__MASK_IP_");
        let profile = RuleProfile::new("test", None, vec![rule]).unwrap();

        let first = mask_text_with_stores(&mut stores, "profile-1".to_string(), &profile, "10.0.0.1");
        assert_eq!(first.text, "__MASK_IP_1__");

        // 同じ値をもう一度マスクすると、同じ番号が再利用される(新規のカウントアップはしない)。
        let second =
            mask_text_with_stores(&mut stores, "profile-1".to_string(), &profile, "10.0.0.1 10.0.0.9");
        assert_eq!(second.text, "__MASK_IP_1__ __MASK_IP_2__");
    }

    #[test]
    fn different_profile_id_gets_independent_numbering() {
        let mut stores = HashMap::new();
        let rule = sequential_rule("ip", r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}", "__MASK_IP_");
        let profile = RuleProfile::new("test", None, vec![rule]).unwrap();

        mask_text_with_stores(&mut stores, "profile-1".to_string(), &profile, "10.0.0.1");
        // 別のprofile_idなら、同じ値でもカウンタは1から始まる。
        let other =
            mask_text_with_stores(&mut stores, "profile-2".to_string(), &profile, "10.0.0.1");
        assert_eq!(other.text, "__MASK_IP_1__");
    }

    // CompiledProfile::compileが無効化ルールを事前にフィルタするため(masker.rs)、
    // 無効化されたルールはmatch_countsに件数0としてすら現れず、Vec自体から消える。
    #[test]
    fn disabled_rule_is_not_applied_and_omitted_from_match_counts() {
        let mut stores = HashMap::new();
        let rule = Rule::new(
            "ip",
            PatternType::Regex,
            r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
            Mode::Sequential,
            None,
            Some("__MASK_IP_".to_string()),
            false,
            None,
        )
        .unwrap();
        let profile = RuleProfile::new("test", None, vec![rule]).unwrap();

        let result =
            mask_text_with_stores(&mut stores, "profile-1".to_string(), &profile, "10.0.0.1");

        assert_eq!(result.text, "10.0.0.1");
        assert_eq!(result.match_counts, vec![]);
    }
}
