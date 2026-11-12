//! A snapshot of the board's store ([`Home::snapshot`]): taken only while no server runs on it,
//! listed newest first, the newest three kept, a leftover clone cut short removed; taken by the
//! launcher before each start of a board that takes them, and by none that does not; one that
//! cannot be cloned is none, never a plain copy, and never keeps the server from starting; and a
//! store put back from one, a clone of it in another folder, opens at its schema with every task and
//! message it held when it was taken.

use crate::common::Scratch;
use serde_json::json;
use symphony_board::rpc::Client;
use symphony_board::server::{Home, Running, snapshot::KEEP};
use symphony_board::{NewTask, Schema, Store};

const T0: f64 = 1_794_469_500.0;

fn board(running: &Running) -> Store {
    Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap()
}

/// How many rows the table `table` of `store`'s board holds.
fn count(store: &Store, table: &str) -> i64 {
    store
        .client()
        .query(
            &format!("RETURN array::len(SELECT VALUE id FROM {table});"),
            json!({}),
        )
        .unwrap()
        .last()
        .and_then(serde_json::Value::as_i64)
        .unwrap()
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
    let done = std::process::Command::new("cp")
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
    without.stop().unwrap();
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
        refused.as_ref().is_err_and(
            |e| e.starts_with(&why) && e.ends_with("), and a snapshot is never a plain copy")
        ),
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
