//! A snapshot of the board's store ([`Home::snapshot`]): taken only while no server runs on it,
//! listed newest first, the newest three kept, a leftover clone cut short removed; taken by the
//! launcher before each start of a board that takes them, and by none that does not; one that
//! cannot be cloned is none, no plain copy, and never keeps the server from starting; and a
//! store put back from one, a clone of it in another folder, opens at its schema with every task and
//! message it held when it was taken.

use crate::children::Child as _;
use crate::common::{Scratch, gone, signal};
use crate::home_fixture::healthy_server;
use std::convert::Infallible;
use std::path::Path;
use std::time::{Duration, Instant};
use symphony_board::port::{ATTEMPTS, Start, listens, start_on_free_port};
use symphony_board::rpc::{self, Client};
use symphony_board::server::snapshot::{KEEP, KEPT, holds_log};
use symphony_board::server::{Home, Running, Storage};
use symphony_board::sys::{self, Child, Command};
use symphony_board::{NewTask, Schema, Store};

const T0: f64 = 1_794_469_500.0;

/// A server of the test's own, started by `surreal` in `dir` on the store `data` as spelled there
/// (`surrealkv://…`), which no record names and whose command line is not the board's: up once it
/// answers. Ended by its own handle.
fn foreign_server(surreal: &Path, dir: &Path, data: &str) -> Child {
    let log = dir.join("foreign.log");
    let until = Instant::now() + test_time::backstop(Duration::from_secs(60));
    let (child, _) = start_on_free_port(ATTEMPTS, until, |reserved| {
        let addr = reserved.to_string();
        let out = std::fs::File::create(&log).unwrap();
        let mut child = Command::of(surreal)
            .args([
                "start",
                "--unauthenticated",
                "--log",
                "error",
                "--bind",
                &addr,
                data,
            ])
            .current_dir(dir)
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .unwrap();
        // Up once it answers on its port; ended once it exits first.
        let up = test_time::until(
            format!("the server on {addr} answers or ends"),
            until.saturating_duration_since(Instant::now()),
            || {
                if child.try_wait().unwrap().is_some() {
                    return Some(false);
                }
                (rpc::healthy(&addr) && listens(child.id(), reserved, sys::output).unwrap())
                    .then_some(true)
            },
        )
        .unwrap();
        if up {
            return Ok::<_, Infallible>(Start::Up(child));
        }
        Ok(Start::Ended(std::fs::read_to_string(&log).unwrap()))
    })
    .unwrap_or_else(|why| panic!("the server ended: {why}"));
    child
}

fn board(running: &Running) -> Store {
    Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap()
}

/// How many rows the table `table` of `store`'s board holds.
fn count(store: &Store, table: &str) -> i64 {
    i64::try_from(store.raw().table(table).count().unwrap()).unwrap()
}

/// `count` tasks, each with a message on its thread.
fn fill(store: &Store, from: usize, count: usize) {
    for n in from..from + count {
        let slug = format!("snap/t{n}");
        store
            .create(&NewTask::new(&slug, "A task the snapshot holds", "owner"))
            .unwrap();
        store.say(&slug, "owner", "a message", None).unwrap();
    }
}

fn names(home: &Home) -> Vec<String> {
    home.snapshots()
        .unwrap()
        .into_iter()
        .map(|one| one.name)
        .collect()
}

