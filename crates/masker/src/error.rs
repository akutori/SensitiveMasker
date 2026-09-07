//! CLI全体で使うエラー型。既存crateのDisplay実装を透過的に利用する。

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Store(#[from] profile_store::ProfileStoreError),
    #[error(transparent)]
    Path(#[from] profile_store::PathError),
    #[error(transparent)]
    Profile(#[from] masking_core::RuleProfileError),
    #[error("入出力エラー: {0}")]
    Io(#[from] std::io::Error),
    #[error("入出力エラー({path}): {source}")]
    IoAt {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("JSONの解析に失敗しました({path}): {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "アクティブなプロファイルがありません。\
         `masker profile create <name> [--from-json <file>]`で作成するか--profileで指定してください"
    )]
    NoActiveProfile,
    #[error("mask引数が不正です: {0}")]
    InvalidMaskArgs(String),
    #[error("パスフレーズが一致しません")]
    PassphraseMismatch,
}
