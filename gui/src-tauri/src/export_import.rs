use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use masking_core::{Mode, PatternType, Rule, RuleProfile};
use profile_store::{
    decrypt_import_payload, decrypt_import_payload_with_key_file, write_key_file, AllImportEntry, AppPaths, DecryptedPayload,
    FileProtection, ImportMethod, ImportPreview, ProfileStoreError, SecretString, KEY_FILE_EXTENSION,
};
use tauri::webview::PageLoadEvent;
use tauri::{Emitter, Manager, RunEvent};

use crate::profiles::{resolve_paths, with_store, ProfileStoreState};
use crate::tray::MAIN_WINDOW_LABEL;

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

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

fn has_smx_extension(path: &Path) -> bool {
    has_extension(path, "smx")
}

// パス検証本体(正規化・特殊形式拒否・アプリのデータフォルダ除外)はprofile_store側で
// CLI(masker export/mask --output)と共有する。ここでは.smx拡張子の要求のみGUI固有。
// データフォルダの解決(resolve_paths。環境変数を読む)は呼び出し側で行い、引数で受け取る
// (テストが、実際のデータフォルダの有無に依存しないようにする。text_file_io.rsのwrite_text_file_impl
// と同じ方針)。
fn validate_export_dest_path(dest_path: &str, app_paths: Result<AppPaths, String>) -> Result<PathBuf, String> {
    validate_dest_path(dest_path, app_paths, "smx", "保存先には拡張子.smxを指定してください")
}

// 鍵ファイルの保存先の検証。エクスポートしたファイルと同じ規則(アプリのデータフォルダの内側には保存しない)で、
// 拡張子だけが違う。
fn validate_key_file_dest_path(key_dest_path: &str, app_paths: Result<AppPaths, String>) -> Result<PathBuf, String> {
    validate_dest_path(key_dest_path, app_paths, KEY_FILE_EXTENSION, "鍵ファイルの保存先には拡張子.smxkeyを指定してください")
}

fn validate_dest_path(
    dest_path: &str,
    app_paths: Result<AppPaths, String>,
    extension: &str,
    wrong_extension_message: &str,
) -> Result<PathBuf, String> {
    let path = profile_store::normalize_and_reject_special_forms(dest_path).map_err(|e| e.to_string())?;
    if !has_extension(&path, extension) {
        return Err(wrong_extension_message.to_string());
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

/// 鍵ファイル(秘密鍵1つとコメントだけの、小さなテキスト)の大きさの上限。これを超えるファイルは、鍵ファイルではない。
const MAX_KEY_FILE_BYTES: u64 = 4096;

/// 復号に使う鍵ファイルを読む。拡張子・大きさ・文字コードを、読む前後に検証する(秘密鍵は、JavaScriptへ渡さない)。
fn read_key_file(key_path: &str) -> Result<SecretString, ExportImportError> {
    let path = profile_store::normalize_and_reject_special_forms(key_path)
        .map_err(|e| ExportImportError::InvalidInput(e.to_string()))?;
    if !has_extension(&path, KEY_FILE_EXTENSION) {
        return Err(ExportImportError::InvalidInput("拡張子が.smxkeyのファイルを選択してください".to_string()));
    }
    let metadata = std::fs::metadata(&path).map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?;
    if metadata.len() > MAX_KEY_FILE_BYTES {
        return Err(ExportImportError::InvalidInput("鍵ファイルのサイズが大きすぎます".to_string()));
    }
    let contents = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::InvalidData {
            ExportImportError::InvalidInput("鍵ファイルの形式が正しくありません".to_string())
        } else {
            ExportImportError::Failed(GENERIC_IO_ERROR.to_string())
        }
    })?;
    Ok(SecretString::from(contents))
}

/// 取り込むファイルの中身を読む。大きすぎるファイルは、読む前に拒否する(メモリ枯渇・ハングを避けるため)。
fn read_import_file(source_path: &Path) -> Result<Vec<u8>, ExportImportError> {
    let metadata = std::fs::metadata(source_path).map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?;
    if metadata.len() > MAX_IMPORT_FILE_BYTES {
        return Err(ExportImportError::InvalidInput("ファイルサイズが大きすぎます".to_string()));
    }
    std::fs::read(source_path).map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))
}

/// エクスポートしたファイルの、復号の方式(ファイルの先頭にある、平文のヘッダーから判別する)。
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMethodDto {
    Passphrase,
    KeyFile,
}

fn detect_import_method_impl(source_path: &str) -> Result<ImportMethodDto, ExportImportError> {
    let source_path = validate_import_source_path(source_path).map_err(ExportImportError::InvalidInput)?;
    let data = read_import_file(&source_path)?;
    match profile_store::detect_import_method(&data) {
        Ok(ImportMethod::Passphrase) => Ok(ImportMethodDto::Passphrase),
        Ok(ImportMethod::KeyFile) => Ok(ImportMethodDto::KeyFile),
        Err(e) => Err(ExportImportError::Failed(e.to_string())),
    }
}

/// 保留として同時に保持する復号済みの内容の件数の上限。復号済みの内容(平文)が、プロセス内に
/// 無制限に溜まらないようにする。上限を超えると、最も古い保留から捨てる。
const MAX_PENDING_IMPORTS: usize = 4;

/// preview_importが復号した内容(ImportPreview。ルール本体を含む)そのものを、IPCで往復させないための保持先。
/// commit_pending_import・clear_pending_importが呼ばれるか、メインウィンドウのページの読み込みが始まる
/// (discard_pending_imports_on_page_load)か、アプリが終了する(discard_pending_imports_on_run_event)までの間だけ
/// メモリ上に置く。
///
/// 復号のたびに識別子を払い出して保持し、確定・破棄は、その識別子で、その保留だけを指す。
/// 画面ごとに復号は独立して走るため、復号が重なっても、互いの保留を取り違えたり消したりしない。
/// 保持できる件数には上限(MAX_PENDING_IMPORTS)がある。
#[derive(Default)]
pub struct PendingImportState(Mutex<PendingImports>);

#[derive(Default)]
struct PendingImports {
    /// 次に払い出す識別子。単調に増え、再利用しない(捨てられた保留の識別子が、後から払い出された
    /// 別の保留を指さないようにするため)。
    next_id: u64,
    /// メインウィンドウのページの読み込みが始まるたび・トレイへ格納されるたびに進める世代。復号を始めた時点の世代と、
    /// 結果を保留へ入れる時点の世代が違えば、復号している間にページが読み込み直された・トレイへ格納された(結果を
    /// 受け取る画面がもう無い)ため、その結果は保留しない(insert_if_generation)。
    page_generation: u64,
    /// 保留を、挿入順(古い順)に持つ。
    entries: VecDeque<(u64, ImportPreview)>,
}

impl PendingImportState {
    fn lock(&self) -> MutexGuard<'_, PendingImports> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// いまのページの世代。復号を始める時点で控え、結果を保留へ入れるとき(insert_if_generation)に渡す。
    fn generation(&self) -> u64 {
        self.lock().page_generation
    }

    /// 復号を始めた時点の世代(generation)が、いまの世代と同じ場合に限り、保留を追加し、払い出した識別子を返す
    /// (違えば、preview を捨ててNone)。識別子の払い出しと挿入は、同じロックの中で行う。
    /// 保持数が上限に達している場合は、最も古い保留を捨ててから追加する。
    fn insert_if_generation(&self, generation: u64, preview: ImportPreview) -> Option<u64> {
        let mut pending = self.lock();
        if pending.page_generation != generation {
            return None;
        }
        let id = pending.next_id;
        pending.next_id += 1;
        if pending.entries.len() >= MAX_PENDING_IMPORTS {
            pending.entries.pop_front();
        }
        pending.entries.push_back((id, preview));
        Some(id)
    }

    /// いまの世代で、保留を追加する(テストが、世代を意識せずに保留を作るため)。
    #[cfg(test)]
    fn insert(&self, preview: ImportPreview) -> u64 {
        let generation = self.generation();
        self.insert_if_generation(generation, preview).expect("いまの世代での挿入は、必ず成功する")
    }

    /// 全ての保留を破棄し、世代を進める(メインウィンドウのページが読み込み直されたとき・トレイへ格納されたとき。
    /// どちらも、結果を受け取る画面が、失われる)。
    fn discard_all_and_advance_generation(&self) {
        let mut pending = self.lock();
        pending.entries.clear();
        pending.page_generation += 1;
    }

    /// 指定した識別子の保留を取り出す(他の保留には触れない)。無ければNone。
    fn take(&self, id: u64) -> Option<ImportPreview> {
        let mut pending = self.lock();
        let index = pending.entries.iter().position(|(entry_id, _)| *entry_id == id)?;
        pending.entries.remove(index).map(|(_, preview)| preview)
    }

    /// Some(id)なら、その識別子の保留だけを、Noneなら、全ての保留を破棄する。
    fn discard(&self, id: Option<u64>) {
        let mut pending = self.lock();
        match id {
            Some(id) => pending.entries.retain(|(entry_id, _)| *entry_id != id),
            None => pending.entries.clear(),
        }
    }
}

