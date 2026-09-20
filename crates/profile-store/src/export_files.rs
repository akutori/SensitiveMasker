//! 鍵ファイル方式のエクスポートの、2つのファイル(エクスポートしたファイルと鍵ファイル)の書き出し。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};

use crate::key::{self, FileProtection, KeyError};
use crate::ProfileStoreError;

/// 鍵ファイル方式のエクスポートの、2つのファイルを書く。
///
/// どちらも、保存先と同じフォルダの一時の名前へ、完全に書いてから、両方が書けた後に、本来の名前へ置き換える。置き換えは、
/// 既に有るファイル(利用者が、置き換えを選んだ、前のエクスポートの鍵ファイルなど)を、いったん別の名前へ退避してから行い、
/// どちらかの置き換えに失敗したら、退避したファイルを戻す。そのため、途中で失敗しても、保存先に既に有ったファイルは、壊れない
/// (鍵ファイルだけが新しくなって、前のエクスポートしたファイルが、二度と復号できなくなる、ということが無い)。書き込みを始める前に、
/// 置き換えられない保存先(フォルダ・読み取り専用のファイル)を、拒否する。鍵ファイルは、所有ユーザーだけの権限で書く(制限できない
/// 保管先では、書き込みは成功として、その旨を返す)。
pub fn write_key_file_export(
    data_dest: &Path,
    ciphertext: &[u8],
    key_dest: &Path,
    key_contents: &SecretString,
) -> Result<FileProtection, ProfileStoreError> {
    write_key_file_export_with(data_dest, ciphertext, key_dest, key_contents, |from, to| fs::rename(from, to))
}

// 名前の変更(rename)を、差し替えられる形(テストが、退避・置き換え・戻しの失敗を、再現するため)。
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

    // 鍵ファイル、エクスポートしたファイルの順に置き換える。
    match replace_all(&rename, &[(&key_tmp, key_dest), (&data_tmp, data_dest)]) {
        Ok(()) => Ok(protection),
        Err(error) => {
            remove_best_effort(&data_tmp);
            remove_best_effort(&key_tmp);
            Err(error)
        }
    }
}

// 一時の名前へ、2つのファイルを書く(鍵ファイルは、内容を書く前に権限を制限する。どちらも、新しいファイルとして作り、保管先へ
// 確定させる)。
fn write_temporary_files(
    data_tmp: &Path,
    ciphertext: &[u8],
    key_tmp: &Path,
    key_contents: &SecretString,
) -> Result<FileProtection, ProfileStoreError> {
    let protection = key::write_new_owner_only_file(key_tmp, key_contents.expose_secret().as_bytes())?;
    write_new_file_synced(data_tmp, ciphertext).map_err(KeyError::from)?;
    Ok(protection)
}

