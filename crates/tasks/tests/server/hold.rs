//! The hold ([`Home::hold`]): the board's database stopped cleanly and kept stopped, every start
//! refused naming it, until its release; and how every stop and start treats a server: asked to end
//! and waited for, never killed (`Home::end`), never signalled nor joined by a second while it
//! starts, and a log written under a larger memtable replayed whole before the board's server
//! starts on it (`Home::replay`). Each case's store is a scratch folder of its own, and each server
//! it signals is one it started there.

use crate::common::{Scratch, gone, script, signal};
use crate::home_fixture::{Unresponsive, healthy_server};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::rpc::{Client, healthy};
use symphony_board::server::{Home, Running, SURREAL_VERSION, ServerError, Storage};
use symphony_board::sys::pid_alive;
use symphony_board::{NewTask, Store};

const HANDLE: &str = "russet-lynx";

fn board(running: &Running) -> Store {
    Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap()
}

/// The bytes the write-ahead log of the store in `dir` holds, every segment in its `wal/`.
fn logged(dir: &Path) -> u64 {
    std::fs::read_dir(dir.join("data/wal")).map_or(0, |segments| {
        segments
            .filter_map(Result::ok)
            .filter_map(|segment| segment.metadata().ok())
            .map(|meta| meta.len())
            .sum()
    })
}

#[test]
fn a_hold_stops_the_server_cleanly_and_nothing_starts_it_until_its_release() {
    let s = Scratch::new("hold");
    let home = s.home();
    let running = home.ensure().unwrap();
    board(&running)
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    assert_eq!(home.held(), None);

    let began = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let holding = home.hold(HANDLE, "a snapshot of the data").unwrap();
    assert_eq!(holding.stopped, vec![running.clone()]);
    assert_eq!(holding.data, s.dir.join("data"));
    assert!(gone(running.pid), "the server ended");
    assert_eq!(home.recorded(), None, "its address is forgotten");
    assert_eq!(home.serving(), None);
    // A clean close: what the memtable held went to an sstable and the log is empty.
    assert_eq!(logged(&s.dir), 0, "the log is empty after a clean close");
    let hold = holding.hold;
    assert_eq!(
        (hold.by.as_str(), hold.why.as_str()),
        (HANDLE, "a snapshot of the data")
    );
    assert_eq!(hold.pid, i64::from(std::process::id()));
    assert!(i64::try_from(began).unwrap() <= hold.at, "{hold:?}");
    assert_eq!(home.held(), Some(hold.clone()));
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(s.dir.join("hold")).unwrap()).unwrap();
    assert_eq!(
        written,
        json!({"by": HANDLE, "why": "a snapshot of the data", "at": hold.at, "pid": hold.pid})
    );

    // Every way to the database is refused, naming the hold, and starts nothing.
    let refused = ServerError::Held(hold.clone());
    assert_eq!(home.ensure(), Err(refused.clone()));
    assert_eq!(s.home().ensure(), Err(refused.clone()));
    assert_eq!(home.stop(), Err(refused.clone()));
    assert_eq!(home.reclaim().unwrap_err(), refused);
    let said = refused.to_string();
    assert!(
        said.starts_with(&format!(
            "the board's database is held by {HANDLE} since 20"
        )),
        "{said}"
    );
    assert!(
        said.ends_with(
            ": a snapshot of the data; nothing starts it until the hold is released \
             (`just tasks database release --as HANDLE`)"
        ),
        "{said}"
    );
    assert_eq!(home.serving(), None);
    assert!(!s.dir.join("server.json").exists());

    // Another handle is refused; its setter holding it again keeps the hold as it was.
    assert_eq!(home.hold("vivid-swift", "a swap"), Err(refused.clone()));
    let again = home.hold(HANDLE, "another reason").unwrap();
    assert_eq!((again.hold, again.stopped), (hold.clone(), vec![]));

    // The release starts nothing; the next ensure starts the server on the same data.
    assert_eq!(home.release().unwrap(), Some(hold));
    assert_eq!(home.held(), None);
    assert_eq!(home.serving(), None, "the release started nothing");
    assert_eq!(home.release().unwrap(), None);
    let after = home.ensure().unwrap();
    assert_ne!(after.pid, running.pid);
    assert_eq!(
        board(&after).get("sim/jump").unwrap().title,
        "Jump departure"
    );
}

