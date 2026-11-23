//! A PostgreSQL snapshot put back over its board (`Postgres::restore_over`) and the switch's
//! journal ([`Home::move_to`]), each on a scratch board of its own: a switch cut after each of its
//! durable steps is refused to every start and reach until its holder settles it, undone before
//! its commit point and finished after; a move an earlier release left standing is settled the
//! same way, and no board is moved between the engines; and a board not made yet is made on
//! PostgreSQL, its identity first.

use crate::common::Scratch;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphony_board::rows::Dump;
use symphony_board::server::{Engine, GONE, Home, Kind, STEPS, ServerError, postgres};
use symphony_board::{NewTask, Opening, Store, pg};

const HANDLE: &str = "russet-lynx";
const OTHER: &str = "amber-heron";
const NOW: f64 = 1_800_000_000.0;

fn programs() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    postgres::discover_programs(&root).unwrap()
}

/// A scratch board on PostgreSQL: its server stopped and its hold released before the folder
/// goes.
struct Board {
    scratch: Scratch,
}

impl Board {
    fn new(name: &str) -> Board {
        Board {
            scratch: Scratch::new(name),
        }
    }

    fn home(&self) -> Home {
        (self.scratch.home().telling(|_| {})).supervised(true)
    }

    fn dir(&self) -> &Path {
        &self.scratch.dir
    }

    /// The board's database, its server started: made on PostgreSQL where it is not made yet.
    fn store(&self) -> Store {
        let serving = self.home().postgresql(&programs()).ensure().unwrap();
        Store::opening_on(
            pg::Client::new(&serving.socket, "tasks").into(),
            Box::new(|| NOW),
            Box::new(|_| true),
            "this-host",
            &Opening::write(),
        )
        .unwrap()
    }

    /// A board with two tasks and a row of the tables a switch carries.
    fn seeded(&self) -> Store {
        let store = self.store();
        for slug in ["sim/jump", "sim/land"] {
            store
                .create(&NewTask::new(slug, "A task", "owner"))
                .unwrap();
        }
        let raw = store.raw();
        raw.row("callback_state", HANDLE)
            .create(json!({
                "handle": HANDLE, "address": "uds:/tmp/one.sock", "seen": {"sim/jump": 3},
            }))
            .unwrap();
        raw.row("counter", json!(["hold", "sim/jump"]))
            .create(json!({"last": 0, "ts": NOW}))
            .unwrap();
        store
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        let home = self.home();
        let _ = std::fs::remove_file(self.dir().join("move.json"));
        if let Some(hold) = home.held() {
            let _ = home.release(&hold.by, None);
        }
        let pg = Home::at(self.dir()).postgresql(&programs());
        if pg.data().exists() {
            let _ = pg.stop();
        }
    }
}

/// What a switch must keep of the carried tables: all but the writers' lines, which each store
/// that opens the board writes again.
fn carried(store: &Store) -> Value {
    let mut carried = store.carried().unwrap();
    carried.as_object_mut().unwrap().remove("writer");
    counted(&mut carried);
    carried
}

/// Every number of `value` as what it counts, a whole number of seconds `3.0` as `3`.
fn counted(value: &mut Value) {
    match value {
        Value::Number(number) => *value = json!(number.as_f64()),
        Value::Array(items) => items.iter_mut().for_each(counted),
        Value::Object(fields) => fields.values_mut().for_each(counted),
        _ => {}
    }
}

/// A lease of each kind held on `store`'s board, as a dispatcher, a host and a close hold them:
/// the revision the close holds the session under.
fn leased(store: &Store) -> u64 {
    let raw = store.raw();
    raw.row("callback_lease", HANDLE)
        .put(json!({"until": NOW + 600.0, "revision": 7}))
        .unwrap();
    raw.row("callback_lease", json!(["dispatch", "this-host"]))
        .put(json!({"until": NOW + 600.0, "revision": 3}))
        .unwrap();
    store.take_session(HANDLE, "closing").unwrap()
}

/// When each of [`leased`]'s leases ends and its revision: the recipient's, the host's, the
/// session's.
fn leases(store: &Store) -> [(f64, u64); 3] {
    let raw = store.raw();
    [
        json!(HANDLE),
        json!(["dispatch", "this-host"]),
        json!(["session", HANDLE]),
    ]
    .map(|id| {
        let lease = raw.row("callback_lease", id).get().unwrap();
        (
            lease["until"].as_f64().unwrap(),
            lease["revision"].as_u64().unwrap(),
        )
    })
}

