use std::path::{Path, PathBuf};
use std::sync::Mutex;

use masking_core::{Mode, PatternType, Rule, RuleProfile};
use profile_store::{decrypt_import_payload, AllImportEntry, AppPaths, ImportPreview, SecretString};
use tauri::{Emitter, Manager};

use crate::profiles::{resolve_paths, with_store, ProfileStoreState};

/// この上限を超えるファイルは復号を試みる前に拒否する(巨大ファイル指定による
/// メモリ枯渇・ハングを避けるため)。実際の.smxは数KB〜数百KB程度で足りる。
const MAX_IMPORT_FILE_BYTES: u64 = 8 * 1024 * 1024;

const GENERIC_IO_ERROR: &str = "ファイルを読み書きできませんでした";

/// パスフレーズによる復号を試みる前の事前検証(パス形式・拡張子・サイズ・保存先)で
/// 弾かれたか、それより後(ストア層: 復号・フォーマット検証等)で失敗したかを区別する。
/// フロントエンドはInvalidInputをパスフレーズ入力エラーとして表示してはならない。
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum ExportImportError {
    InvalidInput(String),
    Failed(String),
}

impl ExportImportError {
    fn message(&self) -> &str {
        match self {
            ExportImportError::InvalidInput(m) | ExportImportError::Failed(m) => m,
        }
    }
}

impl std::fmt::Display for ExportImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

fn has_smx_extension(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("smx"))
}

// パス検証本体(正規化・特殊形式拒否・アプリのデータフォルダ除外)はprofile_store側で
// CLI(masker export/mask --output)と共有する。ここでは.smx拡張子の要求のみGUI固有。
// データフォルダの解決(resolve_paths。環境変数を読む)は呼び出し側で行い、引数で受け取る
// (テストが、実際のデータフォルダの有無に依存しないようにする。text_file_io.rsのwrite_text_file_impl
// と同じ方針)。
fn validate_export_dest_path(dest_path: &str, app_paths: Result<AppPaths, String>) -> Result<PathBuf, String> {
    let path = profile_store::normalize_and_reject_special_forms(dest_path).map_err(|e| e.to_string())?;
    if !has_smx_extension(&path) {
        return Err("保存先には拡張子.smxを指定してください".to_string());
    }
    let app_paths = app_paths?;
    let dest_parent = path.parent().ok_or_else(|| GENERIC_IO_ERROR.to_string())?;
    app_paths.reject_if_dir_is_inside_data_dir(dest_parent).map_err(|e| e.to_string())?;
    Ok(path)
}

fn validate_import_source_path(source_path: &str) -> Result<PathBuf, String> {
    let path = profile_store::normalize_and_reject_special_forms(source_path).map_err(|e| e.to_string())?;
    if !has_smx_extension(&path) {
        return Err("拡張子が.smxのファイルを選択してください".to_string());
    }
    Ok(path)
}

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
        tags: Vec<String>,
    },
    All {
        // ファイル内のactive_profile_nameをそのまま返すのではなく、取り込み先の現在の
        // 状態(既にアクティブが設定済みかどうか)も踏まえて「このインポートを実行すると
        // 実際にどのプロファイルがアクティブになるか」を返す(既にアクティブがあれば
        // 常にNone)。commit_import側の判定と同じ条件を確認前に見せるための情報。
        will_activate_profile_name: Option<String>,
        entries: Vec<ImportEntryDto>,
    },
}

#[derive(Debug, serde::Serialize)]
pub struct ImportEntryDto {
    pub original_name: String,
    pub resolved_name: String,
    pub renamed: bool,
    pub rules: Vec<ImportRuleDto>,
    pub tags: Vec<String>,
}

/// インポート確認画面でルールの中身を表示するためのDTO。
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

