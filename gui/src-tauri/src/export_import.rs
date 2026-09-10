use std::sync::Mutex;

use masking_core::{Mode, PatternType, Rule, RuleProfile};
use profile_store::{AllImportEntry, ImportPreview, SecretString};
use tauri::Emitter;

use crate::profiles::{with_store, ProfileStoreState};

/// preview_importが復号した内容(ルール本体を含む)をIPCで往復させないための保持先。
/// commit_pending_importが呼ばれるまでの間だけメモリ上に置く。
#[derive(Default)]
pub struct PendingImportState(Mutex<Option<ImportPreview>>);

#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportPreviewDto {
    Single {
        name: String,
        rules: Vec<ImportRuleDto>,
    },
    All {
        active_profile_name: Option<String>,
        entries: Vec<ImportEntryDto>,
    },
}

#[derive(Debug, serde::Serialize)]
pub struct ImportEntryDto {
    pub original_name: String,
    pub resolved_name: String,
    pub renamed: bool,
    pub rules: Vec<ImportRuleDto>,
}

/// インポート確認画面でルールの中身を表示するためのDTO(SMX-1対応)。
/// 「構文的に有効だが実データの書式と食い違う」細工されたルールに、確定前に
/// 気付けるようにするための情報であり、確定前に必ず提示する。
#[derive(Debug, serde::Serialize)]
pub struct ImportRuleDto {
    pub name: String,
    pub pattern_type: PatternType,
    pub pattern: String,
    pub mode: Mode,
    pub fixed_value: Option<String>,
    pub prefix: Option<String>,
    pub enabled: bool,
}

fn to_rule_dtos(profile: &RuleProfile) -> Vec<ImportRuleDto> {
    profile.rules().iter().map(to_rule_dto).collect()
}

fn to_rule_dto(rule: &Rule) -> ImportRuleDto {
    ImportRuleDto {
        name: rule.name().to_string(),
        pattern_type: rule.pattern_type(),
        pattern: rule.pattern().to_string(),
        mode: rule.mode(),
        fixed_value: rule.fixed_value().map(str::to_string),
        prefix: rule.prefix().map(str::to_string),
        enabled: rule.enabled(),
    }
}

fn to_dto(preview: &ImportPreview) -> ImportPreviewDto {
    match preview {
        ImportPreview::Single { name, exported } => {
            ImportPreviewDto::Single { name: name.clone(), rules: to_rule_dtos(&exported.profile) }
        }
        ImportPreview::All { active_profile_name, entries, exported } => ImportPreviewDto::All {
            active_profile_name: active_profile_name.clone(),
            entries: entries.iter().zip(exported).map(|(entry, exp)| to_entry_dto(entry, &exp.profile)).collect(),
        },
    }
}

fn to_entry_dto(entry: &AllImportEntry, profile: &RuleProfile) -> ImportEntryDto {
    ImportEntryDto {
        original_name: entry.original_name.clone(),
        resolved_name: entry.resolved_name.clone(),
        renamed: entry.renamed,
        rules: to_rule_dtos(profile),
    }
}

// tauri::Stateに依存しない形にして単体テスト可能にする(profiles.rsのwith_storeと同じ方針)。
fn export_profile_to_file_impl(
    state: &ProfileStoreState,
    name: &str,
    passphrase: String,
    dest_path: &str,
) -> Result<(), String> {
    let bytes = with_store(state, |store| store.export_profile(name, SecretString::from(passphrase)))?;
    std::fs::write(dest_path, bytes).map_err(|e| e.to_string())
}

fn export_all_to_file_impl(
    state: &ProfileStoreState,
    passphrase: String,
    dest_path: &str,
) -> Result<(), String> {
    let bytes = with_store(state, |store| store.export_all(SecretString::from(passphrase)))?;
    std::fs::write(dest_path, bytes).map_err(|e| e.to_string())
}

fn preview_import_impl(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    source_path: &str,
    passphrase: String,
) -> Result<ImportPreviewDto, String> {
    let data = std::fs::read(source_path).map_err(|e| e.to_string())?;
    let preview = with_store(state, |store| store.preview_import(&data, SecretString::from(passphrase)))?;
    let dto = to_dto(&preview);
    *pending.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(preview);
    Ok(dto)
}

