//! The commands the shipped `symphony` answers to: the Symphony's own ([`OWN`]), `serve` last, with
//! the commands the plugins this process holds add before it. A name it does not have is refused
//! with the nearest of its own; `--help`, `-h` and `help` list them all, each with one line on what
//! it does. The commands only this repository's own build and release run use are
//! `symphony-build`'s, a package of their own (crates/symphony-build) that no project is given
//! (`init::BUILDS` does not name it), so this library compiles none of them.

use std::io::Write;

mod attach;
mod shot;

/// The Symphony's own commands, in the order its usage names them, each with what it does.
const OWN: [(&str, &str); 17] = [
    ("status", "the review board's open cards and comments"),
    (
        "reply",
        "an agent's line on a card or comment: reply PATH SLUG TEXT",
    ),
    (
        "ask",
        "the owner's asks: list, show ID, answer ID TEXT --as HANDLE",
    ),
    (
        "triage",
        "the owner's verdict statistics, or the queue they see first",
    ),
    (
        "render-card",
        "draw one card as JSON: render-card PATH SLUG",
    ),
    (
        "landing-checks",
        "a branch's document and regeneration checks as JSON",
    ),
    (
        "verdict-asks",
        "turn the open verdict tasks into asks, once (main checkout)",
    ),
    ("docs-audit", "check that every document renders"),
    (
        "settings-check",
        "check the project file's shape: known keys, typed values",
    ),
    (
        "model",
        "set a role's model in the main checkout: model ROLE MODEL",
    ),
    (
        "blocks",
        "count the typed blocks every document parses into",
    ),
    ("shot", "a picture of a page: shot URL --out FILE.png"),
    (
        "attach",
        "store evidence in the main checkout's uploads: attach FILE...",
    ),
    (
        "init",
        "install or refresh the agents' skills, recipes and hooks here",
    ),
    (
        "project",
        "register this project with the machine's server (project add)",
    ),
    ("hook", "the Claude Code hooks: tasks-beat and memory-guard"),
    ("serve", "answer JSON requests on stdin, one a line"),
];

/// The usage of the Symphony's commands that read no options of their own, and what their help says
/// after their line: what `--help` or `-h` before `--` prints ([`plain_help`]).
const PLAIN: [(&str, &str, &str); 5] = [
    (
        "model",
        "usage: symphony model ROLE MODEL",
        "ROLE is orchestrator, executor, reviewer, assessor, or all (all four roles).\n\
         MODEL is any string; use -- before a value beginning with a dash.\n\
         Comments and order are preserved. The main checkout's symphony.toml is\n\
         edited, including from a linked worktree. Checking and landing the edit\n\
         follow the project's settings-only policy. Resumed agents keep their model.\n",
    ),
    (
        "settings-check",
        "usage: symphony settings-check [DIR]",
        "DIR is the project (default: the current folder). It reads symphony.toml as\n\
         the Symphony does and refuses, naming each on stderr (exit 1), a table or key\n\
         the Symphony does not know, a value of the wrong type and a required value left\n\
         out. It judges no name against a list: a model is any text.\n",
    ),
    ("project", "usage: symphony project add [DIR]", ""),
    (
        "hook",
        "usage: symphony hook {tasks-beat,memory-guard} < EVENT",
        "Each reads one Claude Code tool event on stdin; memory-guard refuses a write\n\
         into an agent's memory folder (exit 2).\n",
    ),
    (
        "serve",
        "usage: symphony serve < REQUESTS",
        "A request is {\"fn\": NAME, ...its arguments}, answered {\"ok\": VALUE} or\n\
         {\"error\": KIND, \"message\": TEXT}, until the input ends.\n",
    ),
];

/// The binary, as its listing, its help and its refusals name it.
pub(super) struct Binary {
    /// Its name, as its usage and its refusals say it.
    name: &'static str,
    /// What its listing says before the commands: its usage, how it is run and what it is.
    head: &'static str,
    /// What an unknown command's refusal says lists them.
    listed: &'static str,
    /// Its own commands, in the order its usage names them; the last stays last, after the ones
    /// the plugins add.
    own: &'static [(&'static str, &'static str)],
    /// The usage of its commands that read no options of their own ([`plain_help`]).
    plain: &'static [(&'static str, &'static str, &'static str)],
}

/// The shipped `symphony` (`just symphony-cli`).
pub(super) const SYMPHONY: Binary = Binary {
    name: "symphony",
    head: "usage: symphony COMMAND [ARGS]... (`just symphony-cli COMMAND`)\n\n\
           The Symphony's document CLI. `just symphony-cli help COMMAND`, or\n\
           `just symphony-cli COMMAND --help`, shows a command's arguments and options;\n\
           `symphony --version` says its version.\n\n",
    listed: "`just symphony-cli help`",
    own: &OWN,
    plain: &PLAIN,
};

