//! Each sender's last word (`said`): what `last_messages` answers from it is what the query it
//! replaced answered from every message, after each write the schema takes: a post, a copy (equal
//! ids on two threads among its messages), a message deleted, or edited to another kind or sender,
//! every message deleted with the last words still named, as a copy by a store of schema 12 does;
//! and a board written before it was has it once opened.

use crate::common::{Fixture, NOW, Seed};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use symphony_board::Message;
use symphony_board::rows::MessageRow;

/// Each sender's last word as `last_messages` read it before `said` (976dbd5), from every message:
/// the greatest `[sid, slug]` of the sender's posts and verdicts.
fn read_from_every_message(board: &Fixture) -> BTreeMap<String, Message> {
    let read = "
LET $last = (SELECT sender, array::max(array::group([[sid, slug]])) AS last FROM message WHERE kind IN $delivered GROUP BY sender)
    .map(|$sender| $sender.last[0]);
RETURN SELECT * OMIT id FROM $last.map(|$m| type::record('message', [$m[1], $m[0]]));";
    let mut answers = (board.store.client())
        .query(read, json!({"delivered": ["msg", "verdict"]}))
        .unwrap();
    let rows: Vec<MessageRow> = serde_json::from_value(answers.pop().unwrap()).unwrap();
    rows.into_iter()
        .map(|row| {
            let word = Message {
                id: row.id,
                slug: row.slug,
                sender: row.sender,
                recipient: row.recipient,
                kind: row.kind,
                text: row.text,
                ts: row.ts,
            };
            (word.sender.clone(), word)
        })
        .collect()
}

/// `last_messages`, which reads `said`, and what it says of `sender`.
fn last_words(board: &Fixture) -> BTreeMap<String, Message> {
    board.store.last_messages().unwrap()
}

fn text_of(words: &BTreeMap<String, Message>, sender: &str) -> Option<String> {
    words.get(sender).map(|word| word.text.clone())
}

/// `last_messages` against the query it replaced, after `step`: equal, and naming `sender`'s word
/// as `text` (none: no word), so each step is the case it is meant to be.
fn agrees(board: &Fixture, step: &str, sender: &str, text: Option<&str>) {
    let read = last_words(board);
    let before = read_from_every_message(board);
    assert_eq!(read, before, "after {step}");
    assert_eq!(text_of(&read, sender).as_deref(), text, "after {step}");
}

/// `UPDATE` or `DELETE` of message `word`, as a hand at the server could: the board never edits a
/// message, and deletes them only all at once, in a copy.
fn write(board: &Fixture, sql: &str, word: &Message, other: &str) {
    (board.store.client())
        .query(
            sql,
            json!({"slug": word.slug, "sid": word.id, "other": other}),
        )
        .unwrap();
}

/// A message `sender` said on `slug` with id `sid`, made by its own request.
fn create(board: &Fixture, slug: &str, sid: i64, sender: &str, kind: &str) {
    (board.store.client())
        .query(
            "CREATE type::record('message', [$slug, $sid]) SET sid = $sid, slug = $slug, sender = $sender,
                recipient = NONE, kind = $kind, text = $slug + ' ' + $kind, ts = $now RETURN NONE;",
            json!({"slug": slug, "sid": sid, "sender": sender, "kind": kind, "now": NOW}),
        )
        .unwrap();
}

/// Two messages of one sender with the same id on two threads, which a copy takes: the word is
/// the one on the thread named last, as `[sid, slug]` orders them, whichever was made first.
#[test]
fn equal_ids_on_two_threads_name_the_later_thread_whichever_comes_first() {
    let orders = [["tools/a", "tools/z"], ["tools/z", "tools/a"]];
    for order in orders {
        let board = Fixture::empty();
        for slug in order {
            create(&board, slug, 7, "writer", "msg");
        }
        agrees(
            &board,
            &format!("posts on {order:?}"),
            "writer",
            Some("tools/z msg"),
        );
    }
    let mut seed = Seed::default()
        .session("writer")
        .task("tools/a", "ready")
        .task("tools/z", "ready");
    seed.dump.messages = ["tools/a", "tools/z"]
        .map(|slug| MessageRow {
            id: 7,
            slug: slug.into(),
            sender: "writer".into(),
            recipient: None,
            kind: "msg".into(),
            text: slug.into(),
            ts: NOW,
        })
        .into();
    let board = Fixture::new(seed);
    agrees(&board, "a copy", "writer", Some("tools/z"));
}