fn to_dto(preview: &ImportPreview, destination_has_active_profile: bool) -> ImportPreviewDto {
    match preview {
        ImportPreview::Single { name, exported } => ImportPreviewDto::Single {
            name: name.clone(),
            rules: to_rule_dtos(&exported.profile),
            tags: exported.tags.clone(),
        },
        ImportPreview::All { active_profile_name, entries, exported } => {
            // commit_import側の実際の判定(取り込み先に既にアクティブがあれば変更しない)と
            // 同じ条件をここでも評価する。ここでNoneにしても、実際にcommit_importへ渡す
            // previewそのもの(ImportPreview::All.active_profile_name)は変更しない
            // (表示用の判定と実際のコミット時の判定は独立に評価する設計を保つため)。
            let will_activate_profile_name = (!destination_has_active_profile)
                .then(|| active_profile_name.as_ref())
                .flatten()
                .and_then(|original| entries.iter().find(|e| &e.original_name == original))
                .map(|entry| entry.resolved_name.clone());
            ImportPreviewDto::All {
                will_activate_profile_name,
                entries: entries
                    .iter()
                    .zip(exported)
                    .map(|(entry, exp)| to_entry_dto(entry, &exp.profile, &exp.tags))
                    .collect(),
            }
        }
    }
}

// ExportedProfile自体はprofile-store内部限定の型(privateなbulkモジュール定義)のため
// 名指しできず、.profile/.tagsのフィールド射影を個別の引数として受け取る(既存のprofile
// 引数の扱いと同じ理由)。
fn to_entry_dto(entry: &AllImportEntry, profile: &RuleProfile, tags: &[String]) -> ImportEntryDto {
    ImportEntryDto {
        original_name: entry.original_name.clone(),
        resolved_name: entry.resolved_name.clone(),
        renamed: entry.renamed,
        rules: to_rule_dtos(profile),
        tags: tags.to_vec(),
    }
}

// tauri::Stateに依存しない形にして単体テスト可能にする(profiles.rsのwith_storeと同じ方針)。
// app_pathsは、書き出し先がアプリのデータフォルダの内側でないことの検証に使う。
fn export_profile_to_file_impl(
    state: &ProfileStoreState,
    app_paths: Result<AppPaths, String>,
    name: &str,
    passphrase: SecretString,
    dest_path: &str,
) -> Result<(), ExportImportError> {
    let dest_path =
        validate_export_dest_path(dest_path, app_paths).map_err(ExportImportError::InvalidInput)?;
    let bytes = with_store(state, |store| store.export_profile(name, passphrase)).map_err(ExportImportError::Failed)?;
    std::fs::write(&dest_path, bytes).map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))
}

fn export_all_to_file_impl(
    state: &ProfileStoreState,
    app_paths: Result<AppPaths, String>,
    passphrase: SecretString,
    dest_path: &str,
) -> Result<(), ExportImportError> {
    let dest_path =
        validate_export_dest_path(dest_path, app_paths).map_err(ExportImportError::InvalidInput)?;
    let bytes = with_store(state, |store| store.export_all(passphrase)).map_err(ExportImportError::Failed)?;
    std::fs::write(&dest_path, bytes).map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))
}

fn preview_import_impl(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    source_path: &str,
    passphrase: SecretString,
) -> Result<ImportPreviewDto, ExportImportError> {
    let source_path = validate_import_source_path(source_path).map_err(ExportImportError::InvalidInput)?;
    let metadata = std::fs::metadata(&source_path)
        .map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?;
    if metadata.len() > MAX_IMPORT_FILE_BYTES {
        return Err(ExportImportError::InvalidInput("ファイルサイズが大きすぎます".to_string()));
    }
    let data = std::fs::read(&source_path).map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?;
    // パスフレーズ検証(scrypt、数百ms〜数秒)はストアのロックを握らずに行う。ロック内で
    // 実行すると、他のプロファイル/タグ系コマンドがこの間ずっとブロックされてしまう。
    let payload = decrypt_import_payload(&data, passphrase).map_err(|e| ExportImportError::Failed(e.to_string()))?;
    let preview =
        with_store(state, |store| store.resolve_import_preview(payload)).map_err(ExportImportError::Failed)?;
    let has_active = with_store(state, |store| store.has_active_profile()).map_err(ExportImportError::Failed)?;
    let dto = to_dto(&preview, has_active);
    *pending.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(preview);
    Ok(dto)
}

/// インポート確定後、実際にどのプロファイルがアクティブになったか(無ければNone)。
/// 無言でのアクティブ化(update_profileと同じ問題意識)に気付けるようにするための情報。
#[derive(Debug, serde::Serialize)]
pub struct CommitImportResultDto {
    pub activated_profile_name: Option<String>,
}