fn board(store: &Store) -> (Dump, Value) {
    (store.dump().unwrap(), carried(store))
}

fn refusal<T: std::fmt::Debug>(result: Result<T, ServerError>) -> String {
    result.unwrap_err().to_string()
}

/// A switch that a test cut, or one an earlier release left, is refused to everything but its
/// holder's next move or restore.
fn stands(b: &Board, home: &Home) {
    let unsettled = "until its holder settles it";
    for refused in [
        refusal(home.backend()),
        refusal(home.postgresql(&programs()).ensure()),
        refusal(home.release(HANDLE, None)),
    ] {
        assert!(
            refused.contains(unsettled) || refused.contains("held by"),
            "{refused}"
        );
    }
    assert!(refusal(home.backend()).contains(unsettled));
    assert!(refusal(home.release(HANDLE, None)).contains(unsettled));
    assert!(matches!(
        home.move_to(Engine::PostgreSql, OTHER),
        Err(ServerError::Held(hold)) if hold.by == HANDLE
    ));
    assert_eq!(home.held().map(|hold| hold.by), Some(HANDLE.into()));
    assert!(b.dir().join("move.json").is_file());
}

/// The journal of a move an earlier release left standing in `b`'s home, holding it as
/// [`HANDLE`]: from SurrealDB to PostgreSQL, its old store kept in `aside`, its new data staged in
/// `staged`, its work folder `moving/`.
fn left_by_an_earlier_release(b: &Board, aside: &Path, staged: &Path, committed: bool) {
    let journal = json!({
        "format": 1, "kind": "move", "from": "surrealdb", "to": "postgresql",
        "aside": aside, "staged": staged, "work": b.dir().join("moving"),
        "by": HANDLE, "took": true, "committed": committed,
    });
    std::fs::write(b.dir().join("move.json"), journal.to_string()).unwrap();
    let hold = json!({"by": HANDLE, "why": "moving the board", "at": 1_800_000_000, "pid": 1});
    std::fs::write(b.dir().join("hold"), hold.to_string()).unwrap();
}

/// This build moves no board between the engines: a board on PostgreSQL already, one not made
/// yet and one still on SurrealDB each say so, and nothing is moved. A move an earlier release
/// left standing is settled by its holder's `move_to`, as the switch it is: one that committed is
/// finished, its old store kept aside; one that had not is undone, the board back on SurrealDB,
/// which is then refused, naming how it moves.
#[test]
fn a_move_an_earlier_release_left_is_settled_by_its_holder_and_no_board_is_moved_between_engines() {
    let b = Board::new("move-left");
    let home = b.home();
    let unmade = refusal(home.move_to(Engine::PostgreSql, HANDLE));
    assert!(
        unmade.contains("not made yet: it has nothing to move"),
        "{unmade}"
    );
    let before = board(&b.seeded());
    let already = refusal(home.move_to(Engine::PostgreSql, HANDLE));
    assert!(
        already.contains("on PostgreSQL already: nothing was moved"),
        "{already}"
    );
    let gone = refusal(home.move_to(Engine::SurrealDb, HANDLE));
    assert!(gone.contains(GONE), "{gone}");
    assert_eq!(home.backend().unwrap(), Engine::PostgreSql);
    assert_eq!(home.held(), None);

    // Committed: the board is on PostgreSQL, its SurrealDB store aside; what is left is finished.
    home.postgresql(&programs()).stop().unwrap();
    let aside = b.dir().join("moved/2026-11-12T07-45-00Z-surrealdb");
    std::fs::create_dir_all(aside.join("data")).unwrap();
    std::fs::create_dir_all(b.dir().join("moving")).unwrap();
    let staged = b.dir().join("moving/postgresql");
    left_by_an_earlier_release(&b, &aside, &staged, true);
    stands(&b, &home);
    let moved = home.move_to(Engine::PostgreSql, HANDLE).unwrap();
    assert_eq!(
        (moved.from, moved.to, &moved.aside, moved.held),
        (Engine::SurrealDb, Engine::PostgreSql, &aside, false)
    );
    assert_eq!(home.switching().unwrap(), None);
    assert_eq!(home.held(), None, "the move's own hold is released");
    assert!(!b.dir().join("moving").exists(), "its work folder is gone");
    assert!(aside.join("data").is_dir(), "the old store is kept");
    assert_eq!(home.backend().unwrap(), Engine::PostgreSql);
    assert_eq!(board(&b.store()), before, "the board reads as it did");

    // Not committed: undone, the PostgreSQL data back where it was staged and the SurrealDB store
    // back in place; the board is on SurrealDB as it was, and nothing starts it.
    home.postgresql(&programs()).stop().unwrap();
    std::fs::create_dir_all(b.dir().join("moving")).unwrap();
    std::fs::write(
        b.dir().join("backend.json"),
        r#"{"format":1,"engine":"postgresql"}"#,
    )
    .unwrap();
    left_by_an_earlier_release(&b, &aside, &staged, false);
    stands(&b, &home);
    let undone = refusal(home.move_to(Engine::PostgreSql, HANDLE));
    assert!(
        undone.contains("is now undone: the board is on SurrealDB as it was"),
        "{undone}"
    );
    assert_eq!(home.held(), None);
    assert_eq!(home.switching().unwrap(), None);
    assert!(b.dir().join("data").is_dir() && !aside.join("data").exists());
    assert_eq!(home.backend().unwrap(), Engine::SurrealDb);
    assert!(!b.dir().join("postgresql").exists() && !staged.exists());
    for refused in [
        refusal(home.move_to(Engine::PostgreSql, HANDLE)),
        refusal(home.postgresql(&programs()).ensure()),
        refusal(home.board_server(b.dir(), true)),
    ] {
        assert!(refused.contains(GONE), "{refused}");
    }
}

