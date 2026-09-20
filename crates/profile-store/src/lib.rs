//! プロファイルの永続化・暗号化・鍵管理(Imperative Shell)。masking-coreのRule/RuleProfileを
//! SQLite+対称暗号化で保存する。GUI/CLI/MCPはこのcrate経由でのみプロファイルを読み書きする。

mod bulk;
mod crypto;
mod db;
mod export;
mod key;
mod paths;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bulk::ExportedProfile;
use masking_core::RuleProfile;
use rusqlite::Connection;
use secrecy::SecretBox;
use zeroize::{Zeroize, Zeroizing};

pub use bulk::ExportPayload;
pub use export::{detect_import_method, ImportMethod, KeyFileExport, KEY_FILE_EXTENSION};
pub use key::FileProtection;
pub use paths::{normalize_and_reject_special_forms, AppPaths, PathError};
// masker/gui側がexport_profile/import_profileにパスフレーズを渡す際、profile-storeが
// 実際に使っているsecrecyと同一の型を参照できるようにする(独自にsecrecy依存を追加させない)。
pub use secrecy::{ExposeSecret, SecretString};

#[derive(Debug, thiserror::Error)]
pub enum ProfileStoreError {
    #[error("初期化されていません。`masker init`を実行してください")]
    NotInitialized,
    #[error(
        "鍵ファイルとDBファイルの一方だけが存在する不整合な状態です\
         (鍵: {key_exists} [{key_path}], DB: {db_exists} [{db_path}])。手動で確認してください"
    )]
    PartiallyInitialized { key_exists: bool, db_exists: bool, key_path: PathBuf, db_path: PathBuf },
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Key(#[from] key::KeyError),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Crypto(#[from] crypto::CryptoError),
    #[error(transparent)]
    Export(#[from] export::ExportError),
    #[error("プロファイル '{0}' は既に存在します")]
    ProfileAlreadyExists(String),
    #[error("プロファイル '{0}' が見つかりません")]
    ProfileNotFound(String),
    #[error("アクティブなプロファイルは削除できません")]
    CannotDeleteActiveProfile,
    #[error("保存されているプロファイルのデータが不正です: {0}")]
    CorruptProfileData(String),
    #[error(
        "このファイルのフォーマットバージョン({found})は、このmaskerが対応しているバージョン\
         ({supported})と異なります。エクスポート元・インポート先のmaskerのバージョンを確認してください"
    )]
    UnsupportedFormatVersion { found: u32, supported: u32 },
    #[error("インポートファイル内でプロファイル名が重複しています: '{0}'")]
    DuplicateNameInImportFile(String),
    #[error("インポートファイルの規模が上限を超えています: {0}")]
    ImportTooLarge(String),
    #[error("タグ '{0}' は既に存在します")]
    TagAlreadyExists(String),
    #[error("タグ '{0}' が見つかりません")]
    TagNotFound(String),
    #[error("タグ名が不正です: {0}")]
    InvalidTagName(String),
    #[error("プロファイル名が不正です: {0}")]
    InvalidProfileName(String),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProfileSummary {
    pub id: i64,
    pub name: String,
    pub rule_count: usize,
    // 総ルール数が1件以上あるのに有効なルールが1件も無い(=このプロファイルは
    // 実質何もマスクしない)場合に一覧上で気付けるようにするための件数。
    pub enabled_rule_count: usize,
    pub is_favorite: bool,
    pub is_active: bool,
    pub updated_at: String,
    pub tags: Vec<String>,
}

/// 全体インポートで、ファイル内の元の名前が取り込み先での衝突によりどう解決されたか。
#[derive(Debug, Clone, PartialEq)]
pub struct AllImportEntry {
    pub original_name: String,
    pub resolved_name: String,
    pub renamed: bool,
}

impl Zeroize for AllImportEntry {
    fn zeroize(&mut self) {
        self.original_name.zeroize();
        self.resolved_name.zeroize();
    }
}

/// `preview_import`の結果。DBはまだ変更されていない。`commit_import`にそのまま渡す。
#[derive(Debug, Clone)]
pub enum ImportPreview {
    Single { name: String, exported: ExportedProfile },
    All { active_profile_name: Option<String>, entries: Vec<AllImportEntry>, exported: Vec<ExportedProfile> },
}