#[test]
fn a_snapshot_is_taken_only_while_no_server_runs_and_the_newest_three_are_kept() {
    let s = Scratch::new("snapshot-taken");
    let home = s.home().snapshotting(false);
    assert_eq!(
        home.snapshot(T0),
        Err(format!(
            "the board has no store at {} to take a snapshot of",
            s.dir.join("data").display()
        ))
    );
    let running = home.ensure().unwrap();
    fill(&board(&running), 0, 2);
    let refused = home.snapshot(T0).unwrap_err();
    assert_eq!(
        refused,
        format!(
            "the board's server (pid {}) runs on {}: a snapshot is taken only while no process \
             serves the store, never of files a server is writing, so none was taken; one is taken \
             before the server's next start, and one of a store held stopped meanwhile is taken by \
             this command",
            running.pid,
            s.dir.join("data").display()
        )
    );
    assert!(names(&home).is_empty(), "nothing was taken");

    home.stop().unwrap();
    // A clone a snapshot cut short left: the next one removes it.
    std::fs::create_dir_all(home.snapshots_folder().join(".taking/manifest")).unwrap();
    let first = home.snapshot(T0).unwrap();
    assert_eq!(first.snapshot.name, "2026-11-12T07-45-00Z");
    assert_eq!(
        first.snapshot.path,
        s.dir.join("snapshots/2026-11-12T07-45-00Z")
    );
    assert!(first.removed.is_empty());
    assert!(
        first.snapshot.path.join("manifest").is_dir(),
        "the store whole"
    );
    assert!(!home.snapshots_folder().join(".taking").exists());

    // A second in the same second, then two more: the newest three stay, by date.
    let again = home.snapshot(T0 + 0.5).unwrap();
    assert_eq!(again.snapshot.name, "2026-11-12T07-45-00Z.1");
    assert!(home.snapshot(T0 + 60.0).unwrap().removed.is_empty());
    let fourth = home.snapshot(T0 + 3600.0).unwrap();
    assert_eq!(KEEP, 3);
    assert_eq!(fourth.removed, ["2026-11-12T07-45-00Z"]);
    assert_eq!(
        names(&home),
        [
            "2026-11-12T08-45-00Z",
            "2026-11-12T07-46-00Z",
            "2026-11-12T07-45-00Z.1"
        ]
    );
    assert!(!first.snapshot.path.exists());
    // Only what is named as a snapshot is one, and only those go.
    std::fs::create_dir_all(home.snapshots_folder().join("kept-by-hand")).unwrap();
    home.snapshot(T0 + 7200.0).unwrap();
    assert_eq!(names(&home).len(), 3);
    assert!(home.snapshots_folder().join("kept-by-hand").is_dir());
}

/// The store's own lock is what says a server has it open: a server given the store's path in
/// another spelling (a trailing slash, a link to it, a path relative to where it runs) is not the
/// board's by its command line, and a snapshot is refused all the same.
#[test]
fn a_snapshot_is_refused_while_a_server_has_the_store_open_by_any_spelling_of_its_path() {
    let s = Scratch::new("snapshot-spelled");
    let home = s.home().snapshotting(false);
    fill(&board(&home.ensure().unwrap()), 0, 1);
    home.stop().unwrap();
    let data = s.dir.join("data");
    std::os::unix::fs::symlink(&data, s.dir.join("link")).unwrap();
    let lock = data.join("LOCK");
    for spelling in [
        format!("surrealkv://{}/", data.display()),
        format!("surrealkv://{}", s.dir.join("link").display()),
        "surrealkv://data".to_string(),
    ] {
        let mut server = foreign_server(s.surreal(), &s.dir, &spelling);
        let refused = home.snapshot(T0);
        let _ = server.kill();
        let _ = server.wait();
        assert_eq!(
            refused,
            Err(format!(
                "a process (pid {}) holds the store's own lock, {}, so a server has the store \
                 open, by whatever path it was given: a snapshot is taken only while no process \
                 serves the store, never of files a server is writing, so none was taken; one is \
                 taken before the server's next start, and one of a store held stopped meanwhile \
                 is taken by this command",
                server.id(),
                lock.display()
            )),
            "{spelling}"
        );
        assert!(names(&home).is_empty(), "{spelling}: nothing was taken");
    }
    // Once none has it open, one is taken; the server ended by a kill left its log behind.
    let taken = home.snapshot(T0).unwrap();
    assert!(taken.snapshot.log, "{taken:?}");
    assert!(taken.snapshot.path.join("LOCK").is_file());
}

