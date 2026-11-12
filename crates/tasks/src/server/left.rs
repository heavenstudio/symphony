//! The private replay's record (`sf-tasks/replay.json`), so that a replay's server is never the
//! board's.
//!
//! [`Home::replay`] writes it before it starts its private server, again with that server's pid and
//! address as soon as a start of it says them, and removes it once that server has closed the store
//! cleanly. A record still there names a replay its starter did not see through: a starter that
//! ended in the middle of it (a Symphony stopped while its start ran apart, a command killed), or
//! one that gave up waiting for its server to answer. That server, detached, finishes alone, under
//! the replay's memtable, on a port no record names, and was once adopted as the board's server as
//! soon as it answered ([`Home::unrecorded`]): the board then ran under a memtable its builds never
//! start it with, and its log could grow past what a later start replays whole.
//!
//! Every start ([`Home::ensure`]) and every stop of what serves the data ([`Home::stop_serving`]:
//! the hold's, a reclaim's) settles the record first, under the starters' lock, which a replay
//! under way holds until it is through ([`Home::left_replay_through`]). A server still starting,
//! still replaying, is never signalled: the caller is refused, naming the replay in the words a
//! page reads ([`super::replaying_in`]). One that answers is stopped cleanly by its recorded pid
//! ([`Home::stop_left_replay`]), which writes its memtable to sstables and empties the log, and a
//! start then starts the board's own server under the board's memtable, with nothing left to
//! replay. A record whose server has ended, or that names none while no server runs on the data,
//! is removed. One that names no server (its starter ended between the record and the server's
//! pid, or could not write the pid) takes the server on the data that no record names for the
//! replay's, as no other start runs while it stands; one that does not read is taken as naming
//! none. Only builds that carry the record keep the rule: an older one adopts a replay's server as
//! the board's.

use super::starters::{REPLAYED_BEFORE, REPLAYING};
use super::{DATA, Home, LOG, Running, ServerError, Waited, io, storage};
use std::fs;

/// In the board's folder: the private replay whose server may still run, until it closed the store.
pub(super) const RECORD: &str = "replay.json";

/// What the server's log says of a replay whose record outlived its server: one never said through.
pub(super) const GONE: &str = "· the private replay begun above has no server running and was not \
                               said through: a start replays the store's log again if it still \
                               holds one";

/// What `sf-tasks/replay.json` says: the private replay whose server may still run.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Replay {
    /// The process that started it.
    pub by: i64,
    /// Since when, in seconds since the epoch.
    pub at: f64,
    /// The bytes of the log it replays.
    pub logged: u64,
    /// Its server's memtable, in bytes.
    pub memtable: u64,
    /// Its server, once a start of it said its pid and address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Running>,
}

impl Home {
    /// Write the replay's record whole, so a reader never sees half of it.
    pub(super) fn mark_replay(&self, replay: &Replay) -> Result<(), ServerError> {
        let path = self.dir.join(RECORD);
        let next = self.dir.join(format!("{RECORD}.{}", std::process::id()));
        let text = serde_json::to_string(replay).expect("a replay's record serialises");
        fs::write(&next, text).map_err(io("could not write", &next))?;
        fs::rename(&next, &path).map_err(io("could not write", &path))
    }

    /// The replay's record, while there is one; one that does not read names no server.
    pub(super) fn replay_record(&self) -> Option<Replay> {
        match fs::read_to_string(self.dir.join(RECORD)) {
            Ok(text) => Some(serde_json::from_str(&text).unwrap_or_default()),
            Err(problem) if problem.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => Some(Replay::default()),
        }
    }

    /// The replay's record removed, when there is one.
    pub(super) fn forget_replay(&self) -> Result<(), ServerError> {
        let path = self.dir.join(RECORD);
        match fs::remove_file(&path) {
            Err(problem) if problem.kind() != std::io::ErrorKind::NotFound => {
                Err(io("could not remove", &path)(problem))
            }
            _ => Ok(()),
        }
    }

    /// The server of a private replay whose record stands ([`RECORD`]), while it runs on this
    /// board's data: the one the record names, or, with none named, the server on the data that no
    /// address file names. Asks it nothing. What `tasks database` says is no board's server.
    pub fn left_replay(&self) -> Option<Running> {
        self.left_server(&self.replay_record()?)
    }

    /// The server `replay` names, while it serves this board's data; with none named, the server on
    /// the data that no address file names ([`Home::unrecorded`]).
    fn left_server(&self, replay: &Replay) -> Option<Running> {
        match &replay.server {
            Some(server) => self.serves(server.pid).then(|| server.clone()),
            None => self.unrecorded(),
        }
    }

    /// A private replay whose starter left its server running, settled under the starters' lock:
    /// its server, once it answers, for the caller to stop ([`Home::stop_left_replay`]); none when
    /// no record stands, or when its server has ended or never ran, the record then removed and the
    /// server's log told so ([`GONE`]). A server that runs and has not answered within the wait
    /// ([`Home::answer_wait`]) is still replaying its log: never signalled, nor adopted as the
    /// board's, and the caller refused, naming the replay.
    pub(super) fn left_replay_through(&self) -> Result<Option<Running>, ServerError> {
        let Some(replay) = self.replay_record() else {
            return Ok(None);
        };
        let gone = |home: &Home| {
            home.note_in_log(GONE);
            home.forget_replay().map(|()| None)
        };
        let Some(server) = self.left_server(&replay) else {
            return gone(self);
        };
        match self.answers(&server)? {
            Waited::Answers => Ok(Some(server)),
            Waited::Ended => gone(self),
            Waited::Silent => Err(self.replay_unanswered(&server)),
        }
    }

