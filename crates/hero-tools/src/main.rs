//! `hero-tools`: command line tools for Eiketsuden Reloaded data packs (`validate`,
//! `simulate`, `info`). See `hero-tools --help` and `docs/MODDING.md`.

mod cli;
mod info;
mod simulate;
mod validate;

use cli::Command;
use hero_core::pack::{DirSource, Pack};
use std::path::Path;
use std::process::ExitCode;

/// Exit code for bad command lines (errors found in a pack exit with 1).
const USAGE_ERROR: u8 = 2;

/// Why a command could not do its job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The command line names something the pack does not have, such as `--battle` with an
    /// unknown id: a bad command line (exit 2), not a broken pack.
    Usage(String),
    /// The pack cannot be loaded or checked (exit 1).
    Failed(String),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&args) {
        Ok(command) => command,
        Err(msg) => {
            eprintln!("error: {msg}\n\n{}", cli::USAGE);
            return ExitCode::from(USAGE_ERROR);
        }
    };
    let result = match command {
        Command::Help => {
            println!("{}", cli::USAGE);
            Ok(true)
        }
        Command::Version => {
            println!("hero-tools {}", env!("CARGO_PKG_VERSION"));
            Ok(true)
        }
        Command::Validate { pack } => validate::run(&pack).map_err(Failure::Failed),
        Command::Simulate {
            pack,
            seeds,
            battle,
        } => simulate::run(&pack, seeds, battle.as_deref()),
        Command::Info { pack } => info::run(&pack).map_err(Failure::Failed),
    };
    exit_code(result)
}

/// Report a failure and map the outcome to the documented exit codes: 0 = success, 1 = the
/// pack has errors (or a simulation failed), 2 = bad command line.
fn exit_code(result: Result<bool, Failure>) -> ExitCode {
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(Failure::Usage(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::from(USAGE_ERROR)
        }
        Err(Failure::Failed(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// Load a pack directory, with the directory in the error message.
fn load_pack(dir: &Path) -> Result<Pack, String> {
    if !dir.join("pack.toml").is_file() {
        return Err(format!("{}: no pack.toml found", dir.display()));
    }
    Pack::load(&DirSource {
        root: dir.to_path_buf(),
    })
    .map_err(|e| format!("cannot load pack {}: {e}", dir.display()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The hero-core test fixture: a small pack without validation issues (and without media).
    pub fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../hero-core/tests/fixtures/mini")
    }

    pub fn fixture_pack() -> Pack {
        load_pack(&fixture_dir()).expect("fixture pack loads")
    }

    #[test]
    fn load_errors_name_the_directory() {
        let missing = fixture_dir().join("does-not-exist");
        let err = load_pack(&missing).unwrap_err();
        assert!(err.contains("no pack.toml found"), "{err}");
        assert!(err.contains("does-not-exist"), "{err}");
    }

    #[test]
    fn exit_codes_follow_the_documentation() {
        assert_eq!(exit_code(Ok(true)), ExitCode::SUCCESS);
        assert_eq!(exit_code(Ok(false)), ExitCode::FAILURE);
        assert_eq!(
            exit_code(Err(Failure::Failed("broken pack".into()))),
            ExitCode::FAILURE
        );
        assert_eq!(
            exit_code(Err(Failure::Usage("unknown battle".into()))),
            ExitCode::from(USAGE_ERROR)
        );
    }
}