/// A deleted last word gives way to the greatest `[sid, slug]` before it, a post or a verdict:
/// of two at one id on two threads, the one on the thread named last, whichever kind it is.
#[test]
fn a_deleted_word_gives_way_to_the_later_thread_of_two_at_one_id() {
    for kinds in [["msg", "verdict"], ["verdict", "msg"]] {
        let board = Fixture::empty();
        create(&board, "tools/z", 7, "writer", kinds[0]);
        create(&board, "tools/a", 7, "writer", kinds[1]);
        create(&board, "tools/m", 9, "writer", "msg");
        agrees(&board, "the posts", "writer", Some("tools/m msg"));
        (board.store.client())
            .query("DELETE message:['tools/m', 9];", json!({}))
            .unwrap();
        let later = format!("tools/z {}", kinds[0]);
        agrees(
            &board,
            &format!("its newest deleted, {kinds:?} at one id"),
            "writer",
            Some(&later),
        );
    }
}

/// A copy by a store of schema 12 deletes every message with the last words still named, each
/// sender's word giving way to its next as it goes until none is left, then makes the copied
/// messages, which name theirs again.
#[test]
fn every_message_deleted_with_the_last_words_named_leaves_none_then_the_copied_ones() {
    let board = Fixture::empty();
    let (writer, other) = (board.session("worker"), board.session("worker"));
    board.task("sim/jump");
    board.task("sim/land");
    board.store.say("sim/jump", &writer, "first", None).unwrap();
    (board.store)
        .say("sim/land", &other, "from the other", None)
        .unwrap();
    board
        .store
        .say("sim/land", &writer, "second", None)
        .unwrap();
    let dump = board.store.dump().unwrap();
    board.store.say("sim/jump", &writer, "third", None).unwrap();
    agrees(&board, "the posts", &writer, Some("third"));
    (board.store.client())
        .query("DELETE message RETURN NONE;", json!({}))
        .unwrap();
    agrees(&board, "every message deleted", &writer, None);
    agrees(&board, "every message deleted", &other, None);
    // Each row keyed as the copy keys it: its record id `[slug, id]`, SQLite's id as `sid`.
    let rows: Vec<Value> = (dump.messages.iter())
        .map(|message| {
            let mut row = serde_json::to_value(message).unwrap();
            row["id"] = json!([message.slug, message.id]);
            row["sid"] = json!(message.id);
            row
        })
        .collect();
    (board.store.client())
        .query(
            "INSERT INTO message $rows RETURN NONE;",
            json!({"rows": rows}),
        )
        .unwrap();
    agrees(&board, "the copied messages made", &writer, Some("second"));
    agrees(
        &board,
        "the copied messages made",
        &other,
        Some("from the other"),
    );
}

/// A deleted last word gives the sender's word before it, on any thread; one deleted before it
/// changes nothing; a sender with none left has no word; a thread deleted whole leaves each
/// sender its newest word elsewhere.
#[test]
fn a_deleted_last_word_gives_the_word_before_it() {
    let board = Fixture::empty();
    let (writer, other) = (board.session("worker"), board.session("worker"));
    board.task("sim/jump");
    board.task("sim/land");
    let first = board.store.say("sim/jump", &writer, "first", None).unwrap();
    board
        .store
        .say("sim/jump", &other, "from the other", None)
        .unwrap();
    board
        .store
        .say("sim/land", &writer, "second", None)
        .unwrap();
    board
        .store
        .say("sim/land", &other, "the other again", None)
        .unwrap();
    let third = board.store.say("sim/jump", &writer, "third", None).unwrap();
    let delete = "DELETE type::record('message', [$slug, $sid]);";
    agrees(&board, "the posts", &writer, Some("third"));
    write(&board, delete, &third, "");
    agrees(&board, "its newest deleted", &writer, Some("second"));
    write(&board, delete, &first, "");
    agrees(&board, "an older one deleted", &writer, Some("second"));
    write(
        &board,
        "DELETE message WHERE slug = 'sim/land';",
        &first,
        "",
    );
    agrees(&board, "a thread deleted", &other, Some("from the other"));
    agrees(&board, "a thread deleted", &writer, None);
}

