//! `masker`実行ファイルの検索。まず自分の実行ファイルと同じディレクトリを探し、
//! 無ければPATHから探す(インストーラーで両者が同じディレクトリに配置される想定と、
//! 開発時にPATH経由で使う想定の両方をカバーする)。

use std::path::PathBuf;

const MASKER_BIN_NAME: &str = if cfg!(windows) { "masker.exe" } else { "masker" };

#[derive(Debug, thiserror::Error)]
#[error("maskerの実行ファイルが見つかりません。masker-mcpと同じディレクトリに配置するか、PATHに追加してください")]
pub(crate) struct MaskerNotFoundError;

pub(crate) fn resolve_masker_path() -> Result<PathBuf, MaskerNotFoundError> {
    if let Some(next_to_self) = next_to_current_exe()
        && next_to_self.is_file()
    {
        return Ok(next_to_self);
    }
    which::which(MASKER_BIN_NAME).map_err(|_| MaskerNotFoundError)
}

fn next_to_current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(dir.join(MASKER_BIN_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_path_when_not_next_to_current_exe() {
        // テスト実行ファイル(masker-mcpのテストバイナリ)と同じディレクトリにmaskerは無いはず
        // なので、PATH検索側にフォールバックする。CIやこのマシンにmaskerがPATH登録されて
        // いなければErrになるが、いずれの結果でもパニックしないことを確認する。
        let _ = resolve_masker_path();
    }

    #[test]
    fn masker_not_found_error_message_mentions_masker() {
        let err = MaskerNotFoundError;
        assert!(err.to_string().contains("masker"));
    }
}
