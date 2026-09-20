//! 復号鍵の生成・読み書きと、OSファイル権限による保護(所有ユーザーのみに制限)。
//!
//! 鍵は`SecretBox`で保持する。生の`[u8; 32]`のままだと、これを保持する側の構造体を
//! 誤って`{:?}`(Debug)やログ出力に渡した際に鍵が丸ごと露出してしまうため。

use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;

use rand::RngExt;
use secrecy::SecretBox;
use zeroize::Zeroize;

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
    // 先にヒープを確保してから乱数を直接書き込むことで、生の鍵バイト列が
    // (zeroizeできない)スタック上のコピーとして残らないようにする。
    let mut key = Box::new([0u8; KEY_LEN]);
    rand::rng().fill(key.as_mut());
    if let Err(err) = save_key(key_path, &key) {
        // 権限制限に失敗した鍵ファイルを弱い権限のまま残さない(次回起動時に「初期化済み」と
        // 誤判定され、検出・修復の機会が無いまま使われ続けてしまうため)。
        let _ = fs::remove_file(key_path);
        key.as_mut().zeroize();
        return Err(err);
    }
    Ok(SecretBox::new(key))
}

fn save_key(key_path: &Path, key: &[u8; KEY_LEN]) -> Result<(), KeyError> {
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)?;
    }
    create_restricted_file(key_path, key)?;
    Ok(())
}

pub fn load_key(key_path: &Path) -> Result<SecretBox<[u8; KEY_LEN]>, KeyError> {
    // fs::read()でVec<u8>へ読み込んでから[u8; KEY_LEN]に変換すると、Vecの
    // capacityがKEY_LENと一致しない場合にshrink相当のreallocが発生し、
    // 鍵バイト列を保持していた旧バッファがzeroizeされずに残ってしまう。
    // ファイルサイズを事前に確認した上でBox<[u8; KEY_LEN]>へ直接read_exactし、
    // 鍵バイト列がSecretBoxの管理するメモリ以外に一切コピーされないようにする。
    let mut file = fs::File::open(key_path)?;
    let actual = usize::try_from(file.metadata()?.len()).unwrap_or(usize::MAX);
    if actual != KEY_LEN {
        return Err(KeyError::InvalidLength { actual });
    }
    let mut key = Box::new([0u8; KEY_LEN]);
    if let Err(err) = file.read_exact(key.as_mut()) {
        key.as_mut().zeroize();
        return Err(err.into());
    }
    Ok(SecretBox::new(key))
}

/// 書き出したファイルの権限の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileProtection {
    /// 所有ユーザーだけが読み書きできる。
    OwnerOnly,
    /// 権限を制限できなかった(FAT/exFATのUSBメモリなど、ファイルの権限を持たないファイルシステムの場合がある)。
    NotRestricted,
}

/// 利用者が指定した場所へ、秘密を含むファイル(エクスポートの鍵ファイルなど)を書き、所有ユーザーだけの権限にする。
/// 権限を制限できなくても、書き込み自体は成功として、その旨を返す(保管先が、権限を持たないファイルシステムの場合が
/// ありうるため。呼び出し側が、利用者へ知らせる)。既に有るファイルは、内容を置き換え、権限も制限し直す。
pub fn write_owner_only_file(path: &Path, contents: &[u8]) -> Result<FileProtection, KeyError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file =
            fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        // 既に有るファイルには、作成時のモードが効かないため、明示する(内容を書く前に、権限を絞る)。
        let restricted = file.set_permissions(fs::Permissions::from_mode(0o600)).is_ok();
        file.write_all(contents)?;
        Ok(if restricted { FileProtection::OwnerOnly } else { FileProtection::NotRestricted })
    }
    #[cfg(windows)]
    {
        fs::write(path, contents)?;
        Ok(if restrict_to_current_user(path).is_ok() {
            FileProtection::OwnerOnly
        } else {
            FileProtection::NotRestricted
        })
    }
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

// PATH解決に依存すると、PATH上のicaclsより前にある別の(悪意ある、または単に別の)
// 同名実行ファイルが誤って実行されうる。SystemRoot(Windowsが必ず設定する環境変数)
// から実体の絶対パスを組み立てる。取得できない場合はfail-safe defaultsの方針に従い、
// 誤ったパスを推測で使わずエラーにする。
#[cfg(windows)]
fn icacls_path() -> Result<std::path::PathBuf, KeyError> {
    let system_root = std::env::var("SystemRoot")
        .map_err(|_| KeyError::Permission("SystemRoot環境変数を取得できません".to_string()))?;
    Ok(icacls_path_from_system_root(&system_root))
}

// 実際の環境変数読み取りとパス構築ロジックを分離し、後者だけを引数渡しでテストできる
// ようにする(resolve_paths_with_overrideと同じ方針。std::env::set_varはRust 2024で
// unsafe化されておりテスト間で競合しうるため、テストで直接環境変数を書き換えない)。
#[cfg(windows)]
fn icacls_path_from_system_root(system_root: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(system_root).join("System32").join("icacls.exe")
}

