//! A board's store moved between SurrealDB and PostgreSQL ([`Home::move_to`]), and a PostgreSQL
//! snapshot put back over its board (`Postgres::restore_over`), each on a scratch board of its
//! own: the board and the tables no copy holds read alike after as before, in both directions;
//! each refusal and failure leaves the board on the engine it was on, nothing deleted; and a
//! switch cut after each of its durable steps is refused to every start and reach until its
//! holder settles it, undone before its commit point and finished after.

use crate::common::Scratch;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphony_board::rows::Dump;
use symphony_board::server::{EXPORT, Engine, Home, Kind, STEPS, ServerError, postgres};
use symphony_board::{Database, NewTask, Opening, Schema, Store, pg, rpc};

const HANDLE: &str = "russet-lynx";
const OTHER: &str = "amber-heron";
const NOW: f64 = 1_800_000_000.0;

fn programs() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    postgres::discover_programs(&root).unwrap()
}

/// A scratch board, whichever engine it is on when the test ends: its PostgreSQL stopped and
/// its hold released before the folder goes.
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
        (self.scratch.home().telling(|_| {}))
            .supervised(true)
            .snapshotting(false)
    }

    fn dir(&self) -> &Path {
        &self.scratch.dir
    }

    /// The board's database on the engine its home names, its server started.
    fn database(&self) -> Database {
        let home = self.home();
        match home.backend().unwrap() {
            Engine::SurrealDb => {
                rpc::Client::new(&home.ensure().unwrap().addr, "sf", "tasks").into()
            }
            Engine::PostgreSql => {
                let serving = home.postgresql(&programs()).ensure().unwrap();
                pg::Client::new(&serving.socket, "tasks").into()
            }
        }
    }

    fn store(&self) -> Store {
        self.store_as(&Opening::write())
    }

    fn store_as(&self, how: &Opening) -> Store {
        Store::opening_on(
            self.database(),
            Box::new(|| NOW),
            Box::new(|_| true),
            "this-host",
            how,
        )
        .unwrap()
    }

    /// A board with two tasks and a row of the tables a move carries.
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

    fn move_to(
        &self,
        home: &Home,
        to: Engine,
    ) -> Result<symphony_board::server::Moved, ServerError> {
        home.move_to(to, &programs(), HANDLE, NOW)
    }

    /// The folders under `moved/`, in name order.
    fn moved(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.dir().join("moved"))
            .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        found.sort();
        found
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        let home = self.home();
        let _ = std::fs::remove_file(self.dir().join("move.json"));
        if let Some(hold) = home.held() {
            let _ = home.release(&hold.by, None);
        }
        for dir in [self.dir().to_path_buf(), self.dir().join("moving")] {
            let pg = Home::new(&dir, Path::new("/no/surreal")).postgresql(&programs());
            if pg.data().exists() {
                let _ = pg.stop();
            }
        }
        if let Ok(Engine::SurrealDb) = home.backend() {
            let _ = home.stop();
        }
    }
}

/// What a move must keep of the carried tables: all but the writers' lines, which each store
/// that opens the board writes again.
fn carried(store: &Store) -> Value {
    let mut carried = store.carried().unwrap();
    carried.as_object_mut().unwrap().remove("writer");
    counted(&mut carried);
    carried
}

/// Every number of `value` as what it counts: one engine writes a whole number of seconds
/// `3.0` where the other writes `3`.
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

/// `carried`, a board's before a move ([`carried`], every number as what it counts), as the
/// move writes it: each lease ended and its revision moved on by one.
fn ended(carried: &Value) -> Value {
    let mut ended = carried.clone();
    for lease in ended["callback_lease"].as_array_mut().unwrap() {
        lease["until"] = json!(0.0);
        lease["revision"] = json!(lease["revision"].as_f64().unwrap() + 1.0);
    }
    ended
}

fn board(store: &Store) -> (Dump, Value) {
    (store.dump().unwrap(), carried(store))
}