fn commit_pending_import_impl(
    state: &ProfileStoreState,
    pending: &PendingImportState,
) -> Result<CommitImportResultDto, String> {
    let preview = pending
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| "確認待ちのインポートがありません".to_string())?;
    let outcome = with_store(state, |store| store.commit_import(preview))?;
    Ok(match outcome {
        profile_store::ImportOutcome::Single { name, activated } => {
            CommitImportResultDto { activated_profile_name: activated.then_some(name) }
        }
        profile_store::ImportOutcome::All { activated_profile_name, .. } => {
            CommitImportResultDto { activated_profile_name }
        }
    })
}

#[tauri::command]
pub async fn export_profile_to_file(
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
    passphrase: SecretString,
    dest_path: String,
) -> Result<(), ExportImportError> {
    export_profile_to_file_impl(&state, resolve_paths(), &name, passphrase, &dest_path)
}

#[tauri::command]
pub async fn export_all_to_file(
    state: tauri::State<'_, ProfileStoreState>,
    passphrase: SecretString,
    dest_path: String,
) -> Result<(), ExportImportError> {
    export_all_to_file_impl(&state, resolve_paths(), passphrase, &dest_path)
}

/// DBはまだ変更しない。復号結果はPendingImportStateに保持し、フロントエンドには
/// 表示に必要な要約(名前・リネーム有無)のみを返す(ルール本体を往復させないため)。
///
/// 悪意ある.smxファイルは、パスフレーズの正誤を検証する前に最大2^22相当(≒4GiB)の
/// メモリ確保を要求しうる(export.rsのMAX_WORK_FACTOR_LOG_N参照)。
/// この処理を非同期ランタイムのワーカースレッド上でそのまま実行すると、そのスレッドが
/// 長時間ブロックされ、同じプールを共有する他の全Tauriコマンドの処理まで止まりうる
/// (with_storeのMutex自体は他コマンドと競合するだけだが、ワーカースレッド枯渇は
/// Mutexと無関係なコマンドも巻き込む、より広い影響であるため)。専用のブロッキング
/// スレッドプール(tauri::async_runtime::spawn_blocking)で実行することで避ける。
/// tauri::Stateは'staticではなくこのスレッドへ直接持ち込めないため、AppHandle
/// (Clone可能)経由でスレッド内から改めて状態を取得する。
#[tauri::command]
pub async fn preview_import(
    app: tauri::AppHandle,
    source_path: String,
    passphrase: SecretString,
) -> Result<ImportPreviewDto, ExportImportError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ProfileStoreState>();
        let pending = app.state::<PendingImportState>();
        preview_import_impl(&state, &pending, &source_path, passphrase)
    })
    .await
    .map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?
}

#[tauri::command]
pub async fn commit_pending_import(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    pending: tauri::State<'_, PendingImportState>,
) -> Result<CommitImportResultDto, String> {
    let result = commit_pending_import_impl(&state, &pending)?;
    let _ = app.emit("profiles-changed", ());
    let _ = app.emit("tags-changed", ());
    Ok(result)
}