#[test]
fn a_snapshot_is_put_back_over_its_board_and_the_cluster_it_replaced_is_kept() {
    let b = Board::new("restore-over");
    let home = b.home();
    drop(b.seeded());
    let pg = home.postgresql(&programs());
    let before = board(&b.store());
    pg.hold(HANDLE, "a snapshot").unwrap();
    let snapshot = pg.snapshot(NOW).unwrap();
    pg.release(HANDLE, None).unwrap();
    b.store()
        .create(&NewTask::new("sim/later", "Made after", "owner"))
        .unwrap();
    let after = board(&b.store());
    assert_ne!(after, before);

    // Only a snapshot of this board is put back, and another's hold refuses it.
    let foreign = refusal(pg.restore_over(&b.dir().join("postgresql"), HANDLE, NOW));
    assert!(
        foreign.contains("is no snapshot of this board"),
        "{foreign}"
    );
    let missing = refusal(pg.restore_over(&b.dir().join("nowhere"), HANDLE, NOW));
    assert!(
        missing.contains("is no snapshot of this board"),
        "{missing}"
    );
    pg.hold(OTHER, "its own work").unwrap();
    assert!(matches!(
        pg.restore_over(&snapshot, HANDLE, NOW),
        Err(ServerError::Held(hold)) if hold.by == OTHER
    ));
    pg.release(OTHER, None).unwrap();
    assert_eq!(board(&b.store()), after);

    // Cut after each step: undone before the commit, finished after it.
    let commit = STEPS.iter().position(|step| *step == "committed").unwrap();
    for (at, step) in STEPS.iter().enumerate() {
        let cutting = home.clone().stopping_after(step).postgresql(&programs());
        let cut = refusal(cutting.restore_over(&snapshot, HANDLE, NOW));
        assert!(
            cut.contains(&format!("ended the switch after {step:?}")),
            "{cut}"
        );
        let journal = home.switching().unwrap().unwrap();
        assert_eq!(
            (journal.kind, journal.committed),
            (Kind::Restore, at >= commit)
        );
        assert!(refusal(pg.ensure()).contains("until its holder settles it"));
        assert!(refusal(home.release(HANDLE, None)).contains("until its holder settles it"));
        let settled = pg.restore_over(&snapshot, HANDLE, NOW);
        assert_eq!(home.switching().unwrap(), None, "{step}");
        assert_eq!(home.held(), None, "{step}");
        assert!(!b.dir().join("postgresql.restoring").exists(), "{step}");
        if at < commit {
            assert!(refusal(settled).contains("is now undone"), "{step}");
            assert_eq!(board(&b.store()), after, "{step}: the cluster it had");
        } else {
            let aside = settled.unwrap();
            assert!(
                aside.join("postgresql").join("PG_VERSION").is_file(),
                "{step}"
            );
            assert_eq!(board(&b.store()), before, "{step}: the snapshot's board");
            // The later task again, for the next step's restore to take away.
            b.store()
                .create(&NewTask::new("sim/later", "Made after", "owner"))
                .unwrap();
        }
    }

    // Whole, with the caller's hold left standing and the home bound to no generation.
    let authority = Arc::new(home.recovery("tasks"));
    b.store().recovering(authority.clone()).rebind().unwrap();
    pg.hold(HANDLE, "a restore").unwrap();
    let aside = pg.restore_over(&snapshot, HANDLE, NOW).unwrap();
    assert!(aside.join("postgresql").join("PG_VERSION").is_file());
    assert_eq!(home.held().map(|hold| hold.by), Some(HANDLE.into()));
    pg.release(HANDLE, None).unwrap();
    assert!(
        snapshot.join("PG_VERSION").is_file(),
        "the snapshot is kept"
    );
    let restored = b.store();
    assert_eq!(board(&restored), before);
    assert!(restored.recovering(authority).bound("sim/jump").is_err());

    // A failure before the commit leaves the cluster it had.
    b.store()
        .create(&NewTask::new("sim/later", "Made after", "owner"))
        .unwrap();
    let failing = home.clone().failing_after("aside").postgresql(&programs());
    let failed = refusal(failing.restore_over(&snapshot, HANDLE, NOW));
    assert!(failed.contains("the snapshot was not restored"), "{failed}");
    assert!(
        failed.contains("on the cluster it had, and its hold is released"),
        "{failed}"
    );
    assert_eq!(home.held(), None);
    assert_eq!(board(&b.store()).0.tasks.len(), 3);

    // An undo that fails too never releases: the journal and the hold stand, nothing is lost, and
    // its holder's next restore or move settles it.
    let stuck = home.clone().stuck_after("placed").postgresql(&programs());
    let refused = refusal(stuck.restore_over(&snapshot, HANDLE, NOW));
    assert!(refused.contains("could not be undone"), "{refused}");
    assert!(
        refused.contains("the board stays held and its journal stands"),
        "{refused}"
    );
    stands(&b, &home);
    let undone = refusal(home.move_to(Engine::PostgreSql, HANDLE));
    assert!(undone.contains("is now undone"), "{undone}");
    assert!(undone.contains("its hold released"), "{undone}");
    assert_eq!(home.held(), None);
    assert_eq!(board(&b.store()).0.tasks.len(), 3);
}