/// メインウィンドウのページの読み込みが始まったときに、保留している全ての内容を破棄する。
///
/// ページを読み込み直す(初回の読み込み・再読み込み)と、その画面のJavaScriptの状態が失われ、保留の識別子を
/// 持つ画面が無くなる。確認画面を開いたまま再読み込みされた場合などに、確定も破棄もされなくなった保留は、
/// そのままでは、復号済みの平文(ルールのパターン・固定値)としてRust側に残り続けるため。ページ内の画面遷移
/// (履歴による切り替え)は、WebView2では、ページの読み込みを起こさないので、対象にならない(他のウェブビューでも
/// 同じとは限らない)。
///
/// 読み込みの完了(Finished)では、何もしない。on_page_loadはBuilder全体に登録され、全てのウェブビューへ
/// 適用されるため、メインウィンドウ以外の読み込みでも、何もしない(メインウィンドウの確認画面の保留を、
/// 消さないため)。
pub(crate) fn discard_pending_imports_on_page_load(
    pending: &PendingImportState,
    webview_label: &str,
    event: PageLoadEvent,
) {
    if webview_label == MAIN_WINDOW_LABEL && event == PageLoadEvent::Started {
        pending.discard_all_and_advance_generation();
    }
}

/// アプリの終了の通知(RunEvent::Exit)で、保留している全ての内容を破棄する。
///
/// Tauriは、終了するとき、管理している状態(このPendingImportStateなど)をdropせずに、プロセスを終える。そのため、
/// dropによる消去に任せると、確認されないまま残っていた復号済みの内容(平文)が、消去されずに終了してしまう。
/// 終了の通知以外(取り消されうる終了の要求など)では、何もしない。
pub(crate) fn discard_pending_imports_on_run_event(pending: &PendingImportState, event: &RunEvent) {
    if matches!(event, RunEvent::Exit) {
        pending.discard(None);
    }
}

/// メインウィンドウをトレイへ格納するときに、保留している全ての内容を破棄し、世代を進める。
///
/// 格納しても、WebViewは動き続ける。確認画面を開いたまま格納されると、復号済みの内容(平文)が、再表示されるまで
/// (何時間でも)Rust側に残るため。画面へは、別途、格納したことを知らせ、確認画面を閉じさせる(tray.rsのhide_to_tray)。
/// 世代を進めるのは、格納の前に始めた復号が、格納している間に終わっても、その結果を保留しないため(結果を受け取る
/// 画面は、格納するときに閉じられる。画面側が、閉じた後に届いた結果を捨てる処理は、格納している間、WebViewの処理が
/// 止まっていても、Rust側に保留を残さない、という保証にはならない)。
pub(crate) fn discard_pending_imports_on_hide(pending: &PendingImportState) {
    pending.discard_all_and_advance_generation();
}

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

/// preview_importの結果。pending_idは、保留した復号済みの内容の識別子で、commit_pending_import・
/// clear_pending_importは、この識別子で、その保留だけを指す。passphrase_trimmedは、入力のままでは復号できず、
/// 前後の空白・不可視文字を除いたパスフレーズで復号できたこと(確認画面で、利用者へ知らせる)。
#[derive(Debug, serde::Serialize)]
pub struct PreviewImportResultDto {
    pub pending_id: u64,
    pub preview: ImportPreviewDto,
    pub passphrase_trimmed: bool,
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

/// 鍵ファイル付きのエクスポートの結果。`key_file_restricted`は、鍵ファイルを、所有ユーザーだけの権限にできたか
/// (FAT/exFATのUSBメモリなど、ファイルの権限を持たない保管先では、できない。画面が、利用者へ知らせる)。
#[derive(Debug, serde::Serialize)]
pub struct ExportWithKeyFileResultDto {
    pub key_file_restricted: bool,
}

// 鍵ファイル方式: 新しい鍵を生成し、その鍵宛てに暗号化して、エクスポートしたファイルと、鍵ファイルを書き出す。
// 鍵は、JavaScriptへ渡さない(生成も保存も、Rustが行う)。
// 先に鍵ファイルを書き、次にエクスポートしたファイルを書く。後者の書き込みに失敗したら、鍵ファイルを消す
// (対応するエクスポートしたファイルが無い、鍵ファイルだけが残らないようにするため)。どちらの保存先も、書く前に検証する。
fn export_with_key_file_impl(
    state: &ProfileStoreState,
    app_paths: Result<AppPaths, String>,
    dest_path: &str,
    key_dest_path: &str,
    export: impl FnOnce(&mut profile_store::ProfileStore) -> Result<profile_store::KeyFileExport, ProfileStoreError>,
) -> Result<ExportWithKeyFileResultDto, ExportImportError> {
    let dest_path = validate_export_dest_path(dest_path, app_paths.clone()).map_err(ExportImportError::InvalidInput)?;
    let key_dest_path =
        validate_key_file_dest_path(key_dest_path, app_paths).map_err(ExportImportError::InvalidInput)?;
    let exported = with_store(state, export).map_err(ExportImportError::Failed)?;
    let protection = write_key_file(&key_dest_path, &exported.key_file_contents)
        .map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?;
    if std::fs::write(&dest_path, &exported.ciphertext).is_err() {
        let _ = std::fs::remove_file(&key_dest_path);
        return Err(ExportImportError::Failed(GENERIC_IO_ERROR.to_string()));
    }
    Ok(ExportWithKeyFileResultDto { key_file_restricted: protection == FileProtection::OwnerOnly })
}

fn export_profile_with_key_file_impl(
    state: &ProfileStoreState,
    app_paths: Result<AppPaths, String>,
    name: &str,
    dest_path: &str,
    key_dest_path: &str,
) -> Result<ExportWithKeyFileResultDto, ExportImportError> {
    export_with_key_file_impl(state, app_paths, dest_path, key_dest_path, |store| {
        store.export_profile_with_key_file(name)
    })
}

fn export_all_with_key_file_impl(
    state: &ProfileStoreState,
    app_paths: Result<AppPaths, String>,
    dest_path: &str,
    key_dest_path: &str,
) -> Result<ExportWithKeyFileResultDto, ExportImportError> {
    export_with_key_file_impl(state, app_paths, dest_path, key_dest_path, |store| store.export_all_with_key_file())
}

fn preview_import_impl(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    source_path: &str,
    passphrase: SecretString,
) -> Result<PreviewImportResultDto, ExportImportError> {
    preview_import_with_hook(state, pending, source_path, passphrase, || {})
}

/// preview_import_implの本体。before_decryptは、ページの世代を控えた後、復号を始める直前に呼ぶ。復号している
/// 最中にページが読み込み直される場合(世代を控えた後、結果を保留へ入れる前に、世代が進む場合)を、テストで
/// 再現するための差し込み口で、実際の呼び出しでは、何もしない。
///
/// 復号を始める前のページの世代を控え、結果を保留へ入れるときに、同じ世代であることを確かめる。復号している
/// 最中にページが読み込み直されていれば、その結果を受け取る画面がもう無いため、保留せずに失敗する
/// (復号済みの内容が、確定も破棄もされないまま、Rust側に残らないようにする)。
fn preview_import_with_hook(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    source_path: &str,
    passphrase: SecretString,
    before_decrypt: impl FnOnce(),
) -> Result<PreviewImportResultDto, ExportImportError> {
    let generation = pending.generation();
    preview_import_core(state, pending, generation, source_path, before_decrypt, |data| {
        decrypt_import_payload(data, passphrase)
    })
}

/// 鍵ファイル方式の取り込み(preview_import_with_hookと同じ流れで、復号だけが、鍵ファイルによる)。鍵ファイルは、
/// 復号を始める前に読んで検証する。
fn preview_import_with_key_file_impl(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    source_path: &str,
    key_path: &str,
) -> Result<PreviewImportResultDto, ExportImportError> {
    preview_import_with_key_file_hook(state, pending, source_path, key_path, || {})
}

/// preview_import_with_key_file_implの本体。after_generationは、ページの世代を控えた後、鍵ファイルを読む前に呼ぶ、
/// テスト用の差し込み口(鍵ファイルの読み込みに時間がかかる間[USBメモリなど]に、ページが読み込み直される場合を再現する)。
fn preview_import_with_key_file_hook(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    source_path: &str,
    key_path: &str,
    after_generation: impl FnOnce(),
) -> Result<PreviewImportResultDto, ExportImportError> {
    // 世代は、ファイルを読む前に控える(鍵ファイルの読み込みの間に読み込み直された場合も、その結果を保留しないため)。
    let generation = pending.generation();
    after_generation();
    let key_file_contents = read_key_file(key_path)?;
    preview_import_core(state, pending, generation, source_path, || {}, |data| {
        decrypt_import_payload_with_key_file(data, &key_file_contents)
    })
}

// 取り込みの、復号(パスフレーズ・鍵ファイルのどちらでも)の前後の流れ。generationは、呼び出し側が、ファイルを読む前に控えた、
// ページの世代。decryptは、読み込んだファイルの中身を復号する。
fn preview_import_core(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    generation: u64,
    source_path: &str,
    before_decrypt: impl FnOnce(),
    decrypt: impl FnOnce(&[u8]) -> Result<DecryptedPayload, ProfileStoreError>,
) -> Result<PreviewImportResultDto, ExportImportError> {
    let source_path = validate_import_source_path(source_path).map_err(ExportImportError::InvalidInput)?;
    let data = read_import_file(&source_path)?;
    before_decrypt();
    // パスフレーズ検証(scrypt、数百ms〜数秒)はストアのロックを握らずに行う。ロック内で
    // 実行すると、他のプロファイル/タグ系コマンドがこの間ずっとブロックされてしまう。
    let decrypted = decrypt(&data).map_err(|e| ExportImportError::Failed(e.to_string()))?;
    let passphrase_trimmed = decrypted.passphrase_trimmed;
    let preview = with_store(state, |store| store.resolve_import_preview(decrypted.payload))
        .map_err(ExportImportError::Failed)?;
    let has_active = with_store(state, |store| store.has_active_profile()).map_err(ExportImportError::Failed)?;
    let dto = to_dto(&preview, has_active);
    let pending_id = pending.insert_if_generation(generation, preview).ok_or_else(|| {
        ExportImportError::Failed(
            "ページが読み込み直された、または、ウィンドウがトレイへ格納されたため、復号の結果を破棄しました".to_string(),
        )
    })?;
    Ok(PreviewImportResultDto { pending_id, preview: dto, passphrase_trimmed })
}

/// インポート確定後、実際にどのプロファイルがアクティブになったか(無ければNone)。
/// 無言でのアクティブ化(update_profileと同じ問題意識)に気付けるようにするための情報。
#[derive(Debug, serde::Serialize)]
pub struct CommitImportResultDto {
    pub activated_profile_name: Option<String>,
}

/// 指定した識別子の保留だけを確定する(他の保留には触れない)。その保留が無い(破棄済み・確定済み・
/// 保持数の上限で捨てられた・払い出されていない識別子)場合は失敗する。保留は、確定の成否に関わらず、
/// 取り出した時点で消費される。
fn commit_pending_import_impl(
    state: &ProfileStoreState,
    pending: &PendingImportState,
    pending_id: u64,
) -> Result<CommitImportResultDto, String> {
    let preview = pending.take(pending_id).ok_or_else(|| "確認待ちのインポートがありません".to_string())?;
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

#[tauri::command]
pub async fn export_profile_with_key_file(
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
    dest_path: String,
    key_dest_path: String,
) -> Result<ExportWithKeyFileResultDto, ExportImportError> {
    export_profile_with_key_file_impl(&state, resolve_paths(), &name, &dest_path, &key_dest_path)
}

#[tauri::command]
pub async fn export_all_with_key_file(
    state: tauri::State<'_, ProfileStoreState>,
    dest_path: String,
    key_dest_path: String,
) -> Result<ExportWithKeyFileResultDto, ExportImportError> {
    export_all_with_key_file_impl(&state, resolve_paths(), &dest_path, &key_dest_path)
}

/// エクスポートしたファイルの、復号の方式を判別する(ファイルの先頭の平文ヘッダーだけで判別するため、鍵もパスフレーズも要らない)。
#[tauri::command]
pub async fn detect_import_method(source_path: String) -> Result<ImportMethodDto, ExportImportError> {
    detect_import_method_impl(&source_path)
}

/// preview_importの、鍵ファイル方式(鍵ファイルのパスを受け取り、鍵は、Rustが読む)。
#[tauri::command]
pub async fn preview_import_with_key_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    source_path: String,
    key_path: String,
) -> Result<PreviewImportResultDto, ExportImportError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ProfileStoreState>();
        let pending = app.state::<PendingImportState>();
        preview_import_with_key_file_impl(&state, &pending, &source_path, &key_path)
    })
    .await
    .map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?
}

