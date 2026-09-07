//! `masker profile`サブコマンドの実行ロジック。

use std::path::Path;

use masking_core::{Rule, RuleProfile};
use profile_store::{ProfileStore, SecretString};

use crate::cli::ProfileAction;
use crate::error::CliError;

pub(crate) fn run(action: &ProfileAction, store: &mut ProfileStore) -> Result<(), CliError> {
    match action {
        ProfileAction::List => list(store),
        ProfileAction::Use { name } => use_profile(store, name),
        ProfileAction::Create { name, from_json } => create(store, name, from_json.as_deref()),
        ProfileAction::Delete { name } => delete(store, name),
        ProfileAction::Export { name, output } => export(store, name, output),
        ProfileAction::Import { input } => import(store, input),
    }
}

fn list(store: &ProfileStore) -> Result<(), CliError> {
    let summaries = store.list_profiles()?;
    if summaries.is_empty() {
        println!("プロファイルがありません");
        return Ok(());
    }
    for s in &summaries {
        let active = if s.is_active { "*" } else { " " };
        let favorite = if s.is_favorite { "*" } else { " " };
        println!(
            "{active} {:<20} rules={:<4} favorite={favorite} updated_at={}",
            s.name, s.rule_count, s.updated_at
        );
    }
    Ok(())
}

fn use_profile(store: &mut ProfileStore, name: &str) -> Result<(), CliError> {
    store.set_active_profile(name)?;
    println!("アクティブプロファイルを '{name}' に切り替えました");
    Ok(())
}

// --from-jsonで渡された内容は、DBに書き込む前に必ず全て検証する(load_rules_from_jsonの
// JSON解析+Rule::new経由の検証、続くRuleProfile::newのルール名重複検証)。いずれかに失敗した
// 場合はcreate_profileを呼ばないため、プロファイルが部分的に作成されることはない。
fn create(store: &mut ProfileStore, name: &str, from_json: Option<&Path>) -> Result<(), CliError> {
    let rules = match from_json {
        Some(path) => load_rules_from_json(path)?,
        None => Vec::new(),
    };
    let profile = RuleProfile::new(name, None, rules)?;
    store.create_profile(&profile)?;
    println!("プロファイル '{name}' を作成しました(ルール数: {})", profile.rules().len());
    Ok(())
}

fn load_rules_from_json(path: &Path) -> Result<Vec<Rule>, CliError> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|source| CliError::Json { path: path.to_path_buf(), source })
}

fn delete(store: &mut ProfileStore, name: &str) -> Result<(), CliError> {
    store.delete_profile(name)?;
    println!("プロファイル '{name}' を削除しました");
    Ok(())
}

fn export(store: &ProfileStore, name: &str, output: &Path) -> Result<(), CliError> {
    // 名前の存在確認を先に行う(存在しない名前に対して無駄にパスフレーズを2回入力させない)。
    store.get_profile(name)?;
    let passphrase = prompt_new_passphrase()?;
    export_with_passphrase(store, name, output, passphrase)
}

// TTY読み取り(rpassword)をテスト対象から分離するための本体。
fn export_with_passphrase(
    store: &ProfileStore,
    name: &str,
    output: &Path,
    passphrase: SecretString,
) -> Result<(), CliError> {
    let encrypted = store.export_profile(name, passphrase)?;
    std::fs::write(output, &encrypted).map_err(|source| CliError::IoAt { path: output.to_path_buf(), source })?;
    println!("プロファイル '{name}' を '{}' にエクスポートしました", output.display());
    Ok(())
}

// タイプミス対策として2回入力させ、一致しない場合はエクスポートしない(age -pと同じ挙動)。
fn prompt_new_passphrase() -> Result<SecretString, CliError> {
    let first = rpassword::prompt_password("エクスポート用パスフレーズ: ")?;
    let second = rpassword::prompt_password("パスフレーズ(確認): ")?;
    confirm_passphrase(first, second)
}

// 比較ロジックのみを切り出し、TTY読み取り無しでテストできるようにする。
fn confirm_passphrase(first: String, second: String) -> Result<SecretString, CliError> {
    if first != second {
        return Err(CliError::PassphraseMismatch);
    }
    Ok(SecretString::from(first))
}

fn import(store: &mut ProfileStore, input: &Path) -> Result<(), CliError> {
    // ファイルの存在確認を先に行う(存在しないパスに対して無駄にパスフレーズを入力させない)。
    std::fs::read(input).map_err(|source| CliError::IoAt { path: input.to_path_buf(), source })?;
    let passphrase = SecretString::from(rpassword::prompt_password("インポート用パスフレーズ: ")?);
    import_with_passphrase(store, input, passphrase)
}

