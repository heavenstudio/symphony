//! [`Home::starting_apart`]: a start that replays the store's log runs on a thread of its own, its
//! caller refused at once naming it, for a caller whose thread has others to serve: the Symphony's
//! reads, whose data worker serves its pages too. Such a caller is never held by a replay of
//! minutes, its own start's ([`Home::start_apart`]) or another's ([`Home::waiting_at_most`]).
//!
//! What this process runs apart is kept by the board's folder, process-wide, so each read after the
//! first is refused at once while the start runs, and the next read after a start that failed is
//! told why, once; the one after that starts again.

use crate::lock::Recovered as _;

use super::starters::{Doing, Note};
use super::{Home, ServerError};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// The starts this process runs apart, by their board's folder.
static APART: Mutex<BTreeMap<PathBuf, Apart>> = Mutex::new(BTreeMap::new());

/// A start run apart.
#[derive(Debug, Clone)]
enum Apart {
    /// Under way, as the starters' note says a replay: this process, since when, of how large a log.
    Running(Note),
    /// Ended without the board's server, said, until a read is told.
    Failed(String),
}

/// What is said of a start run apart before it ends: what it does, as the lock's note says it.
const STARTED_APART: &str = "the board's server is started apart from this read, by";

impl Home {
    /// A start that replays the store's log runs apart from its caller, on a thread of its own, and
    /// the caller is refused at once, naming it ([`Home::start_apart`]); so are its next ones while
    /// that start runs. Otherwise a start replays on its caller's thread.
    pub fn starting_apart(self) -> Home {
        Home {
            apart: true,
            ..self
        }
    }

    /// Refused while this process runs a start of this board apart, naming it, or once when the
    /// last one failed, saying why; nothing when there is neither.
    pub(super) fn apart_under_way(&self) -> Result<(), ServerError> {
        let mut apart = APART.lock().recover();
        match apart.get(&self.dir) {
            None => Ok(()),
            Some(Apart::Running(note)) => Err(self.started_apart(note)),
            Some(Apart::Failed(_)) => match apart.remove(&self.dir) {
                Some(Apart::Failed(why)) => Err(ServerError::NotUp(format!(
                    "the board's server, started apart from the Symphony's reads, did not start: \
                     {why}; the next read starts it again"
                ))),
                _ => Ok(()),
            },
        }
    }

    /// Start the board's server on a thread of its own, its store's log of `logged` bytes to be
    /// replayed first ([`Home::replay`]), and say so: the refusal its caller is given. The thread
    /// takes the starters' lock once its caller lets it go, and starts as any start does, its
    /// waits unbounded; whatever it tells goes where this board tells, and its failure too.
    pub(super) fn start_apart(&self, logged: u64) -> ServerError {
        let note = Note {
            pid: i64::from(std::process::id()),
            doing: Doing::Replaying,
            at: crate::store::now(),
            logged: Some(logged),
        };
        let mut apart = APART.lock().recover();
        if let Some(Apart::Running(under_way)) = apart.get(&self.dir) {
            return self.started_apart(under_way);
        }
        let home = Home {
            apart: false,
            lock_wait: None,
            ..self.clone()
        };
        let dir = self.dir.clone();
        let spawned = std::thread::Builder::new()
            .name("board start".into())
            .spawn(move || {
                let started = home.ensure();
                let mut apart = APART.lock().recover();
                match started {
                    Ok(_) => {
                        apart.remove(&dir);
                    }
                    Err(problem) => {
                        let why = problem.to_string();
                        (home.tell)(&format!(
                            "· the board's server, started apart from the Symphony's reads, did \
                             not start: {why}"
                        ));
                        apart.insert(dir, Apart::Failed(why));
                    }
                }
            });
        if let Err(problem) = spawned {
            return ServerError::Io(format!(
                "could not start the board's server apart from this read: {problem}"
            ));
        }
        apart.insert(self.dir.clone(), Apart::Running(note.clone()));
        self.started_apart(&note)
    }

    /// The refusal of a read while a start of this board runs apart, `note` saying what it does.
    fn started_apart(&self, note: &Note) -> ServerError {
        ServerError::NotUp(format!(
            "{STARTED_APART} {}; this read does not wait for it, and the next one asks again",
            self.said(note)
        ))
    }
}

