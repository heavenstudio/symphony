//! Each sender's last word (`said`): what `last_messages` answers from it is what the query it
//! replaced answered from every message, after each write the schema takes: a post, a copy (equal
//! ids on two threads among its messages), a message deleted, or edited to another kind or sender;
//! and a board written before it was has it once opened.

use crate::common::{Fixture, NOW, Seed};
use serde_json::json;
use std::collections::BTreeMap;
use symphony_board::Message;
use symphony_board::rows::MessageRow;

/// Each sender's last word as `last_messages` read it before `said` (976dbd5), from every message:
/// the greatest `[sid, slug]` of the sender's posts and verdicts.
fn read_from_every_message(f: &Fixture) -> BTreeMap<String, Message> {
    let read = "
LET $last = (SELECT sender, array::max(array::group([[sid, slug]])) AS last FROM message WHERE kind IN $delivered GROUP BY sender)
    .map(|$sender| $sender.last[0]);
RETURN SELECT * OMIT id FROM $last.map(|$m| type::record('message', [$m[1], $m[0]]));";
    let mut answers = (f.store.client())
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
fn last_words(f: &Fixture) -> BTreeMap<String, Message> {
    f.store.last_messages().unwrap()
}

fn text_of(words: &BTreeMap<String, Message>, sender: &str) -> Option<String> {
    words.get(sender).map(|word| word.text.clone())
}

/// `last_messages` against the query it replaced, after `step`: equal, and naming `sender`'s word
/// as `text` (none: no word), so each step is the case it is meant to be.
fn agrees(f: &Fixture, step: &str, sender: &str, text: Option<&str>) {
    let read = last_words(f);
    let before = read_from_every_message(f);
    assert_eq!(read, before, "after {step}");
    assert_eq!(text_of(&read, sender).as_deref(), text, "after {step}");
}

/// `UPDATE` or `DELETE` of message `word`, as a hand at the server could: the board never edits a
/// message, and deletes them only all at once, in a copy.
fn write(f: &Fixture, sql: &str, word: &Message, other: &str) {
    (f.store.client())
        .query(
            sql,
            json!({"slug": word.slug, "sid": word.id, "other": other}),
        )
        .unwrap();
}

/// A message `sender` said on `slug` with id `sid`, made by its own request.
fn create(f: &Fixture, slug: &str, sid: i64, sender: &str, kind: &str) {
    (f.store.client())
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
        let f = Fixture::empty();
        for slug in order {
            create(&f, slug, 7, "writer", "msg");
        }
        agrees(
            &f,
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
    let f = Fixture::new(seed);
    agrees(&f, "a copy", "writer", Some("tools/z"));
}

/// A deleted last word gives the sender's word before it, on any thread; one deleted before it
/// changes nothing; a sender with none left has no word; a thread deleted whole leaves each
/// sender its newest word elsewhere.
#[test]
fn a_deleted_last_word_gives_the_word_before_it() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.task("sim/land");
    let first = f.store.say("sim/jump", &a, "first", None).unwrap();
    f.store.say("sim/jump", &b, "from b", None).unwrap();
    f.store.say("sim/land", &a, "second", None).unwrap();
    f.store.say("sim/land", &b, "b again", None).unwrap();
    let third = f.store.say("sim/jump", &a, "third", None).unwrap();
    let delete = "DELETE type::record('message', [$slug, $sid]);";
    agrees(&f, "the posts", &a, Some("third"));
    write(&f, delete, &third, "");
    agrees(&f, "its newest deleted", &a, Some("second"));
    write(&f, delete, &first, "");
    agrees(&f, "an older one deleted", &a, Some("second"));
    write(&f, "DELETE message WHERE slug = 'sim/land';", &first, "");
    agrees(&f, "a thread deleted", &b, Some("from b"));
    agrees(&f, "a thread deleted", &a, None);
}

/// A last word made a record line gives the word before it, and a record line made a post is the
/// word when it is the newest; a post made a verdict stays the word.
#[test]
fn a_word_edited_to_another_kind_counts_as_that_kind() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    f.store.say("sim/jump", &a, "first", None).unwrap();
    let second = f.store.say("sim/jump", &a, "second", None).unwrap();
    write(
        &f,
        "UPDATE type::record('message', [$slug, $sid]) SET kind = 'system';",
        &second,
        "",
    );
    agrees(&f, "its newest made a record line", &a, Some("first"));
    create(&f, "sim/jump", second.id + 1, &a, "system");
    agrees(&f, "a newer record line", &a, Some("first"));
    write(
        &f,
        "UPDATE message SET kind = 'msg' WHERE sid = $sid + 1;",
        &second,
        "",
    );
    agrees(
        &f,
        "the record line made a post",
        &a,
        Some("sim/jump system"),
    );
    write(
        &f,
        "UPDATE message SET kind = 'verdict' WHERE sid = $sid + 1;",
        &second,
        "",
    );
    agrees(&f, "the post made a verdict", &a, Some("sim/jump system"));
    write(
        &f,
        "UPDATE message SET text = 'said again' WHERE sid = $sid + 1;",
        &second,
        "",
    );
    agrees(&f, "the verdict's text edited", &a, Some("said again"));
}

/// A last word given to another sender is that sender's word if it is the newest of theirs, and
/// leaves its first sender the word before it.
#[test]
fn a_word_given_another_sender_moves_to_that_sender() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.store.say("sim/jump", &a, "first", None).unwrap();
    f.store.say("sim/jump", &b, "from b", None).unwrap();
    let second = f.store.say("sim/jump", &a, "second", None).unwrap();
    write(
        &f,
        "UPDATE type::record('message', [$slug, $sid]) SET sender = $other;",
        &second,
        &b,
    );
    agrees(&f, "its newest given to b", &a, Some("first"));
    agrees(&f, "its newest given to b", &b, Some("second"));
    write(
        &f,
        "UPDATE type::record('message', [$slug, $sid]) SET sender = $other;",
        &second,
        &a,
    );
    agrees(&f, "given back", &a, Some("second"));
    agrees(&f, "given back", &b, Some("from b"));
}

/// A board written before `said` was (its version older) has each sender's last word named once
/// the next store opens it, and only then: a board that names any keeps its names.
#[test]
fn a_board_from_before_the_last_words_were_kept_has_them_once_opened() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.store.say("sim/jump", &a, "first", None).unwrap();
    f.store.say("sim/jump", &b, "from b", None).unwrap();
    f.store.say("sim/jump", &a, "second", None).unwrap();
    agrees(&f, "the posts", &a, Some("second"));
    let was = last_words(&f);
    (f.store.client())
        .query(
            "DELETE said; UPDATE meta:schema SET version = 12;",
            json!({}),
        )
        .unwrap();
    assert_eq!(
        last_words(&f),
        BTreeMap::new(),
        "the board as an older store left it"
    );
    assert_eq!(f.other().last_messages().unwrap(), was);
}

/// A copy over a board replaces every message, and the last words with them.
#[test]
fn a_copy_over_a_board_names_the_copied_senders_last_words() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.store.say("sim/jump", &a, "first", None).unwrap();
    let dump = f.store.dump().unwrap();
    f.store.say("sim/jump", &b, "from b", None).unwrap();
    f.store.say("sim/jump", &a, "second", None).unwrap();
    f.store.copy(&dump).unwrap();
    agrees(&f, "the copy", &a, Some("first"));
    agrees(&f, "the copy", &b, None);
}