/// `name ARGS`'s help when `name` is a command of `binary` that reads no options and `args` ask
/// for it.
pub(super) fn plain_help(binary: &Binary, name: &str, args: &[String]) -> Option<String> {
    let (_, usage, more) = binary.plain.iter().find(|(plain, _, _)| *plain == name)?;
    if !args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "-h" || arg == "--help")
    {
        return None;
    }
    let said = binary
        .own
        .iter()
        .find(|(own, _)| *own == name)
        .map(|(_, said)| *said)?;
    let more = if more.is_empty() {
        String::new()
    } else {
        format!("\n{more}")
    };
    Some(format!("{usage}\n\n{said}.\n{more}"))
}

/// `name`, a command the Symphony's `symphony` does not have of its own, run by the first plugin
/// that adds it with `args`; a name no plugin adds is refused, naming the nearest, exit 2. While a
/// plugin of the project answering from its process did not declare, its commands are not known:
/// the name is refused naming each such plugin, why and its log, exit 1, as the command may be
/// one of its. Its exit code.
pub(super) fn plugin(name: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if name == "init" {
        return crate::init::main(args, out, err);
    }
    if name == "shot" {
        return shot::main(args, out, err);
    }
    if name == "attach" {
        return attach::main(args, out, err);
    }
    // Measured as the project the command runs in says, the nearest above the current folder.
    match crate::plugins::command(name, here().as_deref()) {
        Ok(command) => command.run(args, out, err),
        Err(failed) => refuse_unknown(&SYMPHONY, name, &failed, err),
    }
}

/// The refusal of `name`, no command of `binary`'s, on `err`: first each plugin whose process
/// failed (`failed`, [`crate::plugins::Commands::failed`]), as the command may be one of its, exit
/// 1; then the nearest commands ([`unknown`]), exit 2 when no process failed.
fn refuse_unknown(binary: &Binary, name: &str, failed: &[String], err: &mut dyn Write) -> i32 {
    for line in failed {
        let _ = writeln!(err, "{}: {line}", binary.name);
    }
    let _ = writeln!(err, "{}", unknown(binary, name));
    if failed.is_empty() { 2 } else { 1 }
}

/// The project a command runs in: the nearest above the current folder.
fn here() -> Option<std::path::PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|here| crate::project::Project::find(&here))
}

/// The refusal of `name`, a command `binary` does not have: the nearest ones, and where they are
/// all listed.
fn unknown(binary: &Binary, name: &str) -> String {
    // `help` is read before the commands, and a mistyped flag in a command's place is nearest the
    // one flag the binary reads there.
    let mut names = names(binary);
    names.push(
        if name.starts_with('-') {
            "--help"
        } else {
            "help"
        }
        .into(),
    );
    let near: Vec<String> = symphony_args::nearest(name, names.iter().map(String::as_str))
        .iter()
        .map(|near| format!("{near:?}"))
        .collect();
    let near = match near.as_slice() {
        [] => String::new(),
        [one] => format!("; did you mean {one}?"),
        [rest @ .., last] => format!("; did you mean {} or {last}?", rest.join(", ")),
    };
    format!(
        "{}: unknown command {name:?}{near} ({} lists them)",
        binary.name, binary.listed
    )
}

/// `BINARY help [COMMAND]`, `--help`, `-h`: every command with what it does, or `COMMAND --help`.
/// Its exit code.
pub(super) fn help(
    binary: &Binary,
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            let _ = write!(out, "{}", listing(binary));
            0
        }
        Some(name) => {
            let (every, failed) = every(binary);
            if every.iter().any(|(known, _)| known == name) {
                super::main(
                    &[name.to_string(), "--help".into()],
                    &mut &b""[..],
                    out,
                    err,
                )
            } else {
                refuse_unknown(binary, name, &failed, err)
            }
        }
    }
}

/// A command line naming no command: every command, on stderr. Exit 2.
pub(super) fn usage(binary: &Binary, err: &mut dyn Write) -> i32 {
    let _ = write!(err, "{}", listing(binary));
    2
}

/// Every command `binary` runs, with what it does, in the order its usage names them; and a line
/// for each plugin of the project whose process did not declare, so whose commands are not among
/// them ([`crate::plugins::Commands::failed`]).
fn every(binary: &Binary) -> (Vec<(String, String)>, Vec<String>) {
    let line = |(name, said): &(&str, &str)| ((*name).to_string(), (*said).to_string());
    let Some((last, own)) = binary.own.split_last() else {
        return (Vec::new(), Vec::new());
    };
    // The plugins' commands of the project the binary runs in, as [`plugin`] runs them.
    let plugins = crate::plugins::commands_of(here().as_deref());
    let listed = own
        .iter()
        .map(line)
        .chain(plugins.listed.into_iter().map(|command| {
            let said = command.summary();
            let said = if said.is_empty() {
                "a command of the project's plugins"
            } else {
                said
            };
            (command.name().to_string(), said.to_string())
        }))
        .chain([line(last)])
        .collect();
    (listed, plugins.failed)
}

