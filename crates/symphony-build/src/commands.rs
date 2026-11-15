//! The commands `symphony-build` answers to: those only this repository's own build and release
//! run use ([`COMMANDS`]), which no project is given (`init::BUILDS` does not name it). A name it
//! does not have is refused with the nearest of its own; `--help`, `-h` and `help` list them all,
//! each with one line on what it does, as the shipped `symphony` lists its own.

use std::io::Write;
use std::path::Path;

mod crates_check;
mod release_inputs;
mod size_budget;
mod split_repair;
mod synced;

/// Its name, as its usage and its refusals say it.
const NAME: &str = "symphony-build";

/// What its listing says before the commands: its usage, how it is run and what it is.
const HEAD: &str = "usage: symphony-build COMMAND [ARGS]...\n\n\
                    The commands only the Symphony's own build and release run use, never shipped\n\
                    to a project: `tools/rs-bin.sh symphony-build COMMAND` in its checkout.\n\
                    `symphony-build help COMMAND`, or `symphony-build COMMAND --help`, shows a\n\
                    command's arguments and options.\n\n";

/// What an unknown command's refusal says lists them.
const LISTED: &str = "`symphony-build help`";

/// Its commands, in the order its usage names them, each with what it does.
const COMMANDS: [(&str, &str); 7] = [
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
        "synced",
        "time each page from its open to synced on a returning browser",
    ),
    (
        "release",
        "verify main's tip in full and tag it a release when green",
    ),
];

/// The usage of its commands that read no options of their own, and what their help says after
/// their line: what `--help` or `-h` anywhere on their command line prints ([`plain_help`]).
const PLAIN: [(&str, &str, &str); 5] = [
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
        "usage: symphony-build split-repair PROGRAM PUBLIC | --chunk MODULE",
        "PROGRAM is the program dx linked, with its relocations; PUBLIC the folder\n\
         dx bundle --wasm-split wrote. It writes back the merged constants the splitter\n\
         zeroed in the main module, puts each chunk's segments in a fixed order and\n\
         names every module and script it changed by its bytes, in place. --chunk puts\n\
         the segments of MODULE, one wasm-opt wrote for dx, in that order before dx\n\
         names it, and leaves a main module as it is, in place.\n",
    ),
];

/// `symphony-build COMMAND ARGS…`, the whole command line without the program name: one of
/// [`COMMANDS`]. Its exit code.
pub(crate) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let command = args.first().map(String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    if let Some(help) = command.and_then(|name| plain_help(name, rest)) {
        let _ = write!(out, "{help}");
        return 0;
    }
    match command {
        Some("--help" | "-h" | "help") => help(rest, out, err),
        Some("publish-client") => publish_client(rest, err),
        Some("release-inputs") => release_inputs::main(rest, out, err),
        Some("size-budget") => size_budget::main(rest, out, err),
        Some("crates-check") => crates_check::main(rest, out, err),
        Some("split-repair") => split_repair::main(rest, out, err),
        Some("synced") => synced::main(rest, out, err),
        Some("release") => crate::release::main(rest, out, err),
        Some(name) => {
            let _ = writeln!(err, "{}", unknown(name));
            2
        }
        None => usage(err),
    }
}

/// `publish-client STAGE PUBLIC`: a fully assembled client published over the one served with one
/// rename, including over a nonempty directory, keeping the builds published before it
/// ([`symphony::cli::publish_keeping`]). Its exit code.
fn publish_client(args: &[String], err: &mut dyn Write) -> i32 {
    let [stage, public] = args else {
        let _ = writeln!(err, "usage: symphony-build publish-client STAGE PUBLIC");
        return 2;
    };
    match symphony::cli::publish_keeping(Path::new(stage), Path::new(public)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(
                err,
                "symphony-build publish-client: {stage} -> {public}: {error}"
            );
            1
        }
    }
}

/// `name ARGS`'s help when `name` is a command that reads no options and `args` ask for it.
fn plain_help(name: &str, args: &[String]) -> Option<String> {
    let (_, usage, more) = PLAIN.iter().find(|(plain, _, _)| *plain == name)?;
    if !args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return None;
    }
    let said = COMMANDS
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

/// The refusal of `name`, a command it does not have: the nearest ones, and where they are all
/// listed.
fn unknown(name: &str) -> String {
    // `help` is read before the commands, and a mistyped flag in a command's place is nearest the
    // one flag the binary reads there.
    let mut names: Vec<&str> = COMMANDS.iter().map(|(name, _)| *name).collect();
    names.push(if name.starts_with('-') {
        "--help"
    } else {
        "help"
    });
    let near: Vec<String> = symphony_args::nearest(name, names)
        .iter()
        .map(|near| format!("{near:?}"))
        .collect();
    let near = match near.as_slice() {
        [] => String::new(),
        [one] => format!("; did you mean {one}?"),
        [rest @ .., last] => format!("; did you mean {} or {last}?", rest.join(", ")),
    };
    format!("{NAME}: unknown command {name:?}{near} ({LISTED} lists them)")
}

/// `symphony-build help [COMMAND]`, `--help`, `-h`: every command with what it does, or
/// `COMMAND --help`. Its exit code.
fn help(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            let _ = write!(out, "{}", listing());
            0
        }
        Some(name) if COMMANDS.iter().any(|(known, _)| *known == name) => {
            main(&[name.to_string(), "--help".into()], out, err)
        }
        Some(name) => {
            let _ = writeln!(err, "{}", unknown(name));
            2
        }
    }
}

/// A command line naming no command: every command, on stderr. Exit 2.
fn usage(err: &mut dyn Write) -> i32 {
    let _ = write!(err, "{}", listing());
    2
}

