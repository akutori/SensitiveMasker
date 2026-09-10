use std::sync::Mutex;

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
}

fn to_dto(preview: &ImportPreview) -> ImportPreviewDto {
    match preview {
        ImportPreview::Single { name, .. } => ImportPreviewDto::Single { name: name.clone() },
        ImportPreview::All { active_profile_name, entries, .. } => ImportPreviewDto::All {
            active_profile_name: active_profile_name.clone(),
            entries: entries.iter().map(to_entry_dto).collect(),
        },
    }
}

fn to_entry_dto(entry: &AllImportEntry) -> ImportEntryDto {
    ImportEntryDto {
        original_name: entry.original_name.clone(),
        resolved_name: entry.resolved_name.clone(),
        renamed: entry.renamed,
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
            ImportPreviewDto::Single { name } => assert_eq!(name, "元プロファイル"),
            ImportPreviewDto::All { .. } => panic!("単一プロファイルのエクスポートのはず"),
        }

        commit_pending_import_impl(&dest_state, &pending).expect("commitは成功するはず");

        let names = with_store(&dest_state, |s| s.list_profiles()).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "元プロファイル");
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