// preview_import・commit_pending_importは、AppHandleのランタイムを型引数(R)で受け取る。実アプリ(Wry)と、
// テスト用のtauri::test::MockRuntimeの、どちらでも、同じコマンドをIPCの境界(JSON)を通して呼べるようにするため。

/// DBはまだ変更しない。復号結果(ImportPreview)はPendingImportStateに保持し、フロントエンドには、
/// 確認画面の表示用のDTO(名前・ルールの中身・タグなど)と、その保留の識別子(pending_id)だけを返す。
/// 復号済みの内容そのもの(ImportPreview)は往復させない(確定は、識別子だけを受け取る)。
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
pub async fn preview_import<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    source_path: String,
    passphrase: SecretString,
) -> Result<PreviewImportResultDto, ExportImportError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ProfileStoreState>();
        let pending = app.state::<PendingImportState>();
        preview_import_impl(&state, &pending, &source_path, passphrase)
    })
    .await
    .map_err(|_| ExportImportError::Failed(GENERIC_IO_ERROR.to_string()))?
}

#[tauri::command]
pub async fn commit_pending_import<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, ProfileStoreState>,
    pending: tauri::State<'_, PendingImportState>,
    pending_id: u64,
) -> Result<CommitImportResultDto, String> {
    let result = commit_pending_import_impl(&state, &pending, pending_id)?;
    let _ = app.emit("profiles-changed", ());
    let _ = app.emit("tags-changed", ());
    Ok(result)
}

/// 保留を破棄する。preview_importが復号した平文(ルール本体を含む)をプロセス内に残さないためと、
/// 破棄した保留に対して、後からcommit_pending_importが呼ばれても確定しないようにするため
/// (意思決定をRust側の状態にも反映する)。
///
/// Some(id)なら、その識別子の保留だけを破棄する(他の保留は消さない)。画面からは、この形で呼ばれる:
/// 確認画面の取り消し、画面を離れるとき(その画面が所有する保留)、閉じられた画面(離れた画面)へ
/// 遅れて届いた復号結果の保留、復号の結果が届いたときに、確認されないまま残っていた前の保留(上書きする
/// 前に)。Noneなら、全ての保留を破棄する。画面は使わず、E2Eの後片付けが、全消去のために使う。
///
/// 保留が無い場合も含め常に成功する(呼び出し側が、「無かったこと」をエラーとして扱わなくてよいように、
/// 副作用の無い操作として設計する)。
fn clear_pending_import_impl(pending: &PendingImportState, pending_id: Option<u64>) {
    pending.discard(pending_id);
}

