//! Each sender's last word (`said`) on the generated board, as its event keeps it where the board
//! is biggest: a sender's newest word deleted gives way to its word before it, however far back,
//! and every message deleted with the last words still named (the copy of a store of schema 12,
//! which does not empty `said` first) looks back at each sender's word as it goes. Each within a
//! bound of the CPU it costs the board's server ([`crate::cpu`]), from quiet to quiet again,
//! stated once, at the end.
//!
//! The look back walked `message_sid` from the newest message down until 2026-11-07: a delete of
//! a rare sender's newest word, its other word the board's first, did not answer in 600 s on a
//! board just copied. In an event, SurrealKV walks a range backward over the rows it still holds in
//! memory (every row its server took since it started, up to 4 GiB) at a cost out of all
//! proportion: that walk over 50k messages ran past 150 s in an event, where it took 4.6 s as a
//! statement of its own, 0.9 s forward in the event and 1.8 s backward once a restart had written
//! the rows to disk. The look back now reads the last key of two ranges of `message_sender`.

use crate::board::{Board, board};
use crate::cpu;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use symphony_board::Raw;
use symphony_board::db::DbError;
use symphony_board::rpc::Client;

/// A sender the generated board has no message of.
const RARE: &str = "perf-rare-sender";

/// An id past every one the board gives out, so the word made with it is its sender's newest.
const PAST_EVERY_ID: i64 = 1_000_000_000;

/// A message of [`RARE`] with id `sid` on a thread of its own, `kind` a post or a verdict.
fn word(client: &Client, slug: &str, sid: i64, kind: &str) {
    (Raw::on(client).row("message", json!([slug, sid])))
        .create(json!({
            "sid": sid, "slug": slug, "sender": RARE, "recipient": null, "kind": kind,
            "text": "a rare word", "ts": 0.0,
        }))
        .unwrap_or_else(|why| panic!("{RARE}'s word {sid} on {slug}: {why:?}"));
}

/// The word `said` names for [`RARE`], as `[sid, slug]`, or none.
fn rare_word(client: &Client) -> Value {
    let word = Raw::on(client).row("said", json!([RARE]));
    let word = word.fields(&["sid", "slug"]).unwrap();
    if word.is_null() {
        return Value::Null;
    }
    json!([word["sid"], word["slug"]])
}

/// The CPU the board's server took for the delete of message `[slug, sid]`, its event's look back
/// with it, which is all of its work, from quiet to quiet again ([`Board::bill`]), and the
/// delete's wall. A delete of a few milliseconds may read none on a meter of 10 ms ticks (Linux's),
/// so the board's copy, seconds of the server's work, is what shows its meter reads the server
/// (`Board::make`).
fn deleted(board: &Board, slug: &str, sid: i64) -> (Duration, Duration) {
    let (wall, cpu) = board.bill(|| {
        let began = Instant::now();
        (Raw::on(&board.client())
            .row("message", json!([slug, sid]))
            .delete())
        .unwrap_or_else(|why| panic!("the delete of {RARE}'s word {sid} on {slug}: {why:?}"));
        began.elapsed()
    });
    (cpu, wall)
}

/// How many rows `table` holds.
fn rows(client: &Client, table: &str) -> Value {
    json!(Raw::on(client).table(table).count().unwrap())
}

/// A rare sender's newest word deleted gives way to its only other word, a verdict with the
/// board's first id, past 50k messages of others; that one deleted, the sender has none.
#[test]
fn a_rare_senders_newest_word_deleted_gives_way_to_its_first_within_a_bound() {
    let board = board();
    let client = board.client();
    let (first, newest) = ("perf/rare-first", "perf/rare-newest");
    word(&client, first, 1, "verdict");
    word(&client, newest, PAST_EVERY_ID, "msg");
    assert_eq!(rare_word(&client), json!([PAST_EVERY_ID, newest]));
    let (newest_cpu, newest_wall) = deleted(&board, newest, PAST_EVERY_ID);
    assert_eq!(rare_word(&client), json!([1, first]), "after its newest");
    let (last_cpu, last_wall) = deleted(&board, first, 1);
    assert_eq!(rare_word(&client), Value::Null, "after its last");
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

/// Every message deleted in one transaction with `said` still naming each sender's word, as a
/// store of schema 12 copying onto the board deletes them: each sender's word, deleted, gives way to
/// its next, until none is left. Thrown away at its end, which leaves the board as it was.
#[test]
fn every_message_deleted_with_the_last_words_named_looks_back_within_a_bound() {
    let board = board();
    let client = board.client();
    let (messages, words) = (rows(&client, "message"), rows(&client, "said"));
    let ((answer, wall), cpu) = board.bill(|| {
        let began = Instant::now();
        let answer = client.query(
            "BEGIN TRANSACTION; DELETE message RETURN NONE;
             THROW <string> [count(SELECT VALUE id FROM said), count(SELECT VALUE id FROM message)];
             COMMIT TRANSACTION;",
            json!({}),
        );
        (answer, began.elapsed())
    });
    cpu::worked("every message deleted", cpu).unwrap_or_else(|why| panic!("{why}"));
    assert!(
        matches!(&answer, Err(DbError::Thrown(left)) if left == "[0, 0]"),
        "no word and no message left at the end: {answer:?}"
    );
    assert_eq!(
        [rows(&client, "message"), rows(&client, "said")],
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
/// wall, for a machine other agents' runs share. Measured on 2026-11-12 in five runs at load 62
/// to 222, one with a workspace build from scratch beside it: a rare sender's newest word deleted
/// in 1.9 to 7.6 ms of CPU (walls of 8 to 58 ms), its last in 2.0 to 4.7 ms (12 to 42 ms); every
/// message deleted with the last words named in 18.3 to 18.9 s, 25.1 s beside the build (walls of
/// 35 to 85 s, where a bound on the wall stood at 120 s). The delete's bound is over 60 times the
/// slowest, the copy's over twice, and both far under what the look back cost when it walked
/// `message_sid`: no answer in 600 s, past 150 s in an event.
const LOOK_BACK: Duration = Duration::from_millis(500);
const COPY_DELETE: Duration = Duration::from_secs(60);