fn write_new_file_synced(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

// (一時の名前, 本来の名前)の組を、全て置き換える。全て成功するか、何も変わらないか、のどちらかにする: 既に有る本来の名前の
// ファイルは、先に、別の名前へ退避し、どれかが失敗したら、置いたファイルを外して、退避したファイルを戻す。退避は、その名前の
// ファイルを、他のプロセスが開いていて(Windowsの共有違反など)置き換えられない場合を、何も変える前に、見つけるためでもある。
fn replace_all(
    rename: &impl Fn(&Path, &Path) -> std::io::Result<()>,
    replacements: &[(&Path, &Path)],
) -> Result<(), ProfileStoreError> {
    // (本来の名前, 退避した名前)
    let mut backups: Vec<(&Path, PathBuf)> = Vec::new();
    for &(_, dest) in replacements {
        if fs::symlink_metadata(dest).is_err() {
            continue;
        }
        let backup = temp_sibling(dest)?;
        if let Err(error) = rename(dest, &backup) {
            return Err(rolled_back(rename, &backups, &[], error));
        }
        backups.push((dest, backup));
    }

    let mut placed: Vec<&Path> = Vec::new();
    for &(tmp, dest) in replacements {
        if let Err(error) = rename(tmp, dest) {
            return Err(rolled_back(rename, &backups, &placed, error));
        }
        placed.push(dest);
    }

    for (_, backup) in &backups {
        remove_best_effort(backup);
    }
    Ok(())
}

// 置き換えの失敗を、元の状態へ戻して報告する: 置いた新しいファイルを外し、退避したファイルを戻す。戻せなかった退避は、
// その場所を、エラーで伝える(消さない)。
fn rolled_back(
    rename: &impl Fn(&Path, &Path) -> std::io::Result<()>,
    backups: &[(&Path, PathBuf)],
    placed: &[&Path],
    cause: std::io::Error,
) -> ProfileStoreError {
    for dest in placed {
        remove_best_effort(dest);
    }
    let not_restored: Vec<String> = backups
        .iter()
        .filter(|(dest, backup)| rename(backup, dest).is_err())
        .map(|(dest, backup)| format!("{} の前のファイルは、{} にあります", dest.display(), backup.display()))
        .collect();
    if not_restored.is_empty() {
        KeyError::from(cause).into()
    } else {
        ProfileStoreError::PreviousFilesNotRestored(format!(
            "エクスポートに失敗し({cause})、前のファイルを、元の名前へ戻せませんでした。{}",
            not_restored.join("、")
        ))
    }
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

// 保存先と同じフォルダの、まだ無い名前(名前の変更を、同じフォルダの中で行うため)。推測できない名前にする: 予測できる名前だと、
// 保存先のフォルダに書ける別のプロセスが、先に、その名前で、他のファイルへのリンクを置き、秘密が、そのファイルへ書かれうる。
fn temp_sibling(dest: &Path) -> Result<PathBuf, ProfileStoreError> {
    let file_name = dest
        .file_name()
        .ok_or_else(|| KeyError::from(std::io::Error::other("保存先にファイル名がありません")))?
        .to_string_lossy();
    let unique: u64 = rand::random();
    Ok(dest.with_file_name(format!(".{file_name}.{unique:016x}.tmp")))
}

fn remove_best_effort(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

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

    // 何回目の名前の変更が失敗するかを、指定する(番号は、1から)。置き換えが、既に有る2つのファイルに対して行う名前の変更は、
    // 順に、1: 鍵ファイルの退避、2: エクスポートしたファイルの退避、3: 鍵ファイルの置き換え、4: エクスポートしたファイルの置き換え、
    // (失敗した後の)5: 鍵ファイルを戻す、6: エクスポートしたファイルを戻す。
    fn failing_on(numbers: &'static [usize]) -> impl Fn(&Path, &Path) -> std::io::Result<()> {
        let calls = Cell::new(0);
        move |from, to| {
            calls.set(calls.get() + 1);
            if numbers.contains(&calls.get()) {
                Err(std::io::Error::other(format!("test: {}回目の名前の変更の失敗", calls.get())))
            } else {
                fs::rename(from, to)
            }
        }
    }

    // 前のエクスポートの2つのファイルを置いた保存先(フォルダ・データの保存先・鍵の保存先)。
    fn previous_export() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");
        fs::write(&data, b"previous export").unwrap();
        fs::write(&key, b"previous key").unwrap();
        (dir, data, key)
    }

    fn assert_previous_export_untouched(dir: &Path, data: &Path, key: &Path) {
        assert_eq!(fs::read(data).unwrap(), b"previous export", "前のエクスポートしたファイルが、壊された");
        assert_eq!(fs::read(key).unwrap(), b"previous key", "前の鍵ファイルが、壊された");
        assert_eq!(file_names(dir), vec!["export.smx".to_string(), "export.smxkey".to_string()], "一時のファイルが残っている");
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
        assert_eq!(file_names(dir.path()), vec!["export.smx".to_string(), "export.smxkey".to_string()], "退避したファイルが、残っている");
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

    // ---- 置き換えの途中の失敗: どの段階で失敗しても、前のエクスポートの2つのファイルは、そのまま残る ----

    #[test]
    fn a_failed_backup_of_the_key_file_changes_nothing() {
        let (dir, data, key) = previous_export();

        let result = write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), failing_on(&[1]));

        assert!(result.is_err());
        assert_previous_export_untouched(dir.path(), &data, &key);
    }

    // 鍵ファイルを退避した後に、エクスポートしたファイルを退避できなければ(他のプロセスが開いている、など)、鍵ファイルを戻す。
    #[test]
    fn a_failed_backup_of_the_export_file_restores_the_key_file() {
        let (dir, data, key) = previous_export();

        let result = write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), failing_on(&[2]));

        assert!(result.is_err());
        assert_previous_export_untouched(dir.path(), &data, &key);
    }

    #[test]
    fn a_failed_key_file_replacement_restores_both_previous_files() {
        let (dir, data, key) = previous_export();

        let result = write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), failing_on(&[3]));

        assert!(result.is_err());
        assert_previous_export_untouched(dir.path(), &data, &key);
    }

    // 鍵ファイルだけが、新しくなった後に、エクスポートしたファイルの置き換えに失敗しても、前の鍵ファイルを戻す
    // (戻さないと、前のエクスポートしたファイルが、二度と復号できなくなる)。
    #[test]
    fn a_failed_export_file_replacement_restores_the_previous_key_file_too() {
        let (dir, data, key) = previous_export();

        let result = write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), failing_on(&[4]));

        assert!(result.is_err(), "エクスポートしたファイルを置き換えられなかったのに、成功と報告した");
        assert_previous_export_untouched(dir.path(), &data, &key);
    }

    // 保存先に何も無かったときは、失敗した後に、新しく置いたファイルを残さない(鍵ファイルだけが、残らない)。
    #[test]
    fn a_failed_replacement_leaves_no_file_when_nothing_existed_before() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("export.smx");
        let key = dir.path().join("export.smxkey");

        // 何も無いため、退避は起きない: 1回目が鍵ファイルの置き換え、2回目がエクスポートしたファイルの置き換え。
        let result = write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), failing_on(&[2]));

        assert!(result.is_err());
        assert!(file_names(dir.path()).is_empty(), "何かが、残っている: {:?}", file_names(dir.path()));
    }

    // 戻せなかった退避は、消さず、その場所を、エラーで伝える(前のファイルの、最後の1つの写しのため)。
    #[test]
    fn a_previous_file_that_cannot_be_restored_is_kept_and_named_in_the_error() {
        let (dir, data, key) = previous_export();

        // 4回目(エクスポートしたファイルの置き換え)の失敗の後の、5回目(鍵ファイルを戻す)も、失敗する。
        let error =
            write_key_file_export_with(&data, b"ciphertext", &key, &key_contents(), failing_on(&[4, 5])).unwrap_err();

        assert!(matches!(error, ProfileStoreError::PreviousFilesNotRestored(_)), "{error:?}");
        let message = error.to_string();
        let leftovers: Vec<String> =
            file_names(dir.path()).into_iter().filter(|name| name.starts_with(".export.smxkey.")).collect();
        assert_eq!(leftovers.len(), 1, "戻せなかった鍵ファイルの退避が、残っていない: {:?}", file_names(dir.path()));
        assert!(message.contains(&leftovers[0]), "エラーが、退避したファイルの場所を伝えていない: {message}");
        assert_eq!(fs::read(dir.path().join(&leftovers[0])).unwrap(), b"previous key");
        assert_eq!(fs::read(&data).unwrap(), b"previous export", "戻せる方は、戻す");
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

    // 一時の名前は、推測できない(順に増える番号・プロセスの番号だけにすると、保存先に書ける別のプロセスが、先に、その名前で、
    // 他のファイルへのリンクを置ける)。
    #[test]
    fn temporary_names_carry_random_bits_not_a_guessable_counter() {
        let dest = Path::new("some-folder").join("export.smxkey");

        let names: Vec<String> =
            (0..8).map(|_| temp_sibling(&dest).unwrap().file_name().unwrap().to_string_lossy().into_owned()).collect();

        for name in &names {
            let random_part = name.strip_prefix(".export.smxkey.").and_then(|rest| rest.strip_suffix(".tmp")).unwrap();
            assert_eq!(random_part.len(), 16, "{name}");
            assert!(random_part.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
        }
        let high_bits: std::collections::HashSet<char> = names.iter().map(|name| name.chars().nth(".export.smxkey.".len()).unwrap()).collect();
        assert!(high_bits.len() > 1, "8回とも、名前の先頭の桁が同じ(乱数でない): {names:?}");
    }

    // 一時のファイルの名前に、他のファイル(リンクを含む)が既に有っても、そこへは、書かない(秘密が、他のファイルへ書かれない)。
    #[test]
    fn a_file_planted_at_a_temporary_name_is_never_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let planted = dir.path().join("planted.txt");
        fs::write(&planted, b"planted").unwrap();
        let temp = dir.path().join(".export.smxkey.planted.tmp");
        fs::hard_link(&planted, &temp).unwrap();

        let result = key::write_new_owner_only_file(&temp, KEY_CONTENTS.as_bytes());

        assert!(result.is_err(), "既に有る名前へ、書いてしまった");
        assert_eq!(fs::read(&planted).unwrap(), b"planted", "リンクの先の、他のファイルへ、秘密を書いた");
    }

    // エクスポートしたファイルの一時のファイルも、既に有る名前(他のプロセスが置いたリンクを含む)へは、書かない。
    #[test]
    fn the_export_files_temporary_file_never_writes_through_an_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let planted = dir.path().join("planted.txt");
        fs::write(&planted, b"planted").unwrap();
        let temp = dir.path().join(".export.smx.planted.tmp");
        fs::hard_link(&planted, &temp).unwrap();

        let result = write_new_file_synced(&temp, b"ciphertext");

        assert!(result.is_err(), "既に有る名前へ、書いてしまった");
        assert_eq!(fs::read(&planted).unwrap(), b"planted", "リンクの先の、他のファイルへ、書いた");
    }
}
