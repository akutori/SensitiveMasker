//! 復号鍵の生成・読み書きと、OSファイル権限による保護(所有ユーザーのみに制限)。
//!
//! 鍵は`SecretBox`で保持する。生の`[u8; 32]`のままだと、これを保持する側の構造体を
//! 誤って`{:?}`(Debug)やログ出力に渡した際に鍵が丸ごと露出してしまうため。

use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;

use rand::RngExt;
use secrecy::SecretBox;

pub const KEY_LEN: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("鍵ファイルの読み書きに失敗しました: {0}")]
    Io(#[from] std::io::Error),
    #[error("鍵ファイルの権限を設定できませんでした: {0}")]
    Permission(String),
    #[error("鍵ファイルの内容が不正です(サイズが{actual}バイト、期待値は{KEY_LEN}バイト)")]
    InvalidLength { actual: usize },
}

pub fn generate_and_save_key(key_path: &Path) -> Result<SecretBox<[u8; KEY_LEN]>, KeyError> {
    let mut key = [0u8; KEY_LEN];
    rand::rng().fill(&mut key);
    if let Err(err) = save_key(key_path, &key) {
        // 権限制限に失敗した鍵ファイルを弱い権限のまま残さない(次回起動時に「初期化済み」と
        // 誤判定され、検出・修復の機会が無いまま使われ続けてしまうため)。
        let _ = fs::remove_file(key_path);
        return Err(err);
    }
    Ok(SecretBox::new(Box::new(key)))
}

fn save_key(key_path: &Path, key: &[u8; KEY_LEN]) -> Result<(), KeyError> {
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)?;
    }
    create_restricted_file(key_path, key)?;
    Ok(())
}

pub fn load_key(key_path: &Path) -> Result<SecretBox<[u8; KEY_LEN]>, KeyError> {
    let bytes = fs::read(key_path)?;
    let actual = bytes.len();
    let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_| KeyError::InvalidLength { actual })?;
    Ok(SecretBox::new(Box::new(key)))
}

/// Unixでは作成時点でモードを指定できるため、「書き込み後に権限を絞る」窓が生じない。
#[cfg(unix)]
fn create_restricted_file(path: &Path, key: &[u8; KEY_LEN]) -> Result<(), KeyError> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(key)?;
    Ok(())
}

/// Windowsはicacls(既存ファイル向け)を使うため、書き込み→権限制限の間に短い窓が生じる。
/// icacls /inheritance:r で継承エントリ(SYSTEM/Administrators等)を除去し、/grant:r で
/// 現在のユーザーのみにフルコントロールを与える(実機で動作確認済み)。
#[cfg(windows)]
fn create_restricted_file(path: &Path, key: &[u8; KEY_LEN]) -> Result<(), KeyError> {
    use std::process::Command;

    fs::write(path, key)?;

    let username = std::env::var("USERNAME")
        .map_err(|_| KeyError::Permission("USERNAME環境変数を取得できません".to_string()))?;

    let output = Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{username}:F"))
        .output()?;

    if !output.status.success() {
        return Err(KeyError::Permission(format!(
            "icacls failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn generated_key_can_be_saved_and_loaded_back() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("key.bin");

        let generated = generate_and_save_key(&key_path).unwrap();
        let loaded = load_key(&key_path).unwrap();

        assert_eq!(generated.expose_secret(), loaded.expose_secret());
    }

    #[test]
    fn two_generated_keys_are_different() {
        let dir = tempfile::tempdir().unwrap();
        let key_a = generate_and_save_key(&dir.path().join("a.bin")).unwrap();
        let key_b = generate_and_save_key(&dir.path().join("b.bin")).unwrap();
        assert_ne!(key_a.expose_secret(), key_b.expose_secret());
    }

    #[test]
    fn loading_a_key_of_wrong_length_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("key.bin");
        std::fs::write(&key_path, b"too short").unwrap();

        let err = load_key(&key_path).expect_err("短すぎる鍵は拒否されるはず");
        assert!(matches!(err, KeyError::InvalidLength { actual: 9 }));
    }

    #[test]
    #[cfg(unix)]
    fn saved_key_file_has_owner_only_permissions_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("key.bin");
        generate_and_save_key(&key_path).unwrap();

        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    #[cfg(windows)]
    fn saved_key_file_grants_only_current_user_on_windows() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("key.bin");
        generate_and_save_key(&key_path).unwrap();

        let output = Command::new("icacls").arg(&key_path).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let username = std::env::var("USERNAME").unwrap();

        assert!(stdout.contains(&username), "現在のユーザーへの許可が無い: {stdout}");
        assert!(!stdout.contains("SYSTEM"), "SYSTEMへの継承が残っている: {stdout}");
        assert!(!stdout.contains("Administrators"), "Administratorsへの継承が残っている: {stdout}");
    }
}