// TTY読み取り(rpassword)をテスト対象から分離するための本体。
fn import_with_passphrase(store: &mut ProfileStore, input: &Path, passphrase: SecretString) -> Result<(), CliError> {
    let data = std::fs::read(input).map_err(|source| CliError::IoAt { path: input.to_path_buf(), source })?;
    let profile = store.import_profile(&data, passphrase)?;
    println!("プロファイル '{}' をインポートしました(ルール数: {})", profile.profile_name(), profile.rules().len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use profile_store::AppPaths;

    use super::*;

    fn temp_store() -> (tempfile::TempDir, ProfileStore) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path());
        profile_store::init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();
        (dir, store)
    }

    #[test]
    fn create_with_no_from_json_creates_an_empty_profile() {
        let (_dir, mut store) = temp_store();
        create(&mut store, "work", None).unwrap();
        assert_eq!(store.get_profile("work").unwrap().rules().len(), 0);
    }

    #[test]
    fn create_with_valid_from_json_loads_rules() {
        let (_dir, mut store) = temp_store();
        let json_dir = tempfile::tempdir().unwrap();
        let json_path = json_dir.path().join("rules.json");
        std::fs::write(
            &json_path,
            r#"[{"name":"ip","pattern_type":"regex","pattern":"\\d+\\.\\d+\\.\\d+\\.\\d+","mode":"sequential","prefix":"__MASK_IP_"}]"#,
        )
        .unwrap();

        create(&mut store, "work", Some(&json_path)).unwrap();

        let profile = store.get_profile("work").unwrap();
        assert_eq!(profile.rules().len(), 1);
        assert_eq!(profile.rules()[0].name(), "ip");
    }

    #[test]
    fn create_with_invalid_regex_in_json_creates_nothing() {
        let (_dir, mut store) = temp_store();
        let json_dir = tempfile::tempdir().unwrap();
        let json_path = json_dir.path().join("rules.json");
        std::fs::write(
            &json_path,
            r#"[{"name":"bad","pattern_type":"regex","pattern":"(?=lookahead)","mode":"fixed","fixed_value":"X"}]"#,
        )
        .unwrap();

        let err = create(&mut store, "work", Some(&json_path)).expect_err("不正な正規表現は拒否されるはず");
        assert!(matches!(err, CliError::Json { .. }));
        assert!(store.get_profile("work").is_err(), "検証に失敗した場合はプロファイルが作成されてはいけない");
    }

    #[test]
    fn create_with_duplicate_rule_names_in_json_creates_nothing() {
        let (_dir, mut store) = temp_store();
        let json_dir = tempfile::tempdir().unwrap();
        let json_path = json_dir.path().join("rules.json");
        std::fs::write(
            &json_path,
            r#"[
                {"name":"dup","pattern_type":"literal","pattern":"a","mode":"fixed","fixed_value":"X"},
                {"name":"dup","pattern_type":"literal","pattern":"b","mode":"fixed","fixed_value":"Y"}
            ]"#,
        )
        .unwrap();

        let err = create(&mut store, "work", Some(&json_path)).expect_err("ルール名重複は拒否されるはず");
        assert!(matches!(err, CliError::Profile(_)));
        assert!(store.get_profile("work").is_err());
    }

    #[test]
    fn create_with_malformed_json_creates_nothing() {
        let (_dir, mut store) = temp_store();
        let json_dir = tempfile::tempdir().unwrap();
        let json_path = json_dir.path().join("rules.json");
        std::fs::write(&json_path, "not valid json").unwrap();

        let err = create(&mut store, "work", Some(&json_path)).expect_err("不正なJSONは拒否されるはず");
        assert!(matches!(err, CliError::Json { .. }));
        assert!(store.get_profile("work").is_err());
    }

    #[test]
    fn create_with_missing_json_file_fails_cleanly() {
        let (_dir, mut store) = temp_store();
        let err = create(&mut store, "work", Some(Path::new("no/such/file.json")))
            .expect_err("存在しないファイルは失敗するはず");
        assert!(matches!(err, CliError::Io(_)));
        assert!(store.get_profile("work").is_err());
    }

    #[test]
    fn list_use_delete_round_trip() {
        let (_dir, mut store) = temp_store();
        create(&mut store, "first", None).unwrap();
        create(&mut store, "second", None).unwrap();

        use_profile(&mut store, "second").unwrap();
        assert_eq!(store.active_profile().unwrap().unwrap().profile_name(), "second");

        let err = delete(&mut store, "second").expect_err("アクティブなプロファイルの削除は拒否されるはず");
        assert!(matches!(err, CliError::Store(profile_store::ProfileStoreError::CannotDeleteActiveProfile)));

        delete(&mut store, "first").unwrap();
        assert!(store.get_profile("first").is_err());
    }

    fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn export_then_import_round_trips_into_a_different_store() {
        let (_dir_a, mut store_a) = temp_store();
        create(&mut store_a, "work", None).unwrap();
        let export_dir = tempfile::tempdir().unwrap();
        let export_path = export_dir.path().join("work.agemask");

        export_with_passphrase(&store_a, "work", &export_path, passphrase("pw")).unwrap();

        let (_dir_b, mut store_b) = temp_store();
        import_with_passphrase(&mut store_b, &export_path, passphrase("pw")).unwrap();

        assert_eq!(store_b.get_profile("work").unwrap().profile_name(), "work");
    }

    #[test]
    fn importing_with_the_wrong_passphrase_fails_cleanly() {
        let (_dir_a, mut store_a) = temp_store();
        create(&mut store_a, "work", None).unwrap();
        let export_dir = tempfile::tempdir().unwrap();
        let export_path = export_dir.path().join("work.agemask");
        export_with_passphrase(&store_a, "work", &export_path, passphrase("correct")).unwrap();

        let (_dir_b, mut store_b) = temp_store();
        let err = import_with_passphrase(&mut store_b, &export_path, passphrase("wrong"))
            .expect_err("誤ったパスフレーズは拒否されるはず");

        assert!(matches!(err, CliError::Store(profile_store::ProfileStoreError::Export(_))));
    }

    #[test]
    fn confirm_passphrase_accepts_matching_input() {
        let result = confirm_passphrase("same".to_string(), "same".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn confirm_passphrase_rejects_mismatched_input() {
        let err = confirm_passphrase("first".to_string(), "second".to_string())
            .expect_err("不一致は拒否されるはず");
        assert!(matches!(err, CliError::PassphraseMismatch));
    }

    #[test]
    fn importing_a_missing_file_fails_cleanly() {
        let (_dir, mut store) = temp_store();

        let err = import_with_passphrase(&mut store, Path::new("no/such/file.agemask"), passphrase("pw"))
            .expect_err("存在しないファイルは失敗するはず");

        assert!(matches!(err, CliError::IoAt { .. }));
    }
}
