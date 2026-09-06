//! SQLiteスキーマの作成と接続オープン。CRUDロジックは`lib.rs`の`ProfileStore`が持つ。

use std::path::Path;

use rusqlite::Connection;

pub fn open(db_path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(e.to_string()),
            )
        })?;
    }
    let conn = Connection::open(db_path)?;
    // SQLiteは接続ごとに外部キー制約の強制を明示的に有効化する必要がある(既定では無効)。
    // busy_timeoutは、他プロセス(バックアップ・ウイルススキャン等)による短時間のロックで
    // 即座に失敗させず、一定時間まで自動リトライさせるために明示的に設定する。
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 3000;")?;
    create_schema(&conn)?;
    Ok(conn)
}

fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            rules_encrypted BLOB NOT NULL,
            nonce BLOB NOT NULL,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            active_profile_id INTEGER REFERENCES profiles(id)
        );

        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS profile_tags (
            profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (profile_id, tag_id)
        );
        ",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_all_expected_tables() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("profiles.db")).unwrap();

        let mut names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        names.retain(|n| n != "sqlite_sequence");

        assert_eq!(names, vec!["profile_tags", "profiles", "settings", "tags"]);
    }

    #[test]
    fn opening_twice_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("profiles.db");
        open(&db_path).unwrap();
        open(&db_path).unwrap();
    }

    #[test]
    fn foreign_key_enforcement_is_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("profiles.db")).unwrap();

        // profile_id=999は存在しないので、外部キー制約が有効なら失敗するはず。
        let result = conn.execute(
            "INSERT INTO profile_tags (profile_id, tag_id) VALUES (999, 1)",
            [],
        );
        assert!(result.is_err(), "存在しないprofile_idの挿入は外部キー制約で拒否されるはず");
    }
}