/// A snapshot put back brings no lease revision back with it: a holder that took its lease
/// after the snapshot is past the snapshot's, and the restored lease is past the holder's.
#[test]
fn a_restored_snapshots_leases_are_ended_past_the_ones_the_board_held_since() {
    let b = Board::new("restore-leases");
    let home = b.home();
    drop(b.seeded());
    let pg = home.postgresql(&programs());
    let closing = leased(&b.store());
    let held_then = leases(&b.store());
    assert_eq!(held_then.map(|(_, revision)| revision), [7, 3, closing]);
    pg.hold(HANDLE, "a snapshot").unwrap();
    let snapshot = pg.snapshot(NOW).unwrap();
    pg.release(HANDLE, None).unwrap();

    // Since the snapshot: the recipient committed and its lease was taken again, the session
    // was released and taken twice, and a lease the snapshot never held was taken.
    let since = b.store();
    let raw = since.raw();
    raw.row("callback_state", HANDLE)
        .set(json!({"revision": 11}))
        .unwrap();
    raw.row("callback_lease", HANDLE)
        .put(json!({"until": NOW + 600.0, "revision": 12}))
        .unwrap();
    raw.row("callback_lease", json!(["dispatch", "another-host"]))
        .put(json!({"until": NOW + 600.0, "revision": 2}))
        .unwrap();
    since.release_session(HANDLE, closing).unwrap();
    let released = since.take_session(HANDLE, "restoring").unwrap();
    since.release_session(HANDLE, released).unwrap();
    let held = since.take_session(HANDLE, "closing").unwrap();
    assert!(held > closing + 1);
    drop(since);

    pg.restore_over(&snapshot, HANDLE, NOW).unwrap();
    assert!(
        !b.dir().join("restoring").exists(),
        "the work folder is gone"
    );
    let restored = b.store();
    assert_eq!(leases(&restored), [(0.0, 13), (0.0, 4), (0.0, held + 1)]);
    let other = (restored.raw())
        .row("callback_lease", json!(["dispatch", "another-host"]))
        .get()
        .unwrap();
    assert_eq!(
        (other["until"].as_f64(), other["revision"].as_u64()),
        (Some(0.0), Some(3)),
        "a lease taken since the snapshot is there, ended, past its holder's"
    );
    // The holder from after the snapshot holds nothing, and the next taker is past it.
    for old in [closing, released, held] {
        assert!(!restored.renew_session(HANDLE, old).unwrap(), "{old}");
    }
    let taken = restored.take_session(HANDLE, "closing").unwrap();
    assert!(taken > held + 1, "{taken} is past {held}");
    drop(restored);

    // A cluster in place that cannot be served does not stop the way back: the snapshot's
    // leases are moved on by more than any holder took, and the home is told.
    let told = Arc::new(std::sync::Mutex::new(Vec::new()));
    let telling = {
        let told = told.clone();
        (home.clone())
            .telling(move |line| told.lock().unwrap().push(line.to_string()))
            .postgresql(&programs())
    };
    pg.hold(HANDLE, "a restore").unwrap();
    std::fs::remove_file(b.dir().join("postgresql/global/pg_control")).unwrap();
    telling.restore_over(&snapshot, HANDLE, NOW).unwrap();
    pg.release(HANDLE, None).unwrap();
    let told = told.lock().unwrap().join("\n");
    assert!(
        told.contains("the cluster in place could not be read"),
        "{told}"
    );
    let unread = leases(&b.store());
    assert!(unread.iter().all(|(until, _)| *until == 0.0), "{unread:?}");
    assert!(
        unread.iter().all(|(_, revision)| *revision > 1 << 32),
        "{unread:?}"
    );
    assert!(!b.store().renew_session(HANDLE, taken).unwrap());
}