/// What a page says of a board whose refusal (or any text holding it) is `said`, while a start
/// replays its store's log, this process's or another's: what it does and how long it takes; none
/// when `said` holds no replay.
pub fn replaying_in(said: &str) -> Option<String> {
    let at = said.find(super::starters::REPLAYING)?;
    let rest = &said[at + super::starters::REPLAYING.len()..];
    let (of, _) = rest.split_once(super::starters::REPLAYED_BEFORE)?;
    Some(format!(
        "its store's log{of} is replayed in a private server before the board's server starts (a \
         log of gigabytes takes minutes), and the board answers once that is through"
    ))
}

#[cfg(test)]
mod tests {
    use super::{APART, Apart, replaying_in};
    use crate::server::Home;
    use crate::server::starters::{Doing, Note};

    /// The words of a replay, in a refusal of a start run apart or of a wait on another's start, and
    /// in whatever wraps either, are what a page says; any other refusal holds none.
    #[test]
    fn a_page_says_a_replay_from_any_refusal_that_names_one() {
        let dir = std::env::temp_dir().join(format!("apart-words-{}", std::process::id()));
        let home = Home::new(&dir, &dir.join("surreal"));
        let note = |logged| Note {
            pid: 4242,
            doing: Doing::Replaying,
            at: 0.0,
            logged,
        };
        let said = home.started_apart(&note(Some(3 << 20))).to_string();
        assert!(
            said.starts_with(
                "the board's server is started apart from this read, by pid 4242, since "
            ) && said.ends_with(&format!(
                ": a private replay of the store's log of 3.0 MiB before the board's server \
                 starts: a log of gigabytes takes minutes, and its server's lines go to {}; this \
                 read does not wait for it, and the next one asks again",
                dir.join("server.log").display()
            )),
            "{said}"
        );
        let words = "its store's log of 3.0 MiB is replayed in a private server before the board's \
                     server starts (a log of gigabytes takes minutes), and the board answers once \
                     that is through";
        assert_eq!(replaying_in(&said).as_deref(), Some(words));
        assert_eq!(
            replaying_in(&format!("no pass, the board could not be reached: {said}")).as_deref(),
            Some(words)
        );
        // A note that says no size: none said.
        assert_eq!(
            replaying_in(&home.started_apart(&note(None)).to_string()).as_deref(),
            Some(
                "its store's log is replayed in a private server before the board's server starts \
                 (a log of gigabytes takes minutes), and the board answers once that is through"
            )
        );
        for other in [
            "connection refused",
            "the board's starters' lock (x) is held by pid 1, since 10:00: a start of the board's \
             server (or a wait for it to answer)",
            "a private replay of the store's log",
        ] {
            assert_eq!(replaying_in(other), None, "{other}");
        }
    }

    /// A read while a start runs apart is refused at once naming it; the read after one that
    /// failed is told why, once, and the next finds none under way.
    #[test]
    fn a_start_run_apart_refuses_each_read_while_it_runs_and_tells_its_failure_once() {
        let dir = std::env::temp_dir().join(format!("apart-reads-{}", std::process::id()));
        let home = Home::new(&dir, &dir.join("surreal"));
        assert!(home.apart_under_way().is_ok(), "none under way");
        let note = Note {
            pid: 4242,
            doing: Doing::Replaying,
            at: 0.0,
            logged: Some(1 << 30),
        };
        APART
            .lock()
            .unwrap()
            .insert(dir.clone(), Apart::Running(note.clone()));
        let running = home.apart_under_way().unwrap_err();
        assert_eq!(running, home.started_apart(&note));
        assert_eq!(home.apart_under_way().unwrap_err(), running, "each read");
        // A second start apart of the same board starts nothing more, and is refused the same way.
        assert_eq!(home.start_apart(1), running);

        APART
            .lock()
            .unwrap()
            .insert(dir.clone(), Apart::Failed("its surreal is gone".into()));
        assert_eq!(
            home.apart_under_way().unwrap_err().to_string(),
            "the board's server, started apart from the Symphony's reads, did not start: its \
             surreal is gone; the next read starts it again"
        );
        assert!(home.apart_under_way().is_ok(), "told once");
        assert!(!APART.lock().unwrap().contains_key(&dir));
    }
}
