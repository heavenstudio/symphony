//! Each sender's last word (`said`): kept as messages are written, named for a board written
//! before it was, and replaced with the messages by a copy.

use crate::common::Fixture;

/// Each sender's last word as the messages say it, read the way `last_messages` does not.
fn last_words(f: &Fixture) -> Vec<(String, String)> {
    let mut words: Vec<(String, String)> = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .filter(|m| m.kind != "system")
        .map(|m| (m.sender, m.text))
        .rev()
        .collect();
    words.sort_by(|a, b| a.0.cmp(&b.0));
    words.dedup_by(|later, first| later.0 == first.0);
    words
}

fn spoken(f: &Fixture) -> Vec<(String, String)> {
    (f.store.last_messages().unwrap().into_iter())
        .map(|(sender, m)| (sender, m.text))
        .collect()
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
    let was = spoken(&f);
    assert_eq!(was, last_words(&f));
    f.store
        .client()
        .query(
            "DELETE said; UPDATE meta:schema SET version = 12;",
            serde_json::json!({}),
        )
        .unwrap();
    assert!(spoken(&f).is_empty(), "the board as an older store left it");
    let opened = f.other();
    assert_eq!(
        (opened.last_messages().unwrap().into_iter())
            .map(|(sender, m)| (sender, m.text))
            .collect::<Vec<_>>(),
        was
    );
}

/// A copy over a board replaces every message, and the last words with them.
#[test]
fn a_copy_over_a_board_leaves_the_copied_senders_last_words_alone() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.store.say("sim/jump", &a, "first", None).unwrap();
    let dump = f.store.dump().unwrap();
    f.store.say("sim/jump", &b, "from b", None).unwrap();
    f.store.say("sim/jump", &a, "second", None).unwrap();
    f.store.copy(&dump).unwrap();
    assert_eq!(spoken(&f), [(a, "first".to_string())]);
}