/// A snapshot whose write-ahead log holds writes not yet in its tables (one an unclean end left, or
/// a store that went back) may be the only copy of them: it is kept until the owner removes it, and
/// the newest three of the others are kept.
#[test]
fn a_snapshot_whose_log_holds_writes_is_never_pruned_and_the_others_are_kept_by_count() {
    let s = Scratch::new("snapshot-log-kept");
    let home = s.home().snapshotting(false);
    let data = s.dir.join("data");
    let running = home.ensure().unwrap();
    fill(&board(&running), 0, 2);
    assert!(holds_log(&data), "a running server's log holds its writes");
    // Ended uncleanly: its log is the store's only record of what it wrote.
    signal(running.pid, "KILL");
    assert!(gone(running.pid));
    assert!(holds_log(&data));
    let unclean = home.snapshot(T0).unwrap();
    assert_eq!(
        (unclean.snapshot.name.as_str(), unclean.snapshot.log),
        ("2026-11-12T07-45-00Z", true)
    );
    // The next start replays it, and a clean stop leaves the log empty.
    let store = board(&home.ensure().unwrap());
    assert_eq!(count(&store, "task"), 2, "replayed");
    home.stop().unwrap();
    assert!(!holds_log(&data), "a clean stop empties the log");

    let mut taken = Vec::new();
    for minute in 1..=5 {
        let one = home.snapshot(T0 + 60.0 * f64::from(minute)).unwrap();
        assert!(!one.snapshot.log, "{one:?}");
        taken.push(one);
    }
    assert_eq!(KEEP, 3);
    assert!(taken[..3].iter().all(|one| one.removed.is_empty()));
    assert_eq!(taken[3].removed, ["2026-11-12T07-46-00Z"]);
    assert_eq!(taken[4].removed, ["2026-11-12T07-47-00Z"]);
    let kept = home.snapshots().unwrap();
    assert_eq!(
        kept.iter()
            .map(|one| (one.name.as_str(), one.log))
            .collect::<Vec<_>>(),
        [
            ("2026-11-12T07-50-00Z", false),
            ("2026-11-12T07-49-00Z", false),
            ("2026-11-12T07-48-00Z", false),
            ("2026-11-12T07-45-00Z", true),
        ]
    );
    // A store put back from it holds what the killed server wrote.
    assert!(unclean.snapshot.path.join("wal").is_dir());

    // The owner removes it, and pruning goes on by count alone.
    std::fs::remove_dir_all(&unclean.snapshot.path).unwrap();
    let next = home.snapshot(T0 + 360.0).unwrap();
    assert_eq!(next.removed, ["2026-11-12T07-48-00Z"]);
    assert_eq!(names(&home).len(), KEEP);
}

#[test]
fn a_store_put_back_from_a_snapshot_opens_at_its_schema_with_what_it_held() {
    let s = Scratch::new("snapshot-restored");
    let home = s.home().snapshotting(false);
    let store = board(&home.ensure().unwrap());
    fill(&store, 0, 5);
    let held = (count(&store, "task"), count(&store, "message"));
    assert_eq!(held.0, 5);
    assert!(held.1 >= 5, "{held:?}");
    home.stop().unwrap();
    let taken = home.snapshot(T0).unwrap();
    // The board moves on after the snapshot.
    let store = board(&home.ensure().unwrap());
    fill(&store, 5, 3);
    let after = (count(&store, "task"), count(&store, "message"));
    assert_eq!(after.0, 8);
    assert!(after.1 >= held.1 + 3, "{after:?}");
    home.stop().unwrap();

    // Put back: a clone of the snapshot in a folder of its own, never the snapshot itself.
    let other = Scratch::new("snapshot-restored-into");
    let done = std::process::Command::of("cp")
        .args(if cfg!(target_os = "macos") {
            &["-c", "-R"][..]
        } else {
            &["-R", "--reflink=auto"][..]
        })
        .arg(&taken.snapshot.path)
        .arg(other.dir.join("data"))
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
    let restored = other.home().snapshotting(false);
    let store = board(&restored.ensure().unwrap());
    assert_eq!(
        store.board_version().unwrap(),
        Some(Schema::this_build().version())
    );
    assert_eq!((count(&store, "task"), count(&store, "message")), held);
    assert_eq!(store.get("snap/t4").unwrap().slug, "snap/t4");
    assert!(store.get("snap/t5").is_err(), "written after it was taken");
    restored.stop().unwrap();
}

