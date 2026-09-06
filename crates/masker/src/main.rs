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
    let paths = profile_store::AppPaths::resolve()?;
    masker::dispatch(cli, &paths)
}
