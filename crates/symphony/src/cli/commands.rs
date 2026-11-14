//! The commands the workspace's two command-line binaries answer to. The shipped `symphony`: the
//! Symphony's own ([`OWN`]), `serve` last, with the commands the plugins this process holds add
//! before it. `symphony-build`: the commands only this repository's own build and release run use
//! ([`BUILD`]), which no project is given (`init::BUILDS` does not name it), so neither the shipped
//! binary nor a consuming project's host links them. A name a binary does not have is refused with
//! the nearest of its own; `--help`, `-h` and `help` list them all, each with one line on what it
//! does.

use std::io::Write;

mod crates_check;
mod release_inputs;
mod shot;
mod size_budget;
mod split_repair;

/// The Symphony's own commands, in the order its usage names them, each with what it does.
const OWN: [(&str, &str); 12] = [
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
    ("serve", "answer JSON requests on stdin, one a line"),
];

/// The usage of the Symphony's commands that read no options of their own, and what their help says
/// after their line: what `--help` or `-h` anywhere on their command line prints ([`plain_help`]).
const PLAIN: [(&str, &str, &str); 3] = [
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

/// The commands only this repository's own build and release run use, `symphony-build`'s, in the
/// order its usage names them, each with what it does.
const BUILD: [(&str, &str); 6] = [
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
        "crates-check",
        "check each published package on crates.io holds its sources",
    ),
    (
        "split-repair",
        "repair dx's --wasm-split output: safe, same bytes each build",
    ),
    (
        "release",
        "verify main's tip in full and tag it a release when green",
    ),
];

/// The usage of `symphony-build`'s commands that read no options of their own, and what their help
/// says after their line.
const BUILD_PLAIN: [(&str, &str, &str); 5] = [
    (
        "publish-client",
        "usage: symphony-build publish-client STAGE PUBLIC",
        "",
    ),
    (
        "release-inputs",
        "usage: symphony-build release-inputs ROOT METADATA FILES COMPONENT_FILES --key|LOCK",
        "",
    ),
    (
        "size-budget",
        "usage: symphony-build size-budget ROOT RELEASE",
        "",
    ),
    (
        "crates-check",
        "usage: symphony-build crates-check [ROOT]",
        "ROOT is the workspace (default: the current folder). A version crates.io\n\
         lacks has nothing to compare with; one it holds is packaged here, in the\n\
         build folder, and compared file by file with that version there: one whose\n\
         sources moved while its version did not is red (exit 1). A registry it cannot\n\
         read is exit 2.\n",
    ),
    (
        "split-repair",
        "usage: symphony-build split-repair PROGRAM PUBLIC",
        "PROGRAM is the program dx linked, with its relocations; PUBLIC the folder\n\
         dx bundle --wasm-split wrote. It writes back the merged constants the splitter\n\
         zeroed in the main module, puts each chunk's segments in a fixed order and\n\
         names every module and script it changed by its bytes, in place.\n",
    ),
];

/// One of the two binaries, as its listing, its help and its refusals name it.
pub(super) struct Binary {
    /// Its name, as its usage and its refusals say it.
    name: &'static str,
    /// What its listing says before the commands: its usage, how it is run and what it is.
    head: &'static str,
    /// What an unknown command's refusal says lists them.
    listed: &'static str,
    /// Its own commands, in the order its usage names them; the last stays last, after the ones
    /// the plugins add where they add any.
    own: &'static [(&'static str, &'static str)],
    /// The usage of its commands that read no options of their own ([`plain_help`]).
    plain: &'static [(&'static str, &'static str, &'static str)],
    /// Whether the plugins this process holds add commands to it.
    plugins: bool,
    /// The binary itself, with no input: what `help COMMAND` runs as `COMMAND --help`.
    run: fn(&[String], &mut dyn Write, &mut dyn Write) -> i32,
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
    plugins: true,
    run: |args, out, err| super::main(args, &mut &b""[..], out, err),
};

/// `symphony-build` (`tools/rs-bin.sh symphony-build`), this repository's alone.
pub(super) const SYMPHONY_BUILD: Binary = Binary {
    name: "symphony-build",
    head: "usage: symphony-build COMMAND [ARGS]...\n\n\
           The commands only the Symphony's own build and release run use, never shipped\n\
           to a project: `tools/rs-bin.sh symphony-build COMMAND` in its checkout.\n\
           `symphony-build help COMMAND`, or `symphony-build COMMAND --help`, shows a\n\
           command's arguments and options.\n\n",
    listed: "`symphony-build help`",
    own: &BUILD,
    plain: &BUILD_PLAIN,
    plugins: false,
    run: build,
};

