//! `masker profile`サブコマンドの実行ロジック。

use std::path::Path;

use masking_core::{Rule, RuleProfile};
use profile_store::ProfileStore;

use crate::cli::ProfileAction;
use crate::error::CliError;

pub(crate) fn run(action: &ProfileAction, store: &mut ProfileStore) -> Result<(), CliError> {
    match action {
        ProfileAction::List => list(store),
        ProfileAction::Use { name } => use_profile(store, name),
        ProfileAction::Create { name, from_json } => create(store, name, from_json.as_deref()),
        ProfileAction::Delete { name } => delete(store, name),
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

    // Vec<Rule>への型付きデシリアライズは各ルールの正規表現を実際にコンパイルする
    // (Rule::new経由)。それより前に、serde_json::Valueとして構造的に(regexへは
    // 一切触れずに)件数だけを検査する(SMX-4対応: profile-store側のpreview_importと
    // 同じ考え方を、外部からルール一括投入を受け付けるこの経路にも適用する)。
    if let Ok(serde_json::Value::Array(rules)) = serde_json::from_str::<serde_json::Value>(&text) {
        if rules.len() > profile_store::MAX_RULES_PER_PROFILE {
            return Err(CliError::TooManyRulesInJson {
                path: path.to_path_buf(),
                count: rules.len(),
                limit: profile_store::MAX_RULES_PER_PROFILE,
            });
        }
    }

    serde_json::from_str(&text).map_err(|source| CliError::Json { path: path.to_path_buf(), source })
}

fn delete(store: &mut ProfileStore, name: &str) -> Result<(), CliError> {
    store.delete_profile(name)?;
    println!("プロファイル '{name}' を削除しました");
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
    fn create_with_too_many_rules_in_json_creates_nothing() {
        let (_dir, mut store) = temp_store();
        let json_dir = tempfile::tempdir().unwrap();
        let json_path = json_dir.path().join("rules.json");
        let rules: Vec<serde_json::Value> = (0..=profile_store::MAX_RULES_PER_PROFILE)
            .map(|i| {
                serde_json::json!({
                    "name": format!("r{i}"),
                    "pattern_type": "literal",
                    "pattern": format!("v{i}"),
                    "mode": "fixed",
                    "fixed_value": "masked",
                })
            })
            .collect();
        std::fs::write(&json_path, serde_json::to_string(&rules).unwrap()).unwrap();

        let err = create(&mut store, "work", Some(&json_path)).expect_err("ルール数上限超過は拒否されるはず");
        assert!(matches!(err, CliError::TooManyRulesInJson { .. }));
        assert!(store.get_profile("work").is_err(), "上限超過時はプロファイルが作成されてはいけない");
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
}
