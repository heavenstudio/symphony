//! The writer rule (docs/RELEASES.md, "The refresh"): whether a registered project's board can move
//! to a release's schema, by the writers seen there in the last week (its writer record,
//! [`symphony_board::Writer`]). A release's refresh judges each board by it
//! (`crates/symphony-build/src/release/refresh.rs`), and so does the machine's server when it
//! tries a held update again ([`super::held`]): one rule, read here.
//!
//! A writer behind the release's schema is one of four: a release's own, a binary of a release
//! installed in the machine folder on this host, which `current` takes on and the move leaves
//! behind by name; one on this host whose program is gone from its path ([`gone`]), which is not
//! counted; one on this host that is no build its project runs now and has not run there for
//! longer than the project's bound ([`Runs`]), which is not counted either; or any other, which
//! holds the move. The record keeps a program's path and no more, so what is at that path now is
//! looked at, and never run. A writer not counted that runs after the move is refused the board
//! before any write, naming itself as the program to rebuild: a refused write, never a corrupt one.

use std::fs;
use std::path::{Path, PathBuf};
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
    /// Builds on this host its project does not run now, unseen there for longer than its bound
    /// ([`Runs`]): not counted.
    pub unused: Vec<Writer>,
    /// That bound, in hours.
    pub hours: u32,
    /// Whether a writer that holds the move is a build its project runs now: a link in its main
    /// checkout's `.target/bin` names it.
    pub linked: bool,
}

impl Lagging {
    /// Whether a move leaves a writer behind by name (`tasks migrate --leave-behind`): a release's
    /// own, or one not counted.
    pub fn leaves(&self) -> bool {
        !self.own.is_empty() || !self.gone.is_empty() || !self.unused.is_empty()
    }

    /// What a judgement says of the writers it did not count, after its own words, read at `now`:
    /// nothing with none.
    pub fn not_counted(&self, now: f64) -> String {
        format!(
            "{}{}",
            not_counted(&self.gone, now),
            not_run(&self.unused, self.hours, now)
        )
    }

    /// What a judgement says of a board at schema `live` that the writers behind, never none, hold
    /// below `release`'s schema `target`, read at `now`: what its project finishes ([`holding`]).
    pub fn holding(&self, live: i64, release: &str, target: i64, now: f64) -> String {
        format!(
            "{}{}",
            holding(
                live,
                &self.behind,
                release,
                target,
                &self.not_counted(now),
                now
            ),
            if self.linked { OWN_BUILD } else { "" }
        )
    }
}

/// What a judgement adds when a build the project runs now holds its board: the two ways it takes
/// the release.
pub const OWN_BUILD: &str = "; the project's own build of the board's binaries is among them (a \
     link in its `.target/bin`): it takes the release by moving its pin to it and rebuilding them, \
     or, where those binaries are the board's own, by removing those links, so its recipes run the \
     release's (`current/bin` in the machine folder), as a project with no build of its own does, \
     and its board moves with each release";

/// The bound a project names none of: a day.
pub const UNUSED_HOURS: u32 = 24;

/// The key in a project's `[board]` that names another bound.
pub const UNUSED_KEY: &str = "stale_writer_hours";

/// What a project runs now, as its writers are judged: the builds its main checkout's links name
/// (`.target/bin/NAME`, followed to the file each names), and how long another build of this host
/// may go without running there before it holds no move (`[board] stale_writer_hours` in its
/// `symphony.toml`, else [`UNUSED_HOURS`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runs {
    pub builds: Vec<PathBuf>,
    pub hours: u32,
}

impl Runs {
    /// The project whose main checkout is `root`: its links, each named by the file it leads to,
    /// and its bound. A link that leads nowhere names nothing, and a bound its file does not give
    /// as a whole number of hours over 0 is the default, as the project's own settings check
    /// refuses it.
    pub fn of(root: &Path) -> Runs {
        let builds = fs::read_dir(root.join(symphony_board::builds::LINKS))
            .map(|entries| {
                (entries.flatten())
                    .filter_map(|entry| entry.path().canonicalize().ok())
                    .collect()
            })
            .unwrap_or_default();
        Runs {
            builds,
            hours: unused_hours(root),
        }
    }

