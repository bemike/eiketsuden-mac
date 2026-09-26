//! Command line parsing (no dependencies: the grammar is a handful of subcommands and options).

use hero_import::edition::EditionId;
use hero_import::extract::Selection;
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
        does not finish or cannot be set up.

    hero-tools info <pack_dir>
        Print a summary of the pack's content.

    hero-tools original probe <install_dir> [--out <manifest.json>]
        EXPERIMENTAL. Identify which release of the original game (that you own) a folder
        holds and, with --out, write a shareable manifest: file names, sizes, SHA-256
        hashes, the first 16 bytes of each file and container summaries, no game content.
        The folder is only read. See docs/ORIGINAL_DATA.md.

    hero-tools original extract <install_dir> --out <dir> [--text] [--sprites] [--portraits]
                                [--edition korean-dos|chinese-dos]
        EXPERIMENTAL. Convert the original files into a media overlay folder (PNG + UTF-8
        JSON + index.json) for `eiketsuden --original <dir>`. Without a kind option every
        kind is attempted and unsupported ones are only reported; a kind chosen explicitly
        that cannot be extracted fails. --edition skips identification. The output folder
        must be new, empty or a previous extraction, and outside the install. Exits with 1
        when any kind failed.

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
    OriginalProbe {
        dir: PathBuf,
        out: Option<PathBuf>,
    },
    OriginalExtract {
        dir: PathBuf,
        out: PathBuf,
        /// `None` when no kind option was given.
        selection: Option<Selection>,
        edition: Option<EditionId>,
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
        "original" => parse_original(rest),
        other => Err(format!("unknown command `{other}`")),
    }
}

/// Split `--name=value` into `(--name, Some(value))`.
fn split_inline(arg: &str) -> (&str, Option<String>) {
    match arg.split_once('=') {
        Some((n, v)) if arg.starts_with("--") => (n, Some(v.to_string())),
        _ => (arg, None),
    }
}

fn parse_original(rest: &[String]) -> Result<Command, String> {
    let Some((sub, rest)) = rest.split_first() else {
        return Err("`original` needs a subcommand: probe or extract".into());
    };
    let extract = match sub.as_str() {
        "probe" => false,
        "extract" => true,
        other => return Err(format!("unknown `original` subcommand `{other}`")),
    };
    let command = format!("original {sub}");
    let mut dir = None;
    let mut out = None;
    let mut selection: Option<Selection> = None;
    let mut edition = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let (name, inline) = split_inline(arg);
        let mut value = |what: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("`{name}` needs {what}"))
        };
        let mut select = |f: fn(&mut Selection)| {
            f(selection.get_or_insert_with(Selection::default));
        };
        match name {
            "--out" => out = Some(PathBuf::from(value("a path")?)),
            "--text" if extract => select(|s| s.text = true),
            "--sprites" if extract => select(|s| s.sprites = true),
            "--portraits" if extract => select(|s| s.portraits = true),
            "--edition" if extract => {
                let v = value("an edition id")?;
                edition = match EditionId::parse(&v) {
                    Some(id) if id.is_extractable() => Some(id),
                    _ => {
                        return Err(format!(
                            "`--edition` must be korean-dos or chinese-dos, got `{v}`"
                        ))
                    }
                };
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}` for `{command}`"))
            }
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => return Err(format!("`{command}` takes exactly one install directory")),
        }
    }
    let dir = dir.ok_or_else(|| format!("`{command}` needs an install directory"))?;
    if extract {
        Ok(Command::OriginalExtract {
            dir,
            out: out
                .ok_or("`original extract` needs `--out <dir>` (a folder outside the install)")?,
            selection,
            edition,
        })
    } else {
        Ok(Command::OriginalProbe { dir, out })
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
    fn original_commands() {
        assert_eq!(
            parse_str(&["original", "probe", "D:/Games/GAME"]),
            Ok(Command::OriginalProbe {
                dir: "D:/Games/GAME".into(),
                out: None
            })
        );
        assert_eq!(
            parse_str(&["original", "probe", "g", "--out=m.json"]),
            Ok(Command::OriginalProbe {
                dir: "g".into(),
                out: Some("m.json".into())
            })
        );
        assert_eq!(
            parse_str(&["original", "extract", "g", "--out", "o"]),
            Ok(Command::OriginalExtract {
                dir: "g".into(),
                out: "o".into(),
                selection: None,
                edition: None
            })
        );
        assert_eq!(
            parse_str(&[
                "original",
                "extract",
                "--text",
                "g",
                "--portraits",
                "--out",
                "o",
                "--edition=chinese-dos"
            ]),
            Ok(Command::OriginalExtract {
                dir: "g".into(),
                out: "o".into(),
                selection: Some(Selection {
                    text: true,
                    portraits: true,
                    sprites: false
                }),
                edition: Some(EditionId::ChineseDos)
            })
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
            (&["original"][..], "needs a subcommand"),
            (
                &["original", "convert", "g"][..],
                "unknown `original` subcommand",
            ),
            (&["original", "probe"][..], "needs an install directory"),
            (&["original", "probe", "a", "b"][..], "exactly one install"),
            (
                &["original", "probe", "g", "--text"][..],
                "unknown option `--text` for `original probe`",
            ),
            (
                &["original", "probe", "g", "--out"][..],
                "`--out` needs a path",
            ),
            (&["original", "extract", "g"][..], "needs `--out <dir>`"),
            (
                &[
                    "original",
                    "extract",
                    "g",
                    "--out",
                    "o",
                    "--edition",
                    "steam-2017",
                ][..],
                "must be korean-dos or chinese-dos",
            ),
        ] {
            let err = parse_str(args).unwrap_err();
            assert!(err.contains(fragment), "{args:?}: {err}");
        }
    }
}