    /// `server`, a private replay's that its starter left running and that answers, stopped
    /// cleanly by its recorded pid ([`Home::end`]), and the replay's record removed: said to the
    /// server's log before, and after in `ending`, as a replay's end is ([`super::REPLAY_THROUGH`]),
    /// and told.
    pub(super) fn stop_left_replay(
        &self,
        server: &Running,
        ending: &str,
    ) -> Result<(), ServerError> {
        self.note_in_log(&format!(
            "· the private replay's server (pid {}, {}), left running when its starter ended, is \
             through: it is stopped cleanly by its recorded pid, never adopted as the board's server",
            server.pid, server.addr
        ));
        (self.tell)(&format!(
            "· the private replay's server (pid {}), left running when its starter ended, is \
             through: it is stopped cleanly, never adopted as the board's server",
            server.pid
        ));
        self.end(server.pid)?;
        self.forget_replay()?;
        self.note_in_log(ending);
        Ok(())
    }

    /// The refusal while `server`, a private replay's that its starter left running, has not
    /// answered: still replaying, in the words a page reads ([`super::replaying_in`]), with how a
    /// replay is told from a hang ([`Home::unanswered`]).
    fn replay_unanswered(&self, server: &Running) -> ServerError {
        ServerError::NotUp(format!(
            "{REPLAYING} of {}{REPLAYED_BEFORE} runs in its own server (pid {}, {}), left running \
             when its starter ended, and has not answered within {} s: it is never signalled while \
             it replays, nor adopted as the board's server; once it answers, the next start stops \
             it cleanly and starts the board's own. {}. Its log is {}",
            crate::size(storage::logged(&self.dir.join(DATA))),
            server.pid,
            server.addr,
            self.answer_wait().as_secs_f64(),
            self.unanswered(server.pid),
            self.dir.join(LOG).display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{GONE, RECORD, Replay};
    use crate::server::{Home, LOG, REPLAY_ENDS, REPLAY_THROUGH, Running, SURREAL_VERSION};
    use std::fs;
    use std::path::PathBuf;

    /// A board folder of its own under the temporary folder, `name`'s, served by no `surreal`.
    fn folder(name: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("left-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let home = Home::new(&dir, &dir.join("no-surreal"));
        (dir, home)
    }

    /// The record is written whole and read back as written; once removed there is none, and its
    /// removal twice is no error. One that does not read names no server.
    #[test]
    fn the_record_reads_as_written_and_one_that_does_not_read_names_no_server() {
        let (dir, home) = folder("record");
        assert_eq!(home.replay_record(), None);
        let replay = Replay {
            by: 4242,
            at: 1_794_469_500.5,
            logged: 441_648_764,
            memtable: 883_297_528,
            server: Some(Running {
                addr: "127.0.0.1:4100".into(),
                pid: 4343,
                version: SURREAL_VERSION.into(),
            }),
        };
        home.mark_replay(&replay).unwrap();
        assert_eq!(home.replay_record(), Some(replay.clone()));
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, [RECORD], "only the record, written whole");
        // Before its server's pid is known, none is written.
        let unnamed = Replay {
            server: None,
            ..replay
        };
        home.mark_replay(&unnamed).unwrap();
        assert!(
            !fs::read_to_string(dir.join(RECORD))
                .unwrap()
                .contains("server"),
            "no server named"
        );
        assert_eq!(home.replay_record(), Some(unnamed));
        fs::write(dir.join(RECORD), "{not json").unwrap();
        assert_eq!(home.replay_record(), Some(Replay::default()));
        home.forget_replay().unwrap();
        assert_eq!(home.replay_record(), None);
        home.forget_replay().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    /// A record whose server no longer serves the data, and one that names none while no server
    /// runs on it, are settled as gone: removed, and the server's log says the replay was not said
    /// through. No record, nothing.
    #[test]
    fn a_record_whose_server_is_gone_is_removed_and_said() {
        let (dir, home) = folder("gone");
        assert_eq!(home.left_replay_through().unwrap(), None);
        assert!(!dir.join(LOG).exists(), "nothing said without a record");
        let ended = Running {
            addr: "127.0.0.1:4100".into(),
            // No process of this test's data: this test process itself runs no server on it.
            pid: i64::from(std::process::id()),
            version: SURREAL_VERSION.into(),
        };
        for server in [Some(ended), None] {
            home.mark_replay(&Replay {
                server,
                ..Replay::default()
            })
            .unwrap();
            assert_eq!(home.left_replay(), None);
            assert_eq!(home.left_replay_through().unwrap(), None);
            assert_eq!(home.replay_record(), None, "removed");
        }
        assert_eq!(
            fs::read_to_string(dir.join(LOG)).unwrap(),
            format!("{GONE}\n{GONE}\n")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The line a start's replay ends with is a replay's end, as one its starter left ends.
    #[test]
    fn a_replay_ends_in_the_words_every_replays_end_begins_with() {
        assert!(REPLAY_ENDS.starts_with(&format!("{REPLAY_THROUGH}: ")));
    }
}
