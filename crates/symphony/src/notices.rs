//! The server's part of a notice (docs/engineering/NOTICES.md, "The server's part"): which program
//! each way's notices go to, and the message the server writes on that program's stdin.
//!
//! The programs are the machine's, never a project's: whoever runs the server names them in
//! `notices/programs.toml` of the machine folder ([`Programs`]), beside what each plugin keeps there,
//! and no `symphony.toml`, board record or page names one. That file says what the server runs, so a
//! file or folder another account may write, or a link in the place of either, is refused, and
//! nothing is sent until it is put right.
//!
//! The message is the notice format's ([`line`]). The server links no plugin's crate: this crate is
//! published and `symphony-notice` is not, so the format is written here and its tests read it with
//! that crate, as they hold [`came`] and [`unstarted`] to its exit statuses.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The folder of the machine folder where each notice plugin keeps what it needs, and the server
/// names the programs.
pub const FOLDER: &str = "notices";
/// The file in [`FOLDER`] naming each way's program.
pub const PROGRAMS: &str = "programs.toml";
/// The project's record of its notices, under its artifacts folder
/// ([`crate::project::Project::artifact`]).
pub const RECORD: &str = "notices.json";

/// A plugin's exit status for a notice it sent.
pub const SENT: i32 = 0;
/// A plugin's exit status for a notice it did not send this time, worth handing again: sysexits'
/// `EX_TEMPFAIL`.
pub const LATER: i32 = 75;

/// What a notice tells of: the three a profile chooses a way for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A card waits on the person.
    Card,
    /// An agent's question on a task waits on them.
    Question,
    /// A reviewer gave a verdict on a task.
    Verdict,
}

impl Kind {
    /// The kind's word, as the message and a profile's `preferences` write it.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Card => "card",
            Kind::Question => "question",
            Kind::Verdict => "verdict",
        }
    }

    /// The kind `word` names.
    pub fn of(word: &str) -> Option<Kind> {
        [Kind::Card, Kind::Question, Kind::Verdict]
            .into_iter()
            .find(|kind| kind.word() == word)
    }
}

/// The program each way's notices go to, as `notices/programs.toml` names them: a way's word to the
/// absolute path of its program. A way it names none for sends nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Programs {
    ways: BTreeMap<String, PathBuf>,
}

impl Programs {
    /// The programs the machine folder `machine` names: none where it holds no
    /// `notices/programs.toml`.
    ///
    /// # Errors
    ///
    /// The folder or the file is a link, is not this account's, or another account may write it;
    /// the file does not read as TOML, or names a way or a program by another rule. Said by the
    /// path and the rule, and what to do.
    pub fn read(machine: &Path) -> Result<Programs, String> {
        let folder = machine.join(FOLDER);
        match fs::symlink_metadata(&folder) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Programs::default());
            }
            Err(error) => return Err(format!("{}: {error}", folder.display())),
            Ok(meta) if meta.file_type().is_symlink() => return Err(linked(&folder, "folder")),
            Ok(meta) => kept(&folder, &meta, "folder", fs::Metadata::is_dir)?,
        }
        let path = folder.join(PROGRAMS);
        // Opened without following a link, and what is checked is the opened file's own, so nothing
        // swapped in between the check and the read is read.
        let file = match (fs::OpenOptions::new().read(true))
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
        {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Programs::default());
            }
            Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
                return Err(linked(&path, "file"));
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
            Ok(file) => file,
        };
        let meta = file
            .metadata()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        kept(&path, &meta, "file", fs::Metadata::is_file)?;
        let text =
            io::read_to_string(file).map_err(|error| format!("{}: {error}", path.display()))?;
        Programs::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }

    /// The programs `text`, a `programs.toml`, names.
    ///
    /// # Errors
    ///
    /// It does not read as TOML, a key is no way's word, or a value is no absolute path.
    pub fn parse(text: &str) -> Result<Programs, String> {
        let table: toml::Table = text
            .parse()
            .map_err(|error: toml::de::Error| format!("not TOML: {}", error.message()))?;
        let mut ways = BTreeMap::new();
        for (way, value) in table {
            if !is_way(&way) {
                return Err(format!(
                    "{way:?} is no way: lowercase letters and digits, single dashes between them, \
                     as email, slack or whatsapp"
                ));
            }
            let program = value
                .as_str()
                .map(PathBuf::from)
                .filter(|program| program.is_absolute())
                .ok_or_else(|| {
                    format!("{way}: a program is named by its absolute path, as a quoted string")
                })?;
            ways.insert(way, program);
        }
        Ok(Programs { ways })
    }

    /// The program `way`'s notices go to: none where none is named.
    pub fn program(&self, way: &str) -> Option<&Path> {
        self.ways.get(way).map(PathBuf::as_path)
    }

    /// Whether it names no program at all.
    pub fn is_empty(&self) -> bool {
        self.ways.is_empty()
    }
}

