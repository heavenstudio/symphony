//! `symphony release-notes [FROM] [TO]`: what a project changes to take each release after FROM,
//! oldest first, so a project several releases behind reads one list. It reads the notes the
//! newest release installed on the machine holds ([`crate::machine::release::NOTES`], every
//! release's up to it), so it needs no clone of the Symphony's repository and reaches no network.

use std::io::Write;
use std::path::Path;

use symphony_args::parse;

use crate::machine::release;

const USAGE: &str = "usage: symphony release-notes [-h] [FROM] [TO]";
const HELP: &str =
    "What a project changes to take each Symphony release after FROM up to TO, oldest first: its
board's move, the public Rust items replaced or removed, symphony.toml keys, the files
`symphony init` rewrites, tools pinned anew and what to run once. Read from the notes of the
newest release installed on this machine, which hold every release's: no clone and no network.

positional arguments:
  FROM        the release the project is at: release-YYYY-MM-DD, release-YYYY-MM-DD.N or
              that release's commit (default: the release tag, else the commit, the
              project's Cargo.toml pins the Symphony at; with neither, every release)
  TO          the release to take, named the same way (default: the newest the notes hold)

options:
  -h, --help  show this help message and exit

Exit 0 with the notes printed or none to print; 1 when no installed release holds notes, or
FROM or TO is a release or a commit the notes do not hold, said with what they hold: a part of
the steps is never printed as if it were all of them; 2 for a command line it cannot run.";

/// `symphony release-notes [FROM] [TO]`, in the current folder's project. Its exit code.
pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let named = match parse(args, &[]) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        Ok(parsed) if parsed.positionals.len() > 2 => {
            return crate::cli::refuse(err, USAGE, "release-notes", "at most FROM and TO");
        }
        Ok(parsed) => parsed.positionals,
        Err(problem) => return crate::cli::refuse(err, USAGE, "release-notes", &problem),
    };
    let machine = match crate::machine::dir() {
        Ok(machine) => machine,
        Err(why) => {
            let _ = writeln!(err, "symphony release-notes: {why}");
            return 1;
        }
    };
    let pinned = super::here().and_then(|root| pinned(&root));
    let from = named.first().cloned().or(pinned);
    let to = named.get(1).map(String::as_str);
    print(&machine, from.as_deref(), to, out, err)
}

/// The release the project at `root` is at, as its manifest pins the Symphony: the first release
/// tag it names for a dependency (`tag = "release-…"`), else the commit it names for one fetched
/// from a repository of the Symphony's name (`rev = "…"`).
fn pinned(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let lines = || {
        text.lines()
            .filter(|line| !line.trim_start().starts_with('#'))
    };
    let value = |line: &str, key: &str| {
        let (_, after) = line.split_once(key)?;
        let value = after.trim_start().strip_prefix('=')?.trim_start();
        let (value, _) = value.strip_prefix('"')?.split_once('"')?;
        Some(value.to_string())
    };
    lines()
        .find_map(|line| value(line, "tag").filter(|tag| release::named(tag)))
        .or_else(|| {
            lines()
                .filter(|line| line.contains("symphony"))
                .find_map(|line| value(line, "rev"))
        })
}

/// The notes of the releases after `from` up to `to` that the newest noted release in `machine`
/// holds, on `out`. Its exit code.
fn print(
    machine: &Path,
    from: Option<&str>,
    to: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let Some((newest, text)) = release::noted(machine) else {
        let _ = writeln!(
            err,
            "symphony release-notes: no release installed in {} holds notes ({}): the next \
             release's install writes them",
            machine.join(release::RELEASES).display(),
            release::NOTES
        );
        return 1;
    };
    let said = match release::notes_between(&text, from, to) {
        Ok(said) => said,
        Err(problem) => {
            let _ = writeln!(
                err,
                "symphony release-notes: {problem} (the notes of {newest}, the newest installed \
                 here that holds any)"
            );
            return 1;
        }
    };
    let span = match (from, to) {
        (Some(from), Some(to)) => format!("after {from} up to {to}"),
        (Some(from), None) => format!("after {from}"),
        (None, Some(to)) => format!("up to {to}"),
        (None, None) => "the notes hold".to_string(),
    };
    if said.is_empty() {
        let _ = writeln!(
            out,
            "No release {span} in the notes of {newest}, the newest installed here that holds \
             any: nothing to take."
        );
        return 0;
    }
    let _ = writeln!(
        out,
        "What a project changes to take each release {span}, oldest first, from the notes of \
         {newest}:\n\n{}",
        said.trim_end()
    );
    0
}

#[cfg(test)]
#[path = "release_notes_tests.rs"]
mod tests;
