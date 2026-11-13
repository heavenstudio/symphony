//! The commands the binary answers to: the Symphony's own, `serve` last, with the commands the
//! plugins this process holds add before it. A name none of them has is refused with the nearest
//! of them; `--help`, `-h` and `help` list them all, each with one line on what it does.

use std::io::Write;

mod release_inputs;
mod shot;
mod size_budget;

/// The Symphony's own commands, in the order its usage names them, each with what it does.
const OWN: [(&str, &str); 16] = [
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
        "verdict-asks",
        "turn the open verdict tasks into asks, once (main checkout)",
    ),
    ("docs-audit", "check that every document renders"),
    (
        "blocks",
        "count the typed blocks every document parses into",
    ),
    ("shot", "a picture of a page: shot URL --out FILE.png"),
    (
        "init",
        "install or refresh the agents' skills, recipes and hooks here",
    ),
    (
        "project",
        "register this project with the machine's server (project add)",
    ),
    ("hook", "the Claude Code hooks: tasks-beat and memory-guard"),
    (
        "publish-client",
        "publish a built client into the server's embedded copy",
    ),
    (
        "release-inputs",
        "list the files the client's release build reads",
    ),
    (
        "size-budget",
        "check the shipped binaries' sizes against their budget",
    ),
    (
        "release",
        "verify main's tip in full and tag it a release when green",
    ),
    ("serve", "answer JSON requests on stdin, one a line"),
];

/// The usage of the commands that read no options of their own, and what their help says after
/// their line: what `--help` or `-h` anywhere on their command line prints ([`plain_help`]).
const PLAIN: [(&str, &str, &str); 6] = [
    ("project", "usage: symphony project add [DIR]", ""),
    (
        "hook",
        "usage: symphony hook {tasks-beat,memory-guard} < EVENT",
        "Each reads one Claude Code tool event on stdin; memory-guard refuses a write\n\
         into an agent's memory folder (exit 2).\n",
    ),
    (
        "publish-client",
        "usage: symphony publish-client STAGE PUBLIC",
        "",
    ),
    (
        "release-inputs",
        "usage: symphony release-inputs ROOT METADATA FILES COMPONENT_FILES --key|LOCK",
        "",
    ),
    (
        "size-budget",
        "usage: symphony size-budget ROOT RELEASE",
        "",
    ),
    (
        "serve",
        "usage: symphony serve < REQUESTS",
        "A request is {\"fn\": NAME, ...its arguments}, answered {\"ok\": VALUE} or\n\
         {\"error\": KIND, \"message\": TEXT}, until the input ends.\n",
    ),
];

/// `name ARGS`'s help when `name` is a command that reads no options and `args` ask for it.
pub(super) fn plain_help(name: &str, args: &[String]) -> Option<String> {
    let (_, usage, more) = PLAIN.iter().find(|(plain, _, _)| *plain == name)?;
    if !args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return None;
    }
    let said = OWN
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

/// `name`, a command the Symphony does not have, run by the first plugin that adds it with `args`;
/// a name no plugin adds is refused, naming the nearest. Its exit code.
pub(super) fn plugin(name: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if name == "release-inputs" {
        return release_inputs::main(args, out, err);
    }
    if name == "size-budget" {
        return size_budget::main(args, out, err);
    }
    if name == "init" {
        return crate::init::main(args, out, err);
    }
    if name == "shot" {
        return shot::main(args, out, err);
    }
    if name == "release" {
        return crate::release::main(args, out, err);
    }
    // Its name before the rename, unlisted: what a landing's on_land and a build older than the
    // rename still run (`nightly --due`), and a refusal naming the new one.
    if name == crate::release::LEGACY {
        return crate::release::legacy(args, out, err);
    }
    // Measured as the project the command runs in says, the nearest above the current folder.
    let root = std::env::current_dir()
        .ok()
        .and_then(|here| crate::project::Project::find(&here));
    if let Some(command) = crate::plugins::command(name, root.as_deref()) {
        return command.run(args, out, err);
    }
    let _ = writeln!(err, "{}", unknown(name));
    2
}

