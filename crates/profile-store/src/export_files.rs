//! 鍵ファイル方式のエクスポートの、2つのファイル(エクスポートしたファイルと鍵ファイル)の書き出し。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use secrecy::{ExposeSecret, SecretString};

use crate::key::{self, FileProtection, KeyError};
use crate::ProfileStoreError;

/// 鍵ファイル方式のエクスポートの、2つのファイルを書く。
///
/// どちらも、保存先と同じフォルダの一時の名前へ、完全に書いてから、両方が書けた後に、本来の名前へ置き換える。片方の書き込みに
/// 失敗しても、保存先に既に有ったファイル(利用者が、置き換えを選んだ、前のエクスポートの鍵ファイルなど)は、壊さない。書き込みを
/// 始める前に、置き換えられない保存先(フォルダ・読み取り専用のファイル)を、拒否する。鍵ファイルは、所有ユーザーだけの権限で
/// 書く(制限できない保管先では、書き込みは成功として、その旨を返す)。
pub fn write_key_file_export(
    data_dest: &Path,
    ciphertext: &[u8],
    key_dest: &Path,
    key_contents: &SecretString,
) -> Result<FileProtection, ProfileStoreError> {
    write_key_file_export_with(data_dest, ciphertext, key_dest, key_contents, |from, to| fs::rename(from, to))
}