/// `name ARGS`'s help when `name` is a command of `binary` that reads no options and `args` ask
/// for it.
pub(super) fn plain_help(binary: &Binary, name: &str, args: &[String]) -> Option<String> {
    let (_, usage, more) = binary.plain.iter().find(|(plain, _, _)| *plain == name)?;
    if !args.iter().any(|arg| arg == "-h" || arg == "--help") {
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
/// that adds it with `args`; a name no plugin adds is refused, naming the nearest. Its exit code.
pub(super) fn plugin(name: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if name == "init" {
        return crate::init::main(args, out, err);
    }
    if name == "shot" {
        return shot::main(args, out, err);
    }
    // Measured as the project the command runs in says, the nearest above the current folder.
    let root = std::env::current_dir()
        .ok()
        .and_then(|here| crate::project::Project::find(&here));
    if let Some(command) = crate::plugins::command(name, root.as_deref()) {
        return command.run(args, out, err);
    }
    let _ = writeln!(err, "{}", unknown(&SYMPHONY, name));
    2
}

/// `symphony-build COMMAND ARGS…`: one of [`BUILD`]'s commands. Its exit code.
pub(super) fn build(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let command = args.first().map(String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    if let Some(help) = command.and_then(|name| plain_help(&SYMPHONY_BUILD, name, rest)) {
        let _ = write!(out, "{help}");
        return 0;
    }
    match command {
        Some("--help" | "-h" | "help") => help(&SYMPHONY_BUILD, rest, out, err),
        Some("publish-client") => super::publish::publish_client_main(rest, err),
        Some("release-inputs") => release_inputs::main(rest, out, err),
        Some("size-budget") => size_budget::main(rest, out, err),
        Some("crates-check") => crates_check::main(rest, out, err),
        Some("split-repair") => split_repair::main(rest, out, err),
        Some("release") => crate::release::main(rest, out, err),
        Some(name) => {
            let _ = writeln!(err, "{}", unknown(&SYMPHONY_BUILD, name));
            2
        }
        None => usage(&SYMPHONY_BUILD, err),
    }
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
        Some(name) if names(binary).iter().any(|known| known == name) => {
            (binary.run)(&[name.to_string(), "--help".into()], out, err)
        }
        Some(name) => {
            let _ = writeln!(err, "{}", unknown(binary, name));
            2
        }
    }
}

/// A command line naming no command: every command, on stderr. Exit 2.
pub(super) fn usage(binary: &Binary, err: &mut dyn Write) -> i32 {
    let _ = write!(err, "{}", listing(binary));
    2
}

/// Every command `binary` runs, with what it does, in the order its usage names them.
fn every(binary: &Binary) -> Vec<(String, String)> {
    let line = |(name, said): &(&str, &str)| ((*name).to_string(), (*said).to_string());
    let Some((last, own)) = binary.own.split_last() else {
        return Vec::new();
    };
    let plugins = binary
        .plugins
        .then(crate::plugins::commands)
        .unwrap_or_default();
    own.iter()
        .map(line)
        .chain(plugins.into_iter().map(|command| {
            let said = command.summary();
            let said = if said.is_empty() {
                "a command of the project's plugins"
            } else {
                said
            };
            (command.name().to_string(), said.to_string())
        }))
        .chain([line(last)])
        .collect()
}

/// Every command name of `binary`, in the order its usage names them.
fn names(binary: &Binary) -> Vec<String> {
    every(binary).into_iter().map(|(name, _)| name).collect()
}

/// `binary`'s usage and every command with what it does: what its `help` prints.
fn listing(binary: &Binary) -> String {
    let every = every(binary);
    let width = every.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let mut text = format!("{}commands:\n", binary.head);
    for (name, said) in &every {
        text.push_str(&format!("  {name:<width$}  {said}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `symphony-build ARGS`, in process: its exit code, stdout and stderr.
    fn built(args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = build(&args, &mut out, &mut err);
        let text = |bytes| String::from_utf8(bytes).unwrap();
        (code, text(out), text(err))
    }

    #[test]
    fn the_listing_names_every_command_once_with_its_line_within_80_columns() {
        for binary in [&SYMPHONY, &SYMPHONY_BUILD] {
            let listing = listing(binary);
            assert!(
                listing.starts_with(&format!("usage: {} COMMAND ", binary.name)),
                "{listing}"
            );
            for (name, said) in binary.own {
                let rows: Vec<&str> = listing
                    .lines()
                    .filter(|line| line.split_whitespace().next() == Some(name))
                    .collect();
                assert_eq!(rows.len(), 1, "{name}: {listing}");
                assert!(rows[0].ends_with(said), "{name}: {}", rows[0]);
            }
            let (last, _) = binary.own.last().unwrap();
            let end = listing.lines().last().unwrap();
            assert!(end.starts_with(&format!("  {last} ")), "{listing}");
            for line in listing.lines() {
                assert!(line.chars().count() <= 80, "{line}");
            }
        }
        assert!(listing(&SYMPHONY).contains("\n  project "));
        assert!(listing(&SYMPHONY_BUILD).contains("\n  split-repair "));
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
    fn neither_binary_has_a_command_of_the_other() {
        let (shipped, build) = (names(&SYMPHONY), names(&SYMPHONY_BUILD));
        for (name, _) in BUILD {
            assert!(!shipped.iter().any(|known| known == name), "{name}");
        }
        for (name, _) in OWN {
            assert!(!build.iter().any(|known| known == name), "{name}");
        }
        let every = [
            "publish-client",
            "release-inputs",
            "size-budget",
            "crates-check",
            "split-repair",
            "release",
        ];
        assert_eq!(build, every);
        // Each command that reads no options of its own says what the listing says it does.
        for binary in [&SYMPHONY, &SYMPHONY_BUILD] {
            for (name, usage, _) in binary.plain {
                assert!(binary.own.iter().any(|(own, _)| own == name), "{name}");
                assert!(
                    usage.starts_with(&format!("usage: {} {name}", binary.name)),
                    "{usage}"
                );
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
        let build = |name, near: &str| {
            format!(
                "symphony-build: unknown command {name:?}{near} (`symphony-build help` lists them)"
            )
        };
        for (binary, name, said) in [
            (
                &SYMPHONY,
                "statsu",
                shipped("statsu", "; did you mean \"status\"?"),
            ),
            (
                &SYMPHONY,
                "docs-audt",
                shipped("docs-audt", "; did you mean \"docs-audit\"?"),
            ),
            (&SYMPHONY, "frobnicate", shipped("frobnicate", "")),
            (
                &SYMPHONY,
                "--hlep",
                shipped("--hlep", "; did you mean \"--help\"?"),
            ),
            (
                &SYMPHONY,
                "-help",
                shipped("-help", "; did you mean \"--help\"?"),
            ),
            // A command's name is answered with a command, never a flag.
            (
                &SYMPHONY,
                "hlep",
                shipped("hlep", "; did you mean \"help\"?"),
            ),
            // The build's commands are not the shipped binary's, nor its own the build's.
            (&SYMPHONY, "size-budget", shipped("size-budget", "")),
            (&SYMPHONY, "release", shipped("release", "")),
            (
                &SYMPHONY_BUILD,
                "size-budgte",
                build("size-budgte", "; did you mean \"size-budget\"?"),
            ),
            (
                &SYMPHONY_BUILD,
                "relase",
                build("relase", "; did you mean \"release\"?"),
            ),
            (
                &SYMPHONY_BUILD,
                "--hepl",
                build("--hepl", "; did you mean \"--help\"?"),
            ),
            (&SYMPHONY_BUILD, "status", build("status", "")),
            (&SYMPHONY_BUILD, "serve", build("serve", "")),
        ] {
            assert_eq!(unknown(binary, name), said);
        }
    }

    #[test]
    fn the_build_binary_lists_its_commands_and_says_each_ones_usage() {
        let listing = listing(&SYMPHONY_BUILD);
        for args in [
            &["help"][..],
            &["--help"],
            &["-h"],
            &["help", "help"],
            &["help", "-h"],
        ] {
            assert_eq!(built(args), (0, listing.clone(), String::new()), "{args:?}");
        }
        assert_eq!(built(&[]), (2, String::new(), listing), "no command");
        for (name, usage, _) in BUILD_PLAIN {
            for args in [
                &["help", name][..],
                &[name, "--help"],
                &[name, "-h"],
                &[name, "x", "-h"],
            ] {
                let (code, out, err) = built(args);
                assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
                assert!(out.starts_with(&format!("{usage}\n\n")), "{args:?}: {out}");
            }
        }
        assert_eq!(
            built(&["help", "size-budget"]),
            (
                0,
                "usage: symphony-build size-budget ROOT RELEASE\n\n\
                 check the shipped binaries' sizes against their budget.\n"
                    .into(),
                String::new()
            )
        );
        let (code, out, err) = built(&["help", "split-repair"]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.ends_with("in place.\n"), "{out}");
        for args in [&["help", "release"][..], &["release", "--help"]] {
            let (code, out, err) = built(args);
            assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
            assert!(
                out.starts_with("usage: symphony-build release [-h] "),
                "{out}"
            );
        }
    }

    #[test]
    fn the_build_binary_runs_its_commands_and_refuses_any_other() {
        // Each command's own refusal of a command line it cannot read, said under its binary.
        for (name, said) in [
            (
                "publish-client",
                "usage: symphony-build publish-client STAGE PUBLIC\n",
            ),
            (
                "size-budget",
                "usage: symphony-build size-budget ROOT RELEASE\n",
            ),
            (
                "split-repair",
                "usage: symphony-build split-repair PROGRAM PUBLIC\n",
            ),
            (
                "release-inputs",
                "usage: symphony-build release-inputs ROOT METADATA FILES COMPONENT_FILES --key|LOCK\n",
            ),
        ] {
            assert_eq!(
                built(&[name, "one"]),
                (2, String::new(), said.into()),
                "{name}"
            );
        }
        let (code, out, err) = built(&["release", "--frobnicate"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(
            err.starts_with("usage: symphony-build release [-h] "),
            "{err}"
        );
        assert!(err.contains("\nsymphony-build release: error: "), "{err}");
        // A command of the shipped binary's, and the release run's name before the rename, which
        // nothing here runs any more.
        for name in ["status", "nightly"] {
            let refused = format!(
                "symphony-build: unknown command {name:?} (`symphony-build help` lists them)\n"
            );
            for args in [
                &[name][..],
                &[name, "--due"],
                &[name, "--help"],
                &["help", name],
            ] {
                assert_eq!(built(args), (2, String::new(), refused.clone()), "{args:?}");
            }
        }
    }
}