/// The refusal of `name`, a command nobody has: the nearest ones, and where they are all listed.
fn unknown(name: &str) -> String {
    // `help` is read before the commands, and a mistyped flag in a command's place is nearest the
    // one flag the binary reads there.
    let mut names = names();
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
    format!("symphony: unknown command {name:?}{near} (`just symphony-cli help` lists them)")
}

/// `symphony help [COMMAND]`, `--help`, `-h`: every command with what it does, or `COMMAND
/// --help`. Its exit code.
pub(super) fn help(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            let _ = write!(out, "{}", listing());
            0
        }
        Some(name) if names().iter().any(|known| known == name) => super::main(
            &[name.to_string(), "--help".into()],
            &mut &b""[..],
            out,
            err,
        ),
        Some(name) => {
            let _ = writeln!(err, "{}", unknown(name));
            2
        }
    }
}

/// A command line naming no command: every command, on stderr. Exit 2.
pub(super) fn usage(err: &mut dyn Write) -> i32 {
    let _ = write!(err, "{}", listing());
    2
}

/// Every command the binary runs, with what it does, in the order the usage names them.
fn every() -> Vec<(String, String)> {
    let (serve, own) = OWN.split_last().expect("the Symphony's own commands");
    let line = |(name, said): &(&str, &str)| ((*name).to_string(), (*said).to_string());
    own.iter()
        .map(line)
        .chain(crate::plugins::commands().into_iter().map(|command| {
            let said = command.summary();
            let said = if said.is_empty() {
                "a command of the project's plugins"
            } else {
                said
            };
            (command.name().to_string(), said.to_string())
        }))
        .chain([line(serve)])
        .collect()
}

/// Every command name, in the order the usage names them.
fn names() -> Vec<String> {
    every().into_iter().map(|(name, _)| name).collect()
}

/// The usage and every command with what it does: what `help` prints.
fn listing() -> String {
    let every = every();
    let width = every.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let mut text = String::from(
        "usage: symphony COMMAND [ARGS]... (`just symphony-cli COMMAND`)\n\n\
         The Symphony's document CLI. `just symphony-cli help COMMAND`, or\n\
         `just symphony-cli COMMAND --help`, shows a command's arguments and options.\n\n\
         commands:\n",
    );
    for (name, said) in &every {
        text.push_str(&format!("  {name:<width$}  {said}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_names_every_command_once_with_its_line_within_80_columns() {
        let listing = listing();
        for (name, said) in OWN {
            let rows: Vec<&str> = listing
                .lines()
                .filter(|line| line.split_whitespace().next() == Some(name))
                .collect();
            assert_eq!(rows.len(), 1, "{name}: {listing}");
            assert!(rows[0].ends_with(said), "{name}: {}", rows[0]);
        }
        assert!(listing.contains("\n  project "), "{listing}");
        assert!(
            listing.lines().last().unwrap().starts_with("  serve "),
            "{listing}"
        );
        for line in listing.lines() {
            assert!(line.chars().count() <= 80, "{line}");
        }
    }

    #[test]
    fn an_unknown_command_names_the_nearest() {
        for (name, said) in [
            (
                "statsu",
                "symphony: unknown command \"statsu\"; did you mean \"status\"? (`just symphony-cli help` lists them)",
            ),
            (
                "docs-audt",
                "symphony: unknown command \"docs-audt\"; did you mean \"docs-audit\"? (`just symphony-cli help` lists them)",
            ),
            (
                "frobnicate",
                "symphony: unknown command \"frobnicate\" (`just symphony-cli help` lists them)",
            ),
            (
                "--hlep",
                "symphony: unknown command \"--hlep\"; did you mean \"--help\"? (`just symphony-cli help` lists them)",
            ),
            (
                "-help",
                "symphony: unknown command \"-help\"; did you mean \"--help\"? (`just symphony-cli help` lists them)",
            ),
            // A command's name is answered with a command, never a flag.
            (
                "hlep",
                "symphony: unknown command \"hlep\"; did you mean \"help\"? (`just symphony-cli help` lists them)",
            ),
        ] {
            assert_eq!(unknown(name), said);
        }
    }
}