/// A server on the data that no record names (its starter ended between its answer and its record)
/// is found by its data and stopped cleanly by the hold too.
#[test]
fn a_hold_stops_a_server_no_record_names() {
    let s = Scratch::new("hold-unrecorded");
    let home = s.home();
    let running = home.ensure().unwrap();
    std::fs::remove_file(s.dir.join("server.json")).unwrap();
    assert_eq!(home.serving(), Some(running.clone()), "found by its data");
    let holding = home.hold(HANDLE, "a swap").unwrap();
    assert_eq!(holding.stopped, vec![running.clone()]);
    assert!(gone(running.pid));
    assert_eq!(logged(&s.dir), 0, "the log is empty after a clean close");
    assert_eq!(home.serving(), None);
    assert!(home.release().unwrap().is_some());
}

/// A hold file that does not read still holds, by an unknown setter, and the release removes it.
#[test]
fn a_hold_that_does_not_read_still_holds() {
    let s = Scratch::new("hold-garbled");
    let home = s.home();
    std::fs::write(s.dir.join("hold"), "{not json").unwrap();
    let hold = home.held().unwrap();
    assert_eq!(hold.by, "an unknown setter");
    assert!(
        hold.why.starts_with(&format!(
            "{} is there and does not read as a hold (",
            s.dir.join("hold").display()
        )),
        "{}",
        hold.why
    );
    assert_eq!(home.ensure(), Err(ServerError::Held(hold.clone())));
    assert_eq!(
        home.hold(HANDLE, "a swap"),
        Err(ServerError::Held(hold.clone()))
    );
    assert_eq!(home.release().unwrap(), Some(hold));
    assert!(!s.dir.join("hold").exists());
}

/// A server that has not answered is still starting: the hold is written, so nothing starts another
/// meanwhile, and the server is left running, never signalled, its record kept.
#[test]
fn a_hold_never_signals_a_server_still_starting_and_holds_meanwhile() {
    let s = Scratch::new("hold-starting");
    let mut server = Unresponsive::start(&s.data());
    let running = Running {
        addr: server.addr.clone(),
        pid: server.pid(),
        version: SURREAL_VERSION.into(),
    };
    s.record(&running);
    let home = s.home().patience(Duration::from_millis(300));
    let error = home.hold(HANDLE, "a swap").unwrap_err();
    assert!(
        matches!(&error, ServerError::NotUp(why) if
            why.contains(&format!("(pid {}, {})", running.pid, running.addr)) &&
            why.contains("is still starting (replaying its log) or stuck") &&
            why.contains("never signalled before it answers")),
        "{error}"
    );
    assert!(server.is_running(), "the starting server was signalled");
    assert_eq!(home.recorded(), Some(running));
    assert_eq!(home.held().unwrap().by, HANDLE, "the hold is set meanwhile");
    assert!(matches!(home.ensure(), Err(ServerError::Held(_))));
    assert!(server.shutdown().success(), "the helper must exit normally");
    // Its setter again: the server is gone now, so the stop is done and its record forgotten.
    let holding = home.hold(HANDLE, "a swap").unwrap();
    assert_eq!(holding.stopped, vec![]);
    assert_eq!(home.recorded(), None);
}

/// A `surreal` that answers as soon as it listens and ignores SIGTERM: a server still closing its
/// store, as long as a stop waits.
fn deaf(s: &Scratch) -> PathBuf {
    script(
        &s.dir.join("deaf-server"),
        &format!(
            "trap '' TERM\nexec '{}' \"$@\"",
            healthy_server(&s.dir).display()
        ),
    )
}

/// A stop asks once and waits the closing patience; a server still running then is left to finish,
/// never killed, and its record kept.
#[test]
fn a_server_that_does_not_end_when_asked_is_left_closing_never_killed() {
    let s = Scratch::new("closing");
    let closing = Duration::from_millis(600);
    let home = Home::new(&s.dir, &deaf(&s))
        .patience(Duration::from_secs(20))
        .closing(closing);
    let running = home.ensure().unwrap();
    let began = Instant::now();
    let error = home.stop().unwrap_err();
    assert!(began.elapsed() >= closing, "waited {:?}", began.elapsed());
    assert_eq!(
        error,
        ServerError::NotUp(format!(
            "the board's server (pid {}) is still closing its store 0 s after it was asked to \
             end: it is left to finish, never killed; its log is {}",
            running.pid,
            s.dir.join("server.log").display()
        ))
    );
    assert!(pid_alive(running.pid), "the server was killed");
    assert_eq!(home.recorded(), Some(running.clone()));
    // The hold then holds while it closes, and says the stop is not done.
    assert!(matches!(
        home.hold(HANDLE, "a swap"),
        Err(ServerError::NotUp(_))
    ));
    assert_eq!(home.held().unwrap().by, HANDLE);
    assert_eq!(home.serving(), Some(running.clone()));
    s.stop_if_ours(running.pid);
}