    /// Whether `line`'s program is a build its project runs now.
    fn runs(&self, line: &Writer) -> bool {
        let path = Path::new(&line.path);
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.builds.contains(&path)
    }
}

/// The bound `[board] stale_writer_hours` names in the project file of the main checkout `root`,
/// else [`UNUSED_HOURS`]: only that key is read.
fn unused_hours(root: &Path) -> u32 {
    let text = fs::read_to_string(root.join(crate::project::FILE)).unwrap_or_default();
    (text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("board")?.get(UNUSED_KEY)?.as_integer())
        .and_then(|hours| u32::try_from(hours).ok())
        .filter(|hours| *hours > 0)
        .unwrap_or(UNUSED_HOURS)
}

/// The writers of `lines` seen in the last week before `now` that are behind `target`: a release's
/// own (a binary of a release installed in the machine folder `machine`, on this host), which
/// `current` takes on; those on this host whose program is no longer there ([`gone`]), and those on
/// this host that are no build the project `runs` now and were last seen longer ago than its bound,
/// which are not counted; and the rest, which hold a move. Each list by program, host and path.
pub fn lagging(lines: Vec<Writer>, target: i64, now: f64, machine: &Path, runs: &Runs) -> Lagging {
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
    let (mut behind, mut gone, mut unused) = (Vec::new(), Vec::new(), Vec::new());
    let bound = f64::from(runs.hours) * 3600.0;
    for line in rest {
        // Another host's path, and what it runs, are not this machine's to look at.
        if line.host != host {
            behind.push(line);
            continue;
        }
        match self::gone(&line) {
            Some(why) => gone.push((line, why)),
            None if now - line.seen > bound && !runs.runs(&line) => unused.push(line),
            None => behind.push(line),
        }
    }
    behind.sort_by(by_name);
    gone.sort_by(|a, b| by_name(&a.0, &b.0));
    unused.sort_by(by_name);
    let linked = behind
        .iter()
        .any(|line| line.host == host && runs.runs(line));
    Lagging {
        own,
        behind,
        gone,
        unused,
        hours: runs.hours,
        linked,
    }
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

/// What a judgement says of the builds `unused` it did not count, its project running none of them
/// now and none run for longer than `hours`, read at `now`: nothing with none.
pub fn not_run(unused: &[Writer], hours: u32, now: f64) -> String {
    if unused.is_empty() {
        return String::new();
    }
    let each: Vec<String> = (unused.iter()).map(|writer| said(writer, now)).collect();
    format!(
        "; not counted, as its project runs another build now and it has not run here for over \
         {hours} h ([board] {UNUSED_KEY}): {}",
        each.join("; ")
    )
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
/// below `release`'s schema `target`, with what it says of those it did not count
/// (`not_counted`, [`Lagging::not_counted`]), read at `now`: what its project finishes, and that
/// the Symphony moves the board then. A refresh and the machine's server say it alike, so a round
/// compares its words with a refresh's ([`steady`]); which of them moves it is theirs to say, and
/// the project whose board it is reads it on its update's task, so it names no command that
/// project lacks.
pub fn holding(
    live: i64,
    behind: &[Writer],
    release: &str,
    target: i64,
    not_counted: &str,
    now: f64,
) -> String {
    format!(
        "its board stays at schema {live}: {} seen there in the last week {} behind {release}'s \
         schema {target}: {}; rebuild each at schema {target} and run it once, which records it, \
         then the Symphony moves it{}",
        match behind.len() {
            1 => "a writer".to_string(),
            n => format!("{n} writers"),
        },
        if behind.len() == 1 { "is" } else { "are" },
        (behind.iter().map(|writer| said(writer, now)))
            .collect::<Vec<_>>()
            .join("; "),
        not_counted
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
