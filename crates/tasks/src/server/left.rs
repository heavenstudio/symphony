//! The private replay's record (`symphony/tasks/replay.json`), so that a replay's server is never
//! the board's.
//!
//! [`Home::replay`] writes it before it starts its private server, again with that server's pid and
//! address as soon as a start of it says them, and removes it once that server has closed the store
//! cleanly. A record still there names a replay its starter did not see through: a starter that
//! ended in the middle of it (a Symphony stopped while its start ran apart, a command killed), or
//! one that gave up waiting for its server to answer. That server, detached, finishes alone, under
//! the replay's memtable, on a port no address file names, and was once adopted as the board's
//! server as soon as it answered ([`Home::unrecorded`]): the board then ran under a memtable its
//! builds never start it with, and its log could grow past what a later start replays whole.
//!
//! While the record stands, every server on the data is the replay's ([`Home::left_server`]): the
//! one it names, or, when that one is not on the data (a start retried on another port, its starter
//! ended before the record named the retry's server; a name that could not be written), any other a
//! listing of every process finds there, the one the address file names included. Every start
//! ([`Home::ensure`]) and every stop of what serves the data ([`Home::stop_serving`]: the hold's, a
//! reclaim's) settles the record first, under the starters' lock, which a replay under way holds
//! until it is through ([`Home::left_replay_through`]). A server still starting, still replaying,
//! is never signalled: the caller is refused, naming the replay in the words a page reads
//! ([`super::replaying_in`]). One that answers is stopped cleanly, once ([`Home::stop_left_replay`],
//! [`Home::end`]), which writes its memtable to sstables and empties the log, and a start then
//! starts the board's own server under the board's memtable, with nothing left to replay.
//!
//! The record is removed only once nothing it may name runs: the server it names has ended, or it
//! names none, and every listing of every process for a second ([`GONE_FOR`]) shows no server on
//! the data, past a process seen between two programs. The server it names has ended once no
//! process has its pid, or once the process that has it started at another time than the one the
//! record wrote down ([`Home::process_start`]): a pid since reused, after a reboot in the middle of
//! a replay say, is not taken for it. A listing that fails, and a named
//! server that runs while neither it nor any other server is on the data (a process between
//! programs as it starts), cannot be told from a replay that runs: the caller is refused, nothing
//! adopted, started or removed, and the next command asks again. A record that does not read is
//! taken as naming none. Only builds that carry the record keep the rule: an older one adopts a
//! replay's server as the board's.

use super::starters::{REPLAYED_BEFORE, REPLAYING};
use super::{DATA, Home, LOG, Running, ServerError, Waited, io, storage, untold};
use crate::sys::pid_alive;
use std::fs;
use std::time::{Duration, Instant};

/// In the board's folder: the private replay whose server may still run, until it closed the store.
pub(super) const RECORD: &str = "replay.json";

/// What the server's log says of a replay whose record outlived its server: one never said through.
pub(super) const GONE: &str = "· the private replay begun above has no server running and was not \
                               said through: a start replays the store's log again if it still \
                               holds one";

/// How the server's log's line begins when a server on the data is stopped while the private
/// replay's record stands, before which server it is and what becomes of it.
pub const LEFT_STOPS: &str = "· the private replay's record stands, and ";

/// How many times a settling looks again for a server on the data, each one it found having ended
/// while it was waited for, before it is refused as one that cannot be told.
const SETTLES: usize = 3;

/// How long what runs on the data is asked again while it cannot be told, before the caller is
/// refused: long past a `ps` that failed once or a process's move from one program to the next.
const TELLING: Duration = Duration::from_secs(2);

/// How long every look must find nothing the record may name running before the record is
/// settled as gone (or the caller's bound, when shorter): past a process's move from one program to
/// the next, in which no listing names it a server, as a wrapper that runs `surreal` by `exec`
/// leaves one between the two.
const GONE_FOR: Duration = Duration::from_secs(1);

