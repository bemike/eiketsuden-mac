//! Command line parsing (no dependencies: the grammar is three subcommands and two options).

use std::path::PathBuf;

pub const USAGE: &str = "\
hero-tools — command line tools for Eiketsuden Reloaded data packs

USAGE:
    hero-tools validate <pack_dir>
        Load the pack, cross-check every reference, report unknown TOML keys and missing
        media files. Exits with 1 when there are errors.

    hero-tools simulate <pack_dir> [--seeds N] [--battle ID]
        Play battles AI against AI (the player side is run by the AI too) with N seeds each
        (default 4), at most 200 phases per run. Reports win rates and average turns, warns
        about battles that are never or always won, and exits with 1 when a battle panics,
        does not finish or cannot be set up. A --battle ID the pack does not have is a
        command line error (exit 2).

    hero-tools info <pack_dir>
        Print a summary of the pack's content.

    hero-tools help | --help | -h
    hero-tools --version";

/// Default number of seeds per battle for `simulate`.
pub const DEFAULT_SEEDS: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Validate {
        pack: PathBuf,
    },
    Simulate {
        pack: PathBuf,
        seeds: u32,
        battle: Option<String>,
    },
    Info {
        pack: PathBuf,
    },
    Help,
    Version,
}

/// Parse the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((command, rest)) = args.split_first() else {
        return Err("no command given".into());
    };
    match command.as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "--version" | "-V" => Ok(Command::Version),
        "validate" => Ok(Command::Validate {
            pack: only_pack(command, rest)?,
        }),
        "info" => Ok(Command::Info {
            pack: only_pack(command, rest)?,
        }),
        "simulate" => parse_simulate(rest),
        other => Err(format!("unknown command `{other}`")),
    }
}

fn only_pack(command: &str, rest: &[String]) -> Result<PathBuf, String> {
    match rest {
        [pack] if !pack.starts_with('-') => Ok(PathBuf::from(pack)),
        [] => Err(format!("`{command}` needs a pack directory")),
        [flag, ..] if flag.starts_with('-') => {
            Err(format!("unknown option `{flag}` for `{command}`"))
        }
        _ => Err(format!("`{command}` takes exactly one pack directory")),
    }
}

fn parse_simulate(rest: &[String]) -> Result<Command, String> {
    let mut pack = None;
    let mut seeds = DEFAULT_SEEDS;
    let mut battle = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        // Accept both `--seeds 8` and `--seeds=8`.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if arg.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("`{name}` needs {what}"))
        };
        match name {
            "--seeds" => {
                let v = value("a number")?;
                seeds = match v.parse::<u32>() {
                    Ok(n) if n > 0 => n,
                    _ => return Err(format!("`--seeds` needs a positive number, got `{v}`")),
                };
            }
            "--battle" => battle = Some(value("a battle id")?),
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}` for `simulate`"))
            }
            _ if pack.is_none() => pack = Some(PathBuf::from(arg)),
            _ => return Err("`simulate` takes exactly one pack directory".into()),
        }
    }
    Ok(Command::Simulate {
        pack: pack.ok_or("`simulate` needs a pack directory")?,
        seeds,
        battle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(args: &[&str]) -> Result<Command, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&args)
    }

    #[test]
    fn subcommands() {
        assert_eq!(
            parse_str(&["validate", "data/base"]),
            Ok(Command::Validate {
                pack: "data/base".into()
            })
        );
        assert_eq!(
            parse_str(&["info", "data/base"]),
            Ok(Command::Info {
                pack: "data/base".into()
            })
        );
        assert_eq!(parse_str(&["--help"]), Ok(Command::Help));
        assert_eq!(parse_str(&["help"]), Ok(Command::Help));
        assert_eq!(parse_str(&["--version"]), Ok(Command::Version));
    }

    #[test]
    fn simulate_options() {
        assert_eq!(
            parse_str(&["simulate", "data/base"]),
            Ok(Command::Simulate {
                pack: "data/base".into(),
                seeds: DEFAULT_SEEDS,
                battle: None
            })
        );
        let expected = Ok(Command::Simulate {
            pack: "p".into(),
            seeds: 8,
            battle: Some("b03".into()),
        });
        assert_eq!(
            parse_str(&["simulate", "p", "--seeds", "8", "--battle", "b03"]),
            expected
        );
        assert_eq!(
            parse_str(&["simulate", "--battle=b03", "--seeds=8", "p"]),
            expected
        );
    }

    #[test]
    fn usage_errors() {
        for (args, fragment) in [
            (&[][..], "no command"),
            (&["frobnicate"][..], "unknown command"),
            (&["validate"][..], "needs a pack directory"),
            (&["validate", "a", "b"][..], "exactly one pack directory"),
            (&["validate", "--fast"][..], "unknown option `--fast`"),
            (&["simulate"][..], "needs a pack directory"),
            (&["simulate", "p", "q"][..], "exactly one pack directory"),
            (
                &["simulate", "p", "--seeds"][..],
                "`--seeds` needs a number",
            ),
            (&["simulate", "p", "--seeds", "0"][..], "positive number"),
            (&["simulate", "p", "--seeds", "many"][..], "positive number"),
            (
                &["simulate", "p", "--battle="][..],
                "`--battle` needs a battle id",
            ),
            (
                &["simulate", "p", "--turbo"][..],
                "unknown option `--turbo`",
            ),
        ] {
            let err = parse_str(args).unwrap_err();
            assert!(err.contains(fragment), "{args:?}: {err}");
        }
    }
}