#[tauri::command]
pub async fn clear_pending_import(
    pending: tauri::State<'_, PendingImportState>,
    pending_id: Option<u64>,
) -> Result<(), String> {
    clear_pending_import_impl(&pending, pending_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use masking_core::{Mode, PatternType, Rule, RuleProfile};
    use profile_store::{AppPaths, ProfileStore};
    use serde_json::json;

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

    const TEST_PASSPHRASE: &str = "correct horse battery staple";

    /// ルール1件のプロファイル1件を書き出した、.smxのファイル。
    fn export_profile_smx(profile_name: &str) -> tempfile::NamedTempFile {
        let source_dir = tempfile::tempdir().unwrap();
        // source_pathの検証が.smx拡張子を要求するため、テスト用の一時ファイルも
        // 実際の運用(保存ダイアログのフィルタで常に.smxになる)に合わせる。
        let file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();
        let source_state =
            ProfileStoreState::with_store_for_test(init_store_with_one_profile(source_dir.path(), profile_name));
        export_profile_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            profile_name,
            passphrase(TEST_PASSPHRASE),
            file.path().to_str().unwrap(),
        )
        .expect("エクスポートは成功するはず");
        file
    }

    /// 何も取り込まれていない、取り込み先のストア。
    struct EmptyDestination {
        state: ProfileStoreState,
        // stateがDBを閉じた後に、フォルダを消す(フィールドは、宣言した順に破棄される)。
        _dir: tempfile::TempDir,
    }

    impl EmptyDestination {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = AppPaths::at(dir.path());
            profile_store::init_at(&paths).unwrap();
            let store = ProfileStore::open_at(&paths).unwrap();
            Self { state: ProfileStoreState::with_store_for_test(store), _dir: dir }
        }

        fn preview(&self, pending: &PendingImportState, file: &tempfile::NamedTempFile) -> PreviewImportResultDto {
            preview_import_impl(&self.state, pending, file.path().to_str().unwrap(), passphrase(TEST_PASSPHRASE))
                .expect("正しいパスフレーズでのpreviewは成功するはず")
        }

        fn profile_names(&self) -> Vec<String> {
            with_store(&self.state, |s| s.list_profiles()).unwrap().into_iter().map(|p| p.name).collect()
        }
    }

    /// 保留に入れる、実際のImportPreview(1件のプロファイルを、空の取り込み先へプレビューしたもの)。
    fn sample_import_preview(profile_name: &str) -> ImportPreview {
        let source_dir = tempfile::tempdir().unwrap();
        let source_store = init_store_with_one_profile(source_dir.path(), profile_name);
        let bytes = source_store.export_profile(profile_name, passphrase(TEST_PASSPHRASE)).unwrap();
        let dest = EmptyDestination::new();
        with_store(&dest.state, |store| store.preview_import(&bytes, passphrase(TEST_PASSPHRASE))).unwrap()
    }

    /// いま保持している保留の件数。
    fn pending_count(pending: &PendingImportState) -> usize {
        pending.0.lock().unwrap().entries.len()
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

        let preview_result = preview_import_impl(
            &dest_state,
            &pending,
            export_file.path().to_str().unwrap(),
            passphrase("correct horse battery staple"),
        )
        .expect("正しいパスフレーズでのpreviewは成功するはず");
        match preview_result.preview {
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

        let result = commit_pending_import_impl(&dest_state, &pending, preview_result.pending_id)
            .expect("commitは成功するはず");
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
        .expect("正しいパスフレーズでのpreviewは成功するはず")
        .preview;

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
        .expect("正しいパスフレーズでのpreviewは成功するはず")
        .preview;

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
        .expect("正しいパスフレーズでのpreviewは成功するはず")
        .preview;

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

        // previewが失敗した場合、保留が残っていてはならない(どの識別子でも、確定できない)。
        assert_eq!(pending_count(&pending), 0);
        let commit_err = commit_pending_import_impl(&source_state, &pending, 0)
            .expect_err("previewが無いのでcommitも失敗するはず");
        assert_eq!(commit_err, "確認待ちのインポートがありません");
    }

    #[test]
    fn commit_pending_import_without_a_prior_preview_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let store = init_store_with_one_profile(dir.path(), "既存プロファイル");
        let state = ProfileStoreState::with_store_for_test(store);
        let pending = PendingImportState::default();

        let err = commit_pending_import_impl(&state, &pending, 0).expect_err("previewを呼んでいないので失敗するはず");
        assert_eq!(err, "確認待ちのインポートがありません");
    }

    #[test]
    fn clear_pending_import_prevents_a_later_commit_from_succeeding() {
        // キャンセル操作をclear_pending_importで反映した後は、それより後に
        // commit_pending_importが呼ばれても(直接IPCを叩く経路を含め)確定しないはず。
        let export_file = export_profile_smx("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();
        let pending_id = dest.preview(&pending, &export_file).pending_id;

        clear_pending_import_impl(&pending, Some(pending_id));

        let err = commit_pending_import_impl(&dest.state, &pending, pending_id)
            .expect_err("キャンセル済みのはずなのでcommitは拒否されるはず");
        assert_eq!(err, "確認待ちのインポートがありません");
        assert!(dest.profile_names().is_empty(), "拒否したのに取り込まれている");
    }

    // 別の識別子の確定は、失敗し、指定していない保留を消費しない(その保留は、その識別子で、そのまま確定できる)。
    #[test]
    fn commit_with_a_different_id_fails_and_does_not_consume_the_pending_import() {
        let export_file = export_profile_smx("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();
        let pending_id = dest.preview(&pending, &export_file).pending_id;

        let err = commit_pending_import_impl(&dest.state, &pending, pending_id + 1)
            .expect_err("払い出されていない識別子の確定は失敗するはず");
        assert_eq!(err, "確認待ちのインポートがありません");
        assert!(dest.profile_names().is_empty(), "別の識別子の確定で、取り込まれている");
        assert_eq!(pending_count(&pending), 1, "別の識別子の確定が、保留を消費している");

        commit_pending_import_impl(&dest.state, &pending, pending_id).expect("本来の識別子なら、確定できるはず");
        assert_eq!(dest.profile_names(), vec!["元プロファイル".to_string()]);
    }

    // 別の識別子の破棄は、何もしない(その保留は、その識別子で、そのまま確定できる)。
    #[test]
    fn clear_with_a_different_id_does_nothing() {
        let export_file = export_profile_smx("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();
        let pending_id = dest.preview(&pending, &export_file).pending_id;

        clear_pending_import_impl(&pending, Some(pending_id + 1));
        assert_eq!(pending_count(&pending), 1, "別の識別子の破棄が、保留を消している");

        commit_pending_import_impl(&dest.state, &pending, pending_id).expect("本来の識別子なら、確定できるはず");
        assert_eq!(dest.profile_names(), vec!["元プロファイル".to_string()]);
    }

    // 復号が重なった2件(識別子の違う2つの保留)は、互いに独立して、どの順でも確定できる。
    // 確定は、指定した識別子の内容だけを取り込む(取り違えない)。
    #[test]
    fn two_pending_imports_commit_independently_in_either_order() {
        let file_a = export_profile_smx("プロファイルA");
        let file_b = export_profile_smx("プロファイルB");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();
        let id_a = dest.preview(&pending, &file_a).pending_id;
        let id_b = dest.preview(&pending, &file_b).pending_id;
        assert_ne!(id_a, id_b, "復号のたびに、別の識別子が払い出されるはず");

        commit_pending_import_impl(&dest.state, &pending, id_b).expect("Bの確定は成功するはず");
        assert_eq!(dest.profile_names(), vec!["プロファイルB".to_string()], "Bの識別子で、Aが取り込まれている");

        commit_pending_import_impl(&dest.state, &pending, id_a).expect("Bの確定の後でも、Aの確定は成功するはず");
        let mut names = dest.profile_names();
        names.sort();
        assert_eq!(names, vec!["プロファイルA".to_string(), "プロファイルB".to_string()]);
        assert_eq!(pending_count(&pending), 0);
    }

    // 一方の破棄は、もう一方の保留に影響しない。先に復号が終わった側(識別子が小さい方)を破棄しても、
    // 後から復号が終わった側(識別子が大きい方)を破棄しても、残った側は確定できる。
    #[test]
    fn discarding_one_pending_import_leaves_the_other_committable() {
        let file_a = export_profile_smx("プロファイルA");
        let file_b = export_profile_smx("プロファイルB");
        for discard_the_earlier in [true, false] {
            let dest = EmptyDestination::new();
            let pending = PendingImportState::default();
            let earlier = dest.preview(&pending, &file_a).pending_id;
            let later = dest.preview(&pending, &file_b).pending_id;
            let (discarded, kept, kept_name) = if discard_the_earlier {
                (earlier, later, "プロファイルB")
            } else {
                (later, earlier, "プロファイルA")
            };

            clear_pending_import_impl(&pending, Some(discarded));

            let err = commit_pending_import_impl(&dest.state, &pending, discarded)
                .expect_err("破棄した保留の確定は失敗するはず");
            assert_eq!(err, "確認待ちのインポートがありません");
            commit_pending_import_impl(&dest.state, &pending, kept).expect("破棄していない保留は、確定できるはず");
            assert_eq!(dest.profile_names(), vec![kept_name.to_string()]);
        }
    }

    // 確定は、成否に関わらず、指定した保留を消費する(同じ識別子での再度の確定は、失敗する)。
    #[test]
    fn commit_consumes_the_pending_import_even_when_the_commit_fails() {
        let export_file = export_profile_smx("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();
        let pending_id = dest.preview(&pending, &export_file).pending_id;
        // 確認から確定までの間に、同名のプロファイルが作られると、確定は失敗する。
        with_store(&dest.state, |store| {
            store.create_profile(&RuleProfile::new("元プロファイル", None, vec![]).unwrap())
        })
        .unwrap();

        commit_pending_import_impl(&dest.state, &pending, pending_id).expect_err("同名があるので、確定は失敗するはず");

        assert_eq!(pending_count(&pending), 0, "失敗した確定が、保留を残している");
        let err = commit_pending_import_impl(&dest.state, &pending, pending_id)
            .expect_err("消費済みの保留は、再度の確定で失敗するはず");
        assert_eq!(err, "確認待ちのインポートがありません");
    }

    // 保持数の上限を超えると、最も古い保留だけが捨てられ、その確定は、既存と同じ文言で失敗する。
    #[test]
    fn the_oldest_pending_import_is_dropped_when_the_limit_is_exceeded() {
        let sample = sample_import_preview("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let ids: Vec<u64> = (0..=MAX_PENDING_IMPORTS).map(|_| pending.insert(sample.clone())).collect();

        assert_eq!(pending_count(&pending), MAX_PENDING_IMPORTS, "保持数が、上限を超えている");
        let err = commit_pending_import_impl(&dest.state, &pending, ids[0])
            .expect_err("捨てられた最古の保留の確定は失敗するはず");
        assert_eq!(err, "確認待ちのインポートがありません");
        assert!(dest.profile_names().is_empty(), "捨てられた保留が、取り込まれている");

        // 最古以外は、全て残っている。
        commit_pending_import_impl(&dest.state, &pending, ids[MAX_PENDING_IMPORTS])
            .expect("最新の保留は、確定できるはず");
        for id in &ids[1..MAX_PENDING_IMPORTS] {
            assert!(pending.take(*id).is_some(), "上限内の保留(識別子{id})が、捨てられている");
        }
    }

    // 保持できる件数は、docs/gui/README.mdが約束している値(4件)。この値を変えるときは、docsの記述も直す。
    #[test]
    fn the_pending_limit_is_the_documented_value() {
        assert_eq!(MAX_PENDING_IMPORTS, 4);
    }

    // 保持数が上限に達するまでは、何も捨てない。
    #[test]
    fn no_pending_import_is_dropped_up_to_the_limit() {
        let sample = sample_import_preview("元プロファイル");
        let pending = PendingImportState::default();

        let ids: Vec<u64> = (0..MAX_PENDING_IMPORTS).map(|_| pending.insert(sample.clone())).collect();

        assert_eq!(pending_count(&pending), MAX_PENDING_IMPORTS);
        for id in ids {
            assert!(pending.take(id).is_some(), "上限内の保留(識別子{id})が、捨てられている");
        }
    }

    // 識別子は、取り出し・破棄・上限による廃棄の後も、再利用しない(古い識別子が、後から払い出された
    // 別の保留を指さないようにする)。
    #[test]
    fn an_id_is_never_reused_after_its_pending_import_is_gone() {
        let sample = sample_import_preview("元プロファイル");
        let pending = PendingImportState::default();

        let first = pending.insert(sample.clone());
        assert!(pending.take(first).is_some());
        let second = pending.insert(sample.clone());
        pending.discard(Some(second));
        let mut issued = vec![first, second];
        for _ in 0..=MAX_PENDING_IMPORTS {
            issued.push(pending.insert(sample.clone()));
        }

        let mut unique = issued.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), issued.len(), "識別子が再利用されている: {issued:?}");
        assert!(issued.windows(2).all(|pair| pair[0] < pair[1]), "識別子が、払い出した順に増えていない: {issued:?}");

        // 取り出し・破棄で消えた保留の識別子は、何も指さない。
        assert!(pending.take(first).is_none());
        assert!(pending.take(second).is_none());

        // 上限で捨てられた保留(issued[2]。上限を超えた時点で最も古かった保留)の識別子は、その後に払い出された
        // どの識別子とも一致せず、捨てられた後に払い出された保留も指さない。
        let dropped_by_limit = issued[2];
        assert_eq!(pending_count(&pending), MAX_PENDING_IMPORTS, "上限を超えた状態になっていない");
        assert!(pending.take(dropped_by_limit).is_none(), "上限で捨てられた保留を、取り出せている");
        assert!(
            issued[3..].iter().all(|id| *id != dropped_by_limit),
            "上限で捨てられた識別子が、後から払い出されている: {issued:?}"
        );
        let after_the_drop = pending.insert(sample.clone());
        assert!(
            issued.iter().all(|id| *id < after_the_drop),
            "上限による廃棄の後に、識別子が再利用されている: {after_the_drop}, {issued:?}"
        );
        assert!(
            pending.take(dropped_by_limit).is_none(),
            "上限で捨てられた識別子が、後から払い出された保留を指している"
        );
        assert!(pending.take(after_the_drop).is_some());
    }

    // 識別子を指定しない破棄は、全ての保留を破棄する(E2Eの後片付けなどの全消去用)。
    #[test]
    fn clearing_without_an_id_discards_every_pending_import() {
        let sample = sample_import_preview("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();
        let ids: Vec<u64> = (0..MAX_PENDING_IMPORTS).map(|_| pending.insert(sample.clone())).collect();

        clear_pending_import_impl(&pending, None);

        assert_eq!(pending_count(&pending), 0);
        for id in &ids {
            let err = commit_pending_import_impl(&dest.state, &pending, *id)
                .expect_err("全消去の後は、どの保留も確定できないはず");
            assert_eq!(err, "確認待ちのインポートがありません");
        }
        assert!(dest.profile_names().is_empty());

        // 全消去の後に払い出す識別子は、全消去の前に払い出したどの識別子より大きい(全消去の前の識別子が、
        // 後から払い出された保留を指さないようにする)。
        let after_clear = pending.insert(sample.clone());
        let max_before_clear = *ids.iter().max().unwrap();
        assert!(
            after_clear > max_before_clear,
            "全消去の後に、識別子が再利用されている: {after_clear}, {ids:?}"
        );
        for id in &ids {
            assert!(
                pending.take(*id).is_none(),
                "全消去の前の識別子(識別子{id})が、後から払い出された保留を指している"
            );
        }
        assert!(pending.take(after_clear).is_some());
    }

    /// 上限いっぱいまで、保留を挿入する。挿入した識別子を返す。
    fn fill_pending(pending: &PendingImportState) -> Vec<u64> {
        let sample = sample_import_preview("元プロファイル");
        (0..MAX_PENDING_IMPORTS).map(|_| pending.insert(sample.clone())).collect()
    }

    // メインウィンドウのページの読み込みが始まったとき(初回の読み込み・再読み込み)は、保留の識別子を持つ
    // 画面が無くなるため、全ての保留を破棄する。
    #[test]
    fn the_start_of_a_main_window_page_load_discards_every_pending_import() {
        let pending = PendingImportState::default();
        let ids = fill_pending(&pending);

        discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Started);

        assert_eq!(pending_count(&pending), 0);
        for id in ids {
            assert!(pending.take(id).is_none(), "ページの読み込みの開始の後に、保留(識別子{id})が残っている");
        }
    }

    // 読み込みの完了では、破棄しない(読み込みの間に届いた保留を、完了の通知で消さない)。
    #[test]
    fn the_end_of_a_main_window_page_load_keeps_every_pending_import() {
        let pending = PendingImportState::default();
        let ids = fill_pending(&pending);

        discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Finished);

        assert_eq!(pending_count(&pending), MAX_PENDING_IMPORTS);
        for id in ids {
            assert!(pending.take(id).is_some(), "ページの読み込みの完了で、保留(識別子{id})が消えている");
        }
    }

    // メインウィンドウ以外のウェブビューの読み込みでは、メインウィンドウの保留を破棄しない。
    #[test]
    fn a_page_load_of_another_webview_keeps_every_pending_import() {
        let pending = PendingImportState::default();
        let ids = fill_pending(&pending);

        discard_pending_imports_on_page_load(&pending, "other", PageLoadEvent::Started);

        assert_eq!(pending_count(&pending), MAX_PENDING_IMPORTS);
        for id in ids {
            assert!(pending.take(id).is_some(), "別のウェブビューの読み込みで、保留(識別子{id})が消えている");
        }
    }

    // アプリの終了の通知では、Tauriが管理している状態をdropしないため、全ての保留を明示的に破棄する。
    #[test]
    fn the_exit_event_discards_every_pending_import() {
        let pending = PendingImportState::default();
        let ids = fill_pending(&pending);

        discard_pending_imports_on_run_event(&pending, &RunEvent::Exit);

        assert_eq!(pending_count(&pending), 0);
        for id in ids {
            assert!(pending.take(id).is_none(), "終了の通知の後に、保留(識別子{id})が残っている");
        }
    }

    // トレイへ格納するときは、確認画面を開いたままでも、復号済みの内容が、格納している間に残らないよう、全ての保留を破棄する。
    #[test]
    fn hiding_to_the_tray_discards_every_pending_import() {
        let pending = PendingImportState::default();
        let ids = fill_pending(&pending);

        discard_pending_imports_on_hide(&pending);

        assert_eq!(pending_count(&pending), 0);
        for id in ids {
            assert!(pending.take(id).is_none(), "トレイへ格納した後に、保留(識別子{id})が残っている");
        }
    }

    // 格納の前に始めた復号が、格納している間に終わっても、その結果は保留しない(結果を受け取る画面は、格納するときに
    // 閉じられる)。格納は、世代を進める。
    #[test]
    fn a_result_decrypted_before_hiding_to_the_tray_is_not_kept() {
        let pending = PendingImportState::default();
        let generation = pending.generation();

        discard_pending_imports_on_hide(&pending);

        assert!(pending.insert_if_generation(generation, sample_import_preview("元プロファイル")).is_none());
        assert_eq!(pending_count(&pending), 0);
    }

    // 対照: 格納の後(再表示された後)に始めた復号の結果は、保留できる。
    #[test]
    fn a_result_decrypted_after_hiding_to_the_tray_is_kept() {
        let pending = PendingImportState::default();
        discard_pending_imports_on_hide(&pending);
        let generation = pending.generation();

        assert!(pending.insert_if_generation(generation, sample_import_preview("元プロファイル")).is_some());
    }

    // 復号している最中にトレイへ格納されても、preview_importは、結果を保留せずに失敗する(パスフレーズ方式・鍵ファイル方式とも)。
    #[test]
    fn preview_import_fails_and_keeps_nothing_when_hidden_to_the_tray_while_decrypting() {
        let file = export_profile_smx("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let result = preview_import_with_hook(
            &dest.state,
            &pending,
            file.path().to_str().unwrap(),
            passphrase(TEST_PASSPHRASE),
            || discard_pending_imports_on_hide(&pending),
        );

        assert!(result.is_err(), "復号している最中にトレイへ格納されたのに、結果が返った");
        assert_eq!(pending_count(&pending), 0);
    }

    // 鍵ファイルを読んでいる間に読み込み直された場合も、結果を保留しない(世代は、鍵ファイルを読む前に控える)。
    #[test]
    fn a_key_file_preview_fails_and_keeps_nothing_when_the_page_is_reloaded_while_reading_the_key_file() {
        let files = export_with_key_file_files("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let result = preview_import_with_key_file_hook(
            &dest.state,
            &pending,
            files.smx.to_str().unwrap(),
            files.key.to_str().unwrap(),
            || discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Started),
        );

        assert!(result.is_err(), "鍵ファイルを読んでいる間にページが読み込み直されたのに、結果が返った");
        assert_eq!(pending_count(&pending), 0);
    }

    // ×ボタンの「閉じる要求」は、閉じずに(prevent_closeを呼び)、トレイへ格納し、保留を全て破棄する。
    #[test]
    fn a_close_request_of_the_main_window_hides_it_to_the_tray_and_discards_every_pending_import() {
        use std::cell::Cell;
        use tauri::Manager;

        let app = tauri::test::mock_builder()
            .manage(PendingImportState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("MockRuntimeのアプリを組み立てられるはず");
        let webview =
            tauri::WebviewWindowBuilder::new(&app, crate::tray::MAIN_WINDOW_LABEL, Default::default())
                .build()
                .expect("MockRuntimeのウィンドウを作れるはず");
        let window = webview.as_ref().window();
        let pending = app.state::<PendingImportState>();
        fill_pending(&pending);
        let prevented = Cell::new(0);

        crate::tray::on_main_window_close_requested(&window, || prevented.set(prevented.get() + 1));

        assert_eq!(prevented.get(), 1, "閉じる要求は、ちょうど1回、止められるはず");
        assert_eq!(pending_count(&pending), 0, "格納した後に、保留が残っている");
    }

    // 終了以外の通知(起動・再開・イベント処理の区切り)では、破棄しない。
    #[test]
    fn run_events_other_than_exit_keep_every_pending_import() {
        let pending = PendingImportState::default();
        fill_pending(&pending);

        for event in [RunEvent::Ready, RunEvent::Resumed, RunEvent::MainEventsCleared] {
            discard_pending_imports_on_run_event(&pending, &event);
        }

        assert_eq!(pending_count(&pending), MAX_PENDING_IMPORTS);
    }

    // 保留が無いときは、何も起きない(その後の保留は、通常どおり払い出し、確定できる)。
    #[test]
    fn the_start_of_a_page_load_without_pending_imports_does_nothing() {
        let pending = PendingImportState::default();

        discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Started);

        assert_eq!(pending_count(&pending), 0);
        let id = pending.insert(sample_import_preview("元プロファイル"));
        assert!(pending.take(id).is_some());
    }

    // 復号している最中にメインウィンドウのページが読み込み直されると、その復号の結果を受け取る画面が
    // もう無い。復号を始めた時点のページの世代と、結果を保留へ入れる時点の世代が違えば、その結果は保留しない。
    #[test]
    fn a_result_decrypted_before_a_page_load_started_is_not_kept() {
        let pending = PendingImportState::default();
        let generation = pending.generation();

        discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Started);

        assert!(pending.insert_if_generation(generation, sample_import_preview("元プロファイル")).is_none());
        assert_eq!(pending_count(&pending), 0);
    }

    #[test]
    fn a_result_decrypted_within_the_same_page_is_kept() {
        let pending = PendingImportState::default();
        let generation = pending.generation();

        let id = pending
            .insert_if_generation(generation, sample_import_preview("元プロファイル"))
            .expect("同じページの中で復号した結果は、保留されるはず");

        assert!(pending.take(id).is_some());
    }

    // ページの世代が進むのは、メインウィンドウの読み込みの開始だけ。読み込みの完了・別のウェブビューの読み込み・
    // 画面の外からの全消去(E2Eの後片付け)は、復号している最中の結果を、捨てない。
    #[test]
    fn only_the_start_of_a_main_window_page_load_advances_the_page_generation() {
        let pending = PendingImportState::default();
        let generation = pending.generation();

        discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Finished);
        discard_pending_imports_on_page_load(&pending, "other", PageLoadEvent::Started);
        pending.discard(None);

        assert_eq!(pending.generation(), generation);
        assert!(pending.insert_if_generation(generation, sample_import_preview("元プロファイル")).is_some());
    }

    // 世代を控えた後、復号を始める直前にページが読み込み直されると(復号している最中の再読み込みと同じく、
    // 世代を控えた後に、結果を保留へ入れる前に世代が進む)、preview_importは、結果を保留せずに失敗する
    // (応答を受け取る画面がもう無いため、復号済みの内容が、Rust側に残らない)。世代を控える位置が、差し込み口
    // より後(復号の後を含む)にずれると、この読み込みを検出できなくなり、このテストが落ちる。
    #[test]
    fn preview_import_fails_and_keeps_nothing_when_the_page_is_reloaded_while_decrypting() {
        let file = export_profile_smx("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let result = preview_import_with_hook(
            &dest.state,
            &pending,
            file.path().to_str().unwrap(),
            passphrase(TEST_PASSPHRASE),
            || discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Started),
        );

        assert!(result.is_err(), "復号している最中にページが読み込み直されたのに、結果が返った");
        assert_eq!(pending_count(&pending), 0);
    }

    // 対照: 復号している最中に、世代を進めない読み込みの完了が届いても、結果は保留される。
    #[test]
    fn preview_import_keeps_the_result_when_no_new_page_started_while_decrypting() {
        let file = export_profile_smx("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let result = preview_import_with_hook(
            &dest.state,
            &pending,
            file.path().to_str().unwrap(),
            passphrase(TEST_PASSPHRASE),
            || discard_pending_imports_on_page_load(&pending, MAIN_WINDOW_LABEL, PageLoadEvent::Finished),
        )
        .expect("ページが読み込み直されなければ、結果は保留されるはず");

        assert_eq!(pending_count(&pending), 1);
        assert!(pending.take(result.pending_id).is_some());
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

    // 以下は、実際のコマンド(#[tauri::command]の関数)を、tauri::testのMockRuntime上で、IPCの境界(JSON)を通して
    // 呼ぶテスト。フロントエンドとの取り決め(応答のキー名・引数のキー名・エラーの形)は、*_implを直接呼ぶテストでも、
    // フロントエンド側のinvokeを差し替えるテストでも、確かめられない。

    /// 本番と同じ3つのコマンド(preview_import・commit_pending_import・clear_pending_import)と状態を登録した、
    /// MockRuntime上のアプリ。取り込み先は、空のストア(一時フォルダ内)で、開発機の実際のデータフォルダや
    /// 環境変数には依存しない。
    struct IpcHarness {
        // フィールドは、宣言した順に破棄される(ウィンドウ、アプリ(ストアがDBを閉じる)、フォルダの順)。
        webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
        app: tauri::App<tauri::test::MockRuntime>,
        _dir: tempfile::TempDir,
    }

    impl IpcHarness {
        fn new() -> Self {
            let EmptyDestination { state, _dir } = EmptyDestination::new();
            let app = tauri::test::mock_builder()
                .manage(state)
                .manage(PendingImportState::default())
                .invoke_handler(tauri::generate_handler![
                    preview_import,
                    commit_pending_import,
                    clear_pending_import,
                    detect_import_method,
                    preview_import_with_key_file
                ])
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .expect("MockRuntimeのアプリを組み立てられるはず");
            let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .expect("MockRuntimeのウィンドウを作れるはず");
            Self { webview, app, _dir }
        }

        /// コマンドを、JSONの引数で呼ぶ。成功なら応答のJSON、失敗ならエラーのJSONを返す。
        fn invoke(&self, cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
            let request = tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: if cfg!(any(windows, target_os = "android")) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            tauri::test::get_ipc_response(&self.webview, request)
                .map(|body| body.deserialize::<serde_json::Value>().expect("応答は、JSONのはず"))
        }

        /// .smxのファイルを、正しいパスフレーズでpreview_importへ渡した、応答(JSON)。
        fn preview(&self, file: &tempfile::NamedTempFile) -> serde_json::Value {
            self.invoke(
                "preview_import",
                json!({ "sourcePath": file.path().to_str().unwrap(), "passphrase": TEST_PASSPHRASE }),
            )
            .expect("正しいパスフレーズでのpreview_importは成功するはず")
        }

        /// preview_importの応答から、保留の識別子を取り出す。
        fn preview_pending_id(&self, file: &tempfile::NamedTempFile) -> u64 {
            self.preview(file)["pending_id"].as_u64().expect("応答のpending_idは、整数のはず")
        }

        /// 取り込み先にあるプロファイルの名前(昇順)。
        fn profile_names(&self) -> Vec<String> {
            let state = self.app.state::<ProfileStoreState>();
            let mut names: Vec<String> =
                with_store(&state, |s| s.list_profiles()).unwrap().into_iter().map(|p| p.name).collect();
            names.sort();
            names
        }

        /// いま保持している保留の件数。
        fn pending_count(&self) -> usize {
            let pending = self.app.state::<PendingImportState>();
            pending_count(&pending)
        }
    }

    /// プロファイルを、鍵ファイル方式で書き出した、.smxのファイルと、.smxkeyの鍵ファイル。フォルダごと返す(消えないように)。
    struct KeyFileExportFiles {
        _dir: tempfile::TempDir,
        smx: PathBuf,
        key: PathBuf,
    }

    fn export_with_key_file_files(profile_name: &str) -> KeyFileExportFiles {
        let source_dir = tempfile::tempdir().unwrap();
        let source_state =
            ProfileStoreState::with_store_for_test(init_store_with_one_profile(source_dir.path(), profile_name));
        let dir = tempfile::tempdir().unwrap();
        let smx = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        export_profile_with_key_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            profile_name,
            smx.to_str().unwrap(),
            key.to_str().unwrap(),
        )
        .expect("鍵ファイル付きのエクスポートは成功するはず");
        KeyFileExportFiles { _dir: dir, smx, key }
    }

    #[test]
    fn detect_import_method_tells_passphrase_files_from_key_files() {
        let passphrase_file = export_profile_smx("元プロファイル");
        let key_files = export_with_key_file_files("元プロファイル");

        assert_eq!(detect_import_method_impl(passphrase_file.path().to_str().unwrap()).unwrap(), ImportMethodDto::Passphrase);
        assert_eq!(detect_import_method_impl(key_files.smx.to_str().unwrap()).unwrap(), ImportMethodDto::KeyFile);
    }

    #[test]
    fn detect_import_method_rejects_the_wrong_extension_and_files_that_are_not_exports() {
        let dir = tempfile::tempdir().unwrap();
        let wrong_extension = dir.path().join("export.txt");
        std::fs::write(&wrong_extension, b"anything").unwrap();
        let not_an_export = dir.path().join("plain.smx");
        std::fs::write(&not_an_export, b"not an age file at all").unwrap();

        let err = detect_import_method_impl(wrong_extension.to_str().unwrap()).expect_err("拡張子が違う");
        assert!(matches!(err, ExportImportError::InvalidInput(_)));
        let err = detect_import_method_impl(not_an_export.to_str().unwrap()).expect_err("エクスポートしたファイルではない");
        assert!(matches!(err, ExportImportError::Failed(_)));
    }

    #[test]
    fn a_key_file_export_writes_both_files_and_the_key_file_opens_the_export() {
        let files = export_with_key_file_files("元プロファイル");

        let key = read_key_file(files.key.to_str().unwrap()).unwrap();
        let data = std::fs::read(&files.smx).unwrap();
        let decrypted = decrypt_import_payload_with_key_file(&data, &key).expect("鍵ファイルで復号できるはず");
        assert!(matches!(decrypted.payload, profile_store::ExportPayload::Single { .. }));
        assert!(std::fs::read_to_string(&files.key).unwrap().contains("AGE-SECRET-KEY-1"));
    }

    #[test]
    fn a_key_file_export_of_all_profiles_writes_both_files() {
        let source_dir = tempfile::tempdir().unwrap();
        let source_state = ProfileStoreState::with_store_for_test(init_store_with_two_profiles(source_dir.path()));
        let dir = tempfile::tempdir().unwrap();
        let smx = dir.path().join("all.smx");
        let key = dir.path().join("all.smxkey");

        let result = export_all_with_key_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            smx.to_str().unwrap(),
            key.to_str().unwrap(),
        )
        .expect("全体の鍵ファイル付きのエクスポートは成功するはず");

        assert!(result.key_file_restricted, "既定の一時フォルダ(NTFS・ext4など)では、権限を制限できるはず");
        let data = std::fs::read(&smx).unwrap();
        let decrypted = decrypt_import_payload_with_key_file(&data, &read_key_file(key.to_str().unwrap()).unwrap()).unwrap();
        assert!(matches!(decrypted.payload, profile_store::ExportPayload::All { .. }));
    }

    // どちらかの保存先が不正なら、何も書かない(鍵ファイルだけ・エクスポートしたファイルだけが、残らない)。
    #[test]
    fn a_key_file_export_with_an_invalid_destination_writes_nothing() {
        let source_dir = tempfile::tempdir().unwrap();
        let state = ProfileStoreState::with_store_for_test(init_store_with_one_profile(source_dir.path(), "元プロファイル"));
        let out = tempfile::tempdir().unwrap();
        let good_smx = out.path().join("export.smx");
        let good_key = out.path().join("export.smxkey");
        let inside_data_dir = source_dir.path().join("sneaky.smxkey");
        let cases = [
            ("鍵ファイルの拡張子が違う", good_smx.clone(), out.path().join("export.txt")),
            ("エクスポートしたファイルの拡張子が違う", out.path().join("export.txt"), good_key.clone()),
            ("鍵ファイルの保存先が、アプリのデータフォルダの内側", good_smx.clone(), inside_data_dir),
        ];

        for (label, smx, key) in cases {
            let err = export_profile_with_key_file_impl(
                &state,
                Ok(AppPaths::at(source_dir.path())),
                "元プロファイル",
                smx.to_str().unwrap(),
                key.to_str().unwrap(),
            )
            .expect_err(label);
            assert!(matches!(err, ExportImportError::InvalidInput(_)), "{label}: 事前検証で拒否されるはず");
            assert!(!good_smx.exists() && !good_key.exists(), "{label}: 何も書かれていないはず");
        }
    }

    // エクスポートしたファイルを書けなかったときは、対応するファイルの無い、鍵ファイルを残さない。
    #[test]
    fn a_key_file_export_removes_the_key_file_when_the_export_file_cannot_be_written() {
        let source_dir = tempfile::tempdir().unwrap();
        let state = ProfileStoreState::with_store_for_test(init_store_with_one_profile(source_dir.path(), "元プロファイル"));
        let out = tempfile::tempdir().unwrap();
        // 拡張子は.smxだが、実体はフォルダ(検証は通り、書き込みだけが失敗する)。
        let smx_that_is_a_folder = out.path().join("export.smx");
        std::fs::create_dir(&smx_that_is_a_folder).unwrap();
        let key = out.path().join("export.smxkey");

        let err = export_profile_with_key_file_impl(
            &state,
            Ok(AppPaths::at(source_dir.path())),
            "元プロファイル",
            smx_that_is_a_folder.to_str().unwrap(),
            key.to_str().unwrap(),
        )
        .expect_err("書き込めないため、失敗するはず");

        assert!(matches!(err, ExportImportError::Failed(_)));
        assert!(!key.exists(), "鍵ファイルだけが残っている");
    }

    #[test]
    fn a_key_file_export_of_a_missing_profile_writes_nothing() {
        let source_dir = tempfile::tempdir().unwrap();
        let state = ProfileStoreState::with_store_for_test(init_store_with_one_profile(source_dir.path(), "元プロファイル"));
        let out = tempfile::tempdir().unwrap();
        let smx = out.path().join("export.smx");
        let key = out.path().join("export.smxkey");

        let err = export_profile_with_key_file_impl(
            &state,
            Ok(AppPaths::at(source_dir.path())),
            "存在しないプロファイル",
            smx.to_str().unwrap(),
            key.to_str().unwrap(),
        )
        .expect_err("プロファイルが無いため、失敗するはず");

        assert!(matches!(err, ExportImportError::Failed(_)));
        assert!(!smx.exists() && !key.exists());
    }

    #[test]
    fn a_key_file_import_previews_and_commits_into_a_different_store() {
        let files = export_with_key_file_files("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let result = preview_import_with_key_file_impl(
            &dest.state,
            &pending,
            files.smx.to_str().unwrap(),
            files.key.to_str().unwrap(),
        )
        .expect("正しい鍵ファイルなら、確認画面へ進めるはず");

        assert!(!result.passphrase_trimmed, "鍵ファイルには、空白を除く再試行が無い");
        assert_eq!(pending_count(&pending), 1);
        commit_pending_import_impl(&dest.state, &pending, result.pending_id).expect("確定できるはず");
        let names: Vec<String> =
            with_store(&dest.state, |s| s.list_profiles()).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["元プロファイル".to_string()]);
    }

    #[test]
    fn a_key_file_import_is_rejected_for_a_wrong_key_a_wrong_extension_a_huge_file_or_bytes_that_are_not_text() {
        let files = export_with_key_file_files("元プロファイル");
        let other = export_with_key_file_files("別のプロファイル");
        let dir = tempfile::tempdir().unwrap();
        let wrong_extension = dir.path().join("key.txt");
        std::fs::copy(&files.key, &wrong_extension).unwrap();
        let huge = dir.path().join("huge.smxkey");
        std::fs::write(&huge, vec![b'#'; (MAX_KEY_FILE_BYTES + 1) as usize]).unwrap();
        let not_text = dir.path().join("binary.smxkey");
        std::fs::write(&not_text, [0xff, 0xfe, 0xfd]).unwrap();
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let cases: [(&str, &PathBuf, bool); 4] = [
            ("別のエクスポートの鍵ファイル", &other.key, false),
            ("拡張子が違う", &wrong_extension, true),
            ("大きすぎる", &huge, true),
            ("文字として読めない", &not_text, true),
        ];
        for (label, key, invalid_input) in cases {
            let err = preview_import_with_key_file_impl(
                &dest.state,
                &pending,
                files.smx.to_str().unwrap(),
                key.to_str().unwrap(),
            )
            .expect_err(label);
            assert_eq!(matches!(err, ExportImportError::InvalidInput(_)), invalid_input, "{label}: {err}");
            assert_eq!(pending_count(&pending), 0, "{label}: 失敗した取り込みは、保留を残さない");
        }
    }

    #[test]
    fn the_wrong_key_file_error_says_the_key_file_does_not_match() {
        let files = export_with_key_file_files("元プロファイル");
        let other = export_with_key_file_files("別のプロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let err = preview_import_with_key_file_impl(
            &dest.state,
            &pending,
            files.smx.to_str().unwrap(),
            other.key.to_str().unwrap(),
        )
        .expect_err("別の鍵ファイルでは、復号できないはず");

        assert!(err.message().contains("この鍵ファイルでは復号できません"), "{err}");
    }

    // パスフレーズの取り込みに、鍵ファイル方式のファイルを渡すと、鍵ファイルが必要なことを知らせる。
    #[test]
    fn the_passphrase_import_says_a_key_file_is_needed_for_a_key_file_export() {
        let files = export_with_key_file_files("元プロファイル");
        let dest = EmptyDestination::new();
        let pending = PendingImportState::default();

        let err = preview_import_impl(&dest.state, &pending, files.smx.to_str().unwrap(), passphrase(TEST_PASSPHRASE))
            .expect_err("パスフレーズでは、復号できないはず");

        assert!(err.message().contains("鍵ファイル"), "{err}");
    }

    // 以下は、鍵ファイル方式のIPCの境界(引数・応答のキー名)を固定する。
    #[test]
    fn detect_import_method_over_ipc_takes_source_path_and_answers_snake_case() {
        let harness = IpcHarness::new();
        let passphrase_file = export_profile_smx("元プロファイル");
        let key_files = export_with_key_file_files("元プロファイル");

        let passphrase_answer =
            harness.invoke("detect_import_method", json!({ "sourcePath": passphrase_file.path().to_str().unwrap() }));
        let key_answer = harness.invoke("detect_import_method", json!({ "sourcePath": key_files.smx.to_str().unwrap() }));

        assert_eq!(passphrase_answer, Ok(json!("passphrase")));
        assert_eq!(key_answer, Ok(json!("key_file")));
    }

    #[test]
    fn preview_import_with_key_file_over_ipc_takes_source_path_and_key_path_and_answers_like_preview_import() {
        let harness = IpcHarness::new();
        let files = export_with_key_file_files("元プロファイル");

        let answer = harness
            .invoke(
                "preview_import_with_key_file",
                json!({ "sourcePath": files.smx.to_str().unwrap(), "keyPath": files.key.to_str().unwrap() }),
            )
            .expect("正しい鍵ファイルでの取り込みは成功するはず");

        assert_eq!(sorted_keys(&answer), vec!["passphrase_trimmed", "pending_id", "preview"]);
        assert!(answer["pending_id"].is_u64());
        assert_eq!(answer["passphrase_trimmed"], json!(false));
        assert_eq!(harness.pending_count(), 1);
    }

    /// 2件のプロファイル(プロファイルA・B)を、全体エクスポートした、.smxのファイル。
    fn export_all_smx() -> tempfile::NamedTempFile {
        let source_dir = tempfile::tempdir().unwrap();
        let file = tempfile::Builder::new().suffix(".smx").tempfile().unwrap();
        let source_state = ProfileStoreState::with_store_for_test(init_store_with_two_profiles(source_dir.path()));
        export_all_to_file_impl(
            &source_state,
            Ok(AppPaths::at(source_dir.path())),
            passphrase(TEST_PASSPHRASE),
            file.path().to_str().unwrap(),
        )
        .expect("エクスポートは成功するはず");
        file
    }

    fn sorted_keys(value: &serde_json::Value) -> Vec<&str> {
        let mut keys: Vec<&str> =
            value.as_object().expect("JSONのオブジェクトのはず").keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys
    }

    /// 確認画面に出す、ルール1件のJSON(有効な、連番のリテラルのルール)。
    fn rule_json(name: &str, pattern: &str, prefix: &str) -> serde_json::Value {
        json!({
            "name": name,
            "pattern_type": "literal",
            "pattern": pattern,
            "mode": "sequential",
            "fixed_value": null,
            "prefix": prefix,
            "enabled": true,
        })
    }

    #[test]
    fn preview_import_over_ipc_returns_pending_id_and_a_single_profile_preview() {
        let harness = IpcHarness::new();

        let response = harness.preview(&export_profile_smx("元プロファイル"));

        // フロントエンドは、この3つのキー名で応答を読む。
        assert_eq!(sorted_keys(&response), ["passphrase_trimmed", "pending_id", "preview"]);
        assert!(response["pending_id"].is_u64(), "pending_idが整数でない: {response}");
        assert_eq!(response["passphrase_trimmed"], json!(false), "入力どおりで復号できたときは、除いていない");
        assert_eq!(
            response["preview"],
            json!({
                "kind": "single",
                "name": "元プロファイル",
                "rules": [rule_json("電話番号", "0120", "TEL")],
                "tags": [],
            })
        );
        assert_eq!(harness.pending_count(), 1, "応答した識別子の保留が、Rust側に残っているはず");
    }

    #[test]
    fn preview_import_over_ipc_reports_that_the_passphrase_was_trimmed() {
        let harness = IpcHarness::new();
        let file = export_profile_smx("元プロファイル");

        // 貼り付けで、前後に空白・改行が混ざったパスフレーズ。入力どおりでは復号できず、除いて再試行して成功する。
        let response = harness
            .invoke(
                "preview_import",
                json!({ "sourcePath": file.path().to_str().unwrap(), "passphrase": format!("  {TEST_PASSPHRASE}\n") }),
            )
            .expect("前後の空白を除けば、復号できるはず");

        assert_eq!(response["passphrase_trimmed"], json!(true));
        assert_eq!(response["preview"]["name"], "元プロファイル");
    }

    #[test]
    fn preview_import_over_ipc_returns_the_all_profiles_preview() {
        let harness = IpcHarness::new();

        let response = harness.preview(&export_all_smx());

        assert_eq!(sorted_keys(&response), ["passphrase_trimmed", "pending_id", "preview"]);
        let preview = &response["preview"];
        assert_eq!(sorted_keys(preview), ["entries", "kind", "will_activate_profile_name"]);
        assert_eq!(preview["kind"], "all");
        // 取り込み先はアクティブ未設定のため、取り込み元でアクティブだった(先に作成した)プロファイルAが、
        // このインポートでアクティブになる。
        assert_eq!(preview["will_activate_profile_name"], "プロファイルA");
        let entries = preview["entries"].as_array().expect("entriesは配列のはず");
        assert_eq!(entries.len(), 2);
        // エントリの並びには依存せず、名前で探す。
        let entry = |name: &str| {
            entries
                .iter()
                .find(|e| e["original_name"] == name)
                .unwrap_or_else(|| panic!("{name}のエントリがあるはず: {entries:?}"))
        };
        assert_eq!(
            *entry("プロファイルA"),
            json!({
                "original_name": "プロファイルA",
                "resolved_name": "プロファイルA",
                "renamed": false,
                "rules": [rule_json("Aルール", "AAA", "A")],
                "tags": [],
            })
        );
        assert_eq!(
            *entry("プロファイルB"),
            json!({
                "original_name": "プロファイルB",
                "resolved_name": "プロファイルB",
                "renamed": false,
                "rules": [rule_json("Bルール", "BBB", "B")],
                "tags": [],
            })
        );
    }

    #[test]
    fn preview_import_over_ipc_reports_a_rejected_source_as_kind_and_message() {
        let harness = IpcHarness::new();
        let dir = tempfile::tempdir().unwrap();
        let wrong_extension = dir.path().join("import.txt");

        let err = harness
            .invoke(
                "preview_import",
                json!({ "sourcePath": wrong_extension.to_str().unwrap(), "passphrase": TEST_PASSPHRASE }),
            )
            .expect_err("拡張子が.smxでないファイルは、拒否されるはず");

        // フロントエンド(isExportImportError)は、kindとmessageの2つのキーで、エラーを判別する。
        assert_eq!(err, json!({ "kind": "invalid_input", "message": "拡張子が.smxのファイルを選択してください" }));
        assert_eq!(harness.pending_count(), 0);
    }

    #[test]
    fn commit_pending_import_over_ipc_commits_only_the_pending_import_of_the_given_id() {
        let harness = IpcHarness::new();
        let id_a = harness.preview_pending_id(&export_profile_smx("プロファイルA"));
        let id_b = harness.preview_pending_id(&export_profile_smx("プロファイルB"));
        assert_ne!(id_a, id_b, "復号のたびに、別の識別子が払い出されるはず");

        // 識別子は、キーpendingIdで渡す(フロントエンドが、この名前で渡す)。
        let result = harness
            .invoke("commit_pending_import", json!({ "pendingId": id_b }))
            .expect("Bの識別子の確定は成功するはず");
        // 取り込み先はアクティブ未設定だったため、Bがアクティブになる。
        assert_eq!(result, json!({ "activated_profile_name": "プロファイルB" }));
        assert_eq!(harness.profile_names(), ["プロファイルB"], "Bの識別子で、Aが取り込まれている");
        assert_eq!(harness.pending_count(), 1, "指定していないAの保留が、消費されている");

        let result = harness
            .invoke("commit_pending_import", json!({ "pendingId": id_a }))
            .expect("Bの確定の後でも、Aの識別子の確定は成功するはず");
        // 既にBがアクティブなため、Aはアクティブにならない。
        assert_eq!(result, json!({ "activated_profile_name": null }));
        assert_eq!(harness.profile_names(), ["プロファイルA", "プロファイルB"]);
        assert_eq!(harness.pending_count(), 0);
    }

    #[test]
    fn commit_pending_import_over_ipc_fails_for_a_missing_or_wrong_id_and_consumes_nothing() {
        let harness = IpcHarness::new();
        let id = harness.preview_pending_id(&export_profile_smx("元プロファイル"));

        // キー(pendingId)が無い呼び出しは、失敗する。エラーは、足りないキーの名前を含む。
        let err = harness.invoke("commit_pending_import", json!({})).expect_err("キーが無いので、失敗するはず");
        assert!(err.as_str().is_some_and(|message| message.contains("pendingId")), "予期しないエラー: {err}");
        // 別の綴り(pending_id)のキーでは、識別子を受け取れない。
        harness
            .invoke("commit_pending_import", json!({ "pending_id": id }))
            .expect_err("別の綴りのキーでは、失敗するはず");
        // 払い出されていない識別子は、既存と同じ文言で失敗する。
        let err = harness
            .invoke("commit_pending_import", json!({ "pendingId": id + 1 }))
            .expect_err("払い出されていない識別子は、失敗するはず");
        assert_eq!(err, json!("確認待ちのインポートがありません"));

        assert!(harness.profile_names().is_empty(), "失敗した確定で、取り込まれている");
        assert_eq!(harness.pending_count(), 1, "失敗した確定が、保留を消費している");
        harness
            .invoke("commit_pending_import", json!({ "pendingId": id }))
            .expect("本来の識別子なら、確定できるはず");
        assert_eq!(harness.profile_names(), ["元プロファイル"]);
    }

    #[test]
    fn clear_pending_import_over_ipc_discards_only_the_pending_import_of_the_given_id() {
        let harness = IpcHarness::new();
        let id_a = harness.preview_pending_id(&export_profile_smx("プロファイルA"));
        let id_b = harness.preview_pending_id(&export_profile_smx("プロファイルB"));

        // 識別子は、キーpendingIdで渡す(フロントエンドが、この名前で渡す)。
        let response = harness
            .invoke("clear_pending_import", json!({ "pendingId": id_a }))
            .expect("識別子を指定した破棄は成功するはず");
        assert_eq!(response, serde_json::Value::Null);

        assert_eq!(harness.pending_count(), 1, "指定していないBの保留が、消えている");
        let err = harness
            .invoke("commit_pending_import", json!({ "pendingId": id_a }))
            .expect_err("破棄したAは、確定できないはず");
        assert_eq!(err, json!("確認待ちのインポートがありません"));
        harness
            .invoke("commit_pending_import", json!({ "pendingId": id_b }))
            .expect("破棄していないBは、確定できるはず");
        assert_eq!(harness.profile_names(), ["プロファイルB"]);
    }

    // キーが無い呼び出しは、全ての保留を破棄する(E2Eの後片付けが使う)。
    #[test]
    fn clear_pending_import_over_ipc_without_a_key_discards_every_pending_import() {
        let harness = IpcHarness::new();
        let id_a = harness.preview_pending_id(&export_profile_smx("プロファイルA"));
        let id_b = harness.preview_pending_id(&export_profile_smx("プロファイルB"));

        harness.invoke("clear_pending_import", json!({})).expect("キーの無い破棄は成功するはず");

        assert_eq!(harness.pending_count(), 0);
        for id in [id_a, id_b] {
            let err = harness
                .invoke("commit_pending_import", json!({ "pendingId": id }))
                .expect_err("全消去の後は、どの保留も確定できないはず");
            assert_eq!(err, json!("確認待ちのインポートがありません"));
        }
        assert!(harness.profile_names().is_empty());
    }
}
