//! プロファイル管理画面の「envインポート」で使う、.env内容のプレビュー。
//! ファイルI/Oは行わない(既存のread_text_fileで読んだ内容を受け取るだけ)。
//! 実際の判定ロジック(除外ヒューリスティック含む)はmasking-core::parse_env_candidates
//! が持ち、ここはDTOへの詰め替えのみを行う薄いラッパー。

use masking_core::parse_env_candidates;
use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub struct EnvCandidateDto {
    key: String,
    value: String,
    included_by_default: bool,
}

fn preview_env_import_impl(content: &str) -> Vec<EnvCandidateDto> {
    parse_env_candidates(content)
        .into_iter()
        .map(|c| EnvCandidateDto { key: c.key, value: c.value, included_by_default: c.included_by_default })
        .collect()
}

// 大きな.env内容の処理でUIスレッドをブロックしないよう、masking.rsのmask_textと
// 同じ理由で非同期コマンドにする(Tauriは非asyncコマンドをメインスレッドで実行するため)。
#[tauri::command]
pub async fn preview_env_import(content: String) -> Vec<EnvCandidateDto> {
    preview_env_import_impl(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_masking_core_fields_onto_the_dto_unchanged() {
        let result = preview_env_import_impl("API_KEY=sk_live_1234567890");
        assert_eq!(
            result,
            vec![EnvCandidateDto {
                key: "API_KEY".to_string(),
                value: "sk_live_1234567890".to_string(),
                included_by_default: true,
            }]
        );
    }

    #[test]
    fn a_denylisted_key_maps_to_included_by_default_false() {
        let result = preview_env_import_impl("PORT=1234567890");
        assert!(!result[0].included_by_default);
    }
}
