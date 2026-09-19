//! `masker export`/`masker import`の実行ロジック。

use std::io::{IsTerminal, Write};
use std::path::Path;

use profile_store::{AllImportEntry, AppPaths, ExposeSecret, ImportOutcome, ImportPreview, ProfileStore, SecretString};

use crate::error::CliError;
use crate::paths::validate_output_file;

pub(crate) fn export(
    store: &ProfileStore,
    profile: Option<&str>,
    output: &Path,
    app_paths: &AppPaths,
) -> Result<(), CliError> {
    // 存在確認を先に行う(存在しない名前に対して無駄にパスフレーズ入力させない)。
    if let Some(name) = profile {
        store.get_profile(name)?;
    }
    let passphrase = prompt_new_passphrase()?;
    export_with_passphrase(store, profile, output, passphrase, app_paths)
}

// TTY読み取り(rpassword)をテスト対象から分離するための本体。
fn export_with_passphrase(
    store: &ProfileStore,
    profile: Option<&str>,
    output: &Path,
    passphrase: SecretString,
    app_paths: &AppPaths,
) -> Result<(), CliError> {
    // 出力先がアプリのデータフォルダ(暗号化DB・鍵ファイルの保存場所)の内側でないことを、
    // 実際の暗号化処理より前に確認する(誤ってprofiles.db等を上書きするのを防ぐ。
    // GUIの保存ダイアログ経由のエクスポートと同じ理由)。
    let validated_output = validate_output_file(output, app_paths)?;
    let encrypted = match profile {
        Some(name) => store.export_profile(name, passphrase)?,
        None => store.export_all(passphrase)?,
    };
    std::fs::write(&validated_output, &encrypted)
        .map_err(|source| CliError::IoAt { path: output.to_path_buf(), source })?;
    match profile {
        Some(name) => println!("プロファイル '{name}' を '{}' にエクスポートしました", output.display()),
        None => println!("全プロファイルを '{}' にエクスポートしました", output.display()),
    }
    Ok(())
}

// タイプミス対策として2回入力させ、一致しない場合はエクスポートしない(age -pと同じ挙動)。
// 弱いパスフレーズの場合は警告し、続行するか再入力するかを確認する(強制はしない)。
fn prompt_new_passphrase() -> Result<SecretString, CliError> {
    loop {
        let first = rpassword::prompt_password("エクスポート用パスフレーズ: ")?;
        let second = rpassword::prompt_password("パスフレーズ(確認): ")?;
        let passphrase = confirm_passphrase(first, second)?;

        match weak_passphrase_warning(passphrase.expose_secret()) {
            None => return Ok(passphrase),
            Some(warning) => {
                eprintln!("{warning}");
                if prompt_yes_no("このまま続行しますか? (y/N): ")? {
                    return Ok(passphrase);
                }
                eprintln!("パスフレーズを再入力してください");
            }
        }
    }
}

// 比較ロジックのみを切り出し、TTY読み取り無しでテストできるようにする。
fn confirm_passphrase(first: String, second: String) -> Result<SecretString, CliError> {
    if first != second {
        return Err(CliError::PassphraseMismatch);
    }
    Ok(SecretString::from(first))
}

// zxcvbnはスコア3未満(0-4の5段階)を「弱い」の目安として明記している。文字種の組み合わせを
// 強制する方式(cracklib的な文字種クオータ)より新しい経験的モデル。個々の警告理由(Warning/
// Suggestion)はzxcvbn側が英語専用のため翻訳せず、汎用の日本語メッセージのみ表示する。
fn weak_passphrase_warning(passphrase: &str) -> Option<String> {
    let entropy = zxcvbn::zxcvbn(passphrase, &[]);
    if entropy.score() >= zxcvbn::Score::Three {
        return None;
    }
    Some(format!(
        "警告: このパスフレーズは推測されやすい可能性があります(強度: {}/4、推奨: 3以上)",
        entropy.score()
    ))
}

fn prompt_yes_no(prompt: &str) -> Result<bool, CliError> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES"))
}

pub(crate) fn import(store: &mut ProfileStore, input: &Path, yes: bool) -> Result<(), CliError> {
    // ファイルの存在確認を先に行う(存在しないパスに対して無駄にパスフレーズ入力させない)。
    std::fs::read(input).map_err(|source| CliError::IoAt { path: input.to_path_buf(), source })?;
    let passphrase = SecretString::from(rpassword::prompt_password("インポート用パスフレーズ: ")?);
    import_with_passphrase(store, input, passphrase, yes, std::io::stdin().is_terminal())
}