/// How long a settling waits after one look before the next ([`Home::left_server`]).
const LOOK_AGAIN: Duration = Duration::from_millis(50);

/// What `symphony/tasks/replay.json` says: the private replay whose server may still run.
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
    /// When that server's process started ([`Home::process_start`]), so its pid, reused, is told
    /// from it; none where `ps` could not say, the pid then taken for it while it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
}

/// A server on this board's data while the private replay's record stands, which takes it for the
/// replay's ([`Home::left_replay`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LeftReplay {
    /// The server.
    pub server: Running,
    /// Whether the record names it; one it does not name is another server on the data.
    pub named: bool,
}

/// The refusal when the record could not name `server`, a start of the replay's: it runs on, never
/// signalled before it answers, and the record that stands takes it for the replay's.
pub(super) fn unnamed(server: &Running, problem: &ServerError) -> ServerError {
    ServerError::NotUp(format!(
        "the private replay's server (pid {}, {}) started, and its record could not name it: \
         {problem}. It is left running, never signalled before it answers; the record that stands \
         takes every server on the data for the replay's, so the next start stops it cleanly once \
         it answers and starts the board's own",
        server.pid, server.addr
    ))
}

impl Home {
    /// Write the replay's record whole, so a reader never sees half of it; a write that fails
    /// leaves the record as it was, and no half-written file beside it.
    pub(super) fn mark_replay(&self, replay: &Replay) -> Result<(), ServerError> {
        let path = self.dir.join(RECORD);
        let next = self.dir.join(format!("{RECORD}.{}", std::process::id()));
        let text = serde_json::to_string(replay).expect("a replay's record serialises");
        fs::write(&next, text)
            .map_err(io("could not write", &next))
            .and_then(|()| fs::rename(&next, &path).map_err(io("could not write", &path)))
            .inspect_err(|_| {
                let _ = fs::remove_file(&next);
            })
    }

    /// The replay's record, while there is one; one that does not read names no server.
    pub(super) fn replay_record(&self) -> Option<Replay> {
        match fs::read_to_string(self.dir.join(RECORD)) {
            Ok(text) => Some(crate::objects::parse(&text).unwrap_or_default()),
            Err(problem) if problem.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => Some(Replay::default()),
        }
    }

    /// The replay's record removed, when there is one, with what a write of it cut short left.
    pub(super) fn forget_replay(&self) -> Result<(), ServerError> {
        let path = self.dir.join(RECORD);
        if let Ok(entries) = fs::read_dir(&self.dir) {
            let half = format!("{RECORD}.");
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&half) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        match fs::remove_file(&path) {
            Err(problem) if problem.kind() != std::io::ErrorKind::NotFound => {
                Err(io("could not remove", &path)(problem))
            }
            _ => Ok(()),
        }
    }

    /// The server on this board's data while a private replay's record stands ([`RECORD`]), which
    /// takes it for the replay's ([`Home::left_server`]), asked once; none when no record stands,
    /// none runs or that cannot be told. Asks the server nothing. What `tasks database` says is no
    /// board's server.
    pub fn left_replay(&self) -> Option<LeftReplay> {
        self.left_replay_told().ok().flatten()
    }

    /// [`Home::left_replay`], with why what runs cannot be told when it cannot: the board's next
    /// command is then refused, and `tasks database` says so.
    pub fn left_replay_told(&self) -> Result<Option<LeftReplay>, String> {
        match self.replay_record() {
            Some(replay) => self.left_server_now(&replay),
            None => Ok(None),
        }
    }

    /// [`Home::left_server_now`], asked again while what runs cannot be told, for at most
    /// [`TELLING`] or the wait ([`Home::answer_wait`]), whichever is shorter: a `ps` that failed
    /// once, a process between programs as it starts. Refused, saying why, once that has passed.
    /// None only once every look for [`GONE_FOR`], or for that bound when it is shorter, found
    /// nothing the record may name: a server seen between two programs is found at the next look.
    fn left_server(&self, replay: &Replay) -> Result<Option<LeftReplay>, ServerError> {
        self.left_server_by(|| self.left_server_now(replay), Instant::now)
    }

