//! Each sender's last word (`said`) on the generated board, as its event keeps it where the board
//! is biggest: a sender's newest word deleted gives way to its word before it, however far back,
//! and every message deleted with the last words still named (the copy of a store of schema 12,
//! which does not empty `said` first) looks back at each sender's word as it goes. Each within a
//! bound stated once, at the end.
//!
//! The look back walked `message_sid` from the newest message down until 2026-11-07: a delete of
//! a rare sender's newest word, its other word the board's first, did not answer in 600 s on a
//! board just copied. In an event, SurrealKV walks a range backward over the rows it still holds in
//! memory (every row its server took since it started, up to 4 GiB) at a cost out of all
//! proportion: that walk over 50k messages ran past 150 s in an event, where it took 4.6 s as a
//! statement of its own, 0.9 s forward in the event and 1.8 s backward once a restart had written
//! the rows to disk. The look back now reads the last key of two ranges of `message_sender`.

use crate::board::board;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use symphony_board::rpc::{Client, DbError};

/// A sender the generated board has no message of.
const RARE: &str = "perf-rare-sender";

/// An id past every one the board gives out, so the word made with it is its sender's newest.
const PAST_EVERY_ID: i64 = 1_000_000_000;

/// A message of [`RARE`] with id `sid` on a thread of its own, `kind` a post or a verdict.
fn word(client: &Client, slug: &str, sid: i64, kind: &str) {
    (client.query(
        "CREATE type::record('message', [$slug, $sid]) SET sid = $sid, slug = $slug, sender = $sender,
            recipient = NONE, kind = $kind, text = 'a rare word', ts = 0.0 RETURN NONE;",
        json!({"slug": slug, "sid": sid, "sender": RARE, "kind": kind}),
    ))
    .unwrap_or_else(|why| panic!("{RARE}'s word {sid} on {slug}: {why:?}"));
}

/// The word `said` names for [`RARE`], as `[sid, slug]`, or none.
fn rare_word(client: &Client) -> Value {
    let mut answers = (client.query(
        "RETURN (SELECT VALUE [sid, slug] FROM ONLY type::record('said', [$sender]));",
        json!({"sender": RARE}),
    ))
    .unwrap();
    answers.pop().unwrap_or(Value::Null)
}

/// How long the delete of message `[slug, sid]` took, its event's look back with it.
fn deleted(client: &Client, slug: &str, sid: i64) -> Duration {
    let began = Instant::now();
    (client.query(
        "DELETE type::record('message', [$slug, $sid]) RETURN NONE;",
        json!({"slug": slug, "sid": sid}),
    ))
    .unwrap_or_else(|why| panic!("the delete of {RARE}'s word {sid} on {slug}: {why:?}"));
    began.elapsed()
}

/// How many rows `table` holds.
fn rows(client: &Client, table: &str) -> Value {
    let mut answers = (client.query(
        &format!("RETURN count(SELECT VALUE id FROM {table});"),
        json!({}),
    ))
    .unwrap();
    answers.pop().unwrap_or(Value::Null)
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
    let newest_took = deleted(&client, newest, PAST_EVERY_ID);
    assert_eq!(rare_word(&client), json!([1, first]), "after its newest");
    let last_took = deleted(&client, first, 1);
    assert_eq!(rare_word(&client), Value::Null, "after its last");
    eprintln!(
        "perf: a rare sender's newest word deleted in {newest_took:.3?}, its last in {last_took:.3?}, bound {LOOK_BACK:.0?}"
    );
    let over: Vec<String> = [("newest", newest_took), ("last", last_took)]
        .iter()
        .filter(|(_, took)| *took > LOOK_BACK)
        .map(|(which, took)| format!("its {which} word deleted in {took:.3?}"))
        .collect();
    assert!(over.is_empty(), "over {LOOK_BACK:.0?}: {over:?}");
}

/// Every message deleted in one transaction with `said` still naming each sender's word, as a
/// store of schema 12 copying onto the board deletes them: each sender's word, deleted, gives way to
/// its next, until none is left. Thrown away at its end, which leaves the board as it was.
#[test]
fn every_message_deleted_with_the_last_words_named_looks_back_within_a_bound() {
    let board = board();
    let client = board.client();
    let (messages, words) = (rows(&client, "message"), rows(&client, "said"));
    let began = Instant::now();
    let answer = client.query(
        "BEGIN TRANSACTION; DELETE message RETURN NONE;
         THROW <string> [count(SELECT VALUE id FROM said), count(SELECT VALUE id FROM message)];
         COMMIT TRANSACTION;",
        json!({}),
    );
    let took = began.elapsed();
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
        "perf: every message deleted, the last words named, in {took:.3?}, bound {COPY_DELETE:.0?}"
    );
    assert!(
        took <= COPY_DELETE,
        "took {took:.3?}, over {COPY_DELETE:.0?}"
    );
}

/// The bounds, for a machine other agents' runs share. Measured on 2026-11-07 in two runs of this
/// suite at load 50 to 90: a rare sender's newest word deleted in 6.6 and 12.8 ms; every message
/// deleted with the last words named in 24.3 and 39.7 s (the slower right after the copy), where a
/// copy of the same board on a server of its own took 19.5 s, and 17.6 s with `said` emptied first,
/// as the copy now does. The delete's bound is a hundred times the slowest, the copy's three
/// times, and both far under what the old look back took: no answer in 600 s.
const LOOK_BACK: Duration = Duration::from_secs(2);
const COPY_DELETE: Duration = Duration::from_secs(120);