// TTY読み取り(rpassword)と、標準入力が端末かの判定をテスト対象から分離するための本体。端末かどうかは、
// テストの起動方法(端末から実行するか)に左右されるため、引数で受け取る。ただし全体インポートで`yes`が
// falseかつ端末の場合のみ、確認のためのプレーンな標準入力読み取りが残る。
fn import_with_passphrase(
    store: &mut ProfileStore,
    input: &Path,
    passphrase: SecretString,
    yes: bool,
    stdin_is_terminal: bool,
) -> Result<(), CliError> {
    let data = std::fs::read(input).map_err(|source| CliError::IoAt { path: input.to_path_buf(), source })?;
    let preview = store.preview_import(&data, passphrase)?;

    if let ImportPreview::All { entries, .. } = &preview {
        print_all_import_plan(entries);
        if !yes {
            if !stdin_is_terminal {
                return Err(CliError::NonInteractiveImportNeedsYesFlag);
            }
            if !prompt_yes_no("続行しますか? (y/N): ")? {
                println!("インポートを中止しました");
                return Ok(());
            }
        }
    }

    match store.commit_import(preview)? {
        ImportOutcome::Single { name, activated } => {
            println!("プロファイル '{name}' をインポートしました");
            if activated {
                println!("(アクティブなプロファイルが未設定だったため、'{name}' をアクティブにしました)");
            }
        }
        ImportOutcome::All { entries, activated_profile_name } => {
            println!("{}件のプロファイルをインポートしました", entries.len());
            if let Some(name) = activated_profile_name {
                println!("(アクティブなプロファイルが未設定だったため、'{name}' をアクティブにしました)");
            }
        }
    }
    Ok(())
}

