//! `masker init`の実行ロジック。

use profile_store::AppPaths;

use crate::error::CliError;

pub(crate) fn run(paths: &AppPaths) -> Result<(), CliError> {
    if profile_store::is_initialized_at(paths)? {
        println!("既に初期化済みです");
        return Ok(());
    }
    profile_store::init_at(paths)?;
    println!("初期化しました");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_initializes_key_and_db() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path());

        run(&paths).unwrap();

        assert!(profile_store::is_initialized_at(&paths).unwrap());
    }

    #[test]
    fn second_run_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path());

        run(&paths).unwrap();
        run(&paths).unwrap();

        assert!(profile_store::is_initialized_at(&paths).unwrap());
    }
}
