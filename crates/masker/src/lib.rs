//! masker CLIの本体ロジック(Imperative Shell)。main.rsから薄く呼ばれる。

pub mod cli;
mod commands;
pub mod error;

use cli::{Cli, Command};
use error::CliError;
use profile_store::{AppPaths, ProfileStore};

pub fn dispatch(cli: Cli, paths: &AppPaths) -> Result<(), CliError> {
    match cli.command {
        Command::Init => commands::init::run(paths),
        Command::Mask(args) => {
            let store = ProfileStore::open_at(paths)?;
            commands::mask::run(&args, &store)
        }
        Command::Profile { action } => {
            let mut store = ProfileStore::open_at(paths)?;
            commands::profile::run(&action, &mut store)
        }
        Command::Export { profile, output } => {
            let store = ProfileStore::open_at(paths)?;
            commands::export::export(&store, profile.as_deref(), &output)
        }
        Command::Import { input, yes } => {
            let mut store = ProfileStore::open_at(paths)?;
            commands::export::import(&mut store, &input, yes)
        }
    }
}
