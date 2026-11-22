//! The hold ([`Home::hold`]): the board's database stopped cleanly and kept stopped, every start
//! refused naming it, until its release; and how every stop and start treats a server: asked to end
//! and waited for, never killed (`Home::end`), never signalled nor joined by a second while it
//! starts, and a log written under a larger memtable replayed whole before the board's server
//! starts on it (`Home::replay`). Each case's store is a scratch folder of its own, and each server
//! it signals is one it started there.

use crate::children::Child as _;
use crate::common::{Scratch, Told, gone, script, signal};
use crate::home_fixture::{Unresponsive, healthy_server};
use serde_json::json;
use std::convert::Infallible;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use symphony_board::port;
use symphony_board::rpc::{Client, healthy};
use symphony_board::server::snapshot::KEPT;
use symphony_board::server::{
    Home, REPLAY_BEGINS, REPLAY_ENDS, Running, SURREAL_VERSION, ServerError, Storage, replay_begins,
};
use symphony_board::sys::{Child, Command, Stdio, pid_alive};
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

/// Ten MiB of rows: more than the small memtable used by the replay cases.
pub(super) const REPLAY_ROWS: u64 = 5000;

/// A written-and-killed store's log, built once and copied before any replay. The case keeps all
/// its own start, replay, kill and row-count assertions; only its unrelated setup is shared.
pub(super) fn replay_into(s: &Scratch) {
    static SEED: OnceLock<PathBuf> = OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let setup = Scratch::new("replay-seed-setup");
        let running = setup.home().storage(Storage::BOARD).ensure().unwrap();
        let client = Client::new(&running.addr, "sf", "replay");
        for batch in 0..REPLAY_ROWS / 100 {
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
        assert_eq!(
            client
                .query("RETURN count(SELECT id FROM row);", json!({}))
                .unwrap(),
            vec![json!(REPLAY_ROWS)]
        );
        signal(running.pid, "KILL");
        assert!(gone(running.pid));
        crate::stores::keep(&setup, "replay-seed")
    });
    crate::stores::clone_data(seed, &s.dir.join("data"));
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
        said.ends_with(&format!(
            ": a snapshot of the data; nothing starts it until its holder releases it (`just \
             tasks database release`, run by {HANDLE}, or by another handle naming it with \
             `--of`): wait for that, or ask {HANDLE}"
        )),
        "{said}"
    );
    assert_eq!(home.serving(), None);
    assert!(!s.dir.join("server.json").exists());

    // Another handle is refused; its setter holding it again keeps the hold as it was.
    assert_eq!(home.hold("vivid-swift", "a swap"), Err(refused.clone()));
    let again = home.hold(HANDLE, "another reason").unwrap();
    assert_eq!((again.hold, again.stopped), (hold.clone(), vec![]));

    // The release starts nothing; the next ensure starts the server on the same data.
    assert_eq!(home.release(HANDLE, None).unwrap(), Some(hold));
    assert_eq!(home.held(), None);
    assert_eq!(home.serving(), None, "the release started nothing");
    assert_eq!(home.release(HANDLE, None).unwrap(), None);
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
    assert!(home.release(HANDLE, None).unwrap().is_some());
}

/// A hold file that does not read still holds, by an unknown setter, and any handle's release
/// removes it, as it names no holder.
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
    assert_eq!(home.release("vivid-swift", None).unwrap(), Some(hold));
    assert!(!s.dir.join("hold").exists());
}