/// Its usage and every command with what it does: what its `help` prints.
fn listing() -> String {
    let width = COMMANDS
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let mut text = format!("{HEAD}commands:\n");
    for (name, said) in &COMMANDS {
        text.push_str(&format!("  {name:<width$}  {said}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    /// `symphony-build ARGS`, in process: its exit code, stdout and stderr.
    fn built(args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(&args, &mut out, &mut err);
        let text = |bytes| String::from_utf8(bytes).unwrap();
        (code, text(out), text(err))
    }

    /// The commands the shipped `symphony` lists, as `symphony help` prints them.
    fn shipped() -> Vec<String> {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = symphony::cli::main(&["help".into()], &mut &b""[..], &mut out, &mut err);
        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
        let listing = String::from_utf8(out).unwrap();
        let (_, commands) = listing.split_once("commands:\n").unwrap();
        commands
            .lines()
            .filter_map(|line| line.strip_prefix("  "))
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn the_listing_names_every_command_once_with_its_line_within_80_columns() {
        let listing = listing();
        assert!(
            listing.starts_with("usage: symphony-build COMMAND "),
            "{listing}"
        );
        for (name, said) in COMMANDS {
            let rows: Vec<&str> = listing
                .lines()
                .filter(|line| line.split_whitespace().next() == Some(name))
                .collect();
            assert_eq!(rows.len(), 1, "{name}: {listing}");
            assert!(rows[0].ends_with(said), "{name}: {}", rows[0]);
        }
        let end = listing.lines().last().unwrap();
        assert!(end.starts_with("  release "), "{listing}");
        for line in listing.lines() {
            assert!(line.chars().count() <= 80, "{line}");
        }
        assert!(listing.contains("\n  split-repair "));
    }

    #[test]
    fn neither_binary_has_a_command_of_the_other() {
        let shipped = shipped();
        assert!(shipped.iter().any(|name| name == "status"), "{shipped:?}");
        for (name, _) in COMMANDS {
            assert!(!shipped.iter().any(|known| known == name), "{name}");
        }
        let build: Vec<&str> = COMMANDS.iter().map(|(name, _)| *name).collect();
        for name in &shipped {
            assert!(!build.contains(&name.as_str()), "{name}");
        }
        assert_eq!(
            build,
            [
                "publish-client",
                "release-inputs",
                "size-budget",
                "crates-check",
                "split-repair",
                "synced",
                "release",
            ]
        );
        // Each command that reads no options of its own says what the listing says it does.
        for (name, usage, _) in PLAIN {
            assert!(COMMANDS.iter().any(|(own, _)| *own == name), "{name}");
            assert!(
                usage.starts_with(&format!("usage: symphony-build {name}")),
                "{usage}"
            );
        }
    }

    #[test]
    fn an_unknown_command_names_the_nearest() {
        let build = |name, near: &str| {
            format!(
                "symphony-build: unknown command {name:?}{near} (`symphony-build help` lists them)"
            )
        };
        for (name, said) in [
            (
                "size-budgte",
                build("size-budgte", "; did you mean \"size-budget\"?"),
            ),
            ("relase", build("relase", "; did you mean \"release\"?")),
            ("--hepl", build("--hepl", "; did you mean \"--help\"?")),
            ("hlep", build("hlep", "; did you mean \"help\"?")),
            // The shipped binary's commands are not the build's.
            ("status", build("status", "")),
            ("serve", build("serve", "")),
        ] {
            assert_eq!(unknown(name), said);
        }
    }

    #[test]
    fn the_build_binary_lists_its_commands_and_says_each_ones_usage() {
        let listing = listing();
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
        for (name, usage, _) in PLAIN {
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
                "usage: symphony-build split-repair PROGRAM PUBLIC | --chunk MODULE\n",
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
        let (code, out, err) = built(&["synced", "--frobnicate"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(
            err.starts_with("usage: symphony-build synced [-h] BASE "),
            "{err}"
        );
        assert!(err.contains("\nsymphony-build synced: error: "), "{err}");
        let (code, out, err) = built(&["help", "synced"]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(
            out.starts_with("usage: symphony-build synced [-h] BASE "),
            "{out}"
        );
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

    /// The publication itself is the library's (`symphony::cli::publish_keeping`, whose tests hold
    /// what it keeps and refuses): the command publishes, or says which folders it could not
    /// publish and why, the published folder left as it was.
    #[test]
    fn publish_client_publishes_or_names_both_folders_and_why() {
        let tree = Tree::new();
        let build = "{\"builds\": [{\"shell\": \"symphony-shell-a.js\", \
                     \"files\": [\"symphony-shell-a.js\"]}]}";
        tree.write(
            "stage/index.html",
            "<script type=\"module\" src=\"/assets/symphony-shell-a.js\"></script>",
        );
        tree.write("stage/assets/symphony-shell-a.js", "the shell");
        tree.write("stage/builds.json", build);
        let [stage, public] = [tree.path("stage"), tree.path("public")];
        let (stage, public) = (stage.display().to_string(), public.display().to_string());
        assert_eq!(
            built(&["publish-client", &stage, &public]),
            (0, String::new(), String::new())
        );
        assert_eq!(tree.read("public/assets/symphony-shell-a.js"), "the shell");
        assert!(!tree.path("stage").exists());

        tree.write("stage/index.html", "<p>no build</p>");
        assert_eq!(
            built(&["publish-client", &stage, &public]),
            (
                1,
                String::new(),
                format!(
                    "symphony-build publish-client: {stage} -> {public}: builds.json names no \
                     build\n"
                )
            )
        );
        assert_eq!(tree.read("public/assets/symphony-shell-a.js"), "the shell");
    }
}