/// Every command name of `binary`, in the order its usage names them.
fn names(binary: &Binary) -> Vec<String> {
    every(binary).0.into_iter().map(|(name, _)| name).collect()
}

/// `binary`'s usage and every command with what it does: what its `help` prints, and after the
/// commands each plugin whose process failed, as an unknown command says it ([`plugin`]).
fn listing(binary: &Binary) -> String {
    let (every, failed) = every(binary);
    let width = every.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let mut text = format!("{}commands:\n", binary.head);
    for (name, said) in &every {
        text.push_str(&format!("  {name:<width$}  {said}\n"));
    }
    for line in &failed {
        text.push_str(&format!("\n{}: {line}\n", binary.name));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_names_every_command_once_with_its_line_within_80_columns() {
        let listing = listing(&SYMPHONY);
        assert!(listing.starts_with("usage: symphony COMMAND "), "{listing}");
        for (name, said) in SYMPHONY.own {
            let rows: Vec<&str> = listing
                .lines()
                .filter(|line| line.split_whitespace().next() == Some(name))
                .collect();
            assert_eq!(rows.len(), 1, "{name}: {listing}");
            assert!(rows[0].ends_with(said), "{name}: {}", rows[0]);
        }
        let (last, _) = SYMPHONY.own.last().unwrap();
        let end = listing.lines().last().unwrap();
        assert!(end.starts_with(&format!("  {last} ")), "{listing}");
        for line in listing.lines() {
            assert!(line.chars().count() <= 80, "{line}");
        }
        assert!(listing.contains("\n  project "));
    }

    #[test]
    fn version_alone_says_the_name_and_the_packages_version_and_the_listing_names_it() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::cli::main(&["--version".into()], &mut &b""[..], &mut out, &mut err);
        assert_eq!(
            (code, String::from_utf8(out).unwrap(), err),
            (
                0,
                format!("symphony {}\n", env!("CARGO_PKG_VERSION")),
                Vec::new()
            )
        );
        assert!(listing(&SYMPHONY).contains("`symphony --version`"));
        // Beside a command it is that command's word, never the version.
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let args = ["status".into(), "--version".into()];
        assert_eq!(
            crate::cli::main(&args, &mut &b""[..], &mut out, &mut err),
            2
        );
        assert!(out.is_empty());
    }

    #[test]
    fn each_command_that_reads_no_options_is_its_own_and_says_its_usage() {
        for (name, usage, _) in SYMPHONY.plain {
            assert!(SYMPHONY.own.iter().any(|(own, _)| own == name), "{name}");
            assert!(
                usage.starts_with(&format!("usage: symphony {name}")),
                "{usage}"
            );
        }
    }

    #[test]
    fn plain_help_flags_after_the_argument_delimiter_are_literal_values() {
        for (name, usage, _) in SYMPHONY.plain {
            for flag in ["--help", "-h"] {
                for args in [vec![flag], vec!["role", flag], vec![flag, "--", "value"]] {
                    let args: Vec<String> = args.into_iter().map(str::to_string).collect();
                    assert!(
                        plain_help(&SYMPHONY, name, &args)
                            .unwrap()
                            .starts_with(usage)
                    );
                }
                for args in [vec!["--", flag], vec!["role", "--", flag]] {
                    let args: Vec<String> = args.into_iter().map(str::to_string).collect();
                    assert_eq!(plain_help(&SYMPHONY, name, &args), None);
                }
            }
        }
    }

    #[test]
    fn an_unknown_command_names_the_nearest() {
        let shipped = |name, near: &str| {
            format!(
                "symphony: unknown command {name:?}{near} (`just symphony-cli help` lists them)"
            )
        };
        for (name, said) in [
            ("statsu", shipped("statsu", "; did you mean \"status\"?")),
            (
                "docs-audt",
                shipped("docs-audt", "; did you mean \"docs-audit\"?"),
            ),
            ("frobnicate", shipped("frobnicate", "")),
            ("--hlep", shipped("--hlep", "; did you mean \"--help\"?")),
            ("-help", shipped("-help", "; did you mean \"--help\"?")),
            // A command's name is answered with a command, never a flag.
            ("hlep", shipped("hlep", "; did you mean \"help\"?")),
            // The build's commands are `symphony-build`'s (crates/symphony-build), not this one's.
            ("size-budget", shipped("size-budget", "")),
            ("release", shipped("release", "")),
        ] {
            assert_eq!(unknown(&SYMPHONY, name), said);
        }
    }
}