fn commit_pending_import_impl(state: &ProfileStoreState, pending: &PendingImportState) -> Result<(), String> {
    let preview = pending
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| "確認待ちのインポートがありません".to_string())?;
    with_store(state, |store| store.commit_import(preview).map(|_| ()))
}

#[tauri::command]
pub async fn export_profile_to_file(
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
    passphrase: String,
    dest_path: String,
) -> Result<(), String> {
    export_profile_to_file_impl(&state, &name, passphrase, &dest_path)
}

#[tauri::command]
pub async fn export_all_to_file(
    state: tauri::State<'_, ProfileStoreState>,
    passphrase: String,
    dest_path: String,
) -> Result<(), String> {
    export_all_to_file_impl(&state, passphrase, &dest_path)
}

/// DBはまだ変更しない。復号結果はPendingImportStateに保持し、フロントエンドには
/// 表示に必要な要約(名前・リネーム有無)のみを返す(ルール本体を往復させないため)。
#[tauri::command]
pub async fn preview_import(
    state: tauri::State<'_, ProfileStoreState>,
    pending: tauri::State<'_, PendingImportState>,
    source_path: String,
    passphrase: String,
) -> Result<ImportPreviewDto, String> {
    preview_import_impl(&state, &pending, &source_path, passphrase)
}