/// Windowsはicacls(既存ファイル向け)を使うため、書き込み→権限制限の間に短い窓が生じる。
#[cfg(windows)]
fn create_restricted_file(path: &Path, key: &[u8; KEY_LEN]) -> Result<(), KeyError> {
    fs::write(path, key)?;
    restrict_to_current_user(path)
}

/// 既存のファイルを、現在のユーザーだけがフルコントロールを持つ状態にする。
/// - /reset: 継承のACLへ戻す。親のフォルダが継承できるエントリを持たないと(管理者権限のプロセスに限らない)、
///   作られたファイルは、既定のDACLとして、SYSTEMなどの明示のエントリを持ち、それは、/inheritance:rでは消えないため
/// - /inheritance:r: 継承のエントリ(SYSTEM/Administrators等)を除去する
/// - /grant:r: 現在のユーザーのみにフルコントロールを与える
#[cfg(windows)]
fn restrict_to_current_user(path: &Path) -> Result<(), KeyError> {
    use std::process::Command;

    let username = std::env::var("USERNAME")
        .map_err(|_| KeyError::Permission("USERNAME環境変数を取得できません".to_string()))?;
    let icacls = icacls_path()?;
    let grant = format!("{username}:F");

    for args in [&["/reset"][..], &["/inheritance:r"][..], &["/grant:r", grant.as_str()][..]] {
        let output = Command::new(&icacls).arg(path).args(args).output()?;
        if !output.status.success() {
            return Err(KeyError::Permission(format!(
                "icacls {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
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

        let output = Command::new(icacls_path().unwrap()).arg(&key_path).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let username = std::env::var("USERNAME").unwrap();

        assert!(stdout.contains(&username), "現在のユーザーへの許可が無い: {stdout}");
        assert!(!stdout.contains("SYSTEM"), "SYSTEMへの継承が残っている: {stdout}");
        assert!(!stdout.contains("Administrators"), "Administratorsへの継承が残っている: {stdout}");
    }

    #[test]
    #[cfg(windows)]
    fn restricting_a_file_also_removes_explicit_entries_for_system_and_administrators() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("key.bin");
        std::fs::write(&key_path, b"x").unwrap();

        // 親のフォルダが継承できるエントリを持たないと、作られたファイルは、既定のDACLとして、SYSTEMなどの
        // 明示のエントリを持つ(継承のエントリではない)。同じ状態を、SIDで指定して作る
        // (*S-1-5-18はSYSTEM、*S-1-5-32-544はAdministrators)。
        for sid in ["*S-1-5-18:F", "*S-1-5-32-544:F"] {
            let output = Command::new(icacls_path().unwrap()).arg(&key_path).arg("/grant").arg(sid).output().unwrap();
            assert!(output.status.success(), "明示のエントリを足せなかった: {}", String::from_utf8_lossy(&output.stderr));
        }

        restrict_to_current_user(&key_path).unwrap();

        let output = Command::new(icacls_path().unwrap()).arg(&key_path).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let username = std::env::var("USERNAME").unwrap();
        assert!(stdout.contains(&username), "現在のユーザーへの許可が無い: {stdout}");
        assert!(!stdout.contains("SYSTEM"), "SYSTEMの明示のエントリが残っている: {stdout}");
        assert!(!stdout.contains("Administrators"), "Administratorsの明示のエントリが残っている: {stdout}");
    }

    #[test]
    fn write_owner_only_file_writes_the_contents_and_replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.smxkey");
        std::fs::write(&path, b"old contents that are longer than the new ones").unwrap();

        let protection = write_owner_only_file(&path, b"new").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(protection, FileProtection::OwnerOnly);
    }

    #[test]
    #[cfg(unix)]
    fn write_owner_only_file_restricts_an_existing_permissive_file_to_the_owner_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.smxkey");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_owner_only_file(&path, b"new").unwrap();

        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    #[cfg(windows)]
    fn write_owner_only_file_grants_only_the_current_user_on_windows() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.smxkey");

        write_owner_only_file(&path, b"secret").unwrap();

        let output = Command::new(icacls_path().unwrap()).arg(&path).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(&std::env::var("USERNAME").unwrap()), "現在のユーザーへの許可が無い: {stdout}");
        assert!(!stdout.contains("SYSTEM"), "SYSTEMへの許可が残っている: {stdout}");
        assert!(!stdout.contains("Administrators"), "Administratorsへの許可が残っている: {stdout}");
    }

    #[test]
    #[cfg(windows)]
    fn icacls_path_is_built_under_system32_of_the_given_system_root() {
        let path = icacls_path_from_system_root(r"C:\Windows");
        assert_eq!(path, std::path::PathBuf::from(r"C:\Windows\System32\icacls.exe"));
    }
}
