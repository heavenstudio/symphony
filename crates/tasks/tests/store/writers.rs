//! Who writes a board: each opening to write keeps its program's line (its name, where it is, its
//! host, the schema it was built at, when it was last seen by this machine's clock), read in the
//! opening's one read and written again only once it names another schema or is an hour old; a
//! reader keeps none, and nor does a build from before the record. A migrate that would leave behind
//! a writer seen in the last week names each and moves nothing unless told to, and once moved past
//! one, that writer is refused naming itself as what to rebuild.
//!
//! A line's time is the machine's, never the store's pinned clock, so these tests age a line by
//! moving its `seen` back by hand.

use crate::common::{HOST, NOW, database, refused};
use serde_json::json;
use symphony_board::Database;
use symphony_board::sweep::trigger::now;
use symphony_board::{Error, NewTask, Opening, Program, Raw, Schema, Store, Writer};

const HOUR: f64 = 3600.0;
const DAY: f64 = 24.0 * HOUR;

/// The store `how` opens on a new client of `client`'s database, on `host`.
fn opened(client: &Database, how: &Opening, host: &str) -> Result<Store, Error> {
    Store::opening_on(
        client.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        host,
        how,
    )
}

/// The board's consuming gate, a program of the project's own that writes it.
fn gate() -> Program {
    Program::named("gate-review", "/work/alpha/.target/debug/gate-review")
}

/// Every writer's line, read through a store that keeps none.
fn lines(client: &Database) -> Vec<Writer> {
    let mut lines = Store::unchecked_on(
        client.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::read(),
    )
    .writers()
    .unwrap();
    lines.sort_by(|a, b| (&a.program, &a.host).cmp(&(&b.program, &b.host)));
    lines
}

/// `program`'s line on `host`, which the test expects there.
fn line(client: &Database, program: &str, host: &str) -> Writer {
    lines(client)
        .into_iter()
        .find(|w| w.program == program && w.host == host)
        .unwrap_or_else(|| panic!("no line of {program} on {host}"))
}

/// `program`'s line on `host` made to have been seen `by` seconds earlier than it was.
fn age(client: &Database, program: &str, host: &str, by: f64) {
    Raw::of(client)
        .table("writer")
        .matching(json!({"program": program, "host": host}))
        .add("seen", -by)
        .unwrap();
}

/// The version `meta:schema` records.
fn version(client: &Database) -> Option<i64> {
    let found = Raw::of(client).row("meta", "schema").field("version");
    found.unwrap().as_i64()
}

fn this() -> i64 {
    Schema::this_build().version()
}

/// A newer build's schema: this one's, then a table of its own.
fn newer() -> Schema {
    Schema::this_build()
        .then_each("CREATE TABLE IF NOT EXISTS fixture_next (id jsonb PRIMARY KEY, doc jsonb);")
}

/// The tables the board's database holds.
fn tables(client: &Database) -> Vec<String> {
    let raw = client.postgresql();
    let listed = raw
        .request(&[symphony_board::pg::Step::bare(
            "SELECT coalesce(jsonb_agg(table_name::text ORDER BY table_name), '[]')::text \
             FROM information_schema.tables WHERE table_schema = current_schema()",
        )])
        .unwrap();
    serde_json::from_str(listed[0].value().unwrap()).unwrap()
}