#[cfg(test)]
thread_local! {
    // ImportPreviewの内容を消去した回数(テスト用。捨てる・確定する、どちらの経路でも消去されることを確かめる)。
    static IMPORT_PREVIEW_WIPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 保留していた復号済みの内容(ルールのパターン・固定値など、マスク対象の実際の値を含みうる)を、メモリ上で消去する。
impl Zeroize for ImportPreview {
    fn zeroize(&mut self) {
        #[cfg(test)]
        IMPORT_PREVIEW_WIPES.with(|wipes| wipes.set(wipes.get() + 1));
        match self {
            ImportPreview::Single { name, exported } => {
                name.zeroize();
                exported.zeroize();
            }
            ImportPreview::All { active_profile_name, entries, exported } => {
                active_profile_name.zeroize();
                entries.zeroize();
                exported.zeroize();
            }
        }
    }
}

/// 確定・破棄・保持数の上限による廃棄など、どの経路で捨てても、内容が消去されるようにする。
/// (Dropを持つため、フィールドをムーブして取り出す書き方はできない。借用で使う。)
impl Drop for ImportPreview {
    fn drop(&mut self) {
        self.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportOutcome {
    // activated: このコミットの結果、取り込んだこのプロファイルがアクティブになったか
    // (取り込み先にアクティブが未設定だった場合のみtrue)。無言でのアクティブ化を
    // 呼び出し側が気付けるようにするための情報。
    Single { name: String, activated: bool },
    All { entries: Vec<AllImportEntry>, activated_profile_name: Option<String> },
}

pub fn is_initialized() -> Result<bool, ProfileStoreError> {
    is_initialized_at(&AppPaths::resolve()?)
}

pub fn is_initialized_at(paths: &AppPaths) -> Result<bool, ProfileStoreError> {
    let key_exists = paths.key_path.exists();
    let db_exists = paths.db_path.exists();
    if key_exists != db_exists {
        return Err(ProfileStoreError::PartiallyInitialized {
            key_exists,
            db_exists,
            key_path: paths.key_path.clone(),
            db_path: paths.db_path.clone(),
        });
    }
    Ok(key_exists && db_exists)
}

/// 鍵ファイル+DBを初期化する。既に初期化済みなら冪等に終了する。
pub fn init() -> Result<(), ProfileStoreError> {
    init_at(&AppPaths::resolve()?)
}

pub fn init_at(paths: &AppPaths) -> Result<(), ProfileStoreError> {
    if is_initialized_at(paths)? {
        return Ok(());
    }
    key::generate_and_save_key(&paths.key_path)?;
    db::open(&paths.db_path)?;
    Ok(())
}

// keyを`SecretBox`で保持しているため、`{:?}`で表示しても`SecretBox<[u8; 32]>([REDACTED])`
// となり生バイトは露出しない(実機で確認済み)。derive(Debug)しても安全なのはこの理由による。
#[derive(Debug)]
pub struct ProfileStore {
    conn: Connection,
    key: SecretBox<[u8; key::KEY_LEN]>,
}

impl ProfileStore {
    pub fn open() -> Result<Self, ProfileStoreError> {
        Self::open_at(&AppPaths::resolve()?)
    }

    pub fn open_at(paths: &AppPaths) -> Result<Self, ProfileStoreError> {
        if !is_initialized_at(paths)? {
            return Err(ProfileStoreError::NotInitialized);
        }
        let key = key::load_key(&paths.key_path)?;
        let conn = db::open(&paths.db_path)?;
        Ok(Self { conn, key })
    }

    /// 新規作成したプロファイルの`id`(SQLiteの内部サロゲートキー)を返す。GUI等の
    /// 呼び出し元は、name(変更されうる)ではなくこのidを永続的な識別子として保持する
    /// 想定(名前変更を跨いで安定した対応付けが必要な場面、例えばマスク処理の連番採番用
    /// マッピングテーブルのキー等で使うため)。
    pub fn create_profile(&mut self, profile: &RuleProfile) -> Result<i64, ProfileStoreError> {
        let name = profile.profile_name();
        let json = Zeroizing::new(serde_json::to_vec(profile).expect("RuleProfileのシリアライズは失敗しない"));
        let encrypted = crypto::encrypt(&self.key, &json, name.as_bytes());

        // INSERTと「アクティブ未設定なら自動的にアクティブにする」settings更新を1つの
        // トランザクションにまとめる。個別実行だと、途中でクラッシュした場合に
        // 「プロファイルは存在するがアクティブが未設定」という不整合な状態が残り、
        // 次に作成したプロファイルが意図せずアクティブを奪ってしまう。
        let tx = self.conn.transaction()?;

        let exists: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1)", [name], |row| row.get(0))?;
        if exists {
            return Err(ProfileStoreError::ProfileAlreadyExists(name.to_string()));
        }

        tx.execute(
            "INSERT INTO profiles (name, rules_encrypted, nonce, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
            (name, &encrypted.ciphertext, &encrypted.nonce),
        )?;
        let id = tx.last_insert_rowid();

        if read_active_profile_name(&tx)?.is_none() {
            upsert_active_profile_name(&tx, name)?;
        }

        tx.commit()?;
        Ok(id)
    }

    /// プロファイル(お気に入り・タグを含む)をパスフレーズで再暗号化したバイト列を返す
    /// (ローカル鍵を経由しない)。呼び出し側がこれをファイルに書き出す。
    pub fn export_profile(&self, name: &str, passphrase: SecretString) -> Result<Vec<u8>, ProfileStoreError> {
        let json = self.single_export_json(name)?;
        Ok(export::encrypt_for_export(&json, passphrase)?)
    }

    /// `export_profile`の、鍵ファイル方式。新しい鍵を生成し、その鍵宛てに暗号化する(鍵は、返り値の`key_file_contents`で
    /// 渡す。呼び出し側が、鍵ファイルとして書き出す)。
    pub fn export_profile_with_key_file(&self, name: &str) -> Result<KeyFileExport, ProfileStoreError> {
        let json = self.single_export_json(name)?;
        Ok(export::encrypt_for_export_with_new_key(&json)?)
    }

    fn single_export_json(&self, name: &str) -> Result<Zeroizing<Vec<u8>>, ProfileStoreError> {
        let exported = self.read_exported_profile(name)?;
        let payload = ExportPayload::Single { format_version: bulk::CURRENT_FORMAT_VERSION, profile: exported };
        Ok(Zeroizing::new(serde_json::to_vec(&payload).expect("ExportPayloadのシリアライズは失敗しない")))
    }

    /// 全プロファイル+タグ+お気に入り+アクティブプロファイル名をパスフレーズで
    /// 再暗号化したバイト列を返す(PC移行用)。
    pub fn export_all(&self, passphrase: SecretString) -> Result<Vec<u8>, ProfileStoreError> {
        let json = self.all_export_json()?;
        Ok(export::encrypt_for_export(&json, passphrase)?)
    }

    /// `export_all`の、鍵ファイル方式(`export_profile_with_key_file`と同じ)。
    pub fn export_all_with_key_file(&self) -> Result<KeyFileExport, ProfileStoreError> {
        let json = self.all_export_json()?;
        Ok(export::encrypt_for_export_with_new_key(&json)?)
    }

    fn all_export_json(&self) -> Result<Zeroizing<Vec<u8>>, ProfileStoreError> {
        let names: Vec<String> = self
            .conn
            .prepare("SELECT name FROM profiles ORDER BY name")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;

        let profiles =
            names.iter().map(|name| self.read_exported_profile(name)).collect::<Result<Vec<_>, _>>()?;
        let active_profile_name = read_active_profile_name(&self.conn)?;

        let payload = ExportPayload::All {
            format_version: bulk::CURRENT_FORMAT_VERSION,
            active_profile_name,
            profiles,
        };
        Ok(Zeroizing::new(serde_json::to_vec(&payload).expect("ExportPayloadのシリアライズは失敗しない")))
    }

    /// `export_profile`/`export_all`が生成したバイト列を復号し、書き込み内容を計算する
    /// (DBはまだ変更しない)。単一プロファイルで名前が既存と重複する場合は、この時点で
    /// `ProfileAlreadyExists`エラーになる(`create_profile`と同じ扱い)。全体の場合は
    /// 各エントリの名前衝突を`bulk::resolve_name`で解決した結果を返すのみで、エラーには
    /// ならない(実際のリネームは`commit_import`が行う)。
    ///
    /// `decrypt_import_payload`(DBに触れない、パスフレーズ検証のみ)と`resolve_import_preview`
    /// (DBへの高速な読み取りのみ)をまとめて呼ぶ利便関数。ロックを握ったまま呼んでも問題ない
    /// 呼び出し元(CLI等、単一スレッドで他の操作と競合しない)向け。GUIのように、ストアの
    /// ロックを他のコマンドと共有していて、かつパスフレーズ検証(scrypt、数百ms〜数秒)を
    /// その間ブロックしたくない場合は、2つを分けて呼ぶこと。
    pub fn preview_import(&self, data: &[u8], passphrase: SecretString) -> Result<ImportPreview, ProfileStoreError> {
        let decrypted = decrypt_import_payload(data, passphrase)?;
        self.resolve_import_preview(decrypted.payload)
    }

    /// `preview_import`のうち、既に復号済みのペイロードから既存プロファイルとの名前衝突を
    /// 解決する後半部分。DBへの高速なSELECTのみのため、ロックを握ったまま呼んでも
    /// 他の操作への影響は小さい(`decrypt_import_payload`と分けて呼ぶことで、時間のかかる
    /// パスフレーズ検証をロック外に追い出せる)。
    pub fn resolve_import_preview(&self, payload: ExportPayload) -> Result<ImportPreview, ProfileStoreError> {
        match payload {
            ExportPayload::Single { format_version, profile } => {
                check_format_version(format_version)?;
                let name = profile.profile.profile_name().to_string();
                let exists: bool = self
                    .conn
                    .query_row("SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1)", [&name], |row| row.get(0))?;
                if exists {
                    return Err(ProfileStoreError::ProfileAlreadyExists(name));
                }
                Ok(ImportPreview::Single { name, exported: profile })
            }
            ExportPayload::All { format_version, active_profile_name, profiles } => {
                // エラーで抜けるときも、名前を消去する(成功したときは、保留する内容へ引き渡す)。
                let mut active_profile_name = Zeroizing::new(active_profile_name);
                check_format_version(format_version)?;

                // 正規のexport_allでは`profiles.name`のUNIQUE制約によりあり得ないが、
                // 手作りされた/破損したファイルでは重複しうる。重複があると、後段の
                // アクティブプロファイル名の解決(先勝ち)が曖昧になるため、ここで
                // ファイル全体を拒否する(部分的な取り込みは行わない)。
                let mut seen = HashSet::new();
                for p in &profiles {
                    let name = p.profile.profile_name();
                    if !seen.insert(name.to_string()) {
                        return Err(ProfileStoreError::DuplicateNameInImportFile(name.to_string()));
                    }
                }

                let mut taken: HashSet<String> = self
                    .conn
                    .prepare("SELECT name FROM profiles")?
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<_, _>>()?;

                let entries = profiles
                    .iter()
                    .map(|p| {
                        let original_name = p.profile.profile_name().to_string();
                        let (resolved_name, renamed) = bulk::resolve_name(&original_name, &mut taken);
                        AllImportEntry { original_name, resolved_name, renamed }
                    })
                    .collect();

                Ok(ImportPreview::All { active_profile_name: active_profile_name.take(), entries, exported: profiles })
            }
        }
    }

    /// `preview_import`の結果を実際に書き込む。全体の場合は1トランザクションにまとめる
    /// (途中の失敗で一部プロファイルだけが作成された状態を残さないため)。
    pub fn commit_import(&mut self, preview: ImportPreview) -> Result<ImportOutcome, ProfileStoreError> {
        // previewは、借用で使う。関数を抜けるとき(成功・失敗のどちらでも)に、dropで内容が消去される。
        match &preview {
            ImportPreview::Single { name, exported } => {
                let encrypted_json =
                    Zeroizing::new(serde_json::to_vec(&exported.profile).expect("RuleProfileのシリアライズは失敗しない"));
                let encrypted = crypto::encrypt(&self.key, &encrypted_json, name.as_bytes());

                let tx = self.conn.transaction()?;
                insert_profile_row(&tx, name, &encrypted, exported.is_favorite, &exported.tags)?;
                let activated = read_active_profile_name(&tx)?.is_none();
                if activated {
                    upsert_active_profile_name(&tx, name)?;
                }
                tx.commit()?;
                Ok(ImportOutcome::Single { name: name.clone(), activated })
            }
            ImportPreview::All { active_profile_name, entries, exported } => {
                // 暗号化はself.keyの借用で完結させ、トランザクション(self.connの可変借用)開始前に
                // 済ませておく(同時に借用しないための構成)。resolved_name/tags/is_favoriteは、previewから
                // 借用する(複製は、previewと違い、消去されないため、作らない)。
                struct PreparedEntry<'a> {
                    resolved_name: &'a str,
                    encrypted: crypto::Encrypted,
                    is_favorite: bool,
                    tags: &'a [String],
                }
                let prepared: Vec<PreparedEntry<'_>> = entries
                    .iter()
                    .zip(exported.iter())
                    .map(|(entry, exported)| {
                        // AAD(entry.resolved_name)・書き込み先のname列(同)と、暗号文に
                        // 閉じ込める平文profile_nameを常に一致させる。名前衝突でリネーム
                        // された場合、ファイル内の元の名前のままシリアライズすると、
                        // 暗号文の自己申告とAAD/DB上の名前が食い違う状態が生まれ、元プロファイル
                        // 削除後に、信頼している名前がインポート由来のルール集合を指すようになりうる。
                        let renamed_profile = renamed_profile_for_import(&entry.resolved_name, exported)?;
                        let json = Zeroizing::new(
                            serde_json::to_vec(&*renamed_profile).expect("RuleProfileのシリアライズは失敗しない"),
                        );
                        Ok(PreparedEntry {
                            resolved_name: &entry.resolved_name,
                            encrypted: crypto::encrypt(&self.key, &json, entry.resolved_name.as_bytes()),
                            is_favorite: exported.is_favorite,
                            tags: &exported.tags,
                        })
                    })
                    .collect::<Result<Vec<PreparedEntry<'_>>, ProfileStoreError>>()?;

                let tx = self.conn.transaction()?;
                for p in &prepared {
                    insert_profile_row(&tx, p.resolved_name, &p.encrypted, p.is_favorite, p.tags)?;
                }

                // アクティブプロファイルの扱い: 取り込み先に既にアクティブなプロファイルが
                // 設定されている場合は変更しない。未設定の場合のみ、ファイル内の値を
                // (衝突でリネームされていれば解決後の名前に読み替えて)採用する。
                let mut activated_profile_name = None;
                if read_active_profile_name(&tx)?.is_none() {
                    if let Some(original) = active_profile_name {
                        if let Some(entry) = entries.iter().find(|e| &e.original_name == original) {
                            upsert_active_profile_name(&tx, &entry.resolved_name)?;
                            activated_profile_name = Some(entry.resolved_name.clone());
                        }
                    }
                }

                tx.commit()?;
                Ok(ImportOutcome::All { entries: entries.clone(), activated_profile_name })
            }
        }
    }

    pub fn get_profile(&self, name: &str) -> Result<RuleProfile, ProfileStoreError> {
        let (ciphertext, nonce): (Vec<u8>, Vec<u8>) = self
            .conn
            .query_row(
                "SELECT rules_encrypted, nonce FROM profiles WHERE name = ?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| not_found_unless_other_db_error(e, name))?;

        self.decrypt_profile(&ciphertext, &nonce, name)
    }

    pub fn list_profiles(&self) -> Result<Vec<ProfileSummary>, ProfileStoreError> {
        let active_name = read_active_profile_name(&self.conn)?;

        let rows: Vec<(i64, String, Vec<u8>, Vec<u8>, bool, String)> = self
            .conn
            .prepare(
                "SELECT id, name, rules_encrypted, nonce, is_favorite, updated_at FROM profiles ORDER BY name",
            )?
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
            })?
            .collect::<Result<_, _>>()?;

        rows.into_iter()
            .map(|(id, name, ciphertext, nonce, is_favorite, updated_at)| {
                let counts = self.decrypt_rule_counts(&ciphertext, &nonce, &name)?;
                let tags = tags_for_profile_id(&self.conn, id)?;
                Ok(ProfileSummary {
                    is_active: Some(&name) == active_name.as_ref(),
                    id,
                    name,
                    rule_count: counts.total,
                    enabled_rule_count: counts.enabled,
                    is_favorite,
                    updated_at,
                    tags,
                })
            })
            .collect()
    }

    /// 既存プロファイルの名前・説明・ルールをまとめて置き換える。名前を変更する場合、
    /// 暗号化のAAD(プロファイル名)が変わるため再暗号化が必要(単純なUPDATEでは済まない)。
    /// アクティブプロファイル名(`settings.active_profile_name`)はON UPDATE CASCADEにより
    /// SQLite側が自動追従するため、ここでの手当ては不要。お気に入り・タグはprofile_id経由の
    /// 参照のため、名前変更の影響を受けない。
    pub fn update_profile(&mut self, old_name: &str, new_profile: &RuleProfile) -> Result<(), ProfileStoreError> {
        let new_name = new_profile.profile_name();
        let json = Zeroizing::new(serde_json::to_vec(new_profile).expect("RuleProfileのシリアライズは失敗しない"));
        let encrypted = crypto::encrypt(&self.key, &json, new_name.as_bytes());

        let tx = self.conn.transaction()?;

        let exists: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1)", [old_name], |row| row.get(0))?;
        if !exists {
            return Err(ProfileStoreError::ProfileNotFound(old_name.to_string()));
        }

        if new_name != old_name {
            let name_taken: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1)", [new_name], |row| row.get(0))?;
            if name_taken {
                return Err(ProfileStoreError::ProfileAlreadyExists(new_name.to_string()));
            }
        }

        tx.execute(
            "UPDATE profiles SET name = ?1, rules_encrypted = ?2, nonce = ?3, updated_at = datetime('now')
             WHERE name = ?4",
            (new_name, &encrypted.ciphertext, &encrypted.nonce, old_name),
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn set_favorite(&mut self, name: &str, is_favorite: bool) -> Result<(), ProfileStoreError> {
        let changed =
            self.conn.execute("UPDATE profiles SET is_favorite = ?1 WHERE name = ?2", (is_favorite, name))?;
        if changed == 0 {
            return Err(ProfileStoreError::ProfileNotFound(name.to_string()));
        }
        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<String>, ProfileStoreError> {
        self.conn
            .prepare("SELECT name FROM tags ORDER BY name")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }

    pub fn create_tag(&mut self, name: &str) -> Result<(), ProfileStoreError> {
        masking_core::validate_display_name(name).map_err(ProfileStoreError::InvalidTagName)?;
        let exists: bool =
            self.conn.query_row("SELECT EXISTS(SELECT 1 FROM tags WHERE name = ?1)", [name], |row| row.get(0))?;
        if exists {
            return Err(ProfileStoreError::TagAlreadyExists(name.to_string()));
        }
        self.conn.execute("INSERT INTO tags (name) VALUES (?1)", [name])?;
        Ok(())
    }

    pub fn rename_tag(&mut self, old_name: &str, new_name: &str) -> Result<(), ProfileStoreError> {
        masking_core::validate_display_name(new_name).map_err(ProfileStoreError::InvalidTagName)?;
        let tx = self.conn.transaction()?;

        let exists: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM tags WHERE name = ?1)", [old_name], |row| row.get(0))?;
        if !exists {
            return Err(ProfileStoreError::TagNotFound(old_name.to_string()));
        }

        if new_name != old_name {
            let name_taken: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM tags WHERE name = ?1)", [new_name], |row| row.get(0))?;
            if name_taken {
                return Err(ProfileStoreError::TagAlreadyExists(new_name.to_string()));
            }
        }

        tx.execute("UPDATE tags SET name = ?1 WHERE name = ?2", [new_name, old_name])?;
        tx.commit()?;
        Ok(())
    }

    /// `profile_tags`はON DELETE CASCADEのため、紐付いていたプロファイルからの
    /// タグ外しは自動的に行われる。
    pub fn delete_tag(&mut self, name: &str) -> Result<(), ProfileStoreError> {
        let changed = self.conn.execute("DELETE FROM tags WHERE name = ?1", [name])?;
        if changed == 0 {
            return Err(ProfileStoreError::TagNotFound(name.to_string()));
        }
        Ok(())
    }

    pub fn profile_tags(&self, profile_name: &str) -> Result<Vec<String>, ProfileStoreError> {
        let profile_id: i64 = self
            .conn
            .query_row("SELECT id FROM profiles WHERE name = ?1", [profile_name], |row| row.get(0))
            .map_err(|e| not_found_unless_other_db_error(e, profile_name))?;
        tags_for_profile_id(&self.conn, profile_id)
    }

    /// 指定したタグ集合で完全に置き換える(既存の紐付けは一旦全て外してから付け直す)。
    /// 存在しないタグ名を渡した場合はget-or-createで自動作成する(インポート経路の
    /// `attach_tags`と同じ挙動に揃え、呼び出し側での存在確認を不要にする)。
    pub fn set_profile_tags(&mut self, profile_name: &str, tags: &[String]) -> Result<(), ProfileStoreError> {
        let profile_id: i64 = self
            .conn
            .query_row("SELECT id FROM profiles WHERE name = ?1", [profile_name], |row| row.get(0))
            .map_err(|e| not_found_unless_other_db_error(e, profile_name))?;

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM profile_tags WHERE profile_id = ?1", [profile_id])?;
        attach_tags(&tx, profile_id, tags)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_profile(&mut self, name: &str) -> Result<(), ProfileStoreError> {
        if read_active_profile_name(&self.conn)?.as_deref() == Some(name) {
            return Err(ProfileStoreError::CannotDeleteActiveProfile);
        }

        let changed = self.conn.execute("DELETE FROM profiles WHERE name = ?1", [name])?;
        if changed == 0 {
            return Err(ProfileStoreError::ProfileNotFound(name.to_string()));
        }
        Ok(())
    }

    pub fn set_active_profile(&mut self, name: &str) -> Result<(), ProfileStoreError> {
        let exists: bool =
            self.conn.query_row("SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1)", [name], |row| row.get(0))?;
        if !exists {
            return Err(ProfileStoreError::ProfileNotFound(name.to_string()));
        }
        upsert_active_profile_name(&self.conn, name)
    }

    pub fn active_profile(&self) -> Result<Option<RuleProfile>, ProfileStoreError> {
        let active_name = match read_active_profile_name(&self.conn)? {
            Some(name) => name,
            None => return Ok(None),
        };
        Ok(Some(self.get_profile(&active_name)?))
    }

    fn decrypt_profile(&self, ciphertext: &[u8], nonce: &[u8], name: &str) -> Result<RuleProfile, ProfileStoreError> {
        let plaintext = Zeroizing::new(crypto::decrypt(&self.key, ciphertext, nonce, name.as_bytes())?);
        serde_json::from_slice(&plaintext).map_err(|e| ProfileStoreError::CorruptProfileData(e.to_string()))
    }

    /// `list_profiles`専用の軽量パス。`RuleProfile`への型付きデシリアライズは各ルールの
    /// 正規表現を実際にコンパイルする(`Rule::new`経由)ため、一覧表示のたびに全件で
    /// 行うと、悪意あるルールを含むプロファイルを一度取り込んだ場合に起動・一覧更新の
    /// たびコンパイルコストが再発してしまう。ルール件数だけが必要な場合は
    /// `serde_json::Value`として構造的に数えるだけに留め、regexには一切触れない。
    fn decrypt_rule_counts(&self, ciphertext: &[u8], nonce: &[u8], name: &str) -> Result<RuleCounts, ProfileStoreError> {
        let plaintext = Zeroizing::new(crypto::decrypt(&self.key, ciphertext, nonce, name.as_bytes())?);
        let value: serde_json::Value =
            serde_json::from_slice(&plaintext).map_err(|e| ProfileStoreError::CorruptProfileData(e.to_string()))?;
        Ok(RuleCounts { total: rule_count_of(&value), enabled: enabled_rule_count_of(&value) })
    }

    /// 現在アクティブなプロファイルが設定されているか(値そのものは不要な場面向けの
    /// 軽量版。インポートのプレビュー画面で「実行するとアクティブになる」を正しく
    /// 判定するために使う。復号を伴わない)。
    pub fn has_active_profile(&self) -> Result<bool, ProfileStoreError> {
        Ok(read_active_profile_name(&self.conn)?.is_some())
    }

    /// 指定したプロファイルのルール・お気に入り・タグをまとめて読み出す
    /// (export_profile/export_allの共通処理)。
    fn read_exported_profile(&self, name: &str) -> Result<ExportedProfile, ProfileStoreError> {
        let (id, is_favorite): (i64, bool) = self
            .conn
            .query_row("SELECT id, is_favorite FROM profiles WHERE name = ?1", [name], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| not_found_unless_other_db_error(e, name))?;

        let profile = self.get_profile(name)?;
        let tags = tags_for_profile_id(&self.conn, id)?;

        Ok(ExportedProfile { is_favorite, tags, profile })
    }
}

