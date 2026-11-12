//! A private replay's server left running when its starter ended (a Symphony stopped in the middle
//! of a replay) is never the board's server: the next start stops it cleanly once it answers, never
//! before, and starts the board's own under the board's memtable. Each case's store is a scratch
//! folder of its own, in the live board's shape (its servers detached), and its starter a process
//! of its own that the case starts and kills by the pid it spawned.

use crate::common::{Scratch, gone, script, signal};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::rpc::{Client, healthy};
use symphony_board::server::{
    Home, REPLAY_BEGINS, REPLAY_ENDS, REPLAY_THROUGH, Running, Storage, replaying_in,
};
use symphony_board::sys::{Command, Stdio, pid_alive};

/// The board's memtable in these cases: small, so a log of a few MiB is replayed in a private
/// server first.
const SMALL: Storage = Storage {
    memtable: 4 << 20,
    ..Storage::BOARD
};

/// How many rows the store's log holds: 4 MB of writes, more than half [`SMALL`]'s memtable.
const ROWS: u64 = 2000;

/// A `surreal` that writes down each start, its pid and its memtable (a line of `starts`), and,
/// under a replay's memtable, waits for the file `go` (for at most a minute) before it runs the
/// guarded `surreal`: a replay that lasts until the case lets it through.
fn gated(s: &Scratch) -> PathBuf {
    script(
        &s.dir.join("gated-surreal"),
        &format!(
            "echo \"$$ $SURREAL_SURREALKV_MAX_MEMTABLE_SIZE\" >> '{starts}'\n\
             if [ \"$SURREAL_SURREALKV_MAX_MEMTABLE_SIZE\" -gt {small} ]; then\n\
             \x20   i=0\n\
             \x20   while [ ! -e '{go}' ] && [ $i -lt 1200 ]; do sleep 0.05; i=$((i + 1)); done\n\
             fi\n\
             exec '{surreal}' \"$@\"",
            starts = s.dir.join("starts").display(),
            go = s.dir.join("go").display(),
            small = SMALL.memtable,
            surreal = s.surreal().display()
        ),
    )
}

/// Each start `gated` wrote down: its pid and its memtable.
fn starts(s: &Scratch) -> Vec<(i64, u64)> {
    std::fs::read_to_string(s.dir.join("starts"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (pid, memtable) = line.split_once(' ')?;
            Some((pid.parse().ok()?, memtable.parse().ok()?))
        })
        .collect()
}

/// The bytes the write-ahead log of the store in `dir` holds: its segments in `wal/`.
fn logged(dir: &Path) -> u64 {
    std::fs::read_dir(dir.join("data/wal")).map_or(0, |segments| {
        segments
            .filter_map(Result::ok)
            .filter(|segment| segment.path().extension().is_some_and(|ext| ext == "wal"))
            .filter_map(|segment| segment.metadata().ok())
            .map(|meta| meta.len())
            .sum()
    })
}

/// How many rows the database `replay` of the server at `addr` holds.
fn rows(addr: &str) -> u64 {
    Client::new(addr, "sf", "replay")
        .query("RETURN count(SELECT id FROM row);", json!({}))
        .unwrap()[0]
        .as_u64()
        .unwrap()
}

/// The board in `s` as a command reaches it, served by `surreal` under [`SMALL`].
fn board(s: &Scratch, surreal: &Path) -> Home {
    Home::new(&s.dir, surreal).storage(SMALL)
}