#[test]
fn a_board_that_is_not_supervised_keeps_its_postgresql_between_its_commands() {
    let b = Board::new("pg-apart");
    drop(b.seeded());
    // As a board outside a scratch place: no lifeline to the process that starts it.
    let apart = b.scratch.home().telling(|_| {}).supervised(false);
    let pg = apart.postgresql(&programs());
    let serving = pg.ensure().unwrap();
    assert_eq!(
        pg.ensure().unwrap().pid,
        serving.pid,
        "a second start takes the first's"
    );
    assert_eq!(pg.current().unwrap().map(|s| s.pid), Some(serving.pid));
    let store = Store::opening_on(
        pg::Client::new(&serving.socket, "tasks").into(),
        Box::new(|| NOW),
        Box::new(|_| true),
        "this-host",
        &Opening::write(),
    )
    .unwrap();
    assert_eq!(store.tasks(None).unwrap().len(), 2);
    assert_eq!(pg.stop().unwrap().map(|s| s.pid), Some(serving.pid));
    assert!(pg.current().unwrap().is_none());
}

#[test]
fn a_board_outside_a_scratch_place_not_made_yet_is_made_on_postgresql_named_first() {
    let b = Board::new("pg-unnamed");
    assert!(
        !symphony_board::server::scratch(b.dir()),
        "the target's folder is no scratch place"
    );
    let home = b.scratch.home().telling(|_| {});
    // Naming its engine in place is a scratch home's alone.
    let made = refusal(home.initialize_backend(Engine::PostgreSql));
    assert!(made.contains("scratch"), "{made}");
    assert!(home.unmade().unwrap());
    assert!(!b.dir().join("backend.json").exists());
    // Its first start makes it on PostgreSQL, as a live board's is: the identity, then the
    // cluster, the server outliving the start as a board's anywhere but a scratch place does.
    let pg = home.postgresql(&programs());
    let serving = pg.ensure().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(
            &std::fs::read_to_string(b.dir().join("backend.json")).unwrap()
        )
        .unwrap(),
        json!({"format": 1, "engine": "postgresql"})
    );
    assert!(pg.data().join("PG_VERSION").is_file());
    assert_eq!(home.backend().unwrap(), Engine::PostgreSql);
    assert!(!home.unmade().unwrap());
    assert_eq!(pg.stop().unwrap().map(|s| s.pid), Some(serving.pid));
    assert!(pg.current().unwrap().is_none());
}
