//! `mask --output`/`export --output`等、ユーザーが指定した書き込み先パスの検証。
//! 正規化・アプリのデータフォルダ除外はprofile_store側の共通ロジックを使う(GUIの
//! 保存ダイアログ経由の書き込みと同じ理由: 誤って暗号化DB・鍵ファイルを上書きするのを
//! 防ぐため)。実際の書き込みには、ここで返す正規化済みパスを使う(表示用のメッセージ
//! には呼び出し元が受け取った元の`Path`を使い続けてよい)。

use std::path::{Path, PathBuf};

use profile_store::AppPaths;

use crate::error::CliError;

/// ファイル1件の書き込み先を検証する(親ディレクトリがアプリのデータフォルダの
/// 内側でないことを確認する)。`mask --output <file>`/`export --output`向け。
pub(crate) fn validate_output_file(raw: &Path, app_paths: &AppPaths) -> Result<PathBuf, CliError> {
    let path = profile_store::normalize_and_reject_special_forms(&raw.to_string_lossy())?;
    if let Some(parent) = path.parent() {
        app_paths.reject_if_dir_is_inside_data_dir(parent)?;
    }
    Ok(path)
}

/// ディレクトリそのものへの書き込み(`mask --batch --output <dir>`)を検証する。
pub(crate) fn validate_output_dir(raw: &Path, app_paths: &AppPaths) -> Result<PathBuf, CliError> {
    let path = profile_store::normalize_and_reject_special_forms(&raw.to_string_lossy())?;
    app_paths.reject_if_dir_is_inside_data_dir(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_output_file_rejects_a_path_inside_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        let sneaky = data_dir.path().join("profiles.db");

        let err = validate_output_file(&sneaky, &app_paths).unwrap_err();

        assert!(matches!(err, CliError::Path(_)));
    }

    #[test]
    fn validate_output_file_allows_a_path_outside_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());

        validate_output_file(&dest_dir.path().join("out.txt"), &app_paths)
            .expect("データフォルダ外への保存は許可されるはず");
    }

    #[test]
    fn validate_output_dir_rejects_the_app_data_dir_itself() {
        let data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());

        let err = validate_output_dir(data_dir.path(), &app_paths).unwrap_err();

        assert!(matches!(err, CliError::Path(_)));
    }

    #[test]
    fn validate_output_dir_allows_a_directory_outside_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());

        validate_output_dir(out_dir.path(), &app_paths).expect("データフォルダ外への出力は許可されるはず");
    }
}