fn tags_for_profile_id(conn: &Connection, profile_id: i64) -> Result<Vec<String>, ProfileStoreError> {
    conn.prepare(
        "SELECT tags.name FROM tags
         JOIN profile_tags ON profile_tags.tag_id = tags.id
         WHERE profile_tags.profile_id = ?1
         ORDER BY tags.name",
    )?
    .query_map([profile_id], |row| row.get(0))?
    .collect::<Result<_, _>>()
    .map_err(Into::into)
}

/// `export_profile`/`export_all`が生成したバイト列をパスフレーズで復号し、構造化された
/// ペイロードを返す。DBには一切アクセスしない(ストアの`&self`を取らない)ため、呼び出し側は
/// ストアのロックを握らずにこれを呼べる。パスフレーズ検証のscrypt処理は数百ms〜数秒
/// かかりうるため、ロックを共有する他の操作(GUIの他のTauriコマンド等)を無関係に
/// 巻き込んで待たせないようにするための分離。
pub fn decrypt_import_payload(data: &[u8], passphrase: SecretString) -> Result<DecryptedPayload, ProfileStoreError> {
    let decrypted = export::decrypt_import_reporting_trim(data, passphrase)?;
    payload_from_plaintext(decrypted.plaintext, decrypted.passphrase_trimmed)
}

/// `decrypt_import_payload`の、鍵ファイル方式(鍵ファイルの中身で復号する。DBには一切アクセスしない)。空白を除いての
/// 再試行は無いため、`passphrase_trimmed`は、常にfalse。
pub fn decrypt_import_payload_with_key_file(
    data: &[u8],
    key_file_contents: &SecretString,
) -> Result<DecryptedPayload, ProfileStoreError> {
    let plaintext = export::decrypt_import_with_key_file(data, key_file_contents)?;
    payload_from_plaintext(plaintext, false)
}

// 復号した平文のJSONを、ペイロードへ読み取る(規模の検査を、型付きのデシリアライズの前に行う)。平文は、使い終えたときに消去する。
fn payload_from_plaintext(plaintext: Vec<u8>, passphrase_trimmed: bool) -> Result<DecryptedPayload, ProfileStoreError> {
    let json = Zeroizing::new(plaintext);
    check_import_size_limits(&json)?;
    let payload =
        serde_json::from_slice(&json).map_err(|e| ProfileStoreError::CorruptProfileData(e.to_string()))?;
    Ok(DecryptedPayload { payload, passphrase_trimmed })
}

/// 鍵ファイルを書き出す(所有ユーザーだけの権限にする。制限できない保管先では、書き込みは成功として、その旨を返す)。
pub fn write_key_file(path: &Path, contents: &SecretString) -> Result<FileProtection, ProfileStoreError> {
    Ok(key::write_owner_only_file(path, contents.expose_secret().as_bytes())?)
}

/// `decrypt_import_payload`の結果。`passphrase_trimmed`は、入力のままでは復号できず、前後の空白・不可視文字を
/// 除いたパスフレーズで復号できたこと(貼り付けで混ざった文字を、利用者へ知らせるための情報)。
pub struct DecryptedPayload {
    pub payload: ExportPayload,
    pub passphrase_trimmed: bool,
}

/// 全体インポートで、名前衝突の解決後の名前を持つプロファイルを作る。ルールの複製(平文)を持つため、
/// 消去する型(`Zeroizing`)で返し、使い終えたときに消去させる。
fn renamed_profile_for_import(
    resolved_name: &str,
    exported: &ExportedProfile,
) -> Result<Zeroizing<RuleProfile>, ProfileStoreError> {
    RuleProfile::new(
        resolved_name,
        exported.profile.description().map(str::to_string),
        exported.profile.rules().to_vec(),
    )
    .map(Zeroizing::new)
    .map_err(|e| ProfileStoreError::InvalidProfileName(e.to_string()))
}

fn check_format_version(found: u32) -> Result<(), ProfileStoreError> {
    if found != bulk::CURRENT_FORMAT_VERSION {
        return Err(ProfileStoreError::UnsupportedFormatVersion { found, supported: bulk::CURRENT_FORMAT_VERSION });
    }
    Ok(())
}

const MAX_PROFILES_PER_IMPORT: usize = 200;
/// 1プロファイルあたりのルール数上限。`.smx`インポートだけでなく、CLIの
/// `masker profile create --from-json`等、外部から一括でルール集合を受け取る
/// 経路全てで同じ値を再利用する。
pub const MAX_RULES_PER_PROFILE: usize = 500;
const MAX_TOTAL_RULES_PER_IMPORT: usize = 2000;

/// `ExportPayload`への型付きデシリアライズ(各ルールの正規表現を実際にコンパイルする
/// `Rule::new`経由)を行う前に、プロファイル数・ルール数を`serde_json::Value`として
/// 構造的に(regexへは一切触れずに)検査する。巨大な数のルールを仕込んだ悪意ある
/// ファイルによるコンパイルコストの積み上げを、型付きデシリアライズ自体が走る前に
/// 防ぐため。JSON自体が不正な形の場合はここでは何も拒否せず、後続の
/// 型付きデシリアライズが持つ`CorruptProfileData`に判断を委ねる。
fn check_import_size_limits(json: &[u8]) -> Result<(), ProfileStoreError> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(json) else {
        return Ok(());
    };

    // 単一/全体の判別は、実際のExportPayloadデシリアライズと同じ"kind"タグを基準にする
    // ("profiles"キーの有無等の周辺的な手がかりで代用すると、実際のデシリアライズが
    // 無視する余分なキーを足すだけで判定をすり抜けられる)。
    let rule_counts: Vec<usize> = match value.get("kind").and_then(serde_json::Value::as_str) {
        Some("all") => {
            let profiles = value.get("profiles").and_then(serde_json::Value::as_array);
            let profiles = match profiles {
                Some(profiles) => profiles,
                None => return Ok(()),
            };
            if profiles.len() > MAX_PROFILES_PER_IMPORT {
                return Err(ProfileStoreError::ImportTooLarge(format!(
                    "プロファイル数が上限({MAX_PROFILES_PER_IMPORT}件)を超えています"
                )));
            }
            profiles.iter().map(rule_count_of).collect()
        }
        Some("single") => match value.get("profile") {
            Some(profile) => vec![rule_count_of(profile)],
            None => return Ok(()),
        },
        // 未知のkindは、この後の型付きデシリアライズが持つCorruptProfileData等の
        // 適切なエラーに判断を委ねる(ここでは何も拒否しない)。
        _ => return Ok(()),
    };

    if let Some(&max) = rule_counts.iter().max() {
        if max > MAX_RULES_PER_PROFILE {
            return Err(ProfileStoreError::ImportTooLarge(format!(
                "1プロファイルあたりのルール数が上限({MAX_RULES_PER_PROFILE}件)を超えています"
            )));
        }
    }

    let total: usize = rule_counts.iter().sum();
    if total > MAX_TOTAL_RULES_PER_IMPORT {
        return Err(ProfileStoreError::ImportTooLarge(format!(
            "インポート全体のルール数合計が上限({MAX_TOTAL_RULES_PER_IMPORT}件)を超えています"
        )));
    }

    Ok(())
}

struct RuleCounts {
    total: usize,
    enabled: usize,
}