#[test]
fn a_writer_is_recorded_as_it_opens_and_its_last_seen_moves_once_an_hour_has_gone() {
    let client = database();
    let start = now();
    opened(&client, &Opening::write().by(gate()), HOST).unwrap();
    let first = line(&client, "gate-review", HOST);
    assert_eq!(
        lines(&client),
        [Writer {
            program: "gate-review".into(),
            path: "/work/alpha/.target/debug/gate-review".into(),
            host: HOST.into(),
            schema: this(),
            seen: first.seen,
        }],
        "made with the board"
    );
    assert!(
        (start..=now()).contains(&first.seen),
        "seen by the machine's clock, not the store's: {}",
        first.seen
    );
    // Within the hour an opening reads its line in its one read, and writes nothing.
    age(&client, "gate-review", HOST, HOUR - 600.0);
    let soon = opened(&client, &Opening::write().by(gate()), HOST).unwrap();
    assert_eq!(soon.database().requests(), 1);
    let kept = line(&client, "gate-review", HOST);
    assert_eq!(kept.seen, first.seen - (HOUR - 600.0), "not written again");
    // An hour on, the line is written again: its last seen moves.
    age(&client, "gate-review", HOST, 600.0);
    let again = now();
    let later = opened(&client, &Opening::write().by(gate()), HOST).unwrap();
    assert_eq!(later.database().requests(), 2, "its read, then its line");
    let moved = line(&client, "gate-review", HOST);
    assert!(moved.seen >= again, "{} moved to {}", kept.seen, moved.seen);
    assert_eq!(
        moved,
        Writer {
            seen: moved.seen,
            ..first.clone()
        }
    );
    // The same program on another host is a writer of its own.
    opened(&client, &Opening::write().by(gate()), "studio").unwrap();
    let studio = line(&client, "gate-review", "studio");
    assert_eq!(studio.path, first.path);
    assert_eq!(lines(&client), [studio.clone(), moved.clone()]);
    // A reader writes no line.
    let reader = Program::named("symphony", "/work/symphony");
    opened(&client, &Opening::read().by(reader), HOST).unwrap();
    assert_eq!(lines(&client), [studio, moved]);
}

/// A build from before the record, which arrived with schema 18 (`store/writers.rs`'s `SINCE`):
/// this build's tables, the writers' among them, at the version below it.
fn before_the_record() -> Schema {
    let mut older = Schema::this_build();
    while older.version() >= 18 {
        older = older.before();
    }
    older
}

#[test]
fn a_build_from_before_the_record_writes_no_line_and_the_build_that_moves_its_board_does() {
    let client = database();
    let older = Program::named("tasks", "/opt/old/tasks");
    opened(
        &client,
        &Opening::write().at(before_the_record()).by(older),
        HOST,
    )
    .unwrap()
    .create(&NewTask::new("old/one", "Made before the record", "owner"))
    .unwrap();
    assert_eq!(lines(&client), []);
    opened(&client, &Opening::write().by(gate()), HOST).unwrap();
    assert_eq!(version(&client), Some(this()));
    let recorded: Vec<(String, i64)> = lines(&client)
        .into_iter()
        .map(|w| (w.program, w.schema))
        .collect();
    assert_eq!(recorded, [("gate-review".to_string(), this())]);
}

/// A board at this build, its gate's line last seen `ago` before now, and the line of the test's own
/// program, which the migrates below are, seen now.
fn board_with_gate(ago: f64) -> Database {
    let client = database();
    opened(&client, &Opening::write().by(gate()), HOST).unwrap();
    age(&client, "gate-review", HOST, ago);
    opened(&client, &Opening::write(), HOST)
        .unwrap()
        .create(&NewTask::new("old/one", "Made at this version", "owner"))
        .unwrap();
    client
}

#[test]
fn a_migrate_names_a_writer_seen_in_the_last_week_behind_it_and_leaves_the_board_unmoved() {
    let client = board_with_gate(3.0 * HOUR);
    let migrate = Opening::write().at(newer()).migrating().of(Some("alpha"));
    let why = refused(opened(&client, &migrate, HOST).map(drop));
    assert_eq!(
        why,
        format!(
            "the alpha board has a writer seen in the last week behind schema {}, the one this \
             migrate moves it to, and each would be refused the board from then on: gate-review \
             at schema {} (/work/alpha/.target/debug/gate-review on {HOST}, last seen 3 h ago). \
             Nothing was moved: rebuild each at schema {} and run it once, which records it, then \
             `just tasks migrate` again; or `just tasks migrate --leave-behind` moves it now and \
             leaves them refused",
            this() + 1,
            this(),
            this() + 1
        )
    );
    assert_eq!(version(&client), Some(this()), "the board did not move");
    let held = tables(&client);
    assert!(
        !held.contains(&"fixture_next".to_string()),
        "nothing of the newer schema was defined: {held:?}"
    );

    // Told to, it moves the board, and the gate left behind is refused, named as what to rebuild.
    opened(&client, &migrate.clone().leave_behind(), HOST).unwrap();
    assert_eq!(version(&client), Some(this() + 1));
    let before = line(&client, "gate-review", HOST);
    let why = refused(
        opened(
            &client,
            &Opening::write().by(gate()).of(Some("alpha")),
            HOST,
        )
        .map(drop),
    );
    assert_eq!(
        why,
        format!(
            "the alpha board is at schema {}, and this build writes only a board at its own \
             schema, {}: nothing was written; rebuild gate-review \
             (/work/alpha/.target/debug/gate-review) at the board's schema, and `just symphony` \
             in alpha brings its Symphony to it",
            this() + 1,
            this()
        )
    );
    assert_eq!(
        line(&client, "gate-review", HOST),
        before,
        "refused before any write, its line three hours old and all"
    );
}