/// What `symphony/tasks/hold.log` says, a JSON value a line.
fn history(s: &Scratch) -> Vec<serde_json::Value> {
    std::fs::read_to_string(s.dir.join("hold.log"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Only its holder releases a hold, or another handle that names the holder; every hold and release
/// is a line of the history, which the release keeps.
#[test]
fn only_its_holder_releases_a_hold_or_another_naming_it_and_the_history_keeps_each() {
    let s = Scratch::new("release-whose");
    let home = s.home();
    let hold = home.hold("dunlin-probe", "a swap").unwrap().hold;
    for of in [None, Some("russet-lynx"), Some("dunlin")] {
        let refused = home.release("vivid-swift", of).unwrap_err();
        assert_eq!(refused, ServerError::NotHolder(hold.clone()), "{of:?}");
        assert_eq!(home.held(), Some(hold.clone()), "{of:?}: released");
    }
    let said = ServerError::NotHolder(hold.clone()).to_string();
    assert!(
        said.starts_with("the board's database is held by dunlin-probe since "),
        "{said}"
    );
    assert!(
        said.ends_with(
            ": a swap; only dunlin-probe releases it, or another handle naming it (`--of \
             dunlin-probe`), so that no hold is released by mistake in the middle of work on the \
             data"
        ),
        "{said}"
    );
    assert_eq!(
        history(&s),
        [json!({"did": "hold", "by": "dunlin-probe", "why": "a swap", "at": hold.at})],
        "a refused release is no line"
    );

    assert_eq!(
        home.release("vivid-swift", Some("dunlin-probe")).unwrap(),
        Some(hold.clone())
    );
    assert_eq!(home.held(), None);
    let lines = history(&s);
    assert_eq!(lines.len(), 2, "{lines:?}");
    let released = &lines[1];
    assert_eq!(
        (
            &released["did"],
            &released["by"],
            &released["of"],
            &released["why"],
            &released["held_at"]
        ),
        (
            &json!("release"),
            &json!("vivid-swift"),
            &json!("dunlin-probe"),
            &json!("a swap"),
            &json!(hold.at)
        )
    );
    assert!(released["at"].as_i64().unwrap() >= hold.at, "{released}");

    // Its holder releases it without naming anyone.
    home.hold(HANDLE, "a snapshot").unwrap();
    assert!(home.release(HANDLE, None).unwrap().is_some());
    let lines = history(&s);
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(
        (&lines[3]["by"], &lines[3]["of"]),
        (&json!(HANDLE), &json!(HANDLE))
    );
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
        .patience(test_time::backstop(Duration::from_secs(20)))
        .closing(closing);
    let running = home.ensure().unwrap();
    let began = Instant::now();
    let error = home.stop().unwrap_err();
    assert!(began.elapsed() >= closing, "waited {:?}", began.elapsed());
    let pid = running.pid;
    assert_eq!(
        error,
        ServerError::NotUp(format!(
            "the board's server (pid {pid}) is still closing its store 0 s after it was asked to \
             end: it is left to finish, never asked again (a second signal ends SurrealDB before \
             its store is closed) nor killed. One writing a large memtable takes minutes; one \
             whose CPU time stands still across a minute (`ps -o time= -p {pid}`) has hung. The \
             owner then holds the board (`just tasks database hold WHY --as HANDLE`, a hold that \
             stands while it says the server still closes), ends it by hand (`kill -9 {pid}`, its \
             log then replayed at the next start) and releases the hold; its ask, {}, is \
             forgotten once it has ended (docs/operations/BOARD_OPERATIONS.md, Stuck servers). Its log is {}",
            s.dir.join("ending").join(pid.to_string()).display(),
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

/// A `surreal` that runs and never answers, its pid in the folder's `starting`, until the test ends
/// it or six minutes of its looks pass (the backstop of the minute it once had, which a loaded
/// machine's test outlived): a server still replaying its log. The shell runs on, never replaced,
/// so its command line names the data the start gave it, as a server's does.
fn starting(s: &Scratch) -> PathBuf {
    script(
        &s.dir.join("starting-server"),
        &format!(
            "echo $$ > '{}'\ni=0\nwhile [ $i -lt 3600 ]; do sleep 0.1; i=$((i + 1)); done",
            s.dir.join("starting").display()
        ),
    )
}

/// The pid a stand-in wrote to `path` as its first act, once it has: a whole line, however long a
/// loaded machine took to run it (the wait's backstop is the old minute's).
fn pid_in(path: &Path) -> i64 {
    test_time::until(
        format!("a pid in {}", path.display()),
        test_time::backstop(Duration::from_secs(60)),
        || {
            let written = std::fs::read_to_string(path).unwrap_or_default();
            written
                .strip_suffix('\n')
                .and_then(|line| line.trim().parse().ok())
        },
    )
    .unwrap()
}

/// A start that does not come up within the patience is left running, never signalled, and no
/// later ensure starts a second server on its data while it runs: each waits for it again, a
/// caller with a bound only that bound.
#[test]
fn a_start_that_does_not_come_up_in_time_is_left_running_and_never_joined() {
    let s = Scratch::new("left-starting");
    let surreal = starting(&s);
    let home = Home::new(&s.dir, &surreal).patience(Duration::from_secs(1));
    let error = home.ensure().unwrap_err();
    // Its patience may run out before a loaded machine ran the stand-in's first line.
    let pid = pid_in(&s.dir.join("starting"));
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
    // How a replay is told from a hang, and what the owner does with one that hangs.
    let guidance = |why: &str| {
        why.contains(". It has run ")
            && why.contains(" of CPU, on a store whose log holds 0.0 MiB: a server replaying its log works the CPU all along and answers once it is through")
            && why.contains("one whose CPU time stands still across a minute hangs")
            && why.contains(&format!(
                "The owner holds the board then (`just tasks database hold WHY --as HANDLE`, \
                 which leaves that server running and keeps anything from starting another), \
                 ends it (`kill {pid}`), and releases the hold. Its log is {}",
                s.dir.join("server.log").display()
            ))
    };
    assert!(
        matches!(&error, ServerError::NotUp(why) if guidance(why)),
        "{error}"
    );
    let again = home.ensure().unwrap_err();
    assert!(
        matches!(&again, ServerError::NotUp(why) if
            why.contains(&format!("(pid {pid}, ")) &&
            why.contains("runs and does not answer within 1 s") &&
            guidance(why)),
        "{again}"
    );
    assert!(pid_alive(pid));
    let started = std::fs::read_to_string(s.dir.join("starting")).unwrap();
    assert_eq!(
        started.trim(),
        pid.to_string(),
        "a second server was started"
    );

    // A caller with a bound (the Symphony's read) waits for it only its bound, not the patience:
    // a replay whose starter has gone, as a Symphony restarted while its start ran apart leaves.
    // The refusal names the limit that ended its wait, the bound and not the minute's patience.
    let read = Home::new(&s.dir, &surreal)
        .waiting_at_most(Duration::from_millis(300))
        .starting_apart();
    let refused = read.ensure().unwrap_err();
    assert!(
        matches!(&refused, ServerError::NotUp(why) if
            why.contains(&format!("(pid {pid}, ")) &&
            why.contains("runs and does not answer within 0.3 s") &&
            guidance(why)),
        "{refused}"
    );
    assert!(pid_alive(pid), "left running");
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
    replay_into(&s);
    let log = logged(&s.dir);
    assert!(log > small.memtable, "the log holds {log} bytes");

    let second = s.home().storage(small).ensure().unwrap();
    assert_eq!(rows(&second.addr), vec![json!(REPLAY_ROWS)]);
    assert!(
        logged(&s.dir) < log / 4,
        "the replay emptied the log: {} bytes",
        logged(&s.dir)
    );
    signal(second.pid, "KILL");
    assert!(gone(second.pid));
    let third = s.home().storage(small).ensure().unwrap();
    assert_eq!(
        rows(&third.addr),
        vec![json!(REPLAY_ROWS)],
        "rows were lost to the kill"
    );
    assert!(healthy(&third.addr));
}

/// How many rows the database `replay` of the server at `addr` holds.
fn rows(addr: &str) -> Vec<serde_json::Value> {
    Client::new(addr, "sf", "replay")
        .query("RETURN count(SELECT id FROM row);", json!({}))
        .unwrap()
}

/// A server on the scratch store started as a build before the private replay starts one: under a
/// memtable of `memtable` bytes, with no replay first. Killed when it drops.
struct Older(Child);

impl Older {
    /// Started, and up once it answers from its own listener: its address.
    fn start(s: &Scratch, memtable: u64) -> (Older, String) {
        let log = s.dir.join("older.log");
        let until = Instant::now() + test_time::backstop(Duration::from_secs(120));
        let ((older, addr), _) = port::start_on_free_port(port::ATTEMPTS, until, |reserved| {
            let addr = reserved.to_string();
            let out = File::create(&log).unwrap();
            let mut older = Older(
                Command::of(s.surreal())
                    .args([
                        "start",
                        "--unauthenticated",
                        "--log",
                        "warn",
                        "--bind",
                        &addr,
                    ])
                    .arg(s.data())
                    .env("SURREAL_SURREALKV_MAX_MEMTABLE_SIZE", memtable.to_string())
                    .env(
                        "SURREAL_SURREALKV_VLOG_THRESHOLD",
                        Storage::BOARD.value_log_threshold.to_string(),
                    )
                    .stdin(Stdio::null())
                    .stdout(out.try_clone().unwrap())
                    .stderr(out)
                    .spawn()
                    .unwrap(),
            );
            // Up once it answers on its port; ended once it exits first.
            let up = test_time::until(
                "the older start answers or ends",
                until.saturating_duration_since(Instant::now()),
                || {
                    if older.0.try_wait().unwrap().is_some() {
                        return Some(false);
                    }
                    (healthy(&addr)
                        && port::listens(older.0.id(), reserved, symphony_board::sys::output)
                            .unwrap())
                    .then_some(true)
                },
            )
            .unwrap();
            if up {
                return Ok::<_, Infallible>(port::Start::Up((older, addr)));
            }
            Ok(port::Start::Ended(std::fs::read_to_string(&log).unwrap()))
        })
        .unwrap_or_else(|why| panic!("the older start: {why:?}"));
        (older, addr)
    }

    /// Killed, as an older build ends a server that has not answered within its patience.
    fn kill(mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }
}

impl Drop for Older {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A store that went back: its log written under the board's memtable, then replayed by a start
/// with no private replay (an older build's) under a smaller one, which fills one memtable after
/// another, records the whole first segment as written at the first flush, and is killed before
/// the next. That segment is on disk, and no server applies it: the private replay's server skips
/// it, and its clean stop removes it. So the start keeps the store before the replay, the segment
/// byte for byte, and says where.
#[test]
fn a_store_that_went_back_keeps_its_unapplied_segment_in_a_clone_before_the_replay() {
    let s = Scratch::new("went-back");
    let small = Storage {
        memtable: 4 << 20,
        ..Storage::BOARD
    };
    replay_into(&s);

    let (older, addr) = Older::start(&s, small.memtable);
    assert_eq!(
        rows(&addr),
        vec![json!(REPLAY_ROWS)],
        "the older start read all"
    );
    older.kill();
    let segment = s.dir.join("data/wal/00000000000000000000.wal");
    let unapplied = std::fs::read(&segment).unwrap();
    assert!(
        unapplied.len() as u64 > small.memtable,
        "{}",
        unapplied.len()
    );

    let told = Told::default();
    let home = s.home().storage(small).telling(told.sink());
    assert_eq!(home.kept(), (vec![], vec![]));
    let started = home.ensure().unwrap();
    let (kept, partial) = home.kept();
    assert_eq!(
        (kept.len(), partial),
        (1, Vec::<PathBuf>::new()),
        "{kept:?}"
    );
    let name = kept[0].file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("20") && kept[0].parent() == Some(home.snapshots_folder().as_path()),
        "a snapshot like any other: {}",
        kept[0].display()
    );
    assert!(
        home.snapshots().unwrap()[0].log,
        "it holds the log, so it is never pruned"
    );
    assert_eq!(
        std::fs::read(kept[0].join("wal/00000000000000000000.wal")).unwrap(),
        unapplied,
        "the clone holds the segment byte for byte"
    );
    // What the clone alone keeps: the replay's stop removed the segment, and the board went back.
    assert!(!segment.exists(), "the replay kept the segment itself");
    let read = rows(&started.addr)[0].as_u64().unwrap();
    assert!(
        read < REPLAY_ROWS,
        "the store did not go back: it reads {read} rows"
    );
    let said = told.lines();
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said[0].starts_with(&format!(
            "· the log of the board's store at {} holds ",
            s.dir.join("data").display()
        )),
        "{said:?}"
    );
    assert!(
        said[0].ends_with(&format!(
            ", more than the board's memtable takes: it is replayed whole in a private server \
             before the board's server starts (a log of gigabytes takes minutes). The store as it \
             was is kept at {}, a copy-on-write clone: on a store that went back, it alone keeps \
             the log segment no server applies and the replay's stop removes. The board never \
             removes it: remove it once the board reads as it should",
            kept[0].display()
        )),
        "{said:?}"
    );
    assert_eq!(
        said[1],
        "· the private replay is through, and its server closed the store cleanly: the board's \
         server starts"
    );
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    let snapshot = format!(
        "· a snapshot of the store before this start: {}{KEPT}\n",
        kept[0].display()
    );
    assert!(
        log.contains(&snapshot),
        "the server's log names it too: {log}"
    );
    // In the server's log, the replay says itself between that snapshot and the board's server,
    // so its clean stop's `Goodbye!` is never read as the board's server's.
    let after = &log[log.find(&snapshot).unwrap() + snapshot.len()..];
    // The log the replay applied is the one the snapshot holds.
    let held: u64 = std::fs::read_dir(kept[0].join("wal"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|segment| segment.path().extension().is_some_and(|ext| ext == "wal"))
        .map(|segment| segment.metadata().unwrap().len())
        .sum();
    assert!(
        after.starts_with(&format!("{}\n", replay_begins(held))),
        "{after}"
    );
    let (replay, board) = after
        .split_once(&format!("{REPLAY_ENDS}\n"))
        .unwrap_or_else(|| panic!("{after}"));
    assert!(
        replay.contains("Goodbye!"),
        "the replay's clean stop: {replay}"
    );
    assert!(!board.contains(REPLAY_BEGINS), "{board}");
}

/// While a start replays the log, a caller waiting on the starters' lock says once what it waits
/// on, the replay and its log's size, and a caller with a bound on its wait (the Symphony's read)
/// is refused once it passes, naming the same. The note is gone with the lock.
#[test]
fn a_wait_on_a_replay_says_what_it_waits_on_and_a_bounded_one_is_refused_naming_it() {
    let s = Scratch::new("waits");
    let small = Storage {
        memtable: 4 << 20,
        ..Storage::BOARD
    };
    // A server that comes up under a replay's memtable only once the test lets it, and at once
    // under the board's: the replay runs as long as the test looks at it, however loaded.
    let slow = replaying_store(&s, small);
    let home = || Home::new(&s.dir, &slow).storage(small);
    let starter_told = Told::default();
    let starter = {
        let starting = home().telling(starter_told.sink());
        std::thread::spawn(move || starting.ensure())
    };
    let replaying = until_replaying(|| home().under_way());
    let by = format!("pid {}, since ", std::process::id());
    let what = format!(
        ": a private replay of the store's log of 3.0 MiB before the board's server starts: a log \
         of gigabytes takes minutes, and its server's lines go to {}",
        s.dir.join("server.log").display()
    );
    assert!(
        replaying.starts_with(&by) && replaying.ends_with(&what),
        "{replaying}"
    );

    let lock = s.dir.join("lock");
    let refused = home()
        .waiting_at_most(Duration::from_millis(300))
        .ensure()
        .unwrap_err();
    assert!(
        matches!(&refused, ServerError::NotUp(why) if
        why.starts_with(&format!(
            "the board's starters' lock ({}) is held by {by}",
            lock.display()
        )) &&
        why.ends_with(&format!(
            "{what}; this read waits at most 0.3 s for it, and the next one tries again"
        ))),
        "{refused}"
    );

    // A command waits on the lock, told once what it waits on, and the replay goes through after.
    let waiter_told = Told::default();
    let waiter = {
        let waiting = home().telling(waiter_told.sink());
        std::thread::spawn(move || waiting.ensure())
    };
    until_told(&waiter_told);
    let_the_replay_through(&s);
    let waited = waiter.join().unwrap().unwrap();
    let started = starter.join().unwrap().unwrap();
    assert_eq!(waited, started);
    let said = waiter_told.lines();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(&format!(
            "· waiting on the board's starters' lock ({}), held by {by}",
            lock.display()
        )) && said[0].ends_with(&what),
        "{said:?}"
    );
    let (kept, _) = home().kept();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(
        std::fs::read(kept[0].join("wal/00000000000000000000.wal")).unwrap(),
        vec![7u8; 3 << 20]
    );
    assert_eq!(starter_told.lines().len(), 2, "{:?}", starter_told.lines());
    assert_eq!(home().under_way(), None, "the note outlived the lock");
    assert!(!s.dir.join("lock.json").exists());
}

/// A scratch store whose log of 3 MiB is more than `small`'s memtable takes, and a `surreal` whose
/// server comes up under a replay's memtable only once [`let_the_replay_through`] (or after its
/// backstop, six times the minute a test once gave it), and at once under the board's.
fn replaying_store(s: &Scratch, small: Storage) -> PathBuf {
    std::fs::create_dir_all(s.dir.join("data/wal")).unwrap();
    std::fs::write(
        s.dir.join("data/wal/00000000000000000000.wal"),
        vec![7u8; 3 << 20],
    )
    .unwrap();
    script(
        &s.dir.join("slow-replay"),
        &format!(
            "if [ \"$SURREAL_SURREALKV_MAX_MEMTABLE_SIZE\" -gt {} ]; then\n  i=0\n  while [ ! -e \
             '{}' ] && [ $i -lt 7200 ]; do sleep 0.05; i=$((i + 1)); done\nfi\nexec '{}' \"$@\"",
            small.memtable,
            s.dir.join(THROUGH).display(),
            healthy_server(&s.dir).display()
        ),
    )
}

/// What lets a [`replaying_store`]'s replay come up, beside its store.
const THROUGH: &str = "replay-through";

/// Let the replay of a [`replaying_store`] come up.
fn let_the_replay_through(s: &Scratch) {
    std::fs::write(s.dir.join(THROUGH), "").unwrap();
}

/// Once `told` has been told a line: the backstop is the old minute's.
fn until_told(told: &Told) {
    test_time::until_true(
        "a line was told",
        test_time::backstop(Duration::from_secs(60)),
        || !told.lines().is_empty(),
    )
    .unwrap();
}

/// What the board says is under way (`under_way`) once it is a private replay: the backstop is the
/// 30 s these cases once gave it.
fn until_replaying(under_way: impl Fn() -> Option<String>) -> String {
    test_time::until(
        "a private replay began",
        test_time::backstop(Duration::from_secs(30)),
        || under_way().filter(|doing| doing.contains("a private replay")),
    )
    .unwrap()
}

/// The board's server once a read (`read`) finds it: the backstop is the minute these cases once
/// gave it, and a stall says what the last read was refused with.
fn until_found(read: impl Fn() -> Result<Running, ServerError>) -> Running {
    let mut refused = None;
    let found = test_time::until(
        "a read found the board's server",
        test_time::backstop(Duration::from_secs(60)),
        || read().map_err(|problem| refused = Some(problem)).ok(),
    );
    found.unwrap_or_else(|stall| panic!("{stall}: {refused:?}"))
}

/// A caller starting apart (the Symphony's read) whose start would replay the store's log is not
/// held by it: the start runs on a thread of its own, and the caller is refused at once, naming the
/// replay in the words its pages read ([`symphony_board::server::replaying_in`]); so is each such
/// read while it runs. A command that waits on the lock is told the replay, and gets the server;
/// once the start is through, a read finds the board's server it started, and the store was kept
/// before the replay as by any start.
#[test]
fn a_read_starting_apart_is_refused_at_once_while_its_replay_runs_on_a_thread_of_its_own() {
    let s = Scratch::new("apart");
    let small = Storage {
        memtable: 4 << 20,
        ..Storage::BOARD
    };
    let slow = replaying_store(&s, small);
    let told = Told::default();
    let home = || Home::new(&s.dir, &slow).storage(small);
    let read = || {
        home()
            .waiting_at_most(Duration::from_secs(5))
            .starting_apart()
            .telling(told.sink())
    };

    // Not held by the replay: its server comes up only once the test lets it, and the thread starts
    // it only once this read lets the lock go, so a read that waited for the start would wait for
    // ever, and one that ended after it would find the board's server recorded and the replay said
    // through. That, not a bound in time, is the check: on a machine loaded enough, a read that
    // waits for nothing takes seconds too.
    let not_through = |when: &str| {
        assert_eq!(home().recorded(), None, "{when} waited for the start");
        assert!(
            !told
                .lines()
                .iter()
                .any(|line| line.starts_with("· the private replay is through")),
            "{when} waited for the replay: {:?}",
            told.lines()
        );
    };
    let refused = read().ensure().unwrap_err();
    not_through("the read");
    let by = format!("pid {}, since ", std::process::id());
    let what = format!(
        ": a private replay of the store's log of 3.0 MiB before the board's server starts: a log \
         of gigabytes takes minutes, and its server's lines go to {}",
        s.dir.join("server.log").display()
    );
    assert!(
        matches!(&refused, ServerError::NotUp(why) if
        why.starts_with(&format!("the board's server is started apart from this read, by {by}"))
        && why.ends_with(&format!(
            "{what}; this read does not wait for it, and the next one asks again"
        ))),
        "{refused}"
    );
    assert_eq!(
        symphony_board::server::replaying_in(&refused.to_string()).as_deref(),
        Some(
            "its store's log of 3.0 MiB is replayed in a private server before the board's server \
             starts (a log of gigabytes takes minutes), and the board answers once that is through"
        )
    );
    assert_eq!(
        read().ensure().unwrap_err(),
        refused,
        "the next read, refused the same way while the start runs"
    );
    not_through("the next read");

    // The thread takes the lock and replays: a command waits on it, told what it waits on.
    let replaying = until_replaying(|| home().under_way());
    assert!(
        replaying.starts_with(&by) && replaying.ends_with(&what),
        "{replaying}"
    );
    let waiter = Told::default();
    let waiting = {
        let home = home().telling(waiter.sink());
        std::thread::spawn(move || home.ensure())
    };
    until_told(&waiter);
    let_the_replay_through(&s);
    let waited = waiting.join().unwrap().unwrap();
    assert!(
        waiter.lines().iter().any(|line| line.ends_with(&what)),
        "{:?}",
        waiter.lines()
    );
    let found = until_found(|| read().ensure());
    assert_eq!(found, waited);
    assert_eq!(home().recorded(), Some(found));
    let (kept, _) = home().kept();
    assert_eq!(kept.len(), 1, "the replay kept the store first: {kept:?}");
    let said = told.lines();
    assert_eq!(said.len(), 2, "the replay's and its end's: {said:?}");
    assert!(
        said[1].starts_with("· the private replay is through"),
        "{said:?}"
    );
}

/// A start run apart that fails is told to the read after it, once, in its own words; the read
/// after that starts it again apart, and once it can, it is through.
#[test]
fn a_start_run_apart_that_fails_is_told_to_the_next_read_once_and_the_one_after_starts_again() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("apart-fails");
    let small = Storage {
        memtable: 4 << 20,
        ..Storage::BOARD
    };
    let slow = replaying_store(&s, small);
    // No one watches the replay here: it comes up as soon as it runs.
    let_the_replay_through(&s);
    let told = Told::default();
    let home = || Home::new(&s.dir, &slow).storage(small);
    let read = || {
        home()
            .waiting_at_most(Duration::from_secs(5))
            .starting_apart()
            .telling(told.sink())
    };
    // A store that can be neither cloned nor copied is never replayed.
    let folder = home().snapshots_folder();
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let apart = "the board's server is started apart from this read, by ";
    let first = read().ensure().unwrap_err().to_string();
    assert!(first.starts_with(apart), "{first}");
    let failed = test_time::until(
        "a read was told the start apart failed",
        test_time::backstop(Duration::from_secs(30)),
        || Some(read().ensure().unwrap_err().to_string()).filter(|said| !said.starts_with(apart)),
    )
    .unwrap();
    let data = s.dir.join("data");
    assert!(
        failed.starts_with(&format!(
            "the board's server, started apart from the Symphony's reads, did not start: the \
             board's store at {} could not be kept before the replay of its log: it could not be \
             cloned (",
            data.display()
        )) && failed.ends_with("; the next read starts it again"),
        "{failed}"
    );
    assert!(
        told.lines().iter().any(|line| line.starts_with(
            "· the board's server, started apart from the Symphony's reads, did not start: the \
             board's store"
        )),
        "the failure goes where the board tells: {:?}",
        told.lines()
    );
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let again = read().ensure().unwrap_err().to_string();
    assert!(again.starts_with(apart), "started again: {again}");
    let found = until_found(|| read().ensure());
    assert_eq!(home().recorded(), Some(found));
    assert_eq!(home().kept().0.len(), 1);
}
