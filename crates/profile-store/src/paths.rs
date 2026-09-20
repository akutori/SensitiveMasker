//! アプリのデータディレクトリ・鍵ファイル・DBファイルのパス解決、および
//! 外部(CLIの引数・GUIの保存ダイアログ)から指定された書き込み先パスの検証。

use std::path::{Path, PathBuf};

/// Tauri GUI側の`app_data_dir()`(`dirs::data_dir().join(identifier)`)と同じ値になるよう、
/// GUI側の`tauri.conf.json`の`identifier`にも必ずこの文字列を設定する。
const BUNDLE_IDENTIFIER: &str = "io.github.akutori.sensitivemasker";

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("OS標準のデータディレクトリを解決できませんでした")]
    UnknownDataDir,
    #[error("ネットワークパスや特殊な形式のパスは指定できません")]
    SpecialForm,
    #[error("アプリのデータフォルダには保存できません")]
    InsideAppDataDir,
    #[error("パスを確認できませんでした")]
    Unresolvable,
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

    /// `dir`が、このAppPathsのデータフォルダ(鍵ファイル・DBファイルの保存場所)自身か
    /// その内側かどうかを検証する。ファイル1件の書き込み先を検証する場合は
    /// `path.parent()`を、ディレクトリそのものへの書き込み(バッチ処理の出力先等)を
    /// 検証する場合はそのディレクトリを渡す。エクスポート・マスク結果の保存等、CLI/GUI
    /// 問わず外部から指定された書き込み先パスは、これを通してから使うことで、ユーザーが
    /// 誤ってその場所を指定した場合に暗号化DB・鍵ファイルを上書きすることを防ぐ。
    /// データフォルダの場所や`dir`の実体を確認できない場合は安全側に倒して拒否する
    /// (fail-safe defaults)。
    pub fn reject_if_dir_is_inside_data_dir(&self, dir: &Path) -> Result<(), PathError> {
        let app_dir = self.key_path.parent().ok_or(PathError::Unresolvable)?;
        if path_is_same_or_inside(dir, app_dir)? {
            return Err(PathError::InsideAppDataDir);
        }
        Ok(())
    }

    /// `path`(ファイル1件の書き込み先)が、鍵ファイル・DBファイルの実体そのものでないことを検証する。親のフォルダが
    /// データフォルダの外でも、ハードリンク(データフォルダの外に作られた、実体を共有する別名)やシンボリックリンクを
    /// 経由して書き込むと、暗号化DB・鍵ファイルを上書きしてしまうため、名前ではなく、実体の同一性で比べる
    /// (`reject_if_dir_is_inside_data_dir`と併せて使う)。書き込み先がまだ存在しない場合は、既存のファイルの別名に
    /// なりえないため、何もしない。鍵ファイル・DBファイルが存在しない(未初期化の)場合も、上書きされる実体が無いため、
    /// 何もしない。確認できない場合(開けない)は、安全側に倒して拒否する。
    pub fn reject_if_file_is_app_data(&self, path: &Path) -> Result<(), PathError> {
        for protected in [&self.key_path, &self.db_path] {
            match same_file::is_same_file(path, protected) {
                Ok(true) => return Err(PathError::InsideAppDataDir),
                Ok(false) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(PathError::Unresolvable),
            }
        }
        Ok(())
    }
}

/// 書き込み先パスとして使う前に文字列を正規化し、UNC(`\\server\share\...`)・
/// ローカルデバイス(`\\.\`)・拡張長(`\\?\`)等の特殊な形式を拒否する
/// (`std::path::absolute`はファイルシステムに触れない字句上の正規化のみ)。
pub fn normalize_and_reject_special_forms(raw: &str) -> Result<PathBuf, PathError> {
    let absolute = std::path::absolute(raw).map_err(|_| PathError::Unresolvable)?;
    if absolute.to_string_lossy().starts_with(r"\\") {
        return Err(PathError::SpecialForm);
    }
    Ok(absolute)
}