/// A private replay's server its starter left running: the store's log of [`ROWS`] rows, written
/// under the board's memtable by a server then killed, is replayed under [`SMALL`] by a starter
/// process of its own, which is killed once its replay's server runs; that server, detached, waits
/// until the case writes `go`. The gated `surreal`, and that server's pid and memtable.
fn left_running(s: &Scratch) -> (PathBuf, i64, u64) {
    let first = s.home().storage(Storage::BOARD).ensure().unwrap();
    let client = Client::new(&first.addr, "sf", "replay");
    for batch in 0..ROWS / 100 {
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
    assert_eq!(rows(&first.addr), ROWS);
    signal(first.pid, "KILL");
    assert!(gone(first.pid));
    std::fs::remove_file(s.dir.join("server.json")).unwrap();
    let log = logged(&s.dir);
    assert!(log * 2 > SMALL.memtable, "the log holds {log} bytes");

    let surreal = gated(s);
    let mut starter = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "left_replay::helper_starts_the_board",
            "--test-threads=1",
        ])
        .env("SF_LEFT_DIR", &s.dir)
        .env("SF_LEFT_SURREAL", &surreal)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let began = Instant::now();
    let (replay, memtable) = loop {
        if let Some(&found) = starts(s).iter().find(|(_, m)| *m > SMALL.memtable) {
            break found;
        }
        assert!(
            starter.try_wait().unwrap().is_none(),
            "the starter ended before its replay began"
        );
        assert!(began.elapsed() < Duration::from_secs(60), "no replay began");
        std::thread::sleep(Duration::from_millis(20));
    };
    starter.kill().unwrap();
    starter.wait().unwrap();
    assert!(
        pid_alive(replay),
        "the replay's server ended with its starter"
    );
    assert_eq!(memtable, log * 2, "a replay's memtable, twice the log");
    assert_eq!(board(s, &surreal).recorded(), None);
    (surreal, replay, memtable)
}

/// The board's server `running` is its own, under [`SMALL`], and the replay's is gone: started
/// once each, the replay under its memtable and the board's under the board's, so nothing was
/// replayed twice; every row is there; the replay's record is gone and the store's log empty; the
/// server's log says the replay began once and ended once, its server's clean stop between.
fn the_boards_own(s: &Scratch, running: &Running, replay: (i64, u64)) {
    assert_ne!(
        running.pid, replay.0,
        "the replay's server, under its memtable of {} bytes, is the board's",
        replay.1
    );
    assert!(gone(replay.0), "the replay's server still runs");
    assert_eq!(starts(s), [replay, (running.pid, SMALL.memtable)]);
    assert_eq!(rows(&running.addr), ROWS);
    assert!(
        !s.dir.join("replay.json").exists(),
        "the record outlived it"
    );
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert_eq!(log.matches(REPLAY_BEGINS).count(), 1, "{log}");
    let through = format!("· the private replay's server (pid {}, ", replay.0);
    let after = &log[log.find(&through).unwrap_or_else(|| panic!("{log}"))..];
    assert!(
        after.lines().next().unwrap().ends_with(
            "), left running when its starter ended, is through: it is stopped cleanly by its \
             recorded pid, never adopted as the board's server"
        ),
        "{after}"
    );
    let (stop, board) = after
        .split_once(&format!("{REPLAY_THROUGH}: "))
        .unwrap_or_else(|| panic!("{after}"));
    assert!(stop.contains("Goodbye!"), "its clean stop: {stop}");
    assert!(
        format!("{REPLAY_THROUGH}: {board}").starts_with(&format!("{REPLAY_ENDS}\n")),
        "{board}"
    );
    assert!(!board.contains(REPLAY_BEGINS), "replayed twice: {board}");
}

/// A Symphony stopped in the middle of its start's replay leaves the replay's server running,
/// detached, to finish alone. While it replays, a read is refused within its bound, naming the
/// replay in the words a page reads, and the server is never signalled. Once it is through, the
/// next command stops it cleanly and starts the board's own server under the board's memtable:
/// nothing is replayed twice and no row is lost, even to a kill of the board's server after.
#[test]
fn a_replays_server_its_starter_left_running_is_stopped_once_through_never_the_boards() {
    let s = Scratch::new("left-replay");
    let (surreal, replay, memtable) = left_running(&s);

    let refused = board(&s, &surreal)
        .waiting_at_most(Duration::from_millis(300))
        .starting_apart()
        .ensure()
        .unwrap_err()
        .to_string();
    assert!(pid_alive(replay), "the replay's server was signalled");
    let (of, rest) = refused
        .strip_prefix("a private replay of the store's log of ")
        .and_then(|rest| rest.split_once(" MiB before the board's server starts runs in its own "))
        .unwrap_or_else(|| panic!("{refused}"));
    assert!(of.parse::<f64>().unwrap() > 2.0, "{refused}");
    assert!(
        rest.starts_with(&format!("server (pid {replay}, "))
            && rest.contains(
                "), left running when its starter ended, and has not answered within 0.3 s: it is \
             never signalled while it replays, nor adopted as the board's server; once it \
             answers, the next start stops it cleanly and starts the board's own. It has run "
            ),
        "{refused}"
    );
    assert!(replaying_in(&refused).is_some(), "{refused}");
    assert_eq!(board(&s, &surreal).recorded(), None);
    assert_eq!(
        board(&s, &surreal).left_replay().map(|r| r.pid),
        Some(replay)
    );

    std::fs::write(s.dir.join("go"), "").unwrap();
    let running = board(&s, &surreal).ensure().unwrap();
    the_boards_own(&s, &running, (replay, memtable));
    assert_eq!(board(&s, &surreal).recorded(), Some(running.clone()));
    assert!(
        logged(&s.dir) < SMALL.memtable / 2,
        "the replay's stop emptied the log: {}",
        logged(&s.dir)
    );

    signal(running.pid, "KILL");
    assert!(gone(running.pid));
    let again = board(&s, &surreal).ensure().unwrap();
    assert_eq!(rows(&again.addr), ROWS, "rows were lost to the kill");
    assert_eq!(starts(&s).last(), Some(&(again.pid, SMALL.memtable)));
}