/// A last word made a record line gives the word before it, and a record line made a post is the
/// word when it is the newest; a post made a verdict stays the word.
#[test]
fn a_word_edited_to_another_kind_counts_as_that_kind() {
    let board = Fixture::empty();
    let writer = board.session("worker");
    board.task("sim/jump");
    board.store.say("sim/jump", &writer, "first", None).unwrap();
    let second = board
        .store
        .say("sim/jump", &writer, "second", None)
        .unwrap();
    write(
        &board,
        "UPDATE type::record('message', [$slug, $sid]) SET kind = 'system';",
        &second,
        "",
    );
    agrees(
        &board,
        "its newest made a record line",
        &writer,
        Some("first"),
    );
    create(&board, "sim/jump", second.id + 1, &writer, "system");
    agrees(&board, "a newer record line", &writer, Some("first"));
    write(
        &board,
        "UPDATE message SET kind = 'msg' WHERE sid = $sid + 1;",
        &second,
        "",
    );
    agrees(
        &board,
        "the record line made a post",
        &writer,
        Some("sim/jump system"),
    );
    write(
        &board,
        "UPDATE message SET kind = 'verdict' WHERE sid = $sid + 1;",
        &second,
        "",
    );
    agrees(
        &board,
        "the post made a verdict",
        &writer,
        Some("sim/jump system"),
    );
    write(
        &board,
        "UPDATE message SET text = 'said again' WHERE sid = $sid + 1;",
        &second,
        "",
    );
    agrees(
        &board,
        "the verdict's text edited",
        &writer,
        Some("said again"),
    );
}

/// A last word given to another sender is that sender's word if it is the newest of theirs, and
/// leaves its first sender the word before it.
#[test]
fn a_word_given_another_sender_moves_to_that_sender() {
    let board = Fixture::empty();
    let (writer, other) = (board.session("worker"), board.session("worker"));
    board.task("sim/jump");
    board.store.say("sim/jump", &writer, "first", None).unwrap();
    board
        .store
        .say("sim/jump", &other, "from the other", None)
        .unwrap();
    let second = board
        .store
        .say("sim/jump", &writer, "second", None)
        .unwrap();
    write(
        &board,
        "UPDATE type::record('message', [$slug, $sid]) SET sender = $other;",
        &second,
        &other,
    );
    agrees(
        &board,
        "its newest given to the other",
        &writer,
        Some("first"),
    );
    agrees(
        &board,
        "its newest given to the other",
        &other,
        Some("second"),
    );
    write(
        &board,
        "UPDATE type::record('message', [$slug, $sid]) SET sender = $other;",
        &second,
        &writer,
    );
    agrees(&board, "given back", &writer, Some("second"));
    agrees(&board, "given back", &other, Some("from the other"));
}

/// A board written before `said` was (its version older) has each sender's last word named once
/// the next store opens it, and only then: a board that names any keeps its names.
#[test]
fn a_board_from_before_the_last_words_were_kept_has_them_once_opened() {
    let board = Fixture::empty();
    let (writer, other) = (board.session("worker"), board.session("worker"));
    board.task("sim/jump");
    board.store.say("sim/jump", &writer, "first", None).unwrap();
    board
        .store
        .say("sim/jump", &other, "from the other", None)
        .unwrap();
    board
        .store
        .say("sim/jump", &writer, "second", None)
        .unwrap();
    agrees(&board, "the posts", &writer, Some("second"));
    let was = last_words(&board);
    (board.store.client())
        .query(
            "DELETE said; UPDATE meta:schema SET version = 12;",
            json!({}),
        )
        .unwrap();
    assert_eq!(
        last_words(&board),
        BTreeMap::new(),
        "the board as an older store left it"
    );
    assert_eq!(board.other().last_messages().unwrap(), was);
}

/// A copy over a board replaces every message, and the last words with them.
#[test]
fn a_copy_over_a_board_names_the_copied_senders_last_words() {
    let board = Fixture::empty();
    let (writer, other) = (board.session("worker"), board.session("worker"));
    board.task("sim/jump");
    board.store.say("sim/jump", &writer, "first", None).unwrap();
    let dump = board.store.dump().unwrap();
    board
        .store
        .say("sim/jump", &other, "from the other", None)
        .unwrap();
    board
        .store
        .say("sim/jump", &writer, "second", None)
        .unwrap();
    board.store.copy(&dump).unwrap();
    agrees(&board, "the copy", &writer, Some("first"));
    agrees(&board, "the copy", &other, None);
}