fn refusal<T: std::fmt::Debug>(result: Result<T, ServerError>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn a_board_goes_to_postgresql_and_back_whole_and_a_reader_that_kept_it_reads_it_anew() {
    let b = Board::new("move-round");
    let home = b.home();
    let store = b.seeded();
    // The home is bound to the board's generation, and a token is read under it.
    let authority = || Arc::new(home.recovery("tasks"));
    let bound = b.store().recovering(authority());
    bound.rebind().unwrap();
    let token = bound.bound("sim/jump").unwrap().bound;
    // A lease of each kind is held when the board is moved: a recipient's, a session's close
    // and a host's dispatch.
    let closing = leased(&store);
    let (before, marker) = (board(&store), store.marker().unwrap());
    drop((store, bound));

    let moved = b.move_to(&home, Engine::PostgreSql).unwrap();
    assert_eq!(
        (moved.from, moved.to),
        (Engine::SurrealDb, Engine::PostgreSql)
    );
    assert!(!moved.finished && !moved.held, "{moved:?}");
    assert!(moved.rows >= 4, "{moved:?}");
    assert_eq!(home.backend().unwrap(), Engine::PostgreSql);
    assert_eq!(home.held(), None, "the move's own hold is released");
    assert_eq!(home.switching().unwrap(), None);
    // Nothing is deleted: the old store, its export and its snapshot are kept; the work is gone.
    assert!(moved.aside.join("data").is_dir(), "{moved:?}");
    let mut export: Value =
        serde_json::from_slice(&std::fs::read(moved.aside.join(EXPORT)).unwrap()).unwrap();
    counted(&mut export["carried"]);
    assert_eq!(export["board"], json!(before.0));
    assert_eq!(export["carried"]["counter"], before.1["counter"]);
    // No lease stands in what the move wrote, and each revision is past its holder's.
    let written = ended(&before.1);
    assert_ne!(written, before.1);
    assert_eq!(
        export["carried"]["callback_lease"],
        written["callback_lease"]
    );
    let before = (before.0, written);
    assert!(moved.snapshot.as_ref().is_some_and(|kept| kept.is_dir()));
    assert!(!b.dir().join("data").exists() && !b.dir().join("moving").exists());

    let there = b.store();
    assert!(there.database().is_postgresql());
    assert_eq!(
        board(&there),
        before,
        "PostgreSQL holds what SurrealDB held"
    );
    let past = there.marker().unwrap();
    assert!(past > marker, "{past} against {marker}");
    assert_eq!(leases(&there), [(0.0, 8), (0.0, 4), (0.0, closing + 1)]);
    assert!(
        !there.renew_session(HANDLE, closing).unwrap(),
        "the close that held the session before the move no longer holds it"
    );
    // The home is bound to none, so the token is refused; and after a rebind it is another's.
    let rebound = b.store().recovering(authority());
    assert!(rebound.bound("sim/jump").is_err());
    rebound.rebind().unwrap();
    let old = rebound.amend_bound("sim/jump", &token, "Changed", &[], "owner");
    assert!(old.is_err(), "{old:?}");
    assert_eq!(there.get("sim/jump").unwrap().title, "A task");

    // A reader keeps the board's rows under its marker, on PostgreSQL's socket.
    let reader = b.store_as(&Opening::read());
    assert_eq!(reader.clock_board().unwrap().tasks.len(), 2);
    drop((there, rebound));

    let back = b.move_to(&home, Engine::SurrealDb).unwrap();
    assert_eq!(
        (back.from, back.to),
        (Engine::PostgreSql, Engine::SurrealDb)
    );
    assert!(back.aside.join("postgresql").join("PG_VERSION").is_file());
    assert!(back.snapshot.as_ref().is_some_and(|kept| kept.is_dir()));
    assert_eq!(home.backend().unwrap(), Engine::SurrealDb);
    assert!(!b.dir().join("postgresql").exists());
    let again = b.store();
    assert!(!again.database().is_postgresql());
    let before = (before.0, ended(&before.1));
    assert_eq!(board(&again), before, "and SurrealDB holds it again");
    assert_eq!(leases(&again), [(0.0, 9), (0.0, 5), (0.0, closing + 2)]);
    assert!(!again.renew_session(HANDLE, closing).unwrap());
    assert!(again.marker().unwrap() > past);
    again
        .create(&NewTask::new("sim/more", "Made on SurrealDB", "owner"))
        .unwrap();
    drop(again);

    // On PostgreSQL once more, where the reader kept it: the same socket, and other rows.
    b.move_to(&home, Engine::PostgreSql).unwrap();
    let last = b.store();
    assert!(last.marker().unwrap() > past);
    // Its connection ended with the server it reached, which is what drops a kept reader
    // (`reader_failed` in the Symphony's server); and one that reaches the board again where
    // it was finds a marker past the one it kept its rows under, so it reads them anew.
    let read = reader.clock_board().or_else(|_| reader.clock_board());
    assert_eq!(read.unwrap().tasks.len(), 3, "the reader read it anew");
    assert_eq!(b.moved().len(), 3, "each move keeps the store it replaced");
}

#[test]
fn a_move_is_refused_with_the_board_left_as_it_was() {
    let b = Board::new("move-refused");
    let home = b.home();
    let store = b.seeded();
    let before = board(&store);

    let same = refusal(b.move_to(&home, Engine::SurrealDb));
    assert!(
        same.contains("is on SurrealDB already: nothing was moved"),
        "{same}"
    );

    // Another's hold refuses it, and that hold stands.
    home.hold(OTHER, "a snapshot").unwrap();
    assert!(matches!(
        b.move_to(&home, Engine::PostgreSql),
        Err(ServerError::Held(hold)) if hold.by == OTHER
    ));
    home.release(OTHER, None).unwrap();

    assert_eq!(home.held(), None);
    assert_eq!(home.backend().unwrap(), Engine::SurrealDb);
    assert_eq!(board(&b.store()), before);
}

/// The names of the databases of the cluster `socket` serves, its own and templates left out.
fn databases(socket: &Path) -> Vec<String> {
    let answers = pg::Client::new(socket, "postgres")
        .simple(
            "SELECT datname FROM pg_database WHERE NOT datistemplate AND datname <> 'postgres' \
             ORDER BY 1",
        )
        .unwrap();
    (answers.iter().flat_map(|answer| &answer.rows))
        .filter_map(|row| row.first().cloned().flatten())
        .collect()
}

/// The databases of the SurrealDB namespace `sf` the server at `addr` holds.
fn namespace(addr: &str) -> Vec<String> {
    let info = rpc::Client::new(addr, "sf", "tasks")
        .query("INFO FOR NS;", json!({}))
        .unwrap();
    let mut names: Vec<String> = (info[0]["databases"].as_object())
        .map(|databases| databases.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_move_leaves_every_other_database_in_the_store_it_keeps_and_names_them() {
    let b = Board::new("move-beside");
    let home = b.home();
    let before = board(&b.seeded());
    // Two scratch boards in the board's SurrealDB store, as `just tasks --database NAME` makes
    // them, each with a row of its own.
    let addr = home.ensure().unwrap().addr;
    for (db, n) in [("beside", 1), ("second", 2)] {
        rpc::Client::new(&addr, "sf", db)
            .define_then(&format!("CREATE row SET n = {n};"), json!({}))
            .unwrap();
    }

    let moved = b.move_to(&home, Engine::PostgreSql).unwrap();
    assert_eq!(moved.left, ["sf/beside", "sf/second"], "{moved:?}");
    assert_eq!(home.backend().unwrap(), Engine::PostgreSql);
    assert_eq!(board(&b.store()), before, "the board moved as it was");
    let serving = home.postgresql(&programs()).ensure().unwrap();
    assert_eq!(
        databases(&serving.socket),
        ["tasks"],
        "only the board moved"
    );
    // The store it replaced is kept whole, the two beside the board's with their rows.
    let kept = Home::new(&moved.aside, b.scratch.surreal())
        .supervised(true)
        .snapshotting(false);
    let running = kept.ensure().unwrap();
    assert_eq!(namespace(&running.addr), ["beside", "second", "tasks"]);
    for (db, n) in [("beside", 1), ("second", 2)] {
        let rows = rpc::Client::new(&running.addr, "sf", db)
            .query("SELECT VALUE n FROM row;", json!({}))
            .unwrap();
        assert_eq!(rows, [json!([n])], "{db}");
    }
    kept.stop().unwrap();

    // The way back: a database beside the board's in its cluster stays in the cluster kept.
    pg::Client::new(&serving.socket, "postgres")
        .simple("CREATE DATABASE beside")
        .unwrap();
    let back = b.move_to(&home, Engine::SurrealDb).unwrap();
    assert_eq!(back.left, ["beside"], "{back:?}");
    assert_eq!(home.backend().unwrap(), Engine::SurrealDb);
    assert_eq!(board(&b.store()), before, "the board came back as it was");
    assert_eq!(
        namespace(&home.ensure().unwrap().addr),
        ["tasks"],
        "nothing beside the board came back"
    );
    // Marked as PostgreSQL's, as putting a kept store back by hand marks it, to be served.
    std::fs::write(
        back.aside.join("backend.json"),
        r#"{"format":1,"engine":"postgresql"}"#,
    )
    .unwrap();
    let cluster = Home::new(&back.aside, Path::new("/no/surreal")).postgresql(&programs());
    let socket = cluster.ensure().unwrap().socket;
    assert_eq!(
        databases(&socket),
        ["beside", "tasks"],
        "the cluster is kept whole"
    );
    cluster.stop().unwrap();
    assert!(!b.dir().join("moving").exists() && !b.dir().join("postgresql").exists());
}

#[test]
fn a_board_at_another_schema_is_not_moved() {
    let b = Board::new("move-schema");
    let home = b.home();
    let newer = Schema::this_build().then_each("DEFINE TABLE fixture_next;", "");
    b.store_as(&Opening::write().at(newer).migrating().leave_behind());
    let refused = refusal(b.move_to(&home, Engine::PostgreSql));
    assert!(refused.contains("a move migrates nothing"), "{refused}");
    assert!(refused.contains("and its hold is released"), "{refused}");
    assert_eq!(home.backend().unwrap(), Engine::SurrealDb);
    assert!(b.dir().join("data").is_dir() && !b.dir().join("moving").exists());
    assert_eq!(home.held(), None);

    // The caller's own hold is left standing by a move that is refused.
    home.hold(HANDLE, "a move").unwrap();
    let held = refusal(b.move_to(&home, Engine::PostgreSql));
    assert!(held.contains("as it was, and still held"), "{held}");
    assert_eq!(home.held().map(|hold| hold.by), Some(HANDLE.into()));
    home.release(HANDLE, None).unwrap();
}

/// A switch that a test cut is refused to everything but its holder's next move.
fn stands(b: &Board, home: &Home) {
    let unsettled = "until its holder settles it";
    for refused in [
        refusal(home.backend()),
        refusal(home.ensure()),
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
        home.move_to(Engine::PostgreSql, &programs(), OTHER, NOW),
        Err(ServerError::Held(hold)) if hold.by == HANDLE
    ));
    assert_eq!(home.held().map(|hold| hold.by), Some(HANDLE.into()));
    assert!(b.dir().join("move.json").is_file());
}

fn cut_at_each_step(name: &str, from: Engine, to: Engine) {
    let commit = STEPS.iter().position(|step| *step == "committed").unwrap();
    let b = Board::new(name);
    let home = b.home();
    let store = b.seeded();
    drop(store);
    if from == Engine::PostgreSql {
        b.move_to(&home, from).unwrap();
    }
    let before = board(&b.store());
    for (at, step) in STEPS.iter().enumerate() {
        let cut = refusal(b.move_to(&home.clone().stopping_after(step), to));
        assert!(
            cut.contains(&format!("ended the switch after {step:?}")),
            "{cut}"
        );
        stands(&b, &home);
        let journal = home.switching().unwrap().unwrap();
        assert_eq!(
            (journal.kind, journal.committed),
            (Kind::Move, at >= commit),
            "{step}"
        );
        // Cut again while it is settled, it is settled by the call after.
        let settled = b.move_to(&home, to);
        if at < commit {
            let undone = refusal(settled);
            assert!(undone.contains("is now undone"), "{step}: {undone}");
            assert!(
                undone.contains(&format!("is on {from} as it was")),
                "{undone}"
            );
            assert_eq!(home.backend().unwrap(), from, "{step}");
            assert!(!b.dir().join(to.data_folder()).exists(), "{step}");
        } else {
            let finished = settled.unwrap();
            assert!(finished.finished && !finished.held, "{step}: {finished:?}");
            assert_eq!(home.backend().unwrap(), to, "{step}");
            assert!(finished.aside.join(from.data_folder()).exists(), "{step}");
        }
        assert_eq!(home.switching().unwrap(), None, "{step}");
        assert_eq!(home.held(), None, "{step}: the move's own hold is released");
        assert!(!b.dir().join("moving").exists(), "{step}");
        assert_eq!(
            board(&b.store()),
            before,
            "{step}: the board reads as it did"
        );
        if at >= commit {
            // Settled again, there is nothing to settle; then back for the next step.
            let already = refusal(b.move_to(&home, to));
            assert!(already.contains("already: nothing was moved"), "{already}");
            b.move_to(&home, from).unwrap();
        }
    }
}

#[test]
fn a_move_to_postgresql_cut_after_each_step_is_undone_before_its_commit_and_finished_after() {
    cut_at_each_step("move-cut-pg", Engine::SurrealDb, Engine::PostgreSql);
}

#[test]
fn a_move_to_surrealdb_cut_after_each_step_is_undone_before_its_commit_and_finished_after() {
    cut_at_each_step("move-cut-surreal", Engine::PostgreSql, Engine::SurrealDb);
}

#[test]
fn a_step_that_fails_is_undone_at_once_or_leaves_the_hold_and_is_finished_after_the_commit() {
    let b = Board::new("move-fails");
    let home = b.home();
    let before = board(&b.seeded());
    let to = Engine::PostgreSql;
    for step in ["journal", "aside", "placed", "identity"] {
        let failed = refusal(b.move_to(&home.clone().failing_after(step), to));
        assert!(
            failed.contains("the board was not moved"),
            "{step}: {failed}"
        );
        assert!(
            failed.contains("on SurrealDB as it was, and its hold is released"),
            "{failed}"
        );
        assert_eq!(home.switching().unwrap(), None, "{step}");
        assert_eq!(home.held(), None, "{step}");
        assert_eq!(home.backend().unwrap(), Engine::SurrealDb, "{step}");
        assert!(!b.dir().join("moving").exists() && !b.dir().join("postgresql").exists());
        assert_eq!(board(&b.store()), before, "{step}");
    }
    // An undo that fails too never releases: the journal and the hold stand, and nothing is lost.
    let stuck = refusal(b.move_to(&home.clone().stuck_after("placed"), to));
    assert!(stuck.contains("could not be undone"), "{stuck}");
    assert!(
        stuck.contains("the board stays held and its journal stands"),
        "{stuck}"
    );
    stands(&b, &home);
    assert!(refusal(b.move_to(&home, to)).contains("is now undone"));
    assert_eq!(board(&b.store()), before);

    // Past the commit a failure is said as a move that committed, and the next call finishes it.
    for step in ["committed", "revoked"] {
        let pending = refusal(b.move_to(&home.clone().failing_after(step), to));
        assert!(
            pending.contains("the move committed: the board is on PostgreSQL"),
            "{pending}"
        );
        assert!(
            pending.contains("the board stays held and its journal stands"),
            "{pending}"
        );
        stands(&b, &home);
        assert!(b.move_to(&home, to).unwrap().finished);
        assert_eq!(board(&b.store()), before, "{step}");
        b.move_to(&home, Engine::SurrealDb).unwrap();
    }
    // The caller's own hold outlives a move that it settles and one that it makes.
    home.hold(HANDLE, "a move").unwrap();
    let moved = b.move_to(&home, to).unwrap();
    assert!(moved.held, "{moved:?}");
    assert_eq!(home.held().map(|hold| hold.by), Some(HANDLE.into()));
    home.release(HANDLE, None).unwrap();
    assert_eq!(board(&b.store()), before);
}

#[test]
fn a_snapshot_is_put_back_over_its_board_and_the_cluster_it_replaced_is_kept() {
    let b = Board::new("restore-over");
    let home = b.home();
    drop(b.seeded());
    b.move_to(&home, Engine::PostgreSql).unwrap();
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
}

/// A snapshot put back brings no lease revision back with it: a holder that took its lease
/// after the snapshot is past the snapshot's, and the restored lease is past the holder's.
#[test]
fn a_restored_snapshots_leases_are_ended_past_the_ones_the_board_held_since() {
    let b = Board::new("restore-leases");
    let home = b.home();
    drop(b.seeded());
    b.move_to(&home, Engine::PostgreSql).unwrap();
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
    let home = b.home();
    drop(b.seeded());
    b.move_to(&home, Engine::PostgreSql).unwrap();
    // As a board outside a scratch place: no lifeline to the process that starts it.
    let apart = b
        .scratch
        .home()
        .telling(|_| {})
        .supervised(false)
        .snapshotting(false);
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
fn postgresql_makes_nothing_for_a_board_outside_a_scratch_place_that_names_no_engine() {
    let b = Board::new("pg-unnamed");
    assert!(
        !symphony_board::server::scratch(b.dir()),
        "the target's folder is no scratch place"
    );
    let home = b.scratch.home().telling(|_| {});
    let pg = home.postgresql(&programs());
    for refused in [
        pg.ensure().map(|_| ()),
        pg.hold(HANDLE, "a snapshot").map(|_| ()),
        pg.snapshot(NOW).map(|_| ()),
        pg.reclaim("tasks"),
        pg.restore(&b.dir().join("nowhere")),
        pg.restore_over(&b.dir().join("nowhere"), HANDLE, NOW)
            .map(|_| ()),
    ] {
        let refused = refusal(refused);
        assert!(
            refused.contains("names no engine and is not in a scratch place"),
            "{refused}"
        );
    }
    // Nor is it made PostgreSQL in place: only a move of its store writes that identity.
    let made = refusal(home.initialize_backend(Engine::PostgreSql));
    assert!(made.contains("scratch"), "{made}");
    assert!(!b.dir().join("postgresql").exists() && !b.dir().join("backend.json").exists());
    assert_eq!(home.held(), None);
}