fn rule_count_of(profile: &serde_json::Value) -> usize {
    profile.get("rules").and_then(serde_json::Value::as_array).map_or(0, Vec::len)
}

// RuleDtoの"enabled"は#[serde(default = "default_enabled")]でtrueが既定のため、
// フィールド自体が無い場合もtrue(有効)として数える。
fn enabled_rule_count_of(profile: &serde_json::Value) -> usize {
    profile.get("rules").and_then(serde_json::Value::as_array).map_or(0, |rules| {
        rules.iter().filter(|r| r.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(true)).count()
    })
}

/// タグ名自体はget-or-createで解決するため衝突しないが、1プロファイルの`tags`内で
/// 同じ名前が複数回渡された場合(手作りされた/破損したファイル由来。正規のexportでは
/// 発生しない)、profile_tags側の複合主キーで重複挿入になるため、そちらもON CONFLICTで
/// 無視する(タグを2回指定しても1回指定と同じ結果になるだけで、エラーにはしない)。
fn attach_tags(conn: &Connection, profile_id: i64, tags: &[String]) -> Result<(), ProfileStoreError> {
    for tag_name in tags {
        // set_profile_tags(ユーザー入力)だけでなく、インポート経由のタグ(ファイル内の
        // 任意の文字列)もここを通るため、この関数自身で検証する。呼び出し元での
        // チェック漏れがあってもDBへの書き込み前に必ず1箇所で弾かれるようにするため。
        masking_core::validate_display_name(tag_name).map_err(ProfileStoreError::InvalidTagName)?;
        conn.execute("INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING", [tag_name])?;
        let tag_id: i64 = conn.query_row("SELECT id FROM tags WHERE name = ?1", [tag_name], |row| row.get(0))?;
        conn.execute(
            "INSERT INTO profile_tags (profile_id, tag_id) VALUES (?1, ?2) ON CONFLICT(profile_id, tag_id) DO NOTHING",
            (profile_id, tag_id),
        )?;
    }
    Ok(())
}

/// 暗号化済みのプロファイル1件をprofilesテーブルに挿入し、お気に入り・タグを反映する。
/// 名前が既に存在する場合は`ProfileAlreadyExists`(呼び出し側は事前にpreview_importで
/// 重複を解決済みのはずだが、念のため二重チェックする)。
fn insert_profile_row(
    conn: &Connection,
    name: &str,
    encrypted: &crypto::Encrypted,
    is_favorite: bool,
    tags: &[String],
) -> Result<(), ProfileStoreError> {
    // 全体インポートの衝突解決(bulk::resolve_name)はサフィックスを付与するだけで
    // 双方向書式文字等は追加しないが、既に上限文字数ぎりぎりの名前だと付与後に
    // 長さ上限を超えうる。タグと同じくこの書き込み直前の1箇所で検証することで、
    // 呼び出し元(単一インポート・全体インポートの衝突解決後いずれも)を問わず
    // 上限超過の書き込みを防ぐ。
    masking_core::validate_display_name(name).map_err(ProfileStoreError::InvalidProfileName)?;

    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1)", [name], |row| row.get(0))?;
    if exists {
        return Err(ProfileStoreError::ProfileAlreadyExists(name.to_string()));
    }

    conn.execute(
        "INSERT INTO profiles (name, rules_encrypted, nonce, is_favorite, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        (name, &encrypted.ciphertext, &encrypted.nonce, is_favorite),
    )?;
    let profile_id = conn.last_insert_rowid();

    attach_tags(conn, profile_id, tags)
}

