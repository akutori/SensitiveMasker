//! メイン画面の「ファイルから」「ファイルに保存」で使うプレーンテキストの読み書き。
//! 書き込み先は、export_import.rsの`.smx`保存と同じ理由(暗号化データベース・鍵
//! ファイルの誤上書き防止)でアプリのデータフォルダの内側を拒否する(拡張子の制限は
//! 汎用テキストファイルのため課さない)。読み込みは、ユーザーが既にOSレベルの読み取り
//! 権限を持つファイルを対象にするだけであり新たな権限を与えるものではないため、
//! 同様の検証は行わない。

use std::borrow::Cow;
use std::path::Path;

use profile_store::AppPaths;

#[derive(Debug, serde::Serialize)]
pub struct ReadTextFileResult {
    text: String,
    had_invalid_utf8: bool,
}

// CLIの`mask`コマンドと同様、UTF-8として解釈できないバイト列があってもエラーで
// 停止せず置き換えて読み込みを継続する。置き換えが発生したかどうかは
// `String::from_utf8_lossy`が実際にコピーを作ったか(Cow::Owned)で判定できる。
fn read_text_file_impl(path: &str) -> Result<ReadTextFileResult, String> {
    let bytes = std::fs::read(path).map_err(|_| "ファイルを読み込めませんでした".to_string())?;
    let decoded = String::from_utf8_lossy(&bytes);
    let had_invalid_utf8 = matches!(decoded, Cow::Owned(_));
    Ok(ReadTextFileResult { text: decoded.into_owned(), had_invalid_utf8 })
}

fn write_text_file_impl(path: &str, content: &str, app_paths: Result<AppPaths, String>) -> Result<(), String> {
    let validated = profile_store::normalize_and_reject_special_forms(path).map_err(|e| e.to_string())?;
    let app_paths = app_paths?;
    let dest_parent = validated.parent().ok_or_else(|| "ファイルを書き込めませんでした".to_string())?;
    app_paths.reject_if_dir_is_inside_data_dir(dest_parent).map_err(|e| e.to_string())?;
    // 親のフォルダが外でも、ハードリンク等で、鍵・DBの実体を指すパスは、拒否する。
    app_paths.reject_if_file_is_app_data(&validated).map_err(|e| e.to_string())?;
    write_validated(&validated, content)
}

fn write_validated(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|_| "ファイルを書き込めませんでした".to_string())
}

// 大きなログファイルの読み書きでUIスレッドと共有される非同期ランタイムの
// ワーカースレッドを塞がないよう、export_import.rsのpreview_importと同じ理由で
// 専用のブロッキングスレッドプールへ逃がす。
#[tauri::command]
pub async fn read_text_file(path: String) -> Result<ReadTextFileResult, String> {
    tauri::async_runtime::spawn_blocking(move || read_text_file_impl(&path))
        .await
        .map_err(|_| "ファイルを読み込めませんでした".to_string())?
}

#[tauri::command]
pub async fn write_text_file(path: String, content: String) -> Result<(), String> {
    let app_paths = crate::profiles::resolve_paths();
    tauri::async_runtime::spawn_blocking(move || write_text_file_impl(&path, &content, app_paths))
        .await
        .map_err(|_| "ファイルを書き込めませんでした".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_valid_utf8_file_without_flagging_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.log");
        std::fs::write(&path, "こんにちは").unwrap();

        let result = read_text_file_impl(path.to_str().unwrap()).unwrap();

        assert_eq!(result.text, "こんにちは");
        assert!(!result.had_invalid_utf8);
    }

    #[test]
    fn replaces_invalid_utf8_bytes_and_flags_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.log");
        std::fs::write(&path, [b'a', 0xff, b'b']).unwrap();

        let result = read_text_file_impl(path.to_str().unwrap()).unwrap();

        assert!(result.text.contains('a'));
        assert!(result.text.contains('b'));
        assert!(result.had_invalid_utf8);
    }

    #[test]
    fn read_text_file_fails_clearly_for_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.log");

        let err = read_text_file_impl(path.to_str().unwrap()).unwrap_err();

        assert!(!err.is_empty());
    }

    #[test]
    fn writes_and_reads_back_the_same_content() {
        // AppPathsが指す場所(data_dir)と実際の書き込み先(dest_dir)を別々の実在する
        // tempdirにする。is_same_fileはOSレベルで実体解決するため、どちらかが早期に
        // dropされ実体が消えると「確認できない」側に倒れて誤って拒否されてしまう
        // (fail-safe defaultsの帰結)。両方をテスト関数の終わりまで生存させる。
        let data_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let path = dest_dir.path().join("output.txt");

        write_text_file_impl(path.to_str().unwrap(), "結果テキスト", Ok(AppPaths::at(data_dir.path())))
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "結果テキスト");
    }

    #[test]
    fn write_text_file_fails_clearly_when_the_parent_directory_does_not_exist() {
        let data_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let path = dest_dir.path().join("no-such-subdir").join("output.txt");

        let err = write_text_file_impl(path.to_str().unwrap(), "x", Ok(AppPaths::at(data_dir.path())))
            .unwrap_err();

        assert!(!err.is_empty());
    }

    // 実際にアプリのデータフォルダ(暗号化DB・鍵ファイルの保存先)へ書き込もうとする
    // 経路が塞がっていることを固定する(ネイティブの保存ダイアログでユーザーが
    // 誤ってその場所を選んだ場合、profiles.db/key.binを平文で上書きしうる欠陥の回帰テスト)。
    #[test]
    fn write_text_file_rejects_paths_inside_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        let sneaky = data_dir.path().join("profiles.db");

        let err = write_text_file_impl(sneaky.to_str().unwrap(), "x", Ok(app_paths)).unwrap_err();

        assert!(err.contains("データフォルダ"), "予期しないエラー文言: {err}");
        assert!(!sneaky.exists(), "検証を通過して実際に書き込まれてしまった");
    }

    #[test]
    fn write_text_file_fails_closed_when_the_app_data_dir_cannot_be_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.txt");

        write_text_file_impl(path.to_str().unwrap(), "x", Err("resolution failed".to_string()))
            .expect_err("データフォルダの場所を解決できない場合は安全側に倒して拒否するはず");
    }
}