    /// [`Home::left_server`], each of its looks made by `look` and the time read from `now`:
    /// [`Home::left_server_now`] and [`Instant::now`], or a test's, which counts the looks, moves a
    /// process from one program to the next between two, or moves its own clock at each look.
    fn left_server_by(
        &self,
        mut look: impl FnMut() -> Result<Option<LeftReplay>, String>,
        now: impl Fn() -> Instant,
    ) -> Result<Option<LeftReplay>, ServerError> {
        let bound = self.answer_wait().min(TELLING);
        let until = now() + bound;
        let mut gone_since = None;
        loop {
            match look() {
                Ok(None) => {
                    let since = *gone_since.get_or_insert_with(&now);
                    if now() - since >= GONE_FOR.min(bound) {
                        return Ok(None);
                    }
                }
                Err(_) if now() < until => gone_since = None,
                told => return told.map_err(|why| self.cannot_tell(&why)),
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: looks again for a session's process to say it left, which no event announces"
            )]
            std::thread::sleep(LOOK_AGAIN);
        }
    }

    /// Whether the server `replay` names still runs: its pid has a process that started when the
    /// record says it did ([`Home::process_start`]), or, with no start written down, any process.
    /// No when it names none, its pid has none, or the process that has it started at another
    /// time; why not, when the kernel does not say when that process started.
    fn named_runs(replay: &Replay) -> Result<bool, String> {
        let Some(server) = &replay.server else {
            return Ok(false);
        };
        if !pid_alive(server.pid) {
            return Ok(false);
        }
        let Some(started) = &replay.started else {
            return Ok(true);
        };
        match Home::process_start(server.pid) {
            Ok(now) => Ok(&now == started),
            Err(_) if !pid_alive(server.pid) => Ok(false),
            Err(problem) => Err(format!(
                "when pid {}, which it names, started cannot be read ({problem})",
                server.pid
            )),
        }
    }

    /// The server a private replay whose record is `replay` leaves on this board's data: the one it
    /// names, while that one runs ([`Home::named_runs`]) and serves the data, else whichever a
    /// listing of every process finds there ([`Home::listed`]), the one the address file names
    /// included. None only once the server it names has ended, or it names none, and a listing
    /// that ran shows no server on the data. Why that cannot be told, when it cannot: a `ps` that
    /// fails, or a named server that runs while neither it nor any other server is on the data.
    fn left_server_now(&self, replay: &Replay) -> Result<Option<LeftReplay>, String> {
        let runs = Self::named_runs(replay);
        if let (Ok(true), Some(server)) = (&runs, &replay.server)
            && self.serves(server.pid)
        {
            return Ok(Some(LeftReplay {
                server: server.clone(),
                named: true,
            }));
        }
        match self.listed() {
            Err(problem) => Err(format!(
                "`ps`, which lists the processes, failed ({problem})"
            )),
            Ok(Some(found)) => {
                let named = runs == Ok(true)
                    && replay
                        .server
                        .as_ref()
                        .is_some_and(|server| server.pid == found.pid);
                Ok(Some(LeftReplay {
                    server: found,
                    named,
                }))
            }
            Ok(None) => match runs {
                Ok(false) => Ok(None),
                Err(why) => Err(why),
                Ok(true) => Err(format!(
                    "the server it names, pid {}, runs, and neither it nor any other process is a \
                     server on this board's data as `ps` lists them: it may be between programs \
                     as it starts",
                    replay.server.as_ref().map_or(0, |server| server.pid)
                )),
            },
        }
    }

    /// A private replay whose starter did not see it through, settled under the starters' lock:
    /// the server on the data it leaves ([`Home::left_server`]), once it answers, for the caller to
    /// stop ([`Home::stop_left_replay`]); none when no record stands, or when nothing it may name
    /// runs, the record then removed and the server's log told so ([`GONE`]). A server that runs
    /// and has not answered within the wait ([`Home::answer_wait`]) is still replaying its log:
    /// never signalled, nor adopted as the board's, and the caller refused, naming the replay. One
    /// that ends while it is waited for is settled again. Refused, and nothing removed, while what
    /// runs cannot be told, or whether its answer is its own ([`untold`]).
    pub(super) fn left_replay_through(&self) -> Result<Option<LeftReplay>, ServerError> {
        let Some(replay) = self.replay_record() else {
            return Ok(None);
        };
        for _ in 0..SETTLES {
            let Some(left) = self.left_server(&replay)? else {
                self.note_in_log(GONE);
                self.forget_replay()?;
                return Ok(None);
            };
            match self.answers(&left.server) {
                Waited::Answers => return Ok(Some(left)),
                // Stopped next, it is never taken as answering on a check that could not say.
                Waited::Unsure(why) => return Err(untold(&left.server, &why)),
                Waited::Silent => return Err(self.replay_unanswered(&left)),
                Waited::Ended => {}
            }
        }
        Err(self.cannot_tell(&format!(
            "{SETTLES} servers on its data in turn ended while each was waited for"
        )))
    }

    /// After a start of the private replay that failed, its record removed, and that said
    /// ([`GONE`]), only once none of the servers it `started` runs and nothing the record may name
    /// does ([`Home::left_server`]); kept otherwise, and while that cannot be told, for the next
    /// start to settle. A removal that fails keeps it too.
    pub(super) fn settle_failed_replay(&self, started: &[i64]) {
        if started.iter().any(|&pid| pid_alive(pid)) {
            return;
        }
        if let Some(replay) = self.replay_record()
            && matches!(self.left_server(&replay), Ok(None))
        {
            self.note_in_log(GONE);
            let _ = self.forget_replay();
        }
    }

    /// `left`, a server on the data that answers while the private replay's record stands, stopped
    /// cleanly ([`Home::end`]) and the record removed: said to the server's log before, which
    /// server it is ([`LEFT_STOPS`]), and after in `ending`, as a replay's end is
    /// ([`super::REPLAY_THROUGH`]), and told.
    pub(super) fn stop_left_replay(
        &self,
        left: &LeftReplay,
        ending: &str,
    ) -> Result<(), ServerError> {
        let recorded = self
            .recorded()
            .is_some_and(|running| running.pid == left.server.pid);
        let (which, besides) = match (left.named, recorded) {
            (true, false) => (
                "the server it names",
                ", left running when its starter ended,",
            ),
            (true, true) => ("the server it names", ", which the address file names too,"),
            (false, true) => (
                "the server the address file names",
                ", which the record does not name,",
            ),
            (false, false) => ("a server on the data that it does not name", ""),
        };
        let said = format!(
            "{LEFT_STOPS}{which} (pid {}, {}){besides} answers: while the record stands every \
             server on the data is taken for the replay's, so this one is stopped cleanly, never \
             adopted as the board's server",
            left.server.pid, left.server.addr
        );
        self.note_in_log(&said);
        (self.tell)(&said);
        self.end(left.server.pid)?;
        self.forget_replay()?;
        self.note_in_log(ending);
        Ok(())
    }

    /// The refusal while `left`, a server on the data while the private replay's record stands,
    /// has not answered: still replaying, in the words a page reads ([`super::replaying_in`]), with
    /// how a replay is told from a hang ([`Home::unanswered`]).
    fn replay_unanswered(&self, left: &LeftReplay) -> ServerError {
        ServerError::NotUp(format!(
            "{REPLAYING} of {}{REPLAYED_BEFORE} runs in {} (pid {}, {}), left running when its \
             starter ended, and has not answered within {} s: it is never signalled while it \
             replays, nor adopted as the board's server; once it answers, the next start stops it \
             cleanly and starts the board's own. {}. Its log is {}",
            crate::size(storage::logged(&self.dir.join(DATA))),
            if left.named {
                "its own server"
            } else {
                "a server on the data its record does not name, taken for its own"
            },
            left.server.pid,
            left.server.addr,
            self.answer_wait().as_secs_f64(),
            self.unanswered(left.server.pid),
            self.dir.join(LOG).display()
        ))
    }

    /// The refusal while whether the private replay's server still runs cannot be told, for `why`.
    fn cannot_tell(&self, why: &str) -> ServerError {
        ServerError::NotUp(format!(
            "whether the private replay's server still runs on this board's data cannot be told: \
             {why}. {}",
            self.untold_rule()
        ))
    }

    /// What holds while whether the private replay's server still runs cannot be told, and what
    /// the owner does should it last: what a refusal and `tasks database` say after the reason.
    pub fn untold_rule(&self) -> String {
        format!(
            "While its record ({}) stands, every server on the data is the replay's: none is \
             adopted as the board's, none is started beside it, and the record is removed only once \
             the server it names has ended and `ps` lists no server on the data. The next command \
             asks again, and settles the record once `ps` can tell. Should it never, remove the \
             record by hand once `ps -ww -axo pid,command` lists no `surreal start` on {} \
             (docs/TASKS.md, Stuck servers)",
            self.dir.join(RECORD).display(),
            self.data()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{GONE, LeftReplay, RECORD, Replay};
    use crate::server::{
        Home, LOG, PS_FAILS, REPLAY_ENDS, REPLAY_THROUGH, Running, SURREAL_VERSION, ServerError,
    };
    use crate::sys::{Stdio, command};
    use std::cell::Cell;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// A board folder of its own under the temporary folder, `name`'s, served by no `surreal`, its
    /// patience short: what cannot be told is asked again for a fifth of a second.
    fn folder(name: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("left-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let home = Home::new(&dir, &dir.join("no-surreal")).patience(Duration::from_millis(200));
        (dir, home)
    }

    /// A server as a record names it, of pid `pid`.
    fn server(pid: i64) -> Option<Running> {
        Some(Running {
            addr: "127.0.0.1:4100".into(),
            pid,
            version: SURREAL_VERSION.into(),
        })
    }

    /// The pid of a process that has ended and been reaped.
    fn ended_pid() -> i64 {
        let mut child = command("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        i64::from(child.id())
    }

    /// The next `count` of `ps` fail while it lives, on this thread; `u32::MAX` for every one.
    struct PsFails;
    impl PsFails {
        fn new(count: u32) -> PsFails {
            PS_FAILS.set(count);
            PsFails
        }
    }
    impl Drop for PsFails {
        fn drop(&mut self) {
            PS_FAILS.set(0);
        }
    }

    /// The record is written whole and read back as written; once removed there is none, nor what
    /// a write cut short left, and its removal twice is no error. One that does not read names no
    /// server.
    #[test]
    fn the_record_reads_as_written_and_one_that_does_not_read_names_no_server() {
        let (dir, home) = folder("record");
        assert_eq!(home.replay_record(), None);
        let replay = Replay {
            by: 4242,
            at: 1_794_469_500.5,
            logged: 441_648_764,
            memtable: 883_297_528,
            server: server(4343),
            started: Some("1794469501.204918".into()),
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
            started: None,
            ..replay
        };
        home.mark_replay(&unnamed).unwrap();
        let written = fs::read_to_string(dir.join(RECORD)).unwrap();
        assert!(
            !written.contains("server") && !written.contains("started"),
            "no server named: {written}"
        );
        assert_eq!(home.replay_record(), Some(unnamed));
        fs::write(dir.join(RECORD), "{not json").unwrap();
        assert_eq!(home.replay_record(), Some(Replay::default()));
        fs::write(dir.join(format!("{RECORD}.4242")), "{\"by\":").unwrap();
        home.forget_replay().unwrap();
        assert_eq!(home.replay_record(), None);
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            0,
            "a cut-short write stays"
        );
        home.forget_replay().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    /// A write of the record that fails leaves nothing half written beside it.
    #[test]
    fn a_write_of_the_record_that_fails_leaves_nothing_half_written() {
        let (dir, home) = folder("unwritten");
        let first = Replay {
            by: 1,
            ..Replay::default()
        };
        home.mark_replay(&first).unwrap();
        fs::remove_file(dir.join(RECORD)).unwrap();
        fs::create_dir(dir.join(RECORD)).unwrap();
        fs::write(dir.join(RECORD).join("inside"), "").unwrap();
        assert!(home.mark_replay(&first).is_err());
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [RECORD], "a half-written record stays beside it");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A record whose named pid has ended, and one that names none, while a listing that ran shows
    /// no server on the data, are settled as gone: removed, and the server's log says the replay
    /// was not said through. No record, nothing.
    #[test]
    fn a_record_whose_server_has_ended_is_removed_and_said() {
        let (dir, home) = folder("gone");
        assert_eq!(home.left_replay_through().unwrap(), None);
        assert!(!dir.join(LOG).exists(), "nothing said without a record");
        for named in [server(ended_pid()), None] {
            home.mark_replay(&Replay {
                server: named,
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

    /// A record whose named pid runs while no server is on the data cannot be told from a replay
    /// between programs as it starts: the caller is refused naming that pid, and the record stays.
    /// So with no start written down, and with the start of the process that has the pid.
    #[test]
    fn a_record_naming_a_pid_that_runs_off_the_data_is_never_removed() {
        let (dir, home) = folder("runs");
        // This test's own process: it runs, and serves no data.
        let pid = i64::from(std::process::id());
        for started in [None, Some(Home::process_start(pid).unwrap())] {
            let replay = Replay {
                server: server(pid),
                started,
                ..Replay::default()
            };
            home.mark_replay(&replay).unwrap();
            let refused = home.left_replay_through().unwrap_err().to_string();
            assert!(
                refused.starts_with(&format!(
                    "whether the private replay's server still runs on this board's data cannot \
                     be told: the server it names, pid {pid}, runs, and neither it nor any other"
                )),
                "{refused}"
            );
            assert_eq!(home.replay_record(), Some(replay));
            home.settle_failed_replay(&[]);
            assert!(
                dir.join(RECORD).exists(),
                "a failed start's settling removed it"
            );
        }
        assert!(
            !dir.join(LOG).exists(),
            "said gone: {:?}",
            fs::read_to_string(dir.join(LOG))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A record naming a pid that a process has which started at another time than the one it
    /// wrote down names a server that has ended, its pid since reused: settled as gone, with no
    /// hand step.
    #[test]
    fn a_record_naming_a_reused_pid_is_settled_as_gone() {
        let (dir, home) = folder("reused");
        home.mark_replay(&Replay {
            server: server(i64::from(std::process::id())),
            started: Some("1.000000".into()),
            ..Replay::default()
        })
        .unwrap();
        assert_eq!(home.left_replay_told(), Ok(None));
        assert_eq!(home.left_replay_through().unwrap(), None);
        assert_eq!(home.replay_record(), None, "removed");
        assert_eq!(
            fs::read_to_string(dir.join(LOG)).unwrap(),
            format!("{GONE}\n")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A server that no listing names one at a look, between two programs as a wrapper runs
    /// `surreal` by `exec`, while the record names a pid that has ended, is found at the next look
    /// once it is one, never settled as gone: the record is settled as gone only after every look
    /// for a second found none. What it is about is counted in looks, never timed: the process
    /// moves to the server's program between the first look and the second, whatever the load.
    #[test]
    fn a_server_seen_between_two_programs_is_found_never_settled_as_gone() {
        let (dir, home) = folder("between");
        let home = home.patience(Duration::from_secs(5));
        // In the place of `surreal`, which the folder's board runs: a program that runs on.
        let surreal = dir.join("no-surreal");
        fs::write(&surreal, "#!/bin/sh\nwhile :; do sleep 0.05; done\n").unwrap();
        fs::set_permissions(&surreal, fs::Permissions::from_mode(0o755)).unwrap();
        let replay = Replay {
            server: server(ended_pid()),
            ..Replay::default()
        };
        home.mark_replay(&replay).unwrap();
        // No server's command line until its stdin closes (its words are in its environment), then
        // a server on the data by `exec`, its pid the same.
        let mut between = command("/bin/sh")
            .arg("-c")
            .arg("read go; exec \"$SYMPHONY_SURREAL\" start --bind 127.0.0.1:1 \"$SYMPHONY_DATA\"")
            .env("SYMPHONY_SURREAL", &surreal)
            .env("SYMPHONY_DATA", home.data())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = i64::from(between.id());
        let mut go = between.stdin.take();
        let (mut looks, mut moved) = (Vec::new(), None);
        let left = home.left_server_by(
            || {
                let found = home.left_server_now(&replay);
                looks.push(found.clone());
                // After the first look, which found none: the process moves to the server's
                // program, and the next look is made once a listing shows it there.
                if let Some(go) = go.take() {
                    drop(go);
                    moved = Some(test_time::until_true(
                        "the process's exec into a server on the data",
                        test_time::backstop(Duration::from_secs(1)),
                        || matches!(home.listed(), Ok(Some(listed)) if listed.pid == pid),
                    ));
                }
                found
            },
            Instant::now,
        );
        between.kill().unwrap();
        between.wait().unwrap();
        moved.expect("a first look").unwrap();
        let found = LeftReplay {
            server: Running {
                addr: "127.0.0.1:1".into(),
                pid,
                version: SURREAL_VERSION.into(),
            },
            named: false,
        };
        assert_eq!(looks, [Ok(None), Ok(Some(found.clone()))]);
        assert_eq!(left, Ok(Some(found)));
        assert!(dir.join(RECORD).exists());
        assert!(!dir.join(LOG).exists(), "said gone");

        // With none ever on the data, it is settled as gone after a second of looks.
        let began = Instant::now();
        assert_eq!(home.left_server(&replay), Ok(None));
        assert!(began.elapsed() >= super::GONE_FOR);
        let _ = fs::remove_dir_all(&dir);
    }

    /// When the kernel does not say when the process that has the named pid started, whether it is
    /// the replay's cannot be told: said why, and the record kept.
    #[test]
    fn a_record_is_kept_while_when_its_named_process_started_cannot_be_read() {
        let (dir, home) = folder("untold-start");
        let pid = i64::from(std::process::id());
        let replay = Replay {
            server: server(pid),
            started: Some("1.000000".into()),
            ..Replay::default()
        };
        home.mark_replay(&replay).unwrap();
        {
            let _failing = PsFails::new(1);
            assert_eq!(
                home.left_replay_told(),
                Err(format!(
                    "when pid {pid}, which it names, started cannot be read (ps failed, as a test \
                     made it)"
                ))
            );
        }
        {
            let _failing = PsFails::new(u32::MAX);
            home.settle_failed_replay(&[]);
        }
        assert_eq!(home.replay_record(), Some(replay));
        assert!(!dir.join(LOG).exists(), "said gone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// While `ps` fails, no record is ever removed, whether it names a pid that has ended or none:
    /// a start is refused saying it cannot tell, and a failed start's settling keeps it.
    #[test]
    fn a_record_is_never_removed_while_ps_fails() {
        let (dir, home) = folder("ps-fails");
        for named in [server(ended_pid()), None] {
            let replay = Replay {
                server: named,
                ..Replay::default()
            };
            home.mark_replay(&replay).unwrap();
            let _failing = PsFails::new(u32::MAX);
            let began = Instant::now();
            let refused = home.left_replay_through().unwrap_err();
            assert!(began.elapsed() >= Duration::from_millis(200), "asked once");
            assert!(
                matches!(&refused, ServerError::NotUp(why) if why.starts_with(
                    "whether the private replay's server still runs on this board's data cannot \
                     be told: `ps`, which lists the processes, failed (ps failed, as a test made \
                     it). While its record ("
                )),
                "{refused}"
            );
            assert_eq!(home.left_replay(), None);
            home.settle_failed_replay(&[]);
            assert_eq!(
                home.replay_record(),
                Some(replay),
                "removed while ps failed"
            );
        }
        assert!(!dir.join(LOG).exists(), "said gone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A `ps` that fails for a moment is asked again, and what it tells once it answers is taken:
    /// here none, its named pid having ended, once every look for the patience found none. One
    /// that still fails once the patience has passed is refused, saying so. Counted in looks,
    /// never timed: the loop reads a clock of the test's, which moves one pause at each look, so
    /// the third look is the last within the folder's patience of 200 ms whatever the load.
    #[test]
    fn a_ps_that_fails_for_a_moment_is_asked_again() {
        let (dir, home) = folder("ps-moment");
        let replay = Replay {
            server: server(ended_pid()),
            ..Replay::default()
        };
        // What each look found and what was told, while the first `fails` of `ps` fail.
        let looked = |fails: u32| {
            let clock = Cell::new(Instant::now());
            let mut looks = Vec::new();
            let _failing = PsFails::new(fails);
            let left = home.left_server_by(
                || {
                    clock.set(clock.get() + super::LOOK_AGAIN);
                    let found = home.left_server_now(&replay);
                    looks.push(found.clone());
                    found
                },
                || clock.get(),
            );
            (looks, left)
        };
        let why = "`ps`, which lists the processes, failed (ps failed, as a test made it)";
        let failed: Result<Option<LeftReplay>, String> = Err(why.into());

        // Failed at 50, 100 and 150 ms, within the patience; none from 200 ms, and at 400 ms every
        // look for the patience has found none.
        let (looks, left) = looked(3);
        let mut expected = vec![failed.clone(); 3];
        expected.extend(vec![Ok(None); 5]);
        assert_eq!(looks, expected);
        assert_eq!(left, Ok(None));

        // Failed at 200 ms as well, once the patience has passed: refused.
        let (looks, left) = looked(4);
        assert_eq!(looks, vec![failed; 4]);
        assert_eq!(left, Err(home.cannot_tell(why)));
        let _ = fs::remove_dir_all(&dir);
    }

    /// A failed start's settling keeps the record while a server it started runs, even one the
    /// record does not name and no listing finds on the data (one between programs as it starts),
    /// and removes it once none runs.
    #[test]
    fn a_failed_starts_settling_keeps_the_record_while_a_server_it_started_runs() {
        let (dir, home) = folder("failed-start");
        home.mark_replay(&Replay::default()).unwrap();
        home.settle_failed_replay(&[ended_pid(), i64::from(std::process::id())]);
        assert!(dir.join(RECORD).exists());
        home.settle_failed_replay(&[ended_pid()]);
        assert!(!dir.join(RECORD).exists());
        assert_eq!(
            fs::read_to_string(dir.join(LOG)).unwrap(),
            format!("{GONE}\n")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A record that could not name a server says so, and that the server is left to the next
    /// start.
    #[test]
    fn a_server_its_record_could_not_name_is_left_to_the_next_start() {
        let said = super::unnamed(
            &server(4343).unwrap(),
            &ServerError::Io("could not write /b/symphony/tasks/replay.json: disk full".into()),
        )
        .to_string();
        assert!(
            said.starts_with(
                "the private replay's server (pid 4343, 127.0.0.1:4100) started, and its record \
                 could not name it: could not write /b/symphony/tasks/replay.json: disk full. It \
                 is left running, never signalled before it answers"
            ),
            "{said}"
        );
    }

    /// The line a start's replay ends with is a replay's end, as one its starter left ends.
    #[test]
    fn a_replay_ends_in_the_words_every_replays_end_begins_with() {
        assert!(REPLAY_ENDS.starts_with(&format!("{REPLAY_THROUGH}: ")));
    }
}