/// `settings`テーブルは「行が無い(未初期化状態)」であればアクティブ未設定として扱うが、
/// それ以外のDBエラー(ロック競合・I/O異常等)は握り潰さずそのまま伝播させる。
fn read_active_profile_name(conn: &Connection) -> Result<Option<String>, ProfileStoreError> {
    match conn.query_row("SELECT active_profile_name FROM settings WHERE id = 1", [], |row| row.get(0)) {
        Ok(name) => Ok(name),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 名前でプロファイルを検索するクエリが「行が無い」場合だけ`ProfileNotFound`に変換し、
/// それ以外のDBエラー(ロック競合・破損等)は`ProfileNotFound`に化けさせずそのまま伝播させる。
fn not_found_unless_other_db_error(err: rusqlite::Error, name: &str) -> ProfileStoreError {
    match err {
        rusqlite::Error::QueryReturnedNoRows => ProfileStoreError::ProfileNotFound(name.to_string()),
        other => other.into(),
    }
}

fn upsert_active_profile_name(conn: &Connection, name: &str) -> Result<(), ProfileStoreError> {
    conn.execute(
        "INSERT INTO settings (id, active_profile_name) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET active_profile_name = ?1",
        [name],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use masking_core::{Mode, PatternType, Rule};
    use secrecy::ExposeSecret;

    // 消去してから解放したかを、解放の直前の中身で確かめる(wipe_check)。
    #[global_allocator]
    static WIPE_CHECK_ALLOCATOR: wipe_check::WipeCheckAllocator = wipe_check::WipeCheckAllocator;

    fn temp_paths() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path());
        (dir, paths)
    }

    fn sample_profile(name: &str) -> RuleProfile {
        let rule = Rule::new(
            "ip",
            PatternType::Regex,
            r"\d+\.\d+\.\d+\.\d+",
            Mode::Sequential,
            None,
            Some("__MASK_IP_".to_string()),
            true,
            None,
        )
        .unwrap();
        RuleProfile::new(name, None, vec![rule]).unwrap()
    }

    // ルール数上限テスト用。literalルールは正規表現コンパイルを伴わないため、
    // 大量生成してもテスト自体は高速なまま。
    fn profile_with_n_rules(name: &str, n: usize) -> RuleProfile {
        let rules = (0..n)
            .map(|i| {
                Rule::new(
                    format!("r{i}"),
                    PatternType::Literal,
                    format!("value{i}"),
                    Mode::Fixed,
                    Some("masked".to_string()),
                    None,
                    true,
                    None,
                )
                .unwrap()
            })
            .collect();
        RuleProfile::new(name, None, rules).unwrap()
    }

    fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn is_initialized_is_false_before_init() {
        let (_dir, paths) = temp_paths();
        assert!(!is_initialized_at(&paths).unwrap());
    }

    #[test]
    fn init_creates_key_and_db_and_is_idempotent() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        assert!(paths.key_path.exists());
        assert!(paths.db_path.exists());
        assert!(is_initialized_at(&paths).unwrap());

        // 2回目もエラーにならない(冪等)。
        init_at(&paths).unwrap();
    }

    #[test]
    fn partially_initialized_state_is_reported_as_an_error() {
        let (_dir, paths) = temp_paths();
        std::fs::create_dir_all(paths.key_path.parent().unwrap()).unwrap();
        std::fs::write(&paths.key_path, [0u8; key::KEY_LEN]).unwrap();
        // db_pathは作らない。

        let err = is_initialized_at(&paths).expect_err("鍵のみ存在する状態はエラーのはず");
        assert!(matches!(
            err,
            ProfileStoreError::PartiallyInitialized { key_exists: true, db_exists: false, .. }
        ));
    }

    #[test]
    fn open_fails_with_not_initialized_before_init() {
        let (_dir, paths) = temp_paths();
        let err = ProfileStore::open_at(&paths).expect_err("初期化前はエラーのはず");
        assert!(matches!(err, ProfileStoreError::NotInitialized));
    }

    #[test]
    fn debug_output_never_contains_the_raw_key_bytes() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        let key_bytes = key::load_key(&paths.key_path).unwrap();
        let debug_output = format!("{store:?}");

        assert!(!debug_output.contains(&format!("{:?}", key_bytes.expose_secret())));
        assert!(debug_output.contains("REDACTED"));
    }

    #[test]
    fn create_and_get_profile_round_trips() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        store.create_profile(&sample_profile("work")).unwrap();
        let loaded = store.get_profile("work").unwrap();

        assert_eq!(loaded.profile_name(), "work");
        assert_eq!(loaded.rules().len(), 1);
    }

    #[test]
    fn rule_pattern_data_is_actually_encrypted_at_rest() {
        // プロファイル名は一覧表示・検索のため設計上平文カラムに保存される
        // (機微情報ではないため問題ない)。暗号化が必要なのはルール本体(パターン等)。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let rule = Rule::new(
            "r",
            PatternType::Literal,
            "VERY-SECRET-PATTERN-XYZ",
            Mode::Fixed,
            Some("DUMMY".to_string()),
            None,
            true,
            None,
        )
        .unwrap();
        store.create_profile(&RuleProfile::new("p", None, vec![rule]).unwrap()).unwrap();

        let raw = std::fs::read(&paths.db_path).unwrap();
        let raw_text = String::from_utf8_lossy(&raw);
        assert!(
            !raw_text.contains("VERY-SECRET-PATTERN-XYZ"),
            "ルールのパターン(機微情報)がDBファイルの生バイト列に平文で含まれてはいけない"
        );
    }

    #[test]
    fn swapping_ciphertext_between_two_profiles_fails_to_decrypt() {
        // AADにプロファイル名を束ねているため、DBファイルを直接書き換えて別行の
        // rules_encrypted/nonceを入れ替えても復号自体が失敗するはず。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        store.create_profile(&sample_profile("personal")).unwrap();

        {
            let conn = Connection::open(&paths.db_path).unwrap();
            let (work_ct, work_nonce): (Vec<u8>, Vec<u8>) = conn
                .query_row("SELECT rules_encrypted, nonce FROM profiles WHERE name = 'work'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
            conn.execute(
                "UPDATE profiles SET rules_encrypted = ?1, nonce = ?2 WHERE name = 'personal'",
                (work_ct, work_nonce),
            )
            .unwrap();
        }

        let err = store.get_profile("personal").expect_err("入れ替えられた暗号文は復号に失敗するはず");
        assert!(matches!(err, ProfileStoreError::Crypto(_)));
    }

    #[test]
    fn creating_a_duplicate_name_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("dup")).unwrap();

        let err = store.create_profile(&sample_profile("dup")).expect_err("重複作成はエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileAlreadyExists(name) if name == "dup"));
    }

    #[test]
    fn database_lock_contention_is_not_mistaken_for_a_missing_profile() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();

        // 別コネクションで排他ロックを取得したまま保持し、本来存在するプロファイルへの
        // アクセスが「見つからない」に化けず、DBエラーとして伝播することを確認する。
        let locker = Connection::open(&paths.db_path).unwrap();
        locker.execute_batch("BEGIN EXCLUSIVE").unwrap();

        let err = store.get_profile("work").expect_err("ロック競合中はエラーになるはず");
        assert!(matches!(err, ProfileStoreError::Db(_)), "ProfileNotFoundに化けてはいけない: {err:?}");

        locker.execute_batch("COMMIT").unwrap();
        store.get_profile("work").expect("ロック解放後は正常に取得できるはず");
    }

    #[test]
    fn creating_a_duplicate_name_does_not_leave_a_transaction_open() {
        // create_profileがトランザクション内でエラーを返した後も、接続が正常に使い続けられる
        // ことを確認する(トランザクション導入によるリグレッションが無いことの回帰テスト)。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("dup")).unwrap();
        let _ = store.create_profile(&sample_profile("dup"));

        store.create_profile(&sample_profile("other")).unwrap();
        assert_eq!(store.list_profiles().unwrap().len(), 2);
    }

    #[test]
    fn getting_a_missing_profile_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        let err = store.get_profile("nope").expect_err("存在しないプロファイルはエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileNotFound(name) if name == "nope"));
    }

    #[test]
    fn first_created_profile_becomes_active_automatically() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        store.create_profile(&sample_profile("first")).unwrap();
        let active = store.active_profile().unwrap().expect("最初のプロファイルは自動的にアクティブになるはず");
        assert_eq!(active.profile_name(), "first");
    }

    #[test]
    fn second_created_profile_does_not_steal_active_status() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        store.create_profile(&sample_profile("first")).unwrap();
        store.create_profile(&sample_profile("second")).unwrap();

        let active = store.active_profile().unwrap().unwrap();
        assert_eq!(active.profile_name(), "first");
    }

    #[test]
    fn set_active_profile_switches_the_active_profile() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("first")).unwrap();
        store.create_profile(&sample_profile("second")).unwrap();

        store.set_active_profile("second").unwrap();

        let active = store.active_profile().unwrap().unwrap();
        assert_eq!(active.profile_name(), "second");
    }

    #[test]
    fn set_active_profile_on_a_missing_name_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.set_active_profile("nope").expect_err("存在しない名前はエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileNotFound(name) if name == "nope"));
    }

    #[test]
    fn deleting_the_active_profile_is_rejected() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("only")).unwrap();

        let err = store.delete_profile("only").expect_err("アクティブなプロファイルの削除はエラーのはず");
        assert!(matches!(err, ProfileStoreError::CannotDeleteActiveProfile));
    }

    #[test]
    fn deleting_a_non_active_profile_succeeds() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("first")).unwrap();
        store.create_profile(&sample_profile("second")).unwrap();

        store.delete_profile("second").unwrap();

        let err = store.get_profile("second").expect_err("削除済みのプロファイルは取得できないはず");
        assert!(matches!(err, ProfileStoreError::ProfileNotFound(_)));
    }

    #[test]
    fn deleting_a_missing_profile_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.delete_profile("nope").expect_err("存在しない名前はエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileNotFound(name) if name == "nope"));
    }

    #[test]
    fn list_profiles_reports_rule_count_favorite_and_active_status() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("alpha")).unwrap();
        store.create_profile(&sample_profile("beta")).unwrap();

        let summaries = store.list_profiles().unwrap();

        assert_eq!(summaries.len(), 2);
        let alpha = summaries.iter().find(|s| s.name == "alpha").unwrap();
        assert_eq!(alpha.rule_count, 1);
        assert_eq!(alpha.enabled_rule_count, 1, "sample_profileのルールは有効のはず");
        assert!(alpha.is_active, "最初に作成したalphaがアクティブなはず");
        assert!(!alpha.is_favorite);
        let beta = summaries.iter().find(|s| s.name == "beta").unwrap();
        assert!(!beta.is_active);
    }

    #[test]
    fn list_profiles_reports_zero_enabled_rules_when_all_are_disabled() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        let disabled_rule = Rule::new(
            "ip",
            PatternType::Regex,
            r"\d+\.\d+\.\d+\.\d+",
            Mode::Sequential,
            None,
            Some("__MASK_IP_".to_string()),
            false,
            None,
        )
        .unwrap();
        store.create_profile(&RuleProfile::new("all-disabled", None, vec![disabled_rule]).unwrap()).unwrap();

        let summary = store.list_profiles().unwrap().into_iter().find(|s| s.name == "all-disabled").unwrap();

        assert_eq!(summary.rule_count, 1, "総ルール数は無効ルールも数えるはず");
        assert_eq!(summary.enabled_rule_count, 0, "無効ルールは有効数に数えないはず");
    }

    #[test]
    fn has_active_profile_reflects_whether_one_is_set() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        assert!(!store.has_active_profile().unwrap(), "作成前はアクティブ未設定のはず");

        store.create_profile(&sample_profile("work")).unwrap();

        assert!(store.has_active_profile().unwrap(), "最初の作成でアクティブになるはず");
    }

    // 復号済みの内容(保留の対象)を持つImportPreviewを、実際のエクスポートから作る。
    fn single_import_preview(profile_name: &str) -> (tempfile::TempDir, ProfileStore, ImportPreview) {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile(profile_name)).unwrap();
        store_a.conn.execute("INSERT INTO tags (name) VALUES ('secret-tag')", []).unwrap();
        store_a
            .conn
            .execute(
                "INSERT INTO profile_tags (profile_id, tag_id)
                 SELECT (SELECT id FROM profiles WHERE name = ?1), (SELECT id FROM tags WHERE name = 'secret-tag')",
                [profile_name],
            )
            .unwrap();
        let exported = store_a.export_profile(profile_name, passphrase("pw")).unwrap();

        let (dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let store_b = ProfileStore::open_at(&paths_b).unwrap();
        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        (dir_b, store_b, preview)
    }

    fn import_preview_wipes() -> usize {
        IMPORT_PREVIEW_WIPES.with(|wipes| wipes.get())
    }

    #[test]
    fn zeroizing_a_single_import_preview_clears_its_contents() {
        let (_dir, _store, mut preview) = single_import_preview("work");
        let ImportPreview::Single { exported, .. } = &preview else { panic!("Singleを期待") };
        assert_eq!(exported.tags, vec!["secret-tag".to_string()], "消去の前は、タグを持っているはず");

        preview.zeroize();

        let ImportPreview::Single { name, exported } = &preview else { panic!("Singleを期待") };
        assert_eq!(name, "");
        assert_eq!(exported.profile.profile_name(), "");
        assert!(exported.profile.rules().is_empty());
        assert!(exported.tags.is_empty());
    }

    #[test]
    fn zeroizing_an_all_import_preview_clears_its_contents() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.create_profile(&sample_profile("home")).unwrap();
        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let store_b = ProfileStore::open_at(&paths_b).unwrap();
        let mut preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        assert!(matches!(&preview, ImportPreview::All { entries, .. } if entries.len() == 2));

        preview.zeroize();

        let ImportPreview::All { active_profile_name, entries, exported } = &preview else { panic!("Allを期待") };
        assert_eq!(active_profile_name, &None);
        assert!(entries.is_empty());
        assert!(exported.is_empty());
    }

    #[test]
    fn dropping_an_import_preview_erases_its_contents() {
        let (_dir, _store, preview) = single_import_preview("work");
        let before = import_preview_wipes();

        drop(preview);

        assert_eq!(import_preview_wipes(), before + 1, "dropするときに、内容が消去されるはず");
    }

    #[test]
    fn committing_an_import_erases_the_preview_it_consumed() {
        let (_dir, mut store, preview) = single_import_preview("work");
        let before = import_preview_wipes();

        let outcome = store.commit_import(preview).unwrap();

        assert_eq!(outcome, ImportOutcome::Single { name: "work".to_string(), activated: true });
        assert_eq!(import_preview_wipes(), before + 1, "確定した後にも、保留していた内容が消去されるはず");
    }

    fn empty_store() -> (tempfile::TempDir, ProfileStore) {
        let (dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();
        (dir, store)
    }

    // 全ての種類の文字列(名前・説明・パターン・固定値・接頭辞・タグ)を持つプロファイル。
    fn profile_with_every_text_field(name: &str) -> ExportedProfile {
        let fixed = Rule::new(
            "fixed-rule",
            PatternType::Literal,
            "fixed-secret-pattern",
            Mode::Fixed,
            Some("fixed-secret-value".to_string()),
            Some("secret-prefix".to_string()),
            true,
            Some("fixed-rule-description".to_string()),
        )
        .unwrap();
        let sequential = Rule::new(
            "sequential-rule",
            PatternType::Regex,
            r"secret-\d+",
            Mode::Sequential,
            None,
            Some("__MASK_SECRET_".to_string()),
            true,
            None,
        )
        .unwrap();
        ExportedProfile {
            is_favorite: false,
            tags: vec!["secret-tag-a".to_string(), "secret-tag-b".to_string()],
            profile: RuleProfile::new(name, Some("profile-description".to_string()), vec![fixed, sequential]).unwrap(),
        }
    }

    // profile_with_every_text_fieldの文字列のうち、プロファイル(名前・説明、2つのルールの各欄)の分と、タグの分。
    const PROFILE_TEXTS: usize = 10;
    const TAG_TEXTS: usize = 2;

    fn track_profile_texts(watch: &mut wipe_check::Watch, profile: &RuleProfile) {
        watch.track(profile.profile_name());
        watch.track_opt(profile.description());
        for rule in profile.rules() {
            watch.track(rule.name());
            watch.track(rule.pattern());
            watch.track_opt(rule.fixed_value());
            watch.track_opt(rule.prefix());
            watch.track_opt(rule.description());
        }
    }

    fn track_exported_profile(watch: &mut wipe_check::Watch, exported: &ExportedProfile) {
        track_profile_texts(watch, &exported.profile);
        for tag in &exported.tags {
            watch.track(tag);
        }
    }

    fn track_import_preview(watch: &mut wipe_check::Watch, preview: &ImportPreview) {
        match preview {
            ImportPreview::Single { name, exported } => {
                watch.track(name);
                track_exported_profile(watch, exported);
            }
            ImportPreview::All { active_profile_name, entries, exported } => {
                watch.track_opt(active_profile_name.as_deref());
                for entry in entries {
                    watch.track(&entry.original_name);
                    watch.track(&entry.resolved_name);
                }
                for profile in exported {
                    track_exported_profile(watch, profile);
                }
            }
        }
    }

    fn single_payload(name: &str) -> ExportPayload {
        ExportPayload::Single { format_version: bulk::CURRENT_FORMAT_VERSION, profile: profile_with_every_text_field(name) }
    }

    fn all_payload(names: &[&str], active_profile_name: Option<&str>) -> ExportPayload {
        ExportPayload::All {
            format_version: bulk::CURRENT_FORMAT_VERSION,
            active_profile_name: active_profile_name.map(str::to_string),
            profiles: names.iter().map(|name| profile_with_every_text_field(name)).collect(),
        }
    }

    fn track_payload(watch: &mut wipe_check::Watch, payload: &ExportPayload) {
        match payload {
            ExportPayload::Single { profile, .. } => track_exported_profile(watch, profile),
            ExportPayload::All { active_profile_name, profiles, .. } => {
                watch.track_opt(active_profile_name.as_deref());
                for profile in profiles {
                    track_exported_profile(watch, profile);
                }
            }
        }
    }

    #[test]
    fn dropping_an_exported_profile_erases_every_text_before_freeing_it() {
        let exported = profile_with_every_text_field("work");
        let mut watch = wipe_check::Watch::new();
        track_exported_profile(&mut watch, &exported);
        assert_eq!(watch.tracked_count(), PROFILE_TEXTS + TAG_TEXTS, "追跡する文字列を、取りこぼしている");

        drop(exported);

        watch.assert_all_wiped_when_freed();
    }

    // 確認画面へ進まずに、エラーで捨てる経路でも、復号したペイロードの内容は、消去されてから解放される。
    #[test]
    fn an_error_while_resolving_an_import_erases_the_decrypted_payload_before_freeing_it() {
        let (_dir, mut store) = empty_store();
        store.create_profile(&sample_profile("existing")).unwrap();

        let unsupported = |payload: ExportPayload| match payload {
            ExportPayload::Single { profile, .. } => {
                ExportPayload::Single { format_version: bulk::CURRENT_FORMAT_VERSION + 1, profile }
            }
            ExportPayload::All { active_profile_name, profiles, .. } => ExportPayload::All {
                format_version: bulk::CURRENT_FORMAT_VERSION + 1,
                active_profile_name,
                profiles,
            },
        };
        let cases: Vec<(&str, ExportPayload, usize)> = vec![
            ("既存の名前(単一)", single_payload("existing"), PROFILE_TEXTS + TAG_TEXTS),
            ("未対応のバージョン(単一)", unsupported(single_payload("new")), PROFILE_TEXTS + TAG_TEXTS),
            ("ファイル内の名前の重複(全体)", all_payload(&["dup", "dup"], Some("active-secret-name")), 2 * (PROFILE_TEXTS + TAG_TEXTS) + 1),
            ("未対応のバージョン(全体)", unsupported(all_payload(&["a", "b"], Some("active-secret-name"))), 2 * (PROFILE_TEXTS + TAG_TEXTS) + 1),
        ];

        for (label, payload, expected_tracked) in cases {
            let mut watch = wipe_check::Watch::new();
            track_payload(&mut watch, &payload);
            assert_eq!(watch.tracked_count(), expected_tracked, "{label}: 追跡する文字列を、取りこぼしている");

            let result = store.resolve_import_preview(payload);

            assert!(result.is_err(), "{label}: エラーになるはず");
            watch.assert_all_wiped_when_freed();
        }
    }

    // 対照: 確認画面へ進む(成功する)と、内容は、保留する内容へ引き渡され、捨てるまで、解放されない。
    #[test]
    fn a_successful_resolve_hands_the_payload_over_to_the_preview_which_erases_it_when_dropped() {
        let (_dir, store) = empty_store();
        let mut watch = wipe_check::Watch::new();
        let payload = all_payload(&["a", "b"], Some("a"));
        track_payload(&mut watch, &payload);

        let preview = store.resolve_import_preview(payload).unwrap();

        let still_alive = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| watch.assert_all_wiped_when_freed()));
        assert!(still_alive.is_err(), "保留している間は、内容は、まだ解放されていないはず");
        drop(preview);
        watch.assert_all_wiped_when_freed();
    }

    #[test]
    fn dropping_an_import_preview_erases_every_text_before_freeing_it() {
        let (_dir, store) = empty_store();
        let previews = [
            ("単一", store.resolve_import_preview(single_payload("single")).unwrap()),
            ("全体", store.resolve_import_preview(all_payload(&["a", "b"], Some("a"))).unwrap()),
        ];

        for (label, preview) in previews {
            let mut watch = wipe_check::Watch::new();
            track_import_preview(&mut watch, &preview);
            assert!(watch.tracked_count() > PROFILE_TEXTS, "{label}: 追跡する文字列を、取りこぼしている");

            drop(preview);

            watch.assert_all_wiped_when_freed();
        }
    }

    #[test]
    fn committing_an_import_erases_every_text_of_the_preview_before_freeing_it() {
        let (_dir, mut store) = empty_store();
        let previews = [
            store.resolve_import_preview(single_payload("single")).unwrap(),
            store.resolve_import_preview(all_payload(&["a", "b"], Some("a"))).unwrap(),
        ];

        for preview in previews {
            let mut watch = wipe_check::Watch::new();
            track_import_preview(&mut watch, &preview);

            store.commit_import(preview).unwrap();

            watch.assert_all_wiped_when_freed();
        }
    }

    // 名前を差し替えたプロファイル(ルールの複製を含む)は、`Zeroizing`で持ち、捨てるときに、消去される。
    #[test]
    fn a_renamed_profile_for_import_is_erased_before_freeing_it() {
        let exported = profile_with_every_text_field("original");

        let renamed: Zeroizing<RuleProfile> = renamed_profile_for_import("original (インポート)", &exported).unwrap();

        assert_eq!(renamed.profile_name(), "original (インポート)");
        assert_eq!(renamed.rules(), exported.profile.rules());
        let mut watch = wipe_check::Watch::new();
        track_profile_texts(&mut watch, &renamed);
        assert_eq!(watch.tracked_count(), PROFILE_TEXTS, "追跡する文字列を、取りこぼしている");
        drop(renamed);
        watch.assert_all_wiped_when_freed();
    }

    #[test]
    fn a_key_file_export_of_a_profile_round_trips_into_a_different_store() {
        let (_dir_a, mut store_a) = empty_store();
        store_a.create_profile(&sample_profile("work")).unwrap();
        let exported = store_a.export_profile_with_key_file("work").unwrap();

        let (_dir_b, mut store_b) = empty_store();
        assert_eq!(detect_import_method(&exported.ciphertext).unwrap(), ImportMethod::KeyFile);
        let decrypted =
            decrypt_import_payload_with_key_file(&exported.ciphertext, &exported.key_file_contents).unwrap();
        assert!(!decrypted.passphrase_trimmed);
        let preview = store_b.resolve_import_preview(decrypted.payload).unwrap();
        store_b.commit_import(preview).unwrap();

        assert_eq!(store_b.get_profile("work").unwrap().rules().len(), 1);
    }

    #[test]
    fn a_key_file_export_of_all_profiles_round_trips_into_a_different_store() {
        let (_dir_a, mut store_a) = empty_store();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.create_profile(&sample_profile("home")).unwrap();
        let exported = store_a.export_all_with_key_file().unwrap();

        let (_dir_b, mut store_b) = empty_store();
        let decrypted =
            decrypt_import_payload_with_key_file(&exported.ciphertext, &exported.key_file_contents).unwrap();
        let preview = store_b.resolve_import_preview(decrypted.payload).unwrap();
        store_b.commit_import(preview).unwrap();

        let names: Vec<String> = store_b.list_profiles().unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"work".to_string()) && names.contains(&"home".to_string()));
    }

    #[test]
    fn a_key_file_export_keeps_the_favorite_and_tags_like_the_passphrase_export() {
        let (_dir_a, mut store_a) = empty_store();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.conn.execute("UPDATE profiles SET is_favorite = 1 WHERE name = 'work'", []).unwrap();
        let exported = store_a.export_profile_with_key_file("work").unwrap();

        let decrypted =
            decrypt_import_payload_with_key_file(&exported.ciphertext, &exported.key_file_contents).unwrap();

        let ExportPayload::Single { profile, .. } = &decrypted.payload else { panic!("Singleを期待") };
        assert!(profile.is_favorite);
    }

    #[test]
    fn the_passphrase_path_refuses_a_key_file_export_and_says_a_key_file_is_needed() {
        let (_dir, mut store) = empty_store();
        store.create_profile(&sample_profile("work")).unwrap();
        let exported = store.export_profile_with_key_file("work").unwrap();

        let error = decrypt_import_payload(&exported.ciphertext, passphrase("pw"))
            .err()
            .expect("パスフレーズでは、復号できないはず");

        assert!(error.to_string().contains("鍵ファイル"), "鍵ファイルが必要なことを知らせるはず: {error}");
    }

    #[test]
    fn a_key_file_that_does_not_match_the_export_is_rejected() {
        let (_dir, mut store) = empty_store();
        store.create_profile(&sample_profile("work")).unwrap();
        let exported = store.export_profile_with_key_file("work").unwrap();
        let other = store.export_profile_with_key_file("work").unwrap();

        let result = decrypt_import_payload_with_key_file(&exported.ciphertext, &other.key_file_contents);

        assert!(result.is_err());
    }

    #[test]
    fn write_key_file_writes_the_key_file_contents_only_the_owner_can_read() {
        let (_dir, mut store) = empty_store();
        store.create_profile(&sample_profile("work")).unwrap();
        let exported = store.export_profile_with_key_file("work").unwrap();
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("export.smxkey");

        let protection = write_key_file(&path, &exported.key_file_contents).unwrap();

        assert_eq!(protection, FileProtection::OwnerOnly);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), exported.key_file_contents.expose_secret());
    }

    #[test]
    fn decrypt_import_payload_reports_whether_the_passphrase_was_trimmed() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        let exported = store.export_profile("work", passphrase("pw")).unwrap();

        let exact = decrypt_import_payload(&exported, passphrase("pw")).unwrap();
        assert!(!exact.passphrase_trimmed);
        assert!(matches!(exact.payload, ExportPayload::Single { .. }));

        let padded = decrypt_import_payload(&exported, passphrase("  pw ")).unwrap();
        assert!(padded.passphrase_trimmed);
        assert!(matches!(padded.payload, ExportPayload::Single { .. }));
    }

    #[test]
    fn export_then_import_round_trips_into_a_different_store() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();

        let exported = store_a.export_profile("work", passphrase("pw")).unwrap();

        // 別ディレクトリ(=別マシンを模したストア。鍵ファイルも別物)でも、パスフレーズだけで
        // 復号・取り込みできることを確認する。
        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        assert!(matches!(&preview, ImportPreview::Single { name, .. } if name == "work"));
        let outcome = store_b.commit_import(preview).unwrap();

        // store_bは新規のためアクティブ未設定 → このインポートでactivated=trueになるはず。
        assert_eq!(outcome, ImportOutcome::Single { name: "work".to_string(), activated: true });
        assert_eq!(store_b.get_profile("work").unwrap().rules().len(), 1);
    }

    #[test]
    fn single_export_import_round_trips_favorite_and_tags() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.conn.execute("UPDATE profiles SET is_favorite = 1 WHERE name = 'work'", []).unwrap();
        store_a.conn.execute("INSERT INTO tags (name) VALUES ('sip')", []).unwrap();
        store_a
            .conn
            .execute(
                "INSERT INTO profile_tags (profile_id, tag_id)
                 SELECT (SELECT id FROM profiles WHERE name = 'work'), (SELECT id FROM tags WHERE name = 'sip')",
                [],
            )
            .unwrap();

        let exported = store_a.export_profile("work", passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        store_b.commit_import(preview).unwrap();

        let summary = store_b.list_profiles().unwrap().into_iter().find(|s| s.name == "work").unwrap();
        assert!(summary.is_favorite, "お気に入り状態が引き継がれるはず");
        let re_exported = store_b.read_exported_profile("work").unwrap();
        assert_eq!(re_exported.tags, vec!["sip".to_string()], "タグが引き継がれるはず");
    }

    #[test]
    fn importing_with_the_wrong_passphrase_fails_and_creates_nothing() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        let exported = store_a.export_profile("work", passphrase("correct")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let store_b = ProfileStore::open_at(&paths_b).unwrap();
        let err = store_b
            .preview_import(&exported, passphrase("wrong"))
            .expect_err("誤ったパスフレーズは拒否されるはず");

        assert!(matches!(err, ProfileStoreError::Export(_)));
        assert_eq!(store_b.list_profiles().unwrap().len(), 0, "失敗時はプロファイルが作成されてはいけない");
    }

    #[test]
    fn previewing_a_single_import_with_a_name_that_already_exists_is_rejected_immediately() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        let exported = store.export_profile("work", passphrase("pw")).unwrap();

        let err = store
            .preview_import(&exported, passphrase("pw"))
            .expect_err("単一インポートの同名重複はpreview時点で拒否されるはず");

        assert!(matches!(err, ProfileStoreError::ProfileAlreadyExists(name) if name == "work"));
    }

    #[test]
    fn exporting_a_missing_profile_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        let err = store
            .export_profile("nope", passphrase("pw"))
            .expect_err("存在しないプロファイルのエクスポートはエラーのはず");

        assert!(matches!(err, ProfileStoreError::ProfileNotFound(name) if name == "nope"));
    }

    #[test]
    fn importing_data_that_decrypts_but_is_not_a_valid_export_payload_fails_cleanly() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        // パスフレーズは正しく復号できるが、中身がExportPayloadのJSONではないデータ
        // (例: 無関係なファイルを誤ってこの形式で再暗号化した場合)を模擬する。
        let not_a_payload = export::encrypt_for_export(b"not an export payload", passphrase("pw")).unwrap();

        let err = store
            .preview_import(&not_a_payload, passphrase("pw"))
            .expect_err("ExportPayloadとして解釈できないデータは拒否されるはず");

        assert!(matches!(err, ProfileStoreError::CorruptProfileData(_)));
    }

    #[test]
    fn export_all_then_import_all_round_trips_all_profiles_and_active_name() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.create_profile(&sample_profile("personal")).unwrap();
        store_a.set_active_profile("personal").unwrap();

        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        let ImportPreview::All { entries, .. } = &preview else { panic!("Allを期待") };
        assert!(entries.iter().all(|e| !e.renamed), "空のDBへのインポートはリネームされないはず");

        let outcome = store_b.commit_import(preview).unwrap();
        let ImportOutcome::All { activated_profile_name, .. } = outcome else { panic!("Allを期待") };
        assert_eq!(activated_profile_name, Some("personal".to_string()));

        let summaries = store_b.list_profiles().unwrap();
        assert_eq!(summaries.len(), 2);
        let active = store_b.active_profile().unwrap().unwrap();
        assert_eq!(active.profile_name(), "personal", "アクティブプロファイル名が引き継がれるはず");
    }

    #[test]
    fn import_all_does_not_overwrite_an_already_active_profile() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        // インポート先には既にアクティブなプロファイルが存在する状態を作る。
        store_b.create_profile(&sample_profile("existing")).unwrap();

        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        let outcome = store_b.commit_import(preview).unwrap();
        let ImportOutcome::All { activated_profile_name, .. } = outcome else { panic!("Allを期待") };
        assert_eq!(activated_profile_name, None, "既にアクティブがある場合はactivated情報も無いはず");

        let active = store_b.active_profile().unwrap().unwrap();
        assert_eq!(active.profile_name(), "existing", "既にアクティブがある場合は上書きされないはず");
    }

    #[test]
    fn import_all_renames_colliding_names_and_creates_the_rest() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.create_profile(&sample_profile("personal")).unwrap();
        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        store_b.create_profile(&sample_profile("work")).unwrap(); // 衝突させる

        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        let ImportPreview::All { entries, .. } = &preview else { panic!("Allを期待") };
        let work_entry = entries.iter().find(|e| e.original_name == "work").unwrap();
        assert!(work_entry.renamed);
        assert_eq!(work_entry.resolved_name, "work (インポート)");
        let personal_entry = entries.iter().find(|e| e.original_name == "personal").unwrap();
        assert!(!personal_entry.renamed);

        store_b.commit_import(preview).unwrap();

        let names: Vec<String> = store_b.list_profiles().unwrap().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"work".to_string()), "元のworkは変更されず残っているはず");
        assert!(names.contains(&"work (インポート)".to_string()));
        assert!(names.contains(&"personal".to_string()));
    }

    #[test]
    fn import_all_embeds_the_resolved_name_in_the_stored_plaintext_after_a_rename() {
        // リネームされたエントリについて、暗号文内(復号後の平文)の
        // profile_nameがAAD/DB上のname列(resolved_name)と食い違わないことを確認する。
        // AAD自体は元々resolved_nameで一致していたため、この不一致があっても復号自体は
        // 成功してしまう(認証はcipheretextとAADの対応のみを保証し、中身の内容までは
        // 保証しない)ため、実際にget_profileした結果のprofile_nameを見て確認する。
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        store_b.create_profile(&sample_profile("work")).unwrap(); // 衝突させる

        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        store_b.commit_import(preview).unwrap();

        let renamed = store_b.get_profile("work (インポート)").unwrap();
        assert_eq!(
            renamed.profile_name(),
            "work (インポート)",
            "暗号文内のprofile_nameもresolved_nameと一致するはず"
        );
    }

    #[test]
    fn import_all_rejects_a_collision_rename_that_would_exceed_the_length_limit() {
        // 衝突解決のサフィックス(" (インポート)")自体は攻撃者由来ではないが、既に
        // 上限文字数ぎりぎりの名前に付与すると上限を超えうる。insert_profile_row側の
        // 検証で拒否され、かつ1トランザクションのため他のプロファイルも巻き込まれて
        // 作成されないことを確認する(全体は1トランザクションにまとめる既存方針通り)。
        let at_limit_name = "a".repeat(masking_core::MAX_DISPLAY_NAME_LENGTH);

        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile(&at_limit_name)).unwrap();
        store_a.create_profile(&sample_profile("personal")).unwrap();
        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        store_b.create_profile(&sample_profile(&at_limit_name)).unwrap(); // 衝突させる

        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        let err = store_b.commit_import(preview).expect_err("リネーム後に上限を超えるので拒否されるはず");
        assert!(matches!(err, ProfileStoreError::InvalidProfileName(_)));

        let names: Vec<String> = store_b.list_profiles().unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec![at_limit_name], "1件でも失敗した場合、他のpersonalも作成されないはず");
    }

    #[test]
    fn import_all_creates_missing_tags_and_reuses_existing_ones() {
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.conn.execute("INSERT INTO tags (name) VALUES ('sip')", []).unwrap();
        store_a
            .conn
            .execute(
                "INSERT INTO profile_tags (profile_id, tag_id)
                 SELECT (SELECT id FROM profiles WHERE name = 'work'), (SELECT id FROM tags WHERE name = 'sip')",
                [],
            )
            .unwrap();
        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        // インポート先には既に同名のタグが存在する状態を作る(使い回されるはず)。
        store_b.conn.execute("INSERT INTO tags (name) VALUES ('sip')", []).unwrap();

        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();
        store_b.commit_import(preview).unwrap();

        let tag_count: i64 = store_b.conn.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0)).unwrap();
        assert_eq!(tag_count, 1, "既存タグが使い回され、重複作成されないはず");
        let re_exported = store_b.read_exported_profile("work").unwrap();
        assert_eq!(re_exported.tags, vec!["sip".to_string()]);
    }

    #[test]
    fn commit_import_all_does_not_partially_write_when_one_entry_fails() {
        // insert_profile_rowの二重チェックが無ければ通ってしまうはずの状況を作る:
        // previewの後、commitの前に同名プロファイルが割り込んで作成されるケース。
        let (_dir_a, paths_a) = temp_paths();
        init_at(&paths_a).unwrap();
        let mut store_a = ProfileStore::open_at(&paths_a).unwrap();
        store_a.create_profile(&sample_profile("work")).unwrap();
        store_a.create_profile(&sample_profile("personal")).unwrap();
        let exported = store_a.export_all(passphrase("pw")).unwrap();

        let (_dir_b, paths_b) = temp_paths();
        init_at(&paths_b).unwrap();
        let mut store_b = ProfileStore::open_at(&paths_b).unwrap();
        let preview = store_b.preview_import(&exported, passphrase("pw")).unwrap();

        // previewはリネーム不要と判定した後で、横から同名プロファイルが作られた状況を模擬する。
        store_b.create_profile(&sample_profile("work")).unwrap();

        let err = store_b.commit_import(preview).expect_err("二重チェックにより失敗するはず");
        assert!(matches!(err, ProfileStoreError::ProfileAlreadyExists(_)));

        let names: Vec<String> = store_b.list_profiles().unwrap().into_iter().map(|s| s.name).collect();
        assert!(!names.contains(&"personal".to_string()), "1件でも失敗したら全体がロールバックされるはず");
    }

    #[test]
    fn importing_a_profile_with_a_duplicate_tag_in_its_own_list_attaches_it_only_once() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let payload = bulk::ExportPayload::All {
            format_version: bulk::CURRENT_FORMAT_VERSION,
            active_profile_name: None,
            profiles: vec![bulk::ExportedProfile {
                is_favorite: false,
                tags: vec!["sip".to_string(), "sip".to_string()],
                profile: sample_profile("work"),
            }],
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let encrypted = export::encrypt_for_export(&json, passphrase("pw")).unwrap();

        let preview = store.preview_import(&encrypted, passphrase("pw")).unwrap();
        store.commit_import(preview).unwrap();

        let exported = store.read_exported_profile("work").unwrap();
        assert_eq!(exported.tags, vec!["sip".to_string()], "重複タグは1回だけ紐付くはず");
    }

    #[test]
    fn check_import_size_limits_rejects_too_many_profiles() {
        let profiles: Vec<serde_json::Value> = (0..(MAX_PROFILES_PER_IMPORT + 1))
            .map(|i| serde_json::json!({"profile_name": format!("p{i}"), "rules": []}))
            .collect();
        let json = serde_json::to_vec(&serde_json::json!({"kind": "all", "profiles": profiles})).unwrap();

        let err = check_import_size_limits(&json).expect_err("プロファイル数上限超過は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::ImportTooLarge(_)));
    }

    #[test]
    fn check_import_size_limits_rejects_too_many_rules_in_one_profile() {
        let rules: Vec<serde_json::Value> = (0..(MAX_RULES_PER_PROFILE + 1)).map(|_| serde_json::json!({})).collect();
        let json = serde_json::to_vec(&serde_json::json!({"kind": "single", "profile": {"rules": rules}})).unwrap();

        let err = check_import_size_limits(&json).expect_err("1プロファイルのルール数上限超過は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::ImportTooLarge(_)));
    }

    #[test]
    fn check_import_size_limits_rejects_total_rules_over_the_aggregate_cap_even_when_each_profile_is_under_the_per_profile_cap()
     {
        // 各プロファイル単体はMAX_RULES_PER_PROFILE未満でも、合計がMAX_TOTAL_RULES_PER_IMPORTを
        // 超える場合は拒否されることを確認する(1プロファイルあたりの上限だけでは防げない経路)。
        let rules_per_profile = MAX_RULES_PER_PROFILE - 1;
        let profile_count = MAX_TOTAL_RULES_PER_IMPORT / rules_per_profile + 2;
        let rules: Vec<serde_json::Value> = (0..rules_per_profile).map(|_| serde_json::json!({})).collect();
        let profiles: Vec<serde_json::Value> = (0..profile_count)
            .map(|i| serde_json::json!({"profile_name": format!("p{i}"), "rules": rules}))
            .collect();
        let json = serde_json::to_vec(&serde_json::json!({"kind": "all", "profiles": profiles})).unwrap();

        let err = check_import_size_limits(&json).expect_err("合計ルール数上限超過は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::ImportTooLarge(_)));
    }

    #[test]
    fn check_import_size_limits_allows_reasonably_sized_imports() {
        let rules: Vec<serde_json::Value> = (0..10).map(|_| serde_json::json!({})).collect();
        let json = serde_json::to_vec(&serde_json::json!({"kind": "single", "profile": {"rules": rules}})).unwrap();

        check_import_size_limits(&json).expect("通常規模のインポートは許可されるはず");
    }

    // kind:"single"のペイロードに空の"profiles":[]を混ぜても、実際に使われる
    // "profile"(単数)側のルール数チェックがすり抜けられないことを固定する回帰テスト
    // (実際のExportPayloadデシリアライズは"kind"タグでのみ判別し、余分な"profiles"
    // キーは無視するため)。
    #[test]
    fn check_import_size_limits_is_not_fooled_by_a_decoy_profiles_key_on_a_single_payload() {
        let rules: Vec<serde_json::Value> =
            (0..(MAX_RULES_PER_PROFILE + 1)).map(|_| serde_json::json!({})).collect();
        let json = serde_json::to_vec(&serde_json::json!({
            "kind": "single",
            "profile": {"rules": rules},
            "profiles": [],
        }))
        .unwrap();

        let err = check_import_size_limits(&json)
            .expect_err("kind:singleでは\"profiles\"デコイに惑わされず\"profile\"側を見るはず");
        assert!(matches!(err, ProfileStoreError::ImportTooLarge(_)));
    }

    #[test]
    fn preview_import_rejects_a_profile_with_too_many_rules_before_reaching_the_type_checked_deserialize() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        let payload = bulk::ExportPayload::Single {
            format_version: bulk::CURRENT_FORMAT_VERSION,
            profile: bulk::ExportedProfile {
                is_favorite: false,
                tags: Vec::new(),
                profile: profile_with_n_rules("big", MAX_RULES_PER_PROFILE + 1),
            },
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let encrypted = export::encrypt_for_export(&json, passphrase("pw")).unwrap();

        let err =
            store.preview_import(&encrypted, passphrase("pw")).expect_err("ルール数上限超過は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::ImportTooLarge(_)));
    }

    #[test]
    fn importing_a_file_with_an_unsupported_format_version_is_rejected() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        let payload = bulk::ExportPayload::Single {
            format_version: bulk::CURRENT_FORMAT_VERSION + 1,
            profile: bulk::ExportedProfile { is_favorite: false, tags: Vec::new(), profile: sample_profile("work") },
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let encrypted = export::encrypt_for_export(&json, passphrase("pw")).unwrap();

        let err = store.preview_import(&encrypted, passphrase("pw")).expect_err("未対応バージョンは拒否されるはず");
        assert!(matches!(
            err,
            ProfileStoreError::UnsupportedFormatVersion { found, supported }
                if found == bulk::CURRENT_FORMAT_VERSION + 1 && supported == bulk::CURRENT_FORMAT_VERSION
        ));
    }

    #[test]
    fn importing_a_file_with_duplicate_profile_names_is_rejected_entirely() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        let payload = bulk::ExportPayload::All {
            format_version: bulk::CURRENT_FORMAT_VERSION,
            active_profile_name: None,
            profiles: vec![
                bulk::ExportedProfile { is_favorite: false, tags: Vec::new(), profile: sample_profile("work") },
                bulk::ExportedProfile { is_favorite: false, tags: Vec::new(), profile: sample_profile("work") },
            ],
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let encrypted = export::encrypt_for_export(&json, passphrase("pw")).unwrap();

        let err = store
            .preview_import(&encrypted, passphrase("pw"))
            .expect_err("ファイル内の名前重複は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::DuplicateNameInImportFile(name) if name == "work"));
        assert_eq!(store.list_profiles().unwrap().len(), 0, "拒否時は何も作成されないはず");
    }

    #[test]
    fn a_file_with_both_an_unsupported_version_and_duplicate_names_reports_the_version_problem_first() {
        // フォーマットバージョンの確認は、ファイル内の名前重複チェックより前に行われるべき
        // (両方に問題がある場合、より根本的な問題であるバージョン不一致を優先して案内する)。
        // この優先順位が将来のリファクタリングで入れ替わらないことを固定するための回帰テスト。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();

        let payload = bulk::ExportPayload::All {
            format_version: bulk::CURRENT_FORMAT_VERSION + 1,
            active_profile_name: None,
            profiles: vec![
                bulk::ExportedProfile { is_favorite: false, tags: Vec::new(), profile: sample_profile("work") },
                bulk::ExportedProfile { is_favorite: false, tags: Vec::new(), profile: sample_profile("work") },
            ],
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let encrypted = export::encrypt_for_export(&json, passphrase("pw")).unwrap();

        let err = store
            .preview_import(&encrypted, passphrase("pw"))
            .expect_err("いずれかの理由で拒否されるはず");
        assert!(
            matches!(err, ProfileStoreError::UnsupportedFormatVersion { .. }),
            "バージョン不一致が名前重複より先に報告されるはず: {err:?}"
        );
    }

    #[test]
    fn updating_a_profile_replaces_its_rules() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();

        let rule_a = Rule::new("a", PatternType::Regex, r"\d+", Mode::Sequential, None, Some("A_".to_string()), true, None).unwrap();
        let rule_b = Rule::new("b", PatternType::Regex, r"\w+", Mode::Sequential, None, Some("B_".to_string()), true, None).unwrap();
        let updated = RuleProfile::new("work", None, vec![rule_a, rule_b]).unwrap();
        store.update_profile("work", &updated).unwrap();

        let loaded = store.get_profile("work").unwrap();
        assert_eq!(loaded.rules().len(), 2);
    }

    #[test]
    fn updating_a_profile_can_rename_it() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("old-name")).unwrap();

        store.update_profile("old-name", &sample_profile("new-name")).unwrap();

        assert!(store.get_profile("old-name").is_err(), "旧名ではもう取得できないはず");
        assert_eq!(store.get_profile("new-name").unwrap().profile_name(), "new-name");
    }

    #[test]
    fn renaming_the_active_profile_updates_the_active_profile_name_too() {
        // settings.active_profile_nameはON UPDATE CASCADEで追従するはず。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("old-name")).unwrap();
        assert_eq!(store.active_profile().unwrap().unwrap().profile_name(), "old-name");

        store.update_profile("old-name", &sample_profile("new-name")).unwrap();

        assert_eq!(store.active_profile().unwrap().unwrap().profile_name(), "new-name");
    }

    #[test]
    fn renaming_a_profile_preserves_its_favorite_and_tags() {
        // profile_tagsはprofile_id(不変のサロゲートキー)経由の紐付けのため、
        // name変更の影響を受けないはず。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("old-name")).unwrap();
        store.set_favorite("old-name", true).unwrap();
        store.set_profile_tags("old-name", &["sip".to_string()]).unwrap();

        store.update_profile("old-name", &sample_profile("new-name")).unwrap();

        let summary = store.list_profiles().unwrap().into_iter().find(|s| s.name == "new-name").unwrap();
        assert!(summary.is_favorite);
        assert_eq!(summary.tags, vec!["sip".to_string()]);
    }

    #[test]
    fn updating_a_missing_profile_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.update_profile("nope", &sample_profile("nope")).expect_err("存在しないプロファイルの更新はエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileNotFound(name) if name == "nope"));
    }

    #[test]
    fn renaming_to_an_already_existing_name_fails_and_does_not_modify_either_profile() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("first")).unwrap();
        store.create_profile(&sample_profile("second")).unwrap();

        let err = store.update_profile("first", &sample_profile("second")).expect_err("既存名への変更はエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileAlreadyExists(name) if name == "second"));

        assert!(store.get_profile("first").is_ok(), "失敗時は元のプロファイルが残っているはず");
        assert!(store.get_profile("second").is_ok());
    }

    #[test]
    fn set_favorite_toggles_the_flag() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();

        store.set_favorite("work", true).unwrap();
        assert!(store.list_profiles().unwrap()[0].is_favorite);

        store.set_favorite("work", false).unwrap();
        assert!(!store.list_profiles().unwrap()[0].is_favorite);
    }

    #[test]
    fn set_favorite_on_a_missing_profile_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.set_favorite("nope", true).expect_err("存在しないプロファイルはエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileNotFound(name) if name == "nope"));
    }

    #[test]
    fn create_tag_adds_it_to_list_tags() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        store.create_tag("sip").unwrap();

        assert_eq!(store.list_tags().unwrap(), vec!["sip".to_string()]);
    }

    #[test]
    fn creating_a_duplicate_tag_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_tag("sip").unwrap();

        let err = store.create_tag("sip").expect_err("重複作成はエラーのはず");
        assert!(matches!(err, ProfileStoreError::TagAlreadyExists(name) if name == "sip"));
    }

    #[test]
    fn rename_tag_renames_it() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_tag("sip").unwrap();

        store.rename_tag("sip", "voip").unwrap();

        assert_eq!(store.list_tags().unwrap(), vec!["voip".to_string()]);
    }

    #[test]
    fn create_tag_with_a_bidi_override_character_is_rejected() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.create_tag("sip\u{202E}").expect_err("双方向書式文字を含むタグ名は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::InvalidTagName(_)));
        assert!(store.list_tags().unwrap().is_empty());
    }

    #[test]
    fn rename_tag_to_an_invalid_name_is_rejected() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_tag("sip").unwrap();

        let too_long = "a".repeat(masking_core::MAX_DISPLAY_NAME_LENGTH + 1);
        let err = store.rename_tag("sip", &too_long).expect_err("長さ上限超過は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::InvalidTagName(_)));
        assert_eq!(store.list_tags().unwrap(), vec!["sip".to_string()], "リネーム失敗時は元の名前のままのはず");
    }

    #[test]
    fn set_profile_tags_with_an_invalid_tag_name_is_rejected() {
        // インポート由来のタグ(attach_tags経由)も同じ検証を通ることの間接的な確認
        // (attach_tagsはprivateなため、公開APIのset_profile_tags経由で検証する)。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();

        let err = store
            .set_profile_tags("work", &["sip".to_string(), "sip\u{202E}".to_string()])
            .expect_err("不正なタグ名を含む場合は拒否されるはず");
        assert!(matches!(err, ProfileStoreError::InvalidTagName(_)));
        assert!(store.list_tags().unwrap().is_empty(), "1件でも不正ならトランザクション全体がロールバックするはず");
    }

    #[test]
    fn renaming_a_missing_tag_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.rename_tag("nope", "voip").expect_err("存在しないタグはエラーのはず");
        assert!(matches!(err, ProfileStoreError::TagNotFound(name) if name == "nope"));
    }

    #[test]
    fn renaming_a_tag_to_an_already_existing_name_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_tag("sip").unwrap();
        store.create_tag("voip").unwrap();

        let err = store.rename_tag("sip", "voip").expect_err("既存名への変更はエラーのはず");
        assert!(matches!(err, ProfileStoreError::TagAlreadyExists(name) if name == "voip"));
    }

    #[test]
    fn delete_tag_removes_it_and_detaches_it_from_profiles() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        store.set_profile_tags("work", &["sip".to_string()]).unwrap();

        store.delete_tag("sip").unwrap();

        assert_eq!(store.list_tags().unwrap(), Vec::<String>::new());
        assert_eq!(store.profile_tags("work").unwrap(), Vec::<String>::new(), "ON DELETE CASCADEで自動的に外れるはず");
    }

    #[test]
    fn deleting_a_missing_tag_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.delete_tag("nope").expect_err("存在しないタグはエラーのはず");
        assert!(matches!(err, ProfileStoreError::TagNotFound(name) if name == "nope"));
    }

    #[test]
    fn set_profile_tags_replaces_the_full_set() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        store.set_profile_tags("work", &["sip".to_string(), "urgent".to_string()]).unwrap();

        store.set_profile_tags("work", &["sip".to_string()]).unwrap();

        assert_eq!(store.profile_tags("work").unwrap(), vec!["sip".to_string()], "urgentは外れているはず");
    }

    #[test]
    fn set_profile_tags_auto_creates_missing_tags() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();

        store.set_profile_tags("work", &["brand-new".to_string()]).unwrap();

        assert_eq!(store.list_tags().unwrap(), vec!["brand-new".to_string()]);
    }

    #[test]
    fn set_profile_tags_on_a_missing_profile_fails() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let err = store.set_profile_tags("nope", &["sip".to_string()]).expect_err("存在しないプロファイルはエラーのはず");
        assert!(matches!(err, ProfileStoreError::ProfileNotFound(name) if name == "nope"));
    }

    #[test]
    fn profile_tags_returns_the_attached_tags_sorted() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        store.set_profile_tags("work", &["voip".to_string(), "sip".to_string()]).unwrap();

        assert_eq!(store.profile_tags("work").unwrap(), vec!["sip".to_string(), "voip".to_string()]);
    }

    #[test]
    fn list_profiles_includes_tags_in_the_summary() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        store.set_profile_tags("work", &["sip".to_string()]).unwrap();

        let summary = store.list_profiles().unwrap().into_iter().find(|s| s.name == "work").unwrap();
        assert_eq!(summary.tags, vec!["sip".to_string()]);
    }

    #[test]
    fn create_profile_returns_an_id_that_matches_list_profiles() {
        // renameを跨いで安定した識別子として使うため、create_profileの戻り値と
        // list_profilesが返すidが同一であることを固定する。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();

        let id = store.create_profile(&sample_profile("work")).unwrap();

        let summary = store.list_profiles().unwrap().into_iter().find(|s| s.name == "work").unwrap();
        assert_eq!(summary.id, id);
    }

    #[test]
    fn a_profiles_id_survives_being_renamed() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        let id = store.create_profile(&sample_profile("old-name")).unwrap();

        store.update_profile("old-name", &sample_profile("new-name")).unwrap();

        let summary = store.list_profiles().unwrap().into_iter().find(|s| s.name == "new-name").unwrap();
        assert_eq!(summary.id, id, "リネームしてもidは変わらないはず");
    }

    #[test]
    fn renaming_a_tag_to_its_own_current_name_is_a_harmless_no_op() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_tag("sip").unwrap();

        store.rename_tag("sip", "sip").unwrap();

        assert_eq!(store.list_tags().unwrap(), vec!["sip".to_string()]);
    }

    #[test]
    fn set_profile_tags_with_an_empty_list_clears_all_tags() {
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        store.set_profile_tags("work", &["sip".to_string()]).unwrap();

        store.set_profile_tags("work", &[]).unwrap();

        assert_eq!(store.profile_tags("work").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn lock_contention_is_not_mistaken_for_missing_on_the_new_mutating_methods() {
        // database_lock_contention_is_not_mistaken_for_a_missing_profileと同じ懸念
        // (ロック競合による失敗が「存在しない」系のエラーに化けないこと)を、
        // 今回追加した書き込み系メソッド全てについて確認する。
        let (_dir, paths) = temp_paths();
        init_at(&paths).unwrap();
        let mut store = ProfileStore::open_at(&paths).unwrap();
        store.create_profile(&sample_profile("work")).unwrap();
        store.create_tag("sip").unwrap();

        let locker = Connection::open(&paths.db_path).unwrap();
        locker.execute_batch("BEGIN EXCLUSIVE").unwrap();

        assert!(
            matches!(store.update_profile("work", &sample_profile("work")), Err(ProfileStoreError::Db(_))),
            "update_profileはロック競合中もProfileNotFoundに化けてはいけない"
        );
        assert!(
            matches!(store.set_favorite("work", true), Err(ProfileStoreError::Db(_))),
            "set_favoriteはロック競合中もProfileNotFoundに化けてはいけない"
        );
        assert!(
            matches!(store.rename_tag("sip", "voip"), Err(ProfileStoreError::Db(_))),
            "rename_tagはロック競合中もTagNotFoundに化けてはいけない"
        );
        assert!(
            matches!(store.delete_tag("sip"), Err(ProfileStoreError::Db(_))),
            "delete_tagはロック競合中もTagNotFoundに化けてはいけない"
        );
        assert!(
            matches!(store.profile_tags("work"), Err(ProfileStoreError::Db(_))),
            "profile_tagsはロック競合中もProfileNotFoundに化けてはいけない"
        );
        assert!(
            matches!(store.set_profile_tags("work", &["sip".to_string()]), Err(ProfileStoreError::Db(_))),
            "set_profile_tagsはロック競合中もProfileNotFoundに化けてはいけない"
        );

        locker.execute_batch("COMMIT").unwrap();
        store.update_profile("work", &sample_profile("work")).expect("ロック解放後は正常に動作するはず");
    }
}