/// Whether `text` is a way's word: lowercase ASCII letters and digits, single dashes between them.
fn is_way(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && !text.ends_with('-')
        && !text.contains("--")
        && (text.bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// What a link at `path` in the place of a `what` is said as.
fn linked(path: &Path, what: &str) -> String {
    format!(
        "{} is a link, so no program is run from it: keep the {what} itself there",
        path.display()
    )
}

/// Whether `meta`, of `path`, is a `what` (`is` says) that this account owns and no other account
/// may write.
fn kept(
    path: &Path,
    meta: &fs::Metadata,
    what: &str,
    is: fn(&fs::Metadata) -> bool,
) -> Result<(), String> {
    let shown = path.display();
    if !is(meta) {
        return Err(format!("{shown} is not a {what}"));
    }
    if meta.uid() != effective_uid() {
        return Err(format!(
            "{shown} is another account's, so no program is run from it: keep a {what} of the \
             account the server runs as"
        ));
    }
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(format!(
            "{shown} may be written by another account, so no program is run from it: chmod go-w it"
        ));
    }
    Ok(())
}

/// The account this process runs as.
#[expect(unsafe_code, reason = "geteuid: libc's own call, which reads nothing of ours")]
fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no argument and cannot fail.
    unsafe { libc::geteuid() }
}

/// The message the server writes on a notice program's stdin, one line without its newline: the
/// notice `id` of `kind`, about the page `link` of the project `project`, to `address` by `way`
/// (docs/engineering/NOTICES.md, "The message"). It goes nowhere else: never an argument, the
/// environment, a log or a file.
pub fn line(id: &str, kind: Kind, project: &str, link: &str, way: &str, address: &str) -> String {
    serde_json::json!({
        "notice": 1,
        "id": id,
        "kind": kind.word(),
        "project": project,
        "link": link,
        "to": {"way": way, "address": address},
    })
    .to_string()
}

/// What came of a hand, by the exit status of its program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Came {
    /// Sent.
    Sent,
    /// Not sent this time, worth handing again.
    Later,
    /// Not sent, and handing it again will not send it.
    Never,
}

/// What a program's end with exit status `code` says: [`SENT`] sent, [`LATER`] worth handing
/// again, any other not sent for good; an end with no status (a signal) is no status the format
/// gives, and is not sent for good either.
pub fn came(code: Option<i32>) -> Came {
    match code {
        Some(SENT) => Came::Sent,
        Some(LATER) => Came::Later,
        _ => Came::Never,
    }
}

/// What came of a notice whose program did not start for `error`, by the plugins' own rule
/// (`symphony_notice::unstarted`): worth handing again where the machine ran short for a moment
/// (of processes, memory or open files, or the program being written as it was started), else not
/// sent for good: a program that is not there, that this account may not run, or that is no
/// program fails the same on every hand.
pub fn unstarted(error: &io::Error) -> Came {
    use io::ErrorKind::{ExecutableFileBusy, OutOfMemory, WouldBlock};
    // ENFILE and EMFILE, the same numbers on Linux and macOS, have no stable kind of their own.
    let short = matches!(error.kind(), WouldBlock | OutOfMemory | ExecutableFileBusy)
        || matches!(error.raw_os_error(), Some(23 | 24));
    if short { Came::Later } else { Came::Never }
}

#[cfg(test)]
#[path = "notices_tests.rs"]
mod tests;
