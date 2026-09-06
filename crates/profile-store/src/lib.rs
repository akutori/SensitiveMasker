//! プロファイルの永続化・暗号化・鍵管理(Imperative Shell)。masking-coreのRule/RuleProfileを
//! SQLite+対称暗号化で保存する。GUI/CLI/MCPはこのcrate経由でのみプロファイルを読み書きする。

mod crypto;
mod db;
mod key;
mod paths;

use std::path::PathBuf;

use masking_core::RuleProfile;
use rusqlite::Connection;
use secrecy::SecretBox;

pub use paths::{AppPaths, PathError};

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
    #[error("プロファイル '{0}' は既に存在します")]
    ProfileAlreadyExists(String),
    #[error("プロファイル '{0}' が見つかりません")]
    ProfileNotFound(String),
    #[error("アクティブなプロファイルは削除できません")]
    CannotDeleteActiveProfile,
    #[error("保存されているプロファイルのデータが不正です: {0}")]
    CorruptProfileData(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSummary {
    pub name: String,
    pub rule_count: usize,
    pub is_favorite: bool,
    pub is_active: bool,
    pub updated_at: String,
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

    pub fn create_profile(&mut self, profile: &RuleProfile) -> Result<(), ProfileStoreError> {
        let name = profile.profile_name();
        let json = serde_json::to_vec(profile).expect("RuleProfileのシリアライズは失敗しない");
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
        let new_id = tx.last_insert_rowid();

        if read_active_profile_id(&tx)?.is_none() {
            upsert_active_profile_id(&tx, new_id)?;
        }

        tx.commit()?;
        Ok(())
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
        let active_id = read_active_profile_id(&self.conn)?;

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
                let profile = self.decrypt_profile(&ciphertext, &nonce, &name)?;
                Ok(ProfileSummary {
                    name,
                    rule_count: profile.rules().len(),
                    is_favorite,
                    is_active: Some(id) == active_id,
                    updated_at,
                })
            })
            .collect()
    }

    pub fn delete_profile(&mut self, name: &str) -> Result<(), ProfileStoreError> {
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM profiles WHERE name = ?1", [name], |row| row.get(0))
            .map_err(|e| not_found_unless_other_db_error(e, name))?;

        if read_active_profile_id(&self.conn)? == Some(id) {
            return Err(ProfileStoreError::CannotDeleteActiveProfile);
        }

        self.conn.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_active_profile(&mut self, name: &str) -> Result<(), ProfileStoreError> {
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM profiles WHERE name = ?1", [name], |row| row.get(0))
            .map_err(|e| not_found_unless_other_db_error(e, name))?;
        upsert_active_profile_id(&self.conn, id)
    }

    pub fn active_profile(&self) -> Result<Option<RuleProfile>, ProfileStoreError> {
        let active_id = match read_active_profile_id(&self.conn)? {
            Some(id) => id,
            None => return Ok(None),
        };
        let name: String =
            self.conn.query_row("SELECT name FROM profiles WHERE id = ?1", [active_id], |row| row.get(0))?;
        Ok(Some(self.get_profile(&name)?))
    }

    fn decrypt_profile(&self, ciphertext: &[u8], nonce: &[u8], name: &str) -> Result<RuleProfile, ProfileStoreError> {
        let plaintext = crypto::decrypt(&self.key, ciphertext, nonce, name.as_bytes())?;
        serde_json::from_slice(&plaintext).map_err(|e| ProfileStoreError::CorruptProfileData(e.to_string()))
    }
}

/// `settings`テーブルは「行が無い(未初期化状態)」であればアクティブ未設定として扱うが、
/// それ以外のDBエラー(ロック競合・I/O異常等)は握り潰さずそのまま伝播させる。
fn read_active_profile_id(conn: &Connection) -> Result<Option<i64>, ProfileStoreError> {
    match conn.query_row("SELECT active_profile_id FROM settings WHERE id = 1", [], |row| row.get(0)) {
        Ok(id) => Ok(id),
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

fn upsert_active_profile_id(conn: &Connection, id: i64) -> Result<(), ProfileStoreError> {
    conn.execute(
        "INSERT INTO settings (id, active_profile_id) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET active_profile_id = ?1",
        [id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use masking_core::{Mode, PatternType, Rule};
    use secrecy::ExposeSecret;

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
        assert!(alpha.is_active, "最初に作成したalphaがアクティブなはず");
        assert!(!alpha.is_favorite);
        let beta = summaries.iter().find(|s| s.name == "beta").unwrap();
        assert!(!beta.is_active);
    }
}