/// A `surreal` that runs and never answers, its pid in the folder's `starting`, for at most a
/// minute: a server still replaying its log.
fn starting(s: &Scratch) -> PathBuf {
    script(
        &s.dir.join("starting-server"),
        &format!(
            "echo $$ > '{}'\ni=0\nwhile [ $i -lt 600 ]; do sleep 0.1; i=$((i + 1)); done",
            s.dir.join("starting").display()
        ),
    )
}

/// A start that does not come up within the patience is left running, never signalled, and no
/// later ensure starts a second server on its data while it runs: each waits for it again.
#[test]
fn a_start_that_does_not_come_up_in_time_is_left_running_and_never_joined() {
    let s = Scratch::new("left-starting");
    let home = Home::new(&s.dir, &starting(&s)).patience(Duration::from_secs(1));
    let error = home.ensure().unwrap_err();
    let pid: i64 = std::fs::read_to_string(s.dir.join("starting"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    struct Ours(i64);
    impl Drop for Ours {
        fn drop(&mut self) {
            signal(self.0, "KILL");
            gone(self.0);
        }
    }
    let _ours = Ours(pid);
    assert!(
        matches!(&error, ServerError::NotUp(why) if
            why.contains(&format!("(pid {pid}, ")) &&
            why.contains("has not answered within 1 s: it is still starting") &&
            why.contains("left running, never ended nor joined by a second on its data")),
        "{error}"
    );
    assert!(pid_alive(pid), "the starting server was ended");
    assert_eq!(home.recorded(), None);
    let again = home.ensure().unwrap_err();
    assert!(
        matches!(&again, ServerError::NotUp(why) if
            why.contains(&format!("(pid {pid}, ")) &&
            why.contains("runs and does not answer within 1 s")),
        "{again}"
    );
    assert!(pid_alive(pid));
    let started = std::fs::read_to_string(s.dir.join("starting")).unwrap();
    assert_eq!(
        started.trim(),
        pid.to_string(),
        "a second server was started"
    );
}

/// A log written under a memtable larger than the board's, replayed under the board's, fills one
/// memtable after another, and SurrealKV records the whole log as written to sstables at the first
/// while its tail is only in memory: a server that ends before its next flush leaves the next start
/// to skip that tail. The start replays such a log in a private server whose memtable holds it, and
/// stops it cleanly, so every row survives a kill of each server after.
#[test]
fn a_log_written_under_a_larger_memtable_is_replayed_whole_and_survives_a_kill_after() {
    let s = Scratch::new("replay");
    let small = Storage {
        memtable: 4 << 20,
        ..Storage::BOARD
    };
    let first = s.home().storage(Storage::BOARD).ensure().unwrap();
    let client = Client::new(&first.addr, "sf", "replay");
    for batch in 0..50 {
        client
            .define_then(
                &format!(
                    "FOR $n IN 0..100 {{ CREATE row CONTENT {{ batch: {batch}, n: $n, text: \
                     string::repeat('x', 2000) }} }};"
                ),
                json!({}),
            )
            .unwrap();
    }
    let count = |running: &Running| {
        Client::new(&running.addr, "sf", "replay")
            .query("RETURN count(SELECT id FROM row);", json!({}))
            .unwrap()
    };
    assert_eq!(count(&first), vec![json!(5000)]);
    signal(first.pid, "KILL");
    assert!(gone(first.pid));
    let log = logged(&s.dir);
    assert!(log > small.memtable, "the log holds {log} bytes");

    let second = s.home().storage(small).ensure().unwrap();
    assert_eq!(count(&second), vec![json!(5000)]);
    assert!(
        logged(&s.dir) < log / 4,
        "the replay emptied the log: {} bytes",
        logged(&s.dir)
    );
    signal(second.pid, "KILL");
    assert!(gone(second.pid));
    let third = s.home().storage(small).ensure().unwrap();
    assert_eq!(
        count(&third),
        vec![json!(5000)],
        "rows were lost to the kill"
    );
    assert!(healthy(&third.addr));
}