/// The Symphony's read, restarted while the replay's server it left still ran, has that server's
/// stop and the board's start run apart once it answers, as a replay runs: refused at once, naming
/// the start, until the board's own server answers.
#[test]
fn a_read_starting_apart_has_a_left_replays_server_stopped_and_the_boards_started_apart() {
    let s = Scratch::new("left-replay-apart");
    let (surreal, replay, memtable) = left_running(&s);
    std::fs::write(s.dir.join("go"), "").unwrap();
    let left = board(&s, &surreal).left_replay().unwrap();
    let began = Instant::now();
    while !healthy(&left.addr) {
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "it never answered"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let read = || {
        board(&s, &surreal)
            .waiting_at_most(Duration::from_secs(5))
            .starting_apart()
    };
    let refused = read().ensure().unwrap_err().to_string();
    assert!(
        refused.starts_with("the board's server is started apart from this read, by pid "),
        "{refused}"
    );
    assert!(replaying_in(&refused).is_some(), "{refused}");
    let running = loop {
        match read().ensure() {
            Ok(running) => break running,
            Err(problem) => {
                assert!(began.elapsed() < Duration::from_secs(120), "{problem}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };
    the_boards_own(&s, &running, (replay, memtable));
}

/// The hold stops a replay's server its starter left running as it stops any server on the data,
/// once it answers, and removes the replay's record: nothing serves the data, and the release's
/// next start replays nothing.
#[test]
fn a_hold_stops_a_left_replays_server_once_through_and_its_record_goes() {
    let s = Scratch::new("left-replay-hold");
    let (surreal, replay, memtable) = left_running(&s);
    std::fs::write(s.dir.join("go"), "").unwrap();
    let home = board(&s, &surreal);
    let holding = home.hold("russet-lynx", "a swap").unwrap();
    assert_eq!(
        holding.stopped.iter().map(|r| r.pid).collect::<Vec<_>>(),
        [replay]
    );
    assert!(gone(replay));
    assert_eq!(home.left_replay(), None);
    assert!(!s.dir.join("replay.json").exists());
    assert_eq!(home.serving(), None);
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert!(
        log.contains(&format!(
            "{REPLAY_THROUGH}: the stop of every server on the data goes on\n"
        )),
        "{log}"
    );
    home.release("russet-lynx", None).unwrap();
    let running = home.ensure().unwrap();
    assert_eq!(
        starts(&s),
        [(replay, memtable), (running.pid, SMALL.memtable)]
    );
    assert_eq!(rows(&running.addr), ROWS);
}

#[test]
#[ignore = "owned helper process: the starter a case kills in the middle of its replay"]
fn helper_starts_the_board() {
    let dir = std::env::var_os("SF_LEFT_DIR").expect("the board's folder");
    let surreal = std::env::var_os("SF_LEFT_SURREAL").expect("its surreal");
    // Killed in the middle of its replay; otherwise through once the board's server answers.
    let _ = Home::new(Path::new(&dir), Path::new(&surreal))
        .storage(SMALL)
        .ensure();
}