#[test]
fn the_launcher_takes_one_before_each_start_of_a_board_that_takes_them() {
    let s = Scratch::new("snapshot-before-start");
    let home = s.home().snapshotting(true);
    // The first start has no store to keep.
    fill(&board(&home.ensure().unwrap()), 0, 1);
    assert!(names(&home).is_empty());
    home.stop().unwrap();
    let running = home.ensure().unwrap();
    let kept = home.snapshots().unwrap();
    assert_eq!(kept.len(), 1, "{kept:?}");
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert!(
        log.contains(&format!(
            "· a snapshot of the store before this start: {}\n",
            kept[0].path.display()
        )),
        "{log}"
    );
    // A server that answers is used as it is: no start, no snapshot.
    assert_eq!(home.ensure().unwrap(), running);
    assert_eq!(home.snapshots().unwrap().len(), 1);

    // One that does not take them takes none.
    home.stop().unwrap();
    let without = s.home().snapshotting(false);
    without.ensure().unwrap();
    assert_eq!(names(&without).len(), 1);

    // After an unclean end the one before the start holds the log, and the log says it is kept.
    let running = without.ensure().unwrap();
    fill(&board(&running), 1, 1);
    signal(running.pid, "KILL");
    assert!(gone(running.pid));
    home.ensure().unwrap();
    let kept = home.snapshots().unwrap();
    assert!(kept[0].log, "{kept:?}");
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert!(
        log.contains(&format!(
            "· a snapshot of the store before this start: {}{KEPT}\n",
            kept[0].path.display()
        )),
        "{log}"
    );
    home.stop().unwrap();
}

#[test]
fn a_snapshot_that_cannot_be_cloned_takes_none_and_never_keeps_the_server_from_starting() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("snapshot-refused");
    let home = s.home().snapshotting(true);
    fill(&board(&home.ensure().unwrap()), 0, 1);
    home.stop().unwrap();
    // A folder no clone can be made in: the clone is refused, and no plain copy stands in for it.
    let folder = home.snapshots_folder();
    std::fs::create_dir_all(&folder).unwrap();
    let writable = |mode| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode));
    writable(0o555).unwrap();
    let refused = home.snapshot(T0);
    let (data, taking) = (s.dir.join("data"), folder.join(".taking"));
    let why = format!(
        "could not clone {} to {} copy-on-write (",
        data.display(),
        taking.display()
    );
    assert!(
        refused.as_ref().is_err_and(|e| e.starts_with(&why)
            && e.ends_with(
                "), and a snapshot is a plain copy only before a replay of the store's log or a \
                 reclaim"
            )),
        "{refused:?}"
    );

    // The launcher says why it took none, and starts the server all the same.
    let running = home.ensure().unwrap();
    assert_eq!(board(&running).get("snap/t0").unwrap().slug, "snap/t0");
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert!(
        log.contains(&format!(
            "· no snapshot of the store before this start: {why}"
        )),
        "{log}"
    );
    writable(0o755).unwrap();
    assert!(names(&home).is_empty() && !taking.exists());
    home.stop().unwrap();
}

/// A replay never runs without its snapshot, on a board that takes none before other starts too:
/// a start whose log is to be replayed and whose store can be neither cloned nor copied is refused
/// before anything is spawned, the log as it was; with room, the same start keeps the store first,
/// a snapshot never pruned, then replays.
#[test]
fn a_replay_never_runs_without_its_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("snapshot-before-replay");
    let small = Storage {
        memtable: 4 << 20,
        ..Storage::BOARD
    };
    let segment = s.dir.join("data/wal/00000000000000000000.wal");
    std::fs::create_dir_all(segment.parent().unwrap()).unwrap();
    std::fs::write(&segment, vec![7u8; 3 << 20]).unwrap();
    let home = || {
        Home::new(&s.dir, &healthy_server(&s.dir))
            .storage(small)
            .snapshotting(false)
            .telling(|_| {})
    };
    let folder = home().snapshots_folder();
    std::fs::create_dir_all(&folder).unwrap();
    let writable = |mode| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode));
    writable(0o555).unwrap();
    let refused = home().ensure().map(|running| running.pid);
    writable(0o755).unwrap();
    let why = format!(
        "the board's store at {} could not be kept before the replay of its log: it could not be \
         cloned (",
        s.dir.join("data").display()
    );
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().starts_with(&why)),
        "{refused:?}"
    );
    assert_eq!(home().recorded(), None, "nothing was started");
    assert_eq!(std::fs::read(&segment).unwrap(), vec![7u8; 3 << 20]);
    assert!(home().snapshots().unwrap().is_empty());

    let running = home().ensure().unwrap();
    assert_eq!(home().recorded(), Some(running));
    let kept = home().snapshots().unwrap();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].log, "it holds the log the replay applied");
    assert_eq!(
        std::fs::read(kept[0].path.join("wal/00000000000000000000.wal")).unwrap(),
        vec![7u8; 3 << 20]
    );
    assert_eq!(home().kept(), (vec![kept[0].path.clone()], vec![]));
}
