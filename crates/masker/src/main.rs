use clap::Parser;
use masker::cli::Cli;
use masker::error::CliError;

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("エラー: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    let paths = resolve_paths()?;
    masker::dispatch(cli, &paths)
}

/// SENSITIVEMASKER_DATA_DIRが設定されていればそこを、無ければOS標準のデータ
/// ディレクトリを使う。gui/src-tauri/src/profiles.rsのresolve_paths_with_overrideと
/// 同じ理由・同じ変数名(E2Eテストが実ユーザーの鍵/DBを書き換えずに、GUIと同じ
/// 一時ディレクトリへ向けてmasker CLIを外部プロセスとして起動できるようにするため)。
/// release buildではこの分岐自体が存在しない。
#[cfg(debug_assertions)]
fn resolve_paths() -> Result<profile_store::AppPaths, profile_store::PathError> {
    match std::env::var("SENSITIVEMASKER_DATA_DIR").ok() {
        Some(dir) => Ok(profile_store::AppPaths::at(dir)),
        None => profile_store::AppPaths::resolve(),
    }
}

#[cfg(not(debug_assertions))]
fn resolve_paths() -> Result<profile_store::AppPaths, profile_store::PathError> {
    profile_store::AppPaths::resolve()
}