fn print_all_import_plan(entries: &[AllImportEntry]) {
    println!("インポート内容:");
    for entry in entries {
        if entry.renamed {
            println!("  {} → '{}' として作成(重複のためリネーム)", entry.original_name, entry.resolved_name);
        } else {
            println!("  {} → そのまま作成", entry.original_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use profile_store::AppPaths;

    use super::*;

    // 戻り値のAppPathsは、export_with_passphraseの検証(出力先がアプリのデータ
    // フォルダの内側でないこと)にそのまま使う。このストア自身の実データフォルダを
    // 「保護すべきアプリのデータフォルダ」として使うのが最も実運用に近い。
    fn temp_store() -> (tempfile::TempDir, AppPaths, ProfileStore) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path());
        profile_store::init_at(&paths).unwrap();
        let store = ProfileStore::open_at(&paths).unwrap();
        (dir, paths, store)
    }

    fn create(store: &mut ProfileStore, name: &str) {
        store.create_profile(&masking_core::RuleProfile::new(name, None, Vec::new()).unwrap()).unwrap();
    }

    fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn single_export_then_import_round_trips_into_a_different_store() {
        let (_dir_a, app_paths_a, mut store_a) = temp_store();
        create(&mut store_a, "work");
        let export_dir = tempfile::tempdir().unwrap();
        let export_path = export_dir.path().join("work.agemask");

        export_with_passphrase(&store_a, Some("work"), &export_path, passphrase("pw"), &app_paths_a).unwrap();

        let (_dir_b, _app_paths_b, mut store_b) = temp_store();
        import_with_passphrase(&mut store_b, &export_path, passphrase("pw"), true, false).unwrap();

        assert_eq!(store_b.get_profile("work").unwrap().profile_name(), "work");
    }

    #[test]
    fn all_export_then_import_round_trips_multiple_profiles() {
        let (_dir_a, app_paths_a, mut store_a) = temp_store();
        create(&mut store_a, "work");
        create(&mut store_a, "personal");
        let export_dir = tempfile::tempdir().unwrap();
        let export_path = export_dir.path().join("all.agemask");

        export_with_passphrase(&store_a, None, &export_path, passphrase("pw"), &app_paths_a).unwrap();

        let (_dir_b, _app_paths_b, mut store_b) = temp_store();
        import_with_passphrase(&mut store_b, &export_path, passphrase("pw"), true, false).unwrap();

        let names: Vec<String> = store_b.list_profiles().unwrap().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"work".to_string()));
        assert!(names.contains(&"personal".to_string()));
    }

    #[test]
    fn importing_with_the_wrong_passphrase_fails_cleanly() {
        let (_dir_a, app_paths_a, mut store_a) = temp_store();
        create(&mut store_a, "work");
        let export_dir = tempfile::tempdir().unwrap();
        let export_path = export_dir.path().join("work.agemask");
        export_with_passphrase(&store_a, Some("work"), &export_path, passphrase("correct"), &app_paths_a).unwrap();

        let (_dir_b, _app_paths_b, mut store_b) = temp_store();
        let err = import_with_passphrase(&mut store_b, &export_path, passphrase("wrong"), true, false)
            .expect_err("誤ったパスフレーズは拒否されるはず");

        assert!(matches!(err, CliError::Store(profile_store::ProfileStoreError::Export(_))));
    }

    #[test]
    fn importing_a_missing_file_fails_cleanly() {
        let (_dir, _app_paths, mut store) = temp_store();

        let err = import_with_passphrase(&mut store, Path::new("no/such/file.agemask"), passphrase("pw"), true, false)
            .expect_err("存在しないファイルは失敗するはず");

        assert!(matches!(err, CliError::IoAt { .. }));
    }

    #[test]
    fn bulk_import_without_yes_fails_fast_instead_of_hanging_when_not_a_tty() {
        // 標準入力が端末でないことを引数で指定するため、テストの起動方法(端末から実行しても)に左右されない。
        let (_dir_a, app_paths_a, mut store_a) = temp_store();
        create(&mut store_a, "work");
        let export_dir = tempfile::tempdir().unwrap();
        let export_path = export_dir.path().join("all.agemask");
        export_with_passphrase(&store_a, None, &export_path, passphrase("pw"), &app_paths_a).unwrap();

        let (_dir_b, _app_paths_b, mut store_b) = temp_store();
        let err = import_with_passphrase(&mut store_b, &export_path, passphrase("pw"), false, false)
            .expect_err("非TTYかつ--yes無しでは即座に失敗するはず");

        assert!(matches!(err, CliError::NonInteractiveImportNeedsYesFlag));
        assert_eq!(store_b.list_profiles().unwrap().len(), 0, "確認前なので何も作成されないはず");
    }

    #[test]
    fn single_import_does_not_require_yes_flag() {
        // 単一インポートはpreview時点で衝突が確定するため、--yesの有無に関わらず
        // 対話的な確認そのものが発生しない。
        let (_dir_a, app_paths_a, mut store_a) = temp_store();
        create(&mut store_a, "work");
        let export_dir = tempfile::tempdir().unwrap();
        let export_path = export_dir.path().join("work.agemask");
        export_with_passphrase(&store_a, Some("work"), &export_path, passphrase("pw"), &app_paths_a).unwrap();

        let (_dir_b, _app_paths_b, mut store_b) = temp_store();
        import_with_passphrase(&mut store_b, &export_path, passphrase("pw"), false, false).unwrap();

        assert_eq!(store_b.get_profile("work").unwrap().profile_name(), "work");
    }

    #[test]
    fn export_rejects_an_output_path_inside_the_app_data_dir() {
        // CLIの--outputへの手入力で、誤ってこのストア自身のデータフォルダ内の
        // ファイル名(profiles.db等)を指定した場合に上書きしないことを固定する。
        let (dir, app_paths, mut store) = temp_store();
        create(&mut store, "work");
        let sneaky_output = dir.path().join("profiles.db");

        let err = export_with_passphrase(&store, Some("work"), &sneaky_output, passphrase("pw"), &app_paths)
            .expect_err("出力先がアプリのデータフォルダの場合は拒否されるはず");

        assert!(matches!(err, CliError::Path(_)));
        assert_eq!(
            store.get_profile("work").unwrap().profile_name(),
            "work",
            "検証を通過して実際にDBファイルが上書きされてしまった"
        );
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
    fn weak_passphrase_warning_flags_a_short_repeated_pattern() {
        let warning = weak_passphrase_warning("AAAAAA");
        assert!(warning.is_some(), "'AAAAAA'のような繰り返しパターンは弱いと判定されるはず");
    }

    #[test]
    fn weak_passphrase_warning_accepts_a_long_random_passphrase() {
        let warning = weak_passphrase_warning("xk3f-9pQ2-mVwZ-7Ltc-random");
        assert!(warning.is_none(), "十分に長くランダムなパスフレーズは警告されないはず");
    }
}