// 名前の変更(rename)を、差し替えられる形(テストが、置き換えの失敗の後始末を、再現するため)。
fn write_key_file_export_with(
    data_dest: &Path,
    ciphertext: &[u8],
    key_dest: &Path,
    key_contents: &SecretString,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<FileProtection, ProfileStoreError> {
    ensure_replaceable(data_dest)?;
    ensure_replaceable(key_dest)?;
    let data_tmp = temp_sibling(data_dest)?;
    let key_tmp = temp_sibling(key_dest)?;

    let protection = match write_temporary_files(&data_tmp, ciphertext, &key_tmp, key_contents) {
        Ok(protection) => protection,
        Err(error) => {
            remove_best_effort(&data_tmp);
            remove_best_effort(&key_tmp);
            return Err(error);
        }
    };

    // 鍵ファイル、エクスポートしたファイルの順に置き換える。置き換えは、同じフォルダの中で、書いたばかりのファイルの名前の
    // 変更のため、事前の検査(ensure_replaceable)の後は、失敗しにくい。
    if let Err(error) = rename(&key_tmp, key_dest) {
        remove_best_effort(&data_tmp);
        remove_best_effort(&key_tmp);
        return Err(KeyError::from(error).into());
    }
    if let Err(error) = rename(&data_tmp, data_dest) {
        remove_best_effort(&data_tmp);
        return Err(KeyError::from(error).into());
    }
    Ok(protection)
}

// 一時の名前へ、2つのファイルを書く(鍵ファイルは、内容を書く前に権限を制限する。どちらも、保管先へ確定させる)。
fn write_temporary_files(
    data_tmp: &Path,
    ciphertext: &[u8],
    key_tmp: &Path,
    key_contents: &SecretString,
) -> Result<FileProtection, ProfileStoreError> {
    let protection = key::write_owner_only_file(key_tmp, key_contents.expose_secret().as_bytes())?;
    write_new_file_synced(data_tmp, ciphertext).map_err(KeyError::from)?;
    Ok(protection)
}

fn write_new_file_synced(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

// 既に有る保存先が、置き換えられるもの(フォルダでも、読み取り専用のファイルでもない)であることを確かめる。
// 置き換えられない場合に、もう一方のファイルだけを置き換えてしまわないよう、書き込みの前に拒否する。
fn ensure_replaceable(dest: &Path) -> Result<(), ProfileStoreError> {
    let Ok(metadata) = fs::metadata(dest) else {
        return Ok(());
    };
    if metadata.is_dir() {
        return Err(KeyError::from(std::io::Error::other("保存先がフォルダです")).into());
    }
    if metadata.permissions().readonly() {
        return Err(KeyError::from(std::io::Error::other("保存先が読み取り専用です")).into());
    }
    Ok(())
}

// 保存先と同じフォルダの、まだ無い名前(名前の変更を、同じフォルダの中で行うため)。
fn temp_sibling(dest: &Path) -> Result<PathBuf, ProfileStoreError> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let file_name = dest
        .file_name()
        .ok_or_else(|| KeyError::from(std::io::Error::other("保存先にファイル名がありません")))?
        .to_string_lossy();
    let unique = format!("{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed));
    Ok(dest.with_file_name(format!(".{file_name}.{unique}.tmp")))
}

fn remove_best_effort(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_CONTENTS: &str = "# key file\nAGE-SECRET-KEY-1DUMMYDUMMYDUMMYDUMMYDUMMYDUMMYDUMMYDUMMYDUMMYDUMMYDUMMYDUMMYDUMMY\n";

    fn key_contents() -> SecretString {
        SecretString::from(KEY_CONTENTS.to_string())
    }

    // 保存先のフォルダにあるファイルの名前(一時のファイルが、残っていないことの確認に使う)。
    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn both_files_are_written_and_no_temporary_file_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");

        let protection = write_key_file_export(&data, b"ciphertext", &key, &key_contents()).unwrap();

        assert_eq!(fs::read(&data).unwrap(), b"ciphertext");
        assert_eq!(fs::read_to_string(&key).unwrap(), KEY_CONTENTS);
        assert_eq!(protection, FileProtection::OwnerOnly);
        assert_eq!(file_names(dir.path()), vec!["export.smx".to_string(), "export.smxkey".to_string()]);
    }

    #[test]
    fn existing_files_are_replaced_when_both_are_written() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        fs::write(&data, b"old data that is longer than the new one").unwrap();
        fs::write(&key, b"old key that is longer than the new one").unwrap();

        write_key_file_export(&data, b"new", &key, &key_contents()).unwrap();

        assert_eq!(fs::read(&data).unwrap(), b"new");
        assert_eq!(fs::read_to_string(&key).unwrap(), KEY_CONTENTS);
        assert_eq!(file_names(dir.path()).len(), 2, "一時のファイルが残っている");
    }

    // エクスポートしたファイルを置き換えられなければ、置き換えを選んだ、前のエクスポートの鍵ファイルを、壊してはならない
    // (前のエクスポートしたファイルを、別の名前で残していると、二度と復号できなくなる)。
    #[test]
    fn an_existing_key_file_is_untouched_when_the_export_file_cannot_be_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        fs::create_dir(&data).unwrap();
        fs::write(&key, b"previous key").unwrap();

        let result = write_key_file_export(&data, b"ciphertext", &key, &key_contents());

        assert!(result.is_err(), "フォルダは、エクスポートしたファイルの保存先にできない");
        assert_eq!(fs::read(&key).unwrap(), b"previous key", "既存の鍵ファイルが、壊された");
        assert_eq!(file_names(dir.path()), vec!["export.smx".to_string(), "export.smxkey".to_string()]);
    }

    #[test]
    fn an_existing_export_file_is_untouched_when_the_key_file_cannot_be_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        fs::write(&data, b"previous export").unwrap();
        fs::create_dir(&key).unwrap();

        let result = write_key_file_export(&data, b"ciphertext", &key, &key_contents());

        assert!(result.is_err());
        assert_eq!(fs::read(&data).unwrap(), b"previous export", "既存のエクスポートしたファイルが、壊された");
        assert_eq!(file_names(dir.path()), vec!["export.smx".to_string(), "export.smxkey".to_string()]);
    }

    #[test]
    fn a_read_only_destination_is_rejected_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        fs::write(&data, b"previous export").unwrap();
        let mut permissions = fs::metadata(&data).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&data, permissions).unwrap();

        let result = write_key_file_export(&data, b"ciphertext", &key, &key_contents());

        assert!(result.is_err(), "読み取り専用のファイルは、置き換えられない");
        assert!(!key.exists(), "鍵ファイルだけが、書き出された");
        assert_eq!(file_names(dir.path()), vec!["export.smx".to_string()]);

        // 後片付け(読み取り専用のファイルは、一時フォルダの削除で消せないことがある)。
        let mut permissions = fs::metadata(&data).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&data, permissions).unwrap();
    }

    // 鍵ファイルの置き換えに失敗したら、何も変えず(既存の2つのファイルは、そのまま)、一時のファイルも残さない。
    #[test]
    fn a_failed_key_file_replacement_changes_nothing_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        fs::write(&data, b"previous export").unwrap();
        fs::write(&key, b"previous key").unwrap();

        let result = write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), |from, to| {
            if to == key { Err(std::io::Error::other("test")) } else { fs::rename(from, to) }
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&data).unwrap(), b"previous export");
        assert_eq!(fs::read(&key).unwrap(), b"previous key");
        assert_eq!(file_names(dir.path()), vec!["export.smx".to_string(), "export.smxkey".to_string()]);
    }

    // エクスポートしたファイルの置き換えに失敗したら、失敗を返し(成功と報告しない)、一時のファイルを残さない。
    #[test]
    fn a_failed_export_file_replacement_is_reported_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        fs::write(&data, b"previous export").unwrap();

        let result = write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), |from, to| {
            if to == data { Err(std::io::Error::other("test")) } else { fs::rename(from, to) }
        });

        assert!(result.is_err(), "エクスポートしたファイルを置き換えられなかったのに、成功と報告した");
        assert_eq!(fs::read(&data).unwrap(), b"previous export");
        assert_eq!(file_names(dir.path()), vec!["export.smx".to_string(), "export.smxkey".to_string()]);
    }

    // 保存先のフォルダが無いなど、一時のファイルを書けなければ、何も残さない。
    #[test]
    fn nothing_is_written_when_the_destination_folder_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("missing-folder").join("export.smx");
        let key = dir.path().join("export.smxkey");

        let result = write_key_file_export(&data, b"ciphertext", &key, &key_contents());

        assert!(result.is_err());
        assert!(file_names(dir.path()).is_empty(), "何かが、残っている: {:?}", file_names(dir.path()));
    }

    #[test]
    fn temporary_names_are_in_the_same_folder_and_never_repeat() {
        let dest = Path::new("some-folder").join("export.smx");

        let first = temp_sibling(&dest).unwrap();
        let second = temp_sibling(&dest).unwrap();

        assert_eq!(first.parent(), dest.parent(), "名前の変更を、同じフォルダの中で行うため");
        assert_ne!(first, second);
        assert_ne!(first, dest);
    }
}
