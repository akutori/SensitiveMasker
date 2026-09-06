//! アプリのデータディレクトリ・鍵ファイル・DBファイルのパス解決。

use std::path::PathBuf;

/// Tauri GUI側の`app_data_dir()`(`dirs::data_dir().join(identifier)`)と同じ値になるよう、
/// GUI側の`tauri.conf.json`の`identifier`にも必ずこの文字列を設定する。
const BUNDLE_IDENTIFIER: &str = "io.github.akutori.sensitivemasker";

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("OS標準のデータディレクトリを解決できませんでした")]
    UnknownDataDir,
}

/// 鍵ファイル・DBファイルの実際のパス。テスト時は`at`で一時ディレクトリを指定することで、
/// 実際のOSデータディレクトリに書き込まずに済む。
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub key_path: PathBuf,
    pub db_path: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self, PathError> {
        let base = dirs::data_dir()
            .ok_or(PathError::UnknownDataDir)?
            .join(BUNDLE_IDENTIFIER);
        Ok(Self::at(base))
    }

    pub fn at(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        Self {
            key_path: base_dir.join("key.bin"),
            db_path: base_dir.join("profiles.db"),
        }
    }
}