#[tauri::command]
pub async fn commit_pending_import(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    pending: tauri::State<'_, PendingImportState>,
) -> Result<(), String> {
    commit_pending_import_impl(&state, &pending)?;
    let _ = app.emit("profiles-changed", ());
    let _ = app.emit("tags-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use masking_core::{Mode, PatternType, Rule, RuleProfile};
    use profile_store::{AppPaths, ProfileStore};

    fn init_store_with_one_profile(dir: &std::path::Path, profile_name: &str) -> ProfileStore {
        let paths = AppPaths::at(dir);
        profile_store::init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        let rule = Rule::new(
            "電話番号",
            PatternType::Literal,
            "0120",
            Mode::Sequential,
            None,
            Some("TEL".to_string()),
            true,
            None,
        )
        .unwrap();
        let profile = RuleProfile::new(profile_name, None, vec![rule]).unwrap();
        store.create_profile(&profile).unwrap();
        store
    }

    fn init_store_with_two_profiles(dir: &std::path::Path) -> ProfileStore {
        let paths = AppPaths::at(dir);
        profile_store::init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        let rule_a =
            Rule::new("Aルール", PatternType::Literal, "AAA", Mode::Sequential, None, Some("A".to_string()), true, None)
                .unwrap();
        let rule_b =
            Rule::new("Bルール", PatternType::Literal, "BBB", Mode::Sequential, None, Some("B".to_string()), true, None)
                .unwrap();
        store.create_profile(&RuleProfile::new("プロファイルA", None, vec![rule_a]).unwrap()).unwrap();
        store.create_profile(&RuleProfile::new("プロファイルB", None, vec![rule_b]).unwrap()).unwrap();
        store
    }

    #[test]
    fn export_then_preview_then_commit_round_trips_into_a_different_store() {
        let source_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let export_file = tempfile::NamedTempFile::new().unwrap();

        let source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_profile_to_file_impl(
            &source_state,
            "元プロファイル",
            "correct horse battery staple".to_string(),
            export_file.path().to_str().unwrap(),
        )
        .expect("エクスポートは成功するはず");

        let dest_paths = AppPaths::at(dest_dir.path());
        profile_store::init_at(&dest_paths).unwrap();
        let dest_store = ProfileStore::open_at(&dest_paths).unwrap();
        let dest_state = ProfileStoreState::with_store_for_test(dest_store);
        let pending = PendingImportState::default();

        let dto = preview_import_impl(
            &dest_state,
            &pending,
            export_file.path().to_str().unwrap(),
            "correct horse battery staple".to_string(),
        )
        .expect("正しいパスフレーズでのpreviewは成功するはず");
        match dto {
            ImportPreviewDto::Single { name, rules } => {
                assert_eq!(name, "元プロファイル");
                // SMX-1対応: 確認前にルールの中身(名前・パターン・有効/無効)が
                // 見える必要があるため、DTOに含まれることをここで固定する。
                assert_eq!(rules.len(), 1);
                assert_eq!(rules[0].name, "電話番号");
                assert_eq!(rules[0].pattern, "0120");
                assert!(rules[0].enabled);
            }
            ImportPreviewDto::All { .. } => panic!("単一プロファイルのエクスポートのはず"),
        }

        commit_pending_import_impl(&dest_state, &pending).expect("commitは成功するはず");

        let names = with_store(&dest_state, |s| s.list_profiles()).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "元プロファイル");
    }

    #[test]
    fn preview_import_all_associates_each_entrys_rules_with_its_own_profile() {
        let source_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let export_file = tempfile::NamedTempFile::new().unwrap();

        let source_store = init_store_with_two_profiles(source_dir.path());
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_all_to_file_impl(
            &source_state,
            "correct horse battery staple".to_string(),
            export_file.path().to_str().unwrap(),
        )
        .expect("エクスポートは成功するはず");

        let dest_paths = AppPaths::at(dest_dir.path());
        profile_store::init_at(&dest_paths).unwrap();
        let dest_store = ProfileStore::open_at(&dest_paths).unwrap();
        let dest_state = ProfileStoreState::with_store_for_test(dest_store);
        let pending = PendingImportState::default();

        let dto = preview_import_impl(
            &dest_state,
            &pending,
            export_file.path().to_str().unwrap(),
            "correct horse battery staple".to_string(),
        )
        .expect("正しいパスフレーズでのpreviewは成功するはず");

        // SMX-1対応: entries[i]とexported[i]のインデックス対応(zip)に依存しているため、
        // 名前でエントリを探した上でそのルールが正しく自分自身のものであることを固定する
        // (取り違えがあれば、内容の入れ替わりとして検出できる)。
        match dto {
            ImportPreviewDto::All { entries, .. } => {
                assert_eq!(entries.len(), 2);
                let a = entries.iter().find(|e| e.original_name == "プロファイルA").expect("プロファイルAが見つかるはず");
                assert_eq!(a.rules.len(), 1);
                assert_eq!(a.rules[0].name, "Aルール");
                assert_eq!(a.rules[0].pattern, "AAA");

                let b = entries.iter().find(|e| e.original_name == "プロファイルB").expect("プロファイルBが見つかるはず");
                assert_eq!(b.rules.len(), 1);
                assert_eq!(b.rules[0].name, "Bルール");
                assert_eq!(b.rules[0].pattern, "BBB");
            }
            ImportPreviewDto::Single { .. } => panic!("全体エクスポートのはず"),
        }
    }

    #[test]
    fn preview_import_with_wrong_passphrase_fails_and_leaves_no_pending_state() {
        let source_dir = tempfile::tempdir().unwrap();
        let export_file = tempfile::NamedTempFile::new().unwrap();
        let source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_profile_to_file_impl(
            &source_state,
            "元プロファイル",
            "correct horse battery staple".to_string(),
            export_file.path().to_str().unwrap(),
        )
        .unwrap();

        let pending = PendingImportState::default();
        let err = preview_import_impl(
            &source_state,
            &pending,
            export_file.path().to_str().unwrap(),
            "wrong passphrase".to_string(),
        )
        .expect_err("誤ったパスフレーズは失敗するはず");
        assert!(!err.is_empty());

        // previewが失敗した場合、commitできる状態が残っていてはならない。
        let commit_err = commit_pending_import_impl(&source_state, &pending)
            .expect_err("previewが無いのでcommitも失敗するはず");
        assert_eq!(commit_err, "確認待ちのインポートがありません");
    }

    #[test]
    fn commit_pending_import_without_a_prior_preview_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let store = init_store_with_one_profile(dir.path(), "既存プロファイル");
        let state = ProfileStoreState::with_store_for_test(store);
        let pending = PendingImportState::default();

        let err = commit_pending_import_impl(&state, &pending).expect_err("previewを呼んでいないので失敗するはず");
        assert_eq!(err, "確認待ちのインポートがありません");
    }
}
