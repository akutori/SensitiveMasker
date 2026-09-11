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
    profile: serde_json::Value,
    text: String,
) -> Result<MaskTextResult, String> {
    // profile: RuleProfileと直接型付けすると、この関数本体が実行される前にTauri自身の
    // 引数デシリアライズで全ルールの正規表現が既にコンパイルされてしまい、
    // profiles.rsのcreate_profile/update_profileと同じ理由でルール数上限を適用できない。
    crate::profiles::check_rule_count(&profile)?;
    let profile: RuleProfile = serde_json::from_value(profile).map_err(|e| e.to_string())?;

    // poison時も後続の呼び出しを永久に失敗させないよう、中身を取り出して継続する
    // (release buildのpanic=abortではpanic自体がプロセスごと終了するため、この回復が
    // 意味を持つのはdebug build時のみ)。
    let mut stores = state.0.lock().unwrap_or_else(|e| e.into_inner());
    Ok(mask_text_with_stores(&mut stores, profile_id, &profile, &text))
}

/// トレイの「クリップボードをマスク」から呼ぶための薄いラッパー。`mask_text`
/// コマンド(フロントエンドからの呼び出し)と同じ`MaskingState`(profile_idごとの
/// MappingStore)を共有し、連番モードの採番をトレイ経由でも一貫させる。
pub(crate) fn mask_text_for_tray(
    state: &MaskingState,
    profile_id: String,
    profile: &RuleProfile,
    text: &str,
) -> String {
    let mut stores = state.0.lock().unwrap_or_else(|e| e.into_inner());
    mask_text_with_stores(&mut stores, profile_id, profile, text).text
}

/// マスク実行のたびに蓄積する「元の値→ダミー値」の対応表(実在の機微情報そのものを
/// 保持している)を、GUIで入力欄をクリアした操作に合わせて破棄する。プロセスを
/// 終了するまで無期限に保持され続けることへの対応。
#[tauri::command]
pub async fn clear_mappings(state: tauri::State<'_, MaskingState>, profile_id: String) -> Result<(), String> {
    let mut stores = state.0.lock().unwrap_or_else(|e| e.into_inner());
    stores.remove(&profile_id);
    Ok(())
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
    fn clearing_a_profiles_mapping_resets_its_sequential_numbering() {
        // クリア操作(clear_mappingsコマンド本体が行うのと同じHashMap::remove)が、
        // 実在の値→ダミー値対応表を実際に破棄していることを、番号採番のリセットで確認する
        // (対応表が残っていれば10.0.0.1は既知の値として2ではなく1のままにはならないはず)。
        let mut stores = HashMap::new();
        let rule = sequential_rule("ip", r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}", "__MASK_IP_");
        let profile = RuleProfile::new("test", None, vec![rule]).unwrap();

        mask_text_with_stores(&mut stores, "profile-1".to_string(), &profile, "10.0.0.1 10.0.0.9");
        stores.remove("profile-1");

        let after_clear = mask_text_with_stores(&mut stores, "profile-1".to_string(), &profile, "10.0.0.9");
        assert_eq!(after_clear.text, "__MASK_IP_1__", "クリア後は既知の値として扱われず1から採番し直されるはず");
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
