//! 「複数起動を許可」設定の永続化。profile-storeの暗号化DB・鍵ファイルとは別に、
//! 機微でない単一のbool値をJSONファイルとして持つ(暗号化・zeroizeは不要)。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const BUNDLE_IDENTIFIER: &str = "io.github.akutori.sensitivemasker";
const FILE_NAME: &str = "instance-settings.json";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSettings {
    #[serde(default)]
    allow_multiple_instances: bool,
}

pub struct InstanceSettingsPath(PathBuf);

impl InstanceSettingsPath {
    /// profile-store::paths::AppPaths::resolve()と同じ`dirs::data_dir()`+bundle identifier配下。
    /// SENSITIVEMASKER_DATA_DIRが設定されていればそこを使う(profiles::resolve_pathsと同じ、
    /// E2Eテストが実ユーザーの設定ファイルを書き換えないためのdebug build専用の迂回路。
    /// release buildではこの分岐自体が存在しない)。
    #[cfg(debug_assertions)]
    pub fn resolve() -> Option<Self> {
        Self::resolve_with_override(std::env::var("SENSITIVEMASKER_DATA_DIR").ok())
    }

    #[cfg(debug_assertions)]
    fn resolve_with_override(override_dir: Option<String>) -> Option<Self> {
        match override_dir {
            Some(dir) => Some(Self::at(dir)),
            None => Some(Self::at(dirs::data_dir()?.join(BUNDLE_IDENTIFIER))),
        }
    }

    #[cfg(not(debug_assertions))]
    pub fn resolve() -> Option<Self> {
        Some(Self::at(dirs::data_dir()?.join(BUNDLE_IDENTIFIER)))
    }

    pub fn at(base_dir: impl Into<PathBuf>) -> Self {
        Self(base_dir.into().join(FILE_NAME))
    }

    /// ファイル未作成・壊れている等、読み取れない場合はfalse(単一起動)にフォールバックする
    /// (fail-safe defaults: 判断が付かない場合は安全側=単一起動を優先する)。
    pub fn allow_multiple_instances(&self) -> bool {
        read(&self.0).unwrap_or_default().allow_multiple_instances
    }

    pub fn set_allow_multiple_instances(&self, value: bool) -> std::io::Result<()> {
        write(&self.0, StoredSettings { allow_multiple_instances: value })
    }
}

fn read(path: &std::path::Path) -> Option<StoredSettings> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 「複数起動を許可」がONの間は複数プロセスが同時に存在でき、各プロセスのトレイメニューから
/// 同じファイルへほぼ同時に書き込みうる。一時ファイルに書いてから同じディレクトリ内で
/// renameする(rename先を直接開いて書かない)ことで、他プロセス・自プロセスの再読み込みに
/// 書き込み途中の中身(切り詰められたJSON)を見せない。renameが競合した場合の挙動は
/// 後勝ちになるが、内容が壊れることはない。
fn write(path: &std::path::Path, settings: StoredSettings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp_path = tmp_path_for(path);
    // 書き込み・rename失敗時は、残った一時ファイルをベストエフォートで片付ける
    // (片付け自体の失敗は無視。既に呼び出し元へ返すエラーがあるため)。
    if let Err(error) = std::fs::write(&tmp_path, json) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(error);
    }
    Ok(())
}

/// 同時に書き込む他プロセスと衝突しないよう、プロセスIDを含める(同一プロセス内の並行呼び出しは
/// 想定しない。呼び出し元はtray.rsのメニューイベントハンドラのみで、OSのメニューイベントは
/// 単一スレッドでシリアライズされるため)。
fn tmp_path_for(path: &std::path::Path) -> PathBuf {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_multiple_instances_defaults_to_false_when_the_file_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let settings = InstanceSettingsPath::at(dir.path());

        assert!(!settings.allow_multiple_instances(), "初回起動(未作成)は単一起動が既定のはず");
    }

    #[test]
    fn set_then_get_round_trips_true() {
        let dir = tempfile::tempdir().unwrap();
        let settings = InstanceSettingsPath::at(dir.path());

        settings.set_allow_multiple_instances(true).expect("書き込みに成功するはず");

        assert!(settings.allow_multiple_instances(), "書き込んだtrueが読み戻せるはず");
    }

    #[test]
    fn set_then_get_round_trips_false_after_true() {
        let dir = tempfile::tempdir().unwrap();
        let settings = InstanceSettingsPath::at(dir.path());
        settings.set_allow_multiple_instances(true).unwrap();

        settings.set_allow_multiple_instances(false).expect("上書きに成功するはず");

        assert!(!settings.allow_multiple_instances(), "falseへの上書きが読み戻せるはず");
    }

    #[test]
    fn allow_multiple_instances_falls_back_to_false_when_the_file_is_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let settings = InstanceSettingsPath::at(dir.path());
        std::fs::write(dir.path().join(FILE_NAME), b"not valid json").unwrap();

        assert!(!settings.allow_multiple_instances(), "壊れたファイルは安全側(単一起動)にフォールバックするはず");
    }

    #[test]
    fn set_allow_multiple_instances_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested").join("deeper");
        let settings = InstanceSettingsPath::at(&nested);

        settings.set_allow_multiple_instances(true).expect("未作成の親ディレクトリごと作成できるはず");

        assert!(settings.allow_multiple_instances());
    }

    #[test]
    fn resolve_with_override_uses_given_directory_when_present() {
        let settings = InstanceSettingsPath::resolve_with_override(Some("e2e-test-data".to_string()))
            .expect("overrideありなら常に成功するはず");

        assert_eq!(settings.0, std::path::Path::new("e2e-test-data").join(FILE_NAME));
    }

    #[test]
    fn resolve_with_override_falls_back_to_os_data_dir_when_absent() {
        let settings =
            InstanceSettingsPath::resolve_with_override(None).expect("OS標準パスの解決自体は失敗しないはず");

        assert!(settings.0.ends_with(FILE_NAME));
    }

    #[test]
    fn writing_does_not_leave_a_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let settings = InstanceSettingsPath::at(dir.path());

        settings.set_allow_multiple_instances(true).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != FILE_NAME)
            .collect();
        assert!(leftovers.is_empty(), "一時ファイルが残っているはずがない: {leftovers:?}");
    }

    #[test]
    fn a_failed_rename_still_cleans_up_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        // renameの行き先をディレクトリにしておくと、ファイルでの置き換えに失敗する
        // (Windows/Unixとも、renameでファイルはディレクトリを上書きできない)。
        let target_path = dir.path().join(FILE_NAME);
        std::fs::create_dir(&target_path).unwrap();

        let result = write(&target_path, StoredSettings { allow_multiple_instances: true });

        assert!(result.is_err(), "ディレクトリへのrenameは失敗するはず");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != FILE_NAME)
            .collect();
        assert!(leftovers.is_empty(), "rename失敗後も一時ファイルは残らないはず: {leftovers:?}");
    }
}
