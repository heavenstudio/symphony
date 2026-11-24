//! Each sender's last word (`said`) on the generated board, as its trigger keeps it where the board
//! is biggest (`store/postgres/schema.rs`'s `board_said`): a sender's newest word deleted gives
//! way to its word before it, however far back, and every message deleted with the last words
//! still named looks back at each sender's word as it goes, each a read of the sender's newest
//! remaining word. Each within a bound of the CPU it costs the board's server ([`crate::cpu`]),
//! from quiet to quiet again, stated once, at the end. The trigger's read is planned on an index
//! by the plan check (`plans::every_statement_the_triggers_run_is_planned_on_an_index`).
//!
//! The look back walked `message_sid` from the newest message down until 2026-11-07, on
//! SurrealDB: a delete of a rare sender's newest word, its other word the board's first, did not
//! answer in 600 s on a board just copied. The look back reads the last key of the sender's range
//! of `message_sender`.

use crate::board::{Board, board};
use crate::cpu;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use symphony_board::db::DbError;
use symphony_board::pg::Step;
use symphony_board::{Database, Raw};

/// A sender the generated board has no message of.
const RARE: &str = "perf-rare-sender";

/// An id past every one the board gives out, so the word made with it is its sender's newest.
const PAST_EVERY_ID: i64 = 1_000_000_000;

/// A message of [`RARE`] with id `sid` on a thread of its own, `kind` a post or a verdict.
fn word(db: &Database, slug: &str, sid: i64, kind: &str) {
    (Raw::of(db).row("message", json!([slug, sid])))
        .create(json!({
            "sid": sid, "slug": slug, "sender": RARE, "recipient": null, "kind": kind,
            "text": "a rare word", "ts": 0.0,
        }))
        .unwrap_or_else(|why| panic!("{RARE}'s word {sid} on {slug}: {why:?}"));
}

/// The word `said` names for [`RARE`], as `[sid, slug]`, or none.
fn rare_word(db: &Database) -> Value {
    let word = Raw::of(db).row("said", json!([RARE]));
    let word = word.fields(&["sid", "slug"]).unwrap();
    if word.is_null() {
        return Value::Null;
    }
    json!([word["sid"], word["slug"]])
}

/// The CPU the board's server took for the delete of message `[slug, sid]`, its trigger's look
/// back with it, which is all of its work, from quiet to quiet again ([`Board::bill`]), and the
/// delete's wall. A delete of a few milliseconds may read none on a meter of 10 ms ticks (Linux's),
/// so the board's copy, seconds of the server's work, is what shows its meter reads the server
/// (`Board::make`).
fn deleted(board: &Board, slug: &str, sid: i64) -> (Duration, Duration) {
    let (wall, cpu) = board.bill(|| {
        let began = Instant::now();
        (Raw::of(&board.database())
            .row("message", json!([slug, sid]))
            .delete())
        .unwrap_or_else(|why| panic!("the delete of {RARE}'s word {sid} on {slug}: {why:?}"));
        began.elapsed()
    });
    (cpu, wall)
}

/// How many rows `table` holds.
fn rows(db: &Database, table: &str) -> Value {
    json!(Raw::of(db).table(table).count().unwrap())
}

/// A rare sender's newest word deleted gives way to its only other word, a verdict with the
/// board's first id, past 50k messages of others; that one deleted, the sender has none.
#[test]
fn a_rare_senders_newest_word_deleted_gives_way_to_its_first_within_a_bound() {
    let board = board();
    let db = board.database();
    let (first, newest) = ("perf/rare-first", "perf/rare-newest");
    word(&db, first, 1, "verdict");
    word(&db, newest, PAST_EVERY_ID, "msg");
    assert_eq!(rare_word(&db), json!([PAST_EVERY_ID, newest]));
    let (newest_cpu, newest_wall) = deleted(&board, newest, PAST_EVERY_ID);
    assert_eq!(rare_word(&db), json!([1, first]), "after its newest");
    let (last_cpu, last_wall) = deleted(&board, first, 1);
    assert_eq!(rare_word(&db), Value::Null, "after its last");
    eprintln!(
        "perf: a rare sender's newest word deleted in CPU {newest_cpu:.3?} (wall {newest_wall:.3?}), its last in {last_cpu:.3?} ({last_wall:.3?}), bound {LOOK_BACK:.1?}"
    );
    let over: Vec<String> = [("newest", newest_cpu), ("last", last_cpu)]
        .iter()
        .filter(|(_, cpu)| *cpu > LOOK_BACK)
        .map(|(which, cpu)| format!("its {which} word deleted in CPU {cpu:.3?}"))
        .collect();
    assert!(over.is_empty(), "over {LOOK_BACK:.1?}: {over:?}");
}

/// Every message deleted in one transaction with `said` still naming each sender's word: each
/// sender's word, deleted, gives way to its next, until none is left. Thrown away at its end, its
/// last statement raising what is left, which rolls it back and leaves the board as it was.
#[test]
fn every_message_deleted_with_the_last_words_named_looks_back_within_a_bound() {
    let board = board();
    let db = board.database();
    let (messages, words) = (rows(&db, "message"), rows(&db, "said"));
    let client = board.client();
    let ((answer, wall), cpu) = board.bill(|| {
        let began = Instant::now();
        let answer = client.request(&[
            Step::bare("DELETE FROM message"),
            Step::bare(
                "DO $perf$ BEGIN RAISE EXCEPTION '%', jsonb_build_array( \
                 (SELECT count(*) FROM said), (SELECT count(*) FROM message))::text; END $perf$",
            ),
        ]);
        (answer, began.elapsed())
    });
    cpu::worked("every message deleted", cpu).unwrap_or_else(|why| panic!("{why}"));
    assert!(
        matches!(&answer, Err(DbError::Thrown(left)) if left == "[0, 0]"),
        "no word and no message left at the end: {answer:?}"
    );
    assert_eq!(
        [rows(&db, "message"), rows(&db, "said")],
        [messages, words],
        "the board as it was"
    );
    eprintln!(
        "perf: every message deleted, the last words named, in CPU {cpu:.3?} (wall {wall:.3?}), bound {COPY_DELETE:.0?}"
    );
    assert!(
        cpu <= COPY_DELETE,
        "took CPU {cpu:.3?}, over {COPY_DELETE:.0?}"
    );
}

/// The bounds of the server's CPU, which the machine's load does not multiply as it multiplies a
/// wall, for a machine other agents' runs share. Measured on SurrealDB on 2026-11-12 in five runs
/// at load 62 to 222, one with a workspace build from scratch beside it: a rare sender's newest
/// word deleted in 1.9 to 7.6 ms of CPU (walls of 8 to 58 ms), its last in 2.0 to 4.7 ms (12 to
/// 42 ms); every message deleted with the last words named in 18.3 to 18.9 s, 25.1 s beside the
/// build (walls of 35 to 85 s, where a bound on the wall stood at 120 s). The delete's bound is
/// over 60 times the slowest, the bulk delete's over twice, and both far under what the look back
/// cost when it walked `message_sid`: no answer in 600 s, past 150 s in an event. Kept on
/// PostgreSQL, whose figures the run prints.
const LOOK_BACK: Duration = Duration::from_millis(500);
const COPY_DELETE: Duration = Duration::from_secs(60);