/// `candidate`が`boundary`自身か、その配下かを判定する。パス文字列の比較(大文字小文字・
/// ジャンクション/シンボリックリンク・ドライブレターやUNC管理共有等の別名表現)では
/// 回避されうるため、OSにファイルの実体を解決させる`same_file::is_same_file`で
/// 祖先を1つずつ比較する(経由したパスの綴りに依存しない)。
///
/// `candidate`側にまだ存在しない祖先(CLIのバッチ出力先のように、これから
/// `create_dir_all`で作る予定のネストしたディレクトリ)は、実在しない以上シンボリック
/// リンク等の別名にはなり得ないため読み飛ばす(存在する祖先が見つかれば、それ以降の
/// 祖先は実在するディレクトリの親である以上必ず実在する)。読み飛ばすのは`candidate`
/// 側のみで、`boundary`(アプリのデータフォルダ)自体が実在しない場合は
/// `same_file::is_same_file`が失敗し、これまで通り安全側に倒して拒否される。
fn path_is_same_or_inside(candidate: &Path, boundary: &Path) -> Result<bool, PathError> {
    for ancestor in candidate.ancestors() {
        if !ancestor.exists() {
            continue;
        }
        if same_file::is_same_file(ancestor, boundary).map_err(|_| PathError::Unresolvable)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // UNCの表記(`\\server\share`)はWindowsのパスの形式で、Unixでは、区切りではない文字を含む
    // 相対パスの名前になるため、Windowsでだけ検証する。
    #[test]
    #[cfg(windows)]
    fn normalize_and_reject_special_forms_rejects_unc_paths() {
        let err = normalize_and_reject_special_forms(r"\\server\share\export.smx").unwrap_err();
        assert!(matches!(err, PathError::SpecialForm));
    }

    #[test]
    fn normalize_and_reject_special_forms_accepts_a_relative_path() {
        normalize_and_reject_special_forms("out.txt").expect("相対パスは正規化できるはず");
    }

    #[test]
    fn reject_if_dir_is_inside_data_dir_rejects_the_app_data_dir_itself() {
        let data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());

        let err = app_paths.reject_if_dir_is_inside_data_dir(data_dir.path()).unwrap_err();

        assert!(matches!(err, PathError::InsideAppDataDir));
    }

    #[test]
    fn reject_if_dir_is_inside_data_dir_rejects_a_subdirectory_of_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let nested = data_dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let app_paths = AppPaths::at(data_dir.path());

        let err = app_paths.reject_if_dir_is_inside_data_dir(&nested).unwrap_err();

        assert!(matches!(err, PathError::InsideAppDataDir));
    }

    #[test]
    fn reject_if_dir_is_inside_data_dir_allows_a_directory_outside_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());

        app_paths
            .reject_if_dir_is_inside_data_dir(elsewhere.path())
            .expect("データフォルダ外への書き込みは許可されるはず");
    }

    // データフォルダの外に作られた、鍵・DBの実体へのハードリンク(実体を共有する別名)は、親のフォルダが外でも、
    // 書き込むと、鍵・DBを上書きする。
    #[test]
    fn reject_if_file_is_app_data_rejects_a_hard_link_to_the_key_file_or_the_db_file() {
        let data_dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        std::fs::write(&app_paths.key_path, b"key").unwrap();
        std::fs::write(&app_paths.db_path, b"db").unwrap();
        for (label, target) in [("鍵", &app_paths.key_path), ("DB", &app_paths.db_path)] {
            let link = elsewhere.path().join(format!("sneaky-{label}.smxkey"));
            std::fs::hard_link(target, &link).unwrap();

            let err = app_paths.reject_if_file_is_app_data(&link).unwrap_err();

            assert!(matches!(err, PathError::InsideAppDataDir), "{label}: {err}");
        }
    }

    #[test]
    fn reject_if_file_is_app_data_rejects_the_key_file_itself() {
        let data_dir = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        std::fs::write(&app_paths.key_path, b"key").unwrap();

        let err = app_paths.reject_if_file_is_app_data(&app_paths.key_path).unwrap_err();

        assert!(matches!(err, PathError::InsideAppDataDir));
    }

    #[test]
    fn reject_if_file_is_app_data_allows_an_unrelated_existing_file_and_a_file_that_does_not_exist() {
        let data_dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        std::fs::write(&app_paths.key_path, b"key").unwrap();
        std::fs::write(&app_paths.db_path, b"db").unwrap();
        let unrelated = elsewhere.path().join("unrelated.smxkey");
        std::fs::write(&unrelated, b"other").unwrap();

        app_paths.reject_if_file_is_app_data(&unrelated).expect("実体が違うファイルは許可されるはず");
        app_paths
            .reject_if_file_is_app_data(&elsewhere.path().join("not-yet.smxkey"))
            .expect("まだ存在しない書き込み先は許可されるはず");
    }

    // 未初期化(鍵・DBが、まだ無い)なら、上書きされる実体が無い。
    #[test]
    fn reject_if_file_is_app_data_allows_any_file_before_the_app_is_initialized() {
        let data_dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(data_dir.path());
        let existing = elsewhere.path().join("existing.smxkey");
        std::fs::write(&existing, b"x").unwrap();

        app_paths.reject_if_file_is_app_data(&existing).expect("鍵・DBが無ければ、拒否する理由は無いはず");
    }

    #[test]
    fn reject_if_dir_is_inside_data_dir_fails_closed_when_the_app_data_dir_does_not_exist_on_disk() {
        let parent = tempfile::tempdir().unwrap();
        let app_paths = AppPaths::at(parent.path().join("does_not_exist"));
        let elsewhere = tempfile::tempdir().unwrap();

        app_paths
            .reject_if_dir_is_inside_data_dir(elsewhere.path())
            .expect_err("データフォルダの実体を確認できない場合は安全側に倒して拒否するはず");
    }

    // CLIのバッチ処理(`mask --batch --output <dir>`)は指定した出力先ディレクトリが
    // まだ存在しない場合にcreate_dir_allで新規作成する、既存の想定された挙動である。
    // candidate側の祖先が実在しないというだけで安全側に倒して拒否すると、この
    // 「まだ存在しない新規ディレクトリへの出力」が常に拒否されてしまう回帰を防ぐ。
    #[test]
    fn reject_if_dir_is_inside_data_dir_allows_a_not_yet_created_nested_directory_outside_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let not_yet_created = elsewhere.path().join("newly").join("nested");
        let app_paths = AppPaths::at(data_dir.path());

        app_paths
            .reject_if_dir_is_inside_data_dir(&not_yet_created)
            .expect("データフォルダ外であれば、まだ存在しないネストしたディレクトリも許可されるはず");
    }

    // 上のテストの「読み飛ばし」が抜け穴にならないことの固定: candidateの実在する
    // 祖先(この場合はdata_dir自身)がboundaryと一致するなら、その配下にまだ存在しない
    // ネストしたパスを続けても拒否され続けなければならない。
    #[test]
    fn reject_if_dir_is_inside_data_dir_still_rejects_a_not_yet_created_nested_directory_inside_the_app_data_dir() {
        let data_dir = tempfile::tempdir().unwrap();
        let sneaky_nested = data_dir.path().join("newly").join("nested");
        let app_paths = AppPaths::at(data_dir.path());

        let err = app_paths.reject_if_dir_is_inside_data_dir(&sneaky_nested).unwrap_err();

        assert!(matches!(err, PathError::InsideAppDataDir));
    }
}
