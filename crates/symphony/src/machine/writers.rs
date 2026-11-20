//! The writer rule (docs/RELEASES.md, "The refresh"): whether a registered project's board can move
//! to a release's schema, by the writers seen there in the last week (its writer record,
//! [`symphony_board::Writer`]). A release's refresh judges each board by it
//! (`crates/symphony-build/src/release/refresh.rs`), and so does the machine's server when it
//! tries a held update again ([`super::held`]): one rule, read here.
//!
//! A writer behind the release's schema is one of three: a release's own, a binary of a release
//! installed in the machine folder on this host, which `current` takes on and the move leaves
//! behind by name; one on this host whose program is gone from its path ([`gone`]), which is not
//! counted; or any other, which holds the move. The record keeps a program's path and no more, so
//! what is at that path now is looked at, and never run.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use symphony_board::Writer;

use super::release;

/// The writers behind a release's schema, by what each does to a move ([`lagging`]).
#[derive(Debug, Default)]
pub struct Lagging {
    /// A release's own, which `current` takes on: left behind by name.
    pub own: Vec<Writer>,
    /// The rest that can still write: each holds the move.
    pub behind: Vec<Writer>,
    /// Those whose program is no longer at its path, each with why: not counted.
    pub gone: Vec<(Writer, Gone)>,
}

/// The writers of `lines` seen in the last week before `now` that are behind `target`: a release's
/// own (a binary of a release installed in the machine folder `machine`, on this host), which
/// `current` takes on; those on this host whose program is no longer there ([`gone`]), which are
/// not counted; and the rest, which hold a move. Each list by program, host and path.
pub fn lagging(lines: Vec<Writer>, target: i64, now: f64, machine: &Path) -> Lagging {
    let releases = machine
        .join(release::RELEASES)
        .canonicalize()
        .unwrap_or_else(|_| machine.join(release::RELEASES));
    let host = symphony_board::sys::hostname();
    let behind = (lines.into_iter())
        .filter(|line| line.schema < target && now - line.seen <= symphony_board::WRITERS_RECENT);
    let (own, rest): (Vec<Writer>, Vec<Writer>) = behind
        .partition(|line| line.host == host && of_a_release(Path::new(&line.path), &releases));
    let by_name = |a: &Writer, b: &Writer| {
        (&a.program, &a.host, &a.path).cmp(&(&b.program, &b.host, &b.path))
    };
    let (mut behind, mut gone) = (Vec::new(), Vec::new());
    for line in rest {
        // Another host's path is not this machine's to look at.
        match Some(&line)
            .filter(|line| line.host == host)
            .and_then(self::gone)
        {
            Some(why) => gone.push((line, why)),
            None => behind.push(line),
        }
    }
    behind.sort_by(by_name);
    gone.sort_by(|a, b| by_name(&a.0, &b.0));
    Lagging { own, behind, gone }
}

/// Why the program that wrote a line can write no more: the record keeps its path alone, and the
/// file there is looked at, never run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gone {
    /// Nothing is at its path.
    Missing,
    /// The file there was written after the line's last write could have been made.
    Replaced,
}

impl Gone {
    /// Why, in words.
    pub fn said(self) -> &'static str {
        match self {
            Gone::Missing => "nothing is at its path now",
            Gone::Replaced => {
                "the file at its path was written after it last ran there, a build that has not opened this board"
            }
        }
    }
}

/// Whether the program that wrote `line`, on this host, is gone from its path. Nothing there is
/// gone. So is a file modified more than [`symphony_board::WRITERS_EVERY`] after the line was
/// seen: a line is written again once it is that old, so every write through the path came within
/// that long of `seen`, and a file written later has never opened this board to write. A path that
/// cannot be read is not gone, and neither is one whose time cannot be.
pub fn gone(line: &Writer) -> Option<Gone> {
    use std::io::ErrorKind::{NotADirectory, NotFound};
    match fs::metadata(&line.path) {
        Err(error) if matches!(error.kind(), NotFound | NotADirectory) => Some(Gone::Missing),
        Err(_) => None,
        Ok(file) => {
            let modified = file.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
            (modified.as_secs_f64() > line.seen + symphony_board::WRITERS_EVERY)
                .then_some(Gone::Replaced)
        }
    }
}

/// What a judgement says of the writers it did not count, after its own words, read at `now`:
/// nothing with none.
pub fn not_counted(gone: &[(Writer, Gone)], now: f64) -> String {
    if gone.is_empty() {
        return String::new();
    }
    let each: Vec<String> = (gone.iter())
        .map(|(writer, why)| format!("{}: {}", said(writer, now), why.said()))
        .collect();
    format!("; not counted, as its program is gone: {}", each.join("; "))
}

/// Whether `path` is a binary of a release in `releases`: `releases/NAME/bin/BINARY`.
pub fn of_a_release(path: &Path, releases: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(releases) else {
        return false;
    };
    let parts: Vec<_> = rest.components().collect();
    parts.len() == 3 && parts[1].as_os_str() == "bin"
}

/// A writer, in words, as read at `now`.
pub fn said(writer: &Writer, now: f64) -> String {
    format!(
        "{} at schema {} ({} on {}, last seen {} ago)",
        writer.program,
        writer.schema,
        writer.path,
        writer.host,
        symphony_board::board_health::ago(now - writer.seen)
    )
}

/// What a judgement says of a board at schema `live` that the writers `behind`, never none, hold
/// below `release`'s schema `target`, with those it did not count (`gone`), read at `now`: what
/// its project finishes.
pub fn holding(
    live: i64,
    behind: &[Writer],
    release: &str,
    target: i64,
    gone: &[(Writer, Gone)],
    now: f64,
) -> String {
    format!(
        "its board stays at schema {live}: {} seen there in the last week {} behind {release}'s \
         schema {target}: {}; rebuild each at schema {target} and run it once, which records it, \
         then an orchestrator's `just release --refresh` moves it{}",
        match behind.len() {
            1 => "a writer".to_string(),
            n => format!("{n} writers"),
        },
        if behind.len() == 1 { "is" } else { "are" },
        (behind.iter().map(|writer| said(writer, now)))
            .collect::<Vec<_>>()
            .join("; "),
        not_counted(gone, now)
    )
}

/// What a judgement says of a board at schema `live`, below the writer record
/// ([`symphony_board::WRITERS_SINCE`]), in the main checkout `root`: never moved by a release, its
/// own migrate brings it up.
pub fn before_the_record(live: i64, root: &Path) -> String {
    format!(
        "its board is at schema {live}, from before the board recorded who writes it (schema {}), \
         so it is never moved here: `cd {} && just tasks migrate`, its own, brings it up",
        symphony_board::WRITERS_SINCE,
        root.display()
    )
}

/// `text`, a judgement's words, without the times they say each writer was last seen: what it
/// says of who holds a board, which a later look says alike while nothing changed.
pub fn steady(text: &str) -> String {
    const SEEN: &str = ", last seen ";
    const AGO: &str = " ago)";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(SEEN) {
        let Some(end) = rest[at..].find(AGO) else {
            break;
        };
        out.push_str(&rest[..at]);
        out.push(')');
        rest = &rest[at + end + AGO.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "writers_tests.rs"]
mod tests;