/// 確認ダイアログのキャンセル・離脱時に呼ぶ。preview_importが復号した平文
/// (ルール本体を含む)をプロセス内に残さないためと、キャンセル後にcommit_pending_import
/// が呼ばれても確定しないようにするため(意思決定をRust側の状態にも反映する)。
/// 保留中の内容が無い場合も含め常に成功する(呼び出し側は「無かったこと」を
/// エラーとして扱う必要が無いように、副作用の無い操作として設計する)。
fn clear_pending_import_impl(pending: &PendingImportState) {
    *pending.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[tauri::command]
pub async fn clear_pending_import(pending: tauri::State<'_, PendingImportState>) -> Result<(), String> {
    clear_pending_import_impl(&pending);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use masking_core::{Mode, PatternType, Rule, RuleProfile};
    use profile_store::{AppPaths, ProfileStore};

    fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

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
        // dest_path/source_pathの検証が.smx拡張子を要求するため、テスト用の一時ファイルも
        // 実際の運用(保存ダイアログのフィルタで常に.smxになる)に合わせる。
        let export_file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();

        let source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_profile_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            "元プロファイル",
            passphrase("correct horse battery staple"),
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
            passphrase("correct horse battery staple"),
        )
        .expect("正しいパスフレーズでのpreviewは成功するはず");
        match dto {
            ImportPreviewDto::Single { name, rules, tags } => {
                assert_eq!(name, "元プロファイル");
                // 確認前にルールの中身(名前・パターン・有効/無効)が見える必要があるため、
                // DTOに含まれることをここで固定する。
                assert_eq!(rules.len(), 1);
                assert_eq!(rules[0].name, "電話番号");
                assert_eq!(rules[0].pattern, "0120");
                assert!(rules[0].enabled);
                assert!(tags.is_empty());
            }
            ImportPreviewDto::All { .. } => panic!("単一プロファイルのエクスポートのはず"),
        }

        let result = commit_pending_import_impl(&dest_state, &pending).expect("commitは成功するはず");
        assert_eq!(
            result.activated_profile_name.as_deref(),
            Some("元プロファイル"),
            "取り込み先は新規ストアでアクティブ未設定のはず"
        );

        let names = with_store(&dest_state, |s| s.list_profiles()).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "元プロファイル");
    }

    #[test]
    fn preview_import_all_associates_each_entrys_rules_with_its_own_profile() {
        let source_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        // dest_path/source_pathの検証が.smx拡張子を要求するため、テスト用の一時ファイルも
        // 実際の運用(保存ダイアログのフィルタで常に.smxになる)に合わせる。
        let export_file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();

        let source_store = init_store_with_two_profiles(source_dir.path());
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_all_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            passphrase("correct horse battery staple"),
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
            passphrase("correct horse battery staple"),
        )
        .expect("正しいパスフレーズでのpreviewは成功するはず");

        // entries[i]とexported[i]のインデックス対応(zip)に依存しているため、名前で
        // エントリを探した上でそのルールが正しく自分自身のものであることを固定する
        // (取り違えがあれば、内容の入れ替わりとして検出できる)。
        match dto {
            ImportPreviewDto::All { entries, will_activate_profile_name } => {
                assert_eq!(entries.len(), 2);
                let a = entries.iter().find(|e| e.original_name == "プロファイルA").expect("プロファイルAが見つかるはず");
                assert_eq!(a.rules.len(), 1);
                assert_eq!(a.rules[0].name, "Aルール");
                assert_eq!(a.rules[0].pattern, "AAA");
                assert!(a.tags.is_empty());

                let b = entries.iter().find(|e| e.original_name == "プロファイルB").expect("プロファイルBが見つかるはず");
                assert_eq!(b.rules.len(), 1);
                assert_eq!(b.rules[0].name, "Bルール");
                assert_eq!(b.rules[0].pattern, "BBB");

                // プロファイルAが先に作成されたため元ストアではアクティブ、取り込み先は
                // 新規ストアでアクティブ未設定のため、このインポートを実行するとAが
                // アクティブになることが事前にわかるはず。
                assert_eq!(will_activate_profile_name.as_deref(), Some("プロファイルA"));
            }
            ImportPreviewDto::Single { .. } => panic!("全体エクスポートのはず"),
        }
    }

    #[test]
    fn preview_import_single_includes_tags() {
        let source_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let export_file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();

        let mut source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        source_store.set_profile_tags("元プロファイル", &["sip".to_string()]).unwrap();
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_profile_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            "元プロファイル",
            passphrase("correct horse battery staple"),
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
            passphrase("correct horse battery staple"),
        )
        .expect("正しいパスフレーズでのpreviewは成功するはず");

        match dto {
            // インポートで無警告のままグローバルなタグ集合へ追加されうる問題への対応
            // (確定前にどのタグが付くか見えるようにする)。
            ImportPreviewDto::Single { tags, .. } => assert_eq!(tags, vec!["sip".to_string()]),
            ImportPreviewDto::All { .. } => panic!("単一プロファイルのエクスポートのはず"),
        }
    }

    #[test]
    fn preview_import_all_reports_no_activation_when_destination_already_has_an_active_profile() {
        let source_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let export_file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();

        let source_store = init_store_with_two_profiles(source_dir.path());
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_all_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            passphrase("correct horse battery staple"),
            export_file.path().to_str().unwrap(),
        )
        .expect("エクスポートは成功するはず");

        // 取り込み先には既にアクティブなプロファイルが存在する状態を作る。
        let dest_store = init_store_with_one_profile(dest_dir.path(), "既存プロファイル");
        let dest_state = ProfileStoreState::with_store_for_test(dest_store);
        let pending = PendingImportState::default();

        let dto = preview_import_impl(
            &dest_state,
            &pending,
            export_file.path().to_str().unwrap(),
            passphrase("correct horse battery staple"),
        )
        .expect("正しいパスフレーズでのpreviewは成功するはず");

        match dto {
            ImportPreviewDto::All { will_activate_profile_name, .. } => {
                assert_eq!(
                    will_activate_profile_name, None,
                    "取り込み先に既にアクティブなプロファイルがある場合は表示しないはず"
                );
            }
            ImportPreviewDto::Single { .. } => panic!("全体エクスポートのはず"),
        }
    }

    #[test]
    fn preview_import_with_wrong_passphrase_fails_and_leaves_no_pending_state() {
        let source_dir = tempfile::tempdir().unwrap();
        // dest_path/source_pathの検証が.smx拡張子を要求するため、テスト用の一時ファイルも
        // 実際の運用(保存ダイアログのフィルタで常に.smxになる)に合わせる。
        let export_file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();
        let source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_profile_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            "元プロファイル",
            passphrase("correct horse battery staple"),
            export_file.path().to_str().unwrap(),
        )
        .unwrap();

        let pending = PendingImportState::default();
        let err = preview_import_impl(
            &source_state,
            &pending,
            export_file.path().to_str().unwrap(),
            passphrase("wrong passphrase"),
        )
        .expect_err("誤ったパスフレーズは失敗するはず");
        assert!(!err.message().is_empty());
        // 事前検証(パス・拡張子・サイズ)は全て通過した上での失敗のため、パスフレーズ
        // エラーとして表示してよい種別(Failed)になっているはず。
        assert!(matches!(err, ExportImportError::Failed(_)));

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

    #[test]
    fn clear_pending_import_prevents_a_later_commit_from_succeeding() {
        // キャンセル操作をclear_pending_importで反映した後は、それより後に
        // commit_pending_importが呼ばれても(直接IPCを叩く経路を含め)確定しないはず。
        let source_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let export_file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();
        let source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        export_profile_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            "元プロファイル",
            passphrase("correct horse battery staple"),
            export_file.path().to_str().unwrap(),
        )
        .unwrap();

        let dest_paths = AppPaths::at(dest_dir.path());
        profile_store::init_at(&dest_paths).unwrap();
        let dest_store = ProfileStore::open_at(&dest_paths).unwrap();
        let dest_state = ProfileStoreState::with_store_for_test(dest_store);
        let pending = PendingImportState::default();
        preview_import_impl(
            &dest_state,
            &pending,
            export_file.path().to_str().unwrap(),
            passphrase("correct horse battery staple"),
        )
        .expect("previewは成功するはず");

        clear_pending_import_impl(&pending);

        let err = commit_pending_import_impl(&dest_state, &pending)
            .expect_err("キャンセル済みのはずなのでcommitは拒否されるはず");
        assert_eq!(err, "確認待ちのインポートがありません");
    }

    // UNCの表記はWindowsのパスの形式で、Unixでは、区切りではない文字を含む相対パスの名前になるため、
    // Windowsでだけ検証する。
    #[test]
    #[cfg(windows)]
    fn validate_export_dest_path_rejects_unc_paths() {
        let err = validate_export_dest_path(r"\\server\share\export.smx", Err("unused".to_string()))
            .expect_err("UNCパスは拒否されるはず");
        assert!(err.contains("特殊な形式"), "予期しないエラー文言: {err}");
    }

    #[test]
    fn validate_export_dest_path_rejects_wrong_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.txt");
        let err = validate_export_dest_path(path.to_str().unwrap(), Err("unused".to_string()))
            .expect_err(".smx以外の拡張子は拒否されるはず");
        assert!(err.contains(".smx"), "予期しないエラー文言: {err}");
    }

    #[test]
    fn validate_export_dest_path_rejects_paths_inside_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        let inside = data_dir.path().join("sneaky.smx");

        let err = validate_export_dest_path(inside.to_str().unwrap(), Ok(app_paths))
            .expect_err("アプリのデータフォルダ内への書き込みは拒否されるはず");
        assert!(err.contains("データフォルダ"), "予期しないエラー文言: {err}");
    }

    #[test]
    fn validate_export_dest_path_allows_normal_paths_outside_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let export_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        let dest = export_dir.path().join("export.smx");

        validate_export_dest_path(dest.to_str().unwrap(), Ok(app_paths))
            .expect("データフォルダ外への正常な保存は許可されるはず");
    }

    // 書き出しの検証は、環境から解決したデータフォルダでなく、渡されたデータフォルダで行う
    // (テストが、実際のデータフォルダの有無に依存しないようにするため、これを固定する)。
    #[test]
    fn export_profile_to_file_rejects_a_destination_inside_the_given_data_dir() {
        let source_dir = tempfile::tempdir().unwrap();
        let source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        let inside = source_dir.path().join("sneaky.smx");

        let err = export_profile_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            "元プロファイル",
            passphrase("correct horse battery staple"),
            inside.to_str().unwrap(),
        )
        .expect_err("渡したデータフォルダの内側への書き出しは拒否されるはず");
        assert!(err.to_string().contains("データフォルダ"), "予期しないエラー文言: {err}");
        assert!(!inside.exists(), "拒否したのにファイルが書かれている");
    }

    #[test]
    fn export_all_to_file_rejects_a_destination_inside_the_given_data_dir() {
        let source_dir = tempfile::tempdir().unwrap();
        let source_store = init_store_with_two_profiles(source_dir.path());
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        let inside = source_dir.path().join("sneaky.smx");

        let err = export_all_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            passphrase("correct horse battery staple"),
            inside.to_str().unwrap(),
        )
        .expect_err("渡したデータフォルダの内側への書き出しは拒否されるはず");
        assert!(err.to_string().contains("データフォルダ"), "予期しないエラー文言: {err}");
        assert!(!inside.exists(), "拒否したのにファイルが書かれている");
    }

    #[test]
    fn export_fails_closed_when_the_data_dir_cannot_be_resolved() {
        let source_dir = tempfile::tempdir().unwrap();
        let source_store = init_store_with_one_profile(source_dir.path(), "元プロファイル");
        let source_state = ProfileStoreState::with_store_for_test(source_store);
        let dest_dir = tempfile::tempdir().unwrap();
        let dest = dest_dir.path().join("export.smx");

        let err = export_profile_to_file_impl(
            &source_state,
            Err("resolution failed".to_string()),
            "元プロファイル",
            passphrase("correct horse battery staple"),
            dest.to_str().unwrap(),
        )
        .expect_err("データフォルダを解決できないときは、書き出さない(fail-closed)はず");
        assert!(matches!(err, ExportImportError::InvalidInput(_)));
        assert!(!dest.exists(), "解決できなかったのにファイルが書かれている");
    }

    // 字句上のstarts_with比較では、NTFSが大文字小文字を区別しないことを利用して
    // 同一フォルダを別表記で指すだけで判定をすり抜けられるため、これを固定する。
    // 大文字小文字を区別しないファイルシステムと、`\`の区切りはWindowsの性質のため、Windowsでだけ検証する。
    #[test]
    #[cfg(windows)]
    fn validate_export_dest_path_rejects_paths_that_differ_only_in_case() {
        let data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        // 実ディレクトリと大文字小文字だけが異なる文字列(Windowsは同一の実体を指す)。
        let differently_cased = data_dir.path().to_string_lossy().to_uppercase();
        let sneaky = format!("{differently_cased}\\SNEAKY.smx");

        let err = validate_export_dest_path(&sneaky, Ok(app_paths))
            .expect_err("大文字小文字が違うだけの同一フォルダも拒否されるはず");
        assert!(err.contains("データフォルダ"), "予期しないエラー文言: {err}");
    }

    // シンボリックリンク/ジャンクションでapp_dir外の場所からapp_dir内を指させると、
    // 字句比較(canonicalize+starts_with)をすり抜け、実際の書き込みはリンク先
    // (app_dir内の実ファイル)に届いてしまうため、これを固定する。ジャンクションは
    // (シンボリックリンクと異なり)開発者モード・管理者権限が無くても作成できるため、
    // こちらを使って検証する。
    #[test]
    fn validate_export_dest_path_rejects_junctions_into_app_data_dir() {
        let real_data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(real_data_dir.path());
        let outside_dir = tempfile::tempdir().unwrap();
        let link_path = outside_dir.path().join("looks_safe");

        #[cfg(windows)]
        let link_created = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &link_path.to_string_lossy(), &real_data_dir.path().to_string_lossy()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        #[cfg(not(windows))]
        let link_created = std::os::unix::fs::symlink(real_data_dir.path(), &link_path).is_ok();

        if !link_created {
            // 権限・環境の制約でリンクを作成できない場合はスキップする(判定ロジック
            // 自体はsame_file::is_same_fileがOSレベルで実体解決するため、経由した
            // パスの種類=ジャンクション/シンボリックリンクを問わず共通の経路を通る)。
            eprintln!("ジャンクション/シンボリックリンクを作成できない環境のためスキップします");
            return;
        }

        let sneaky = link_path.join("sneaky.smx");
        let err = validate_export_dest_path(sneaky.to_str().unwrap(), Ok(app_paths))
            .expect_err("ジャンクション経由でのデータフォルダアクセスも拒否されるはず");
        assert!(err.contains("データフォルダ"), "予期しないエラー文言: {err}");
    }

    // resolve_paths()自体が失敗した場合を.ok()で握り潰すと、データフォルダ保護
    // チェックごと無効化されるfail-open設計になる(CLAUDE.mdのfail-safe defaults
    // 方針に反する)ため、これを固定する。
    #[test]
    fn validate_export_dest_path_fails_closed_when_app_data_dir_cannot_be_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("export.smx");

        validate_export_dest_path(dest.to_str().unwrap(), Err("データディレクトリ解決失敗".to_string()))
            .expect_err("データフォルダの場所を解決できない場合は安全側に倒して拒否するはず");
    }

    // resolve_paths()自体の失敗はfail-closedにする一方、same_file::is_same_file単体の
    // 失敗(比較対象を開けない等)を.unwrap_or(false)で「同じファイルではない」に丸めると
    // fail-openになる。app_dirの実体が何らかの理由で確認できない場合も安全側に倒して
    // 拒否することを確認する。
    #[test]
    fn validate_export_dest_path_fails_closed_when_app_data_dir_does_not_exist_on_disk() {
        let parent = tempfile::tempdir().unwrap();
        // AppPaths::atはディレクトリの実在を要求しないため、存在しないパスを指定できる。
        let app_paths = AppPaths::at(parent.path().join("does_not_exist"));
        let export_dir = tempfile::tempdir().unwrap();
        let dest = export_dir.path().join("export.smx");

        validate_export_dest_path(dest.to_str().unwrap(), Ok(app_paths))
            .expect_err("データフォルダの実体を確認できない場合は安全側に倒して拒否するはず");
    }

    #[test]
    fn validate_import_source_path_rejects_wrong_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("import.zip");
        let err =
            validate_import_source_path(path.to_str().unwrap()).expect_err(".smx以外の拡張子は拒否されるはず");
        assert!(err.contains(".smx"), "予期しないエラー文言: {err}");
    }

    #[test]
    fn preview_import_rejects_files_larger_than_the_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let state = ProfileStoreState::with_store_for_test(init_store_with_one_profile(dir.path(), "既存"));
        let pending = PendingImportState::default();

        let oversized = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();
        std::fs::write(oversized.path(), vec![0u8; (MAX_IMPORT_FILE_BYTES + 1) as usize]).unwrap();

        let err = preview_import_impl(&state, &pending, oversized.path().to_str().unwrap(), passphrase("any"))
            .expect_err("上限を超えるファイルは拒否されるはず");
        assert_eq!(err.message(), "ファイルサイズが大きすぎます");
        // パスフレーズを試す前の事前検証で弾かれているため、フロントエンドが
        // パスフレーズエラーとして誤表示してはならない種別(InvalidInput)のはず。
        assert!(matches!(err, ExportImportError::InvalidInput(_)));
    }
}