#[test]
fn a_migrate_counts_no_writer_unseen_for_a_week_at_its_schema_already_or_itself() {
    // Not seen in the last week.
    let unseen = board_with_gate(7.0 * DAY + 1.0);
    let migrate = Opening::write().at(newer()).migrating();
    opened(&unseen, &migrate, HOST).unwrap();
    assert_eq!(version(&unseen), Some(this() + 1));

    // The migrate's own line, written before it was rebuilt: the migrate is the gate itself.
    let itself = database();
    opened(&itself, &Opening::write().by(gate()), HOST).unwrap();
    age(&itself, "gate-review", HOST, HOUR);
    let rebuilt = Opening::write().at(newer()).by(gate()).migrating();
    let start = now();
    opened(&itself, &rebuilt, HOST).unwrap();
    assert_eq!(version(&itself), Some(this() + 1));
    let moved = line(&itself, "gate-review", HOST);
    assert_eq!(
        (lines(&itself).len(), moved.schema),
        (1, this() + 1),
        "its line moved with it"
    );
    assert!(moved.seen >= start, "seen as it moved the board");

    // A writer already at the schema the migrate moves to is not behind it.
    let already = database();
    opened(&already, &Opening::write().by(gate()), HOST).unwrap();
    Raw::of(&already)
        .table("writer")
        .set(json!({"schema": this() + 1}))
        .unwrap();
    opened(&already, &migrate, HOST).unwrap();
    assert_eq!(version(&already), Some(this() + 1));

    // Two writers behind, on two hosts: both named, by program then host. A line two hours old
    // reads "2 h" for half an hour more, whatever a loaded machine takes between its age and the
    // migrate's read.
    let two = board_with_gate(2.0 * HOUR);
    opened(&two, &Opening::write().by(gate()), "studio").unwrap();
    age(&two, "gate-review", "studio", 2.0 * DAY);
    let why = refused(opened(&two, &migrate, HOST).map(drop));
    assert!(
        why.starts_with(&format!(
            "this board has 2 writers seen in the last week behind schema {}",
            this() + 1
        )),
        "{why}"
    );
    let (studio, here) = (
        why.find("on studio, last seen 48 h ago").unwrap(),
        why.find(&format!("on {HOST}, last seen 2 h ago")).unwrap(),
    );
    assert!(studio < here, "{why}");
    assert_eq!(version(&two), Some(this()));
}

/// A line a store cannot write leaves its opening as it was: the board opens all the same.
#[test]
fn an_opening_whose_line_is_refused_still_opens_its_board() {
    let client = board_with_gate(HOUR);
    // A hand at the server makes the table refuse every line from now on.
    let raw = client.postgresql();
    raw.simple("ALTER TABLE writer ADD CONSTRAINT never CHECK (false) NOT VALID")
        .unwrap();
    let store = opened(&client, &Opening::write().by(gate()), HOST).unwrap();
    assert_eq!(
        store.database().requests(),
        2,
        "its read, then its line, refused"
    );
    assert_eq!(store.get("old/one").unwrap().title, "Made at this version");
}
