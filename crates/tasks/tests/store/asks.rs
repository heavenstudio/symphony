//! The owner's asks: one open ask for each thing the owner waits on, a later line about it joining it
//! while it is open; who holds it and who held it before; the first answer closing it; a close by
//! hand; and the listing.

use crate::common::{Fixture, NOW, Seed, refused};
use serde_json::json;
use symphony_board::rows::HeldRow;
use symphony_board::{Ask, NewAsk};

/// The owner's verdict with a note on the card `owner-asks` in docs/DECISIONS.md, on the main
/// checkout.
fn verdict(words: &str) -> NewAsk {
    NewAsk {
        kind: "verdict".into(),
        path: Some("docs/DECISIONS.md".into()),
        card: Some("owner-asks".into()),
        words: words.into(),
        ..NewAsk::default()
    }
}

fn chat(thread: &str, words: &str) -> NewAsk {
    NewAsk {
        kind: "chat".into(),
        thread: Some(thread.into()),
        words: words.into(),
        ..NewAsk::default()
    }
}

/// A board with the sessions amber-heron and brisk-finch and the task sim/jump.
fn board() -> Fixture {
    Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "ready"),
    )
}

fn ids(asks: &[Ask]) -> Vec<i64> {
    asks.iter().map(|a| a.id).collect()
}

#[test]
fn a_line_opens_an_ask_and_another_about_the_same_card_joins_it_keeping_its_age_and_words() {
    let f = board();
    let first = f.store.open_ask(&verdict("why not\n B?")).unwrap();
    assert_eq!(
        first,
        Ask {
            id: 1,
            kind: "verdict".into(),
            about: r#"["card","","docs/DECISIONS.md","owner-asks"]"#.into(),
            path: Some("docs/DECISIONS.md".into()),
            card: Some("owner-asks".into()),
            words: "why not B?".into(),
            asked: NOW,
            lines: 1,
            last: NOW,
            open: true,
            ..Ask::default()
        }
    );
    f.advance(60.0);
    let message = NewAsk {
        kind: "message".into(),
        ..verdict("and the picture?")
    };
    let joined = f.store.open_ask(&message).unwrap();
    assert_eq!(
        joined,
        Ask {
            lines: 2,
            last: NOW + 60.0,
            ..first.clone()
        },
        "a message on the card joins the verdict's ask: one thing waited on"
    );
    // A line said earlier than the last one counted keeps the last.
    let late = f
        .store
        .open_ask(&NewAsk {
            at: Some(NOW + 5.0),
            ..verdict("one more")
        })
        .unwrap();
    assert_eq!((late.id, late.lines, late.last), (1, 3, NOW + 60.0));
    assert_eq!(ids(&f.store.asks(None).unwrap()), [1]);
}

#[test]
fn a_line_after_the_answer_opens_a_new_ask() {
    let f = board();
    let first = f.store.open_ask(&verdict("why not B?")).unwrap();
    f.store
        .answer_ask(first.id, Some("amber-heron"), "the card")
        .unwrap();
    f.advance(30.0);
    let second = f.store.open_ask(&verdict("and C?")).unwrap();
    assert_eq!(
        (second.id, second.words.as_str(), second.asked, second.lines),
        (2, "and C?", NOW + 30.0, 1)
    );
    assert!(!f.store.ask(1).unwrap().open);
}

#[test]
fn the_same_card_on_a_worktree_and_each_other_thing_is_an_ask_of_its_own() {
    let f = board();
    let main = f.store.open_ask(&verdict("why?")).unwrap();
    let tree = f
        .store
        .open_ask(&NewAsk {
            tree: Some("tools/x".into()),
            ..verdict("why there?")
        })
        .unwrap();
    let other_card = f
        .store
        .open_ask(&NewAsk {
            card: Some("other".into()),
            ..verdict("and this?")
        })
        .unwrap();
    let comment = f
        .store
        .open_ask(&NewAsk {
            kind: "comment".into(),
            path: Some("docs/DECISIONS.md".into()),
            block: Some("12".into()),
            words: "typo".into(),
            ..NewAsk::default()
        })
        .unwrap();
    let talk = f.store.open_ask(&chat("7", "hello?")).unwrap();
    let task = f
        .store
        .open_ask(&NewAsk {
            kind: "task".into(),
            task: Some("sim/jump".into()),
            words: "status?".into(),
            ..NewAsk::default()
        })
        .unwrap();
    assert_eq!(
        [
            main.id,
            tree.id,
            other_card.id,
            comment.id,
            talk.id,
            task.id
        ],
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(tree.tree.as_deref(), Some("tools/x"));
    assert_eq!(task.task.as_deref(), Some("sim/jump"));
}

#[test]
fn an_ask_needs_its_words_and_a_task_on_the_board_and_empty_fields_are_none() {
    let f = board();
    assert_eq!(
        refused(f.store.open_ask(&NewAsk {
            kind: "task".into(),
            task: Some("sim/nothing".into()),
            words: "status?".into(),
            ..NewAsk::default()
        })),
        "no task 'sim/nothing'"
    );
    assert_eq!(
        refused(f.store.open_ask(&verdict(" \n "))),
        "an ask holds the owner's words"
    );
    assert_eq!(
        refused(f.store.open_ask(&NewAsk {
            kind: "approval".into(),
            ..verdict("ok")
        })),
        "an ask's kind is one of verdict, message, comment, chat, task, not 'approval'"
    );
    assert_eq!(f.store.asks(Some(0.0)).unwrap(), []);
    let blank = f
        .store
        .open_ask(&NewAsk {
            tree: Some(String::new()),
            block: Some(String::new()),
            ..verdict("why?")
        })
        .unwrap();
    assert_eq!((blank.tree, blank.block), (None, None));
}

#[test]
fn a_hold_keeps_each_earlier_holder_from_when_until_when_and_why() {
    let f = board();
    let ask = f.store.open_ask(&verdict("why not B?")).unwrap();
    f.advance(10.0);
    let held = f
        .store
        .hold_ask(ask.id, Some("amber-heron"), "first")
        .unwrap();
    assert_eq!(
        (held.holder.as_deref(), held.held, held.before.clone()),
        (Some("amber-heron"), Some(NOW + 10.0), vec![])
    );
    f.advance(600.0);
    let again = f
        .store
        .hold_ask(ask.id, Some("amber-heron"), "again")
        .unwrap();
    assert_eq!(again, held, "holding it as it is held changes nothing");
    let moved = f
        .store
        .hold_ask(ask.id, Some("brisk-finch"), "no answer in 10 min")
        .unwrap();
    let amber = HeldRow {
        handle: "amber-heron".into(),
        since: NOW + 10.0,
        until: NOW + 610.0,
        why: "no answer in 10 min".into(),
    };
    assert_eq!(
        (
            moved.holder.as_deref(),
            moved.held,
            moved.before.clone(),
            moved.asked
        ),
        (
            Some("brisk-finch"),
            Some(NOW + 610.0),
            vec![amber.clone()],
            NOW
        ),
        "the ask's age stays the owner's first line's"
    );
    f.advance(5.0);
    let nobody = f.store.hold_ask(ask.id, None, "brisk-finch left").unwrap();
    assert_eq!(
        (nobody.holder, nobody.held, nobody.before),
        (
            None,
            None,
            vec![
                amber,
                HeldRow {
                    handle: "brisk-finch".into(),
                    since: NOW + 610.0,
                    until: NOW + 615.0,
                    why: "brisk-finch left".into(),
                }
            ]
        )
    );
}

#[test]
fn a_hold_is_refused_for_an_unknown_session_a_missing_ask_or_an_answered_one() {
    let f = board();
    let ask = f.store.open_ask(&verdict("why not B?")).unwrap();
    assert_eq!(
        refused(f.store.hold_ask(ask.id, Some("nobody-here"), "x")),
        f.no_session("nobody-here")
    );
    assert_eq!(
        refused(f.store.hold_ask(9, Some("amber-heron"), "x")),
        "no ask 9"
    );
    f.store.answer_ask(ask.id, None, "the card").unwrap();
    assert_eq!(
        refused(f.store.hold_ask(ask.id, Some("amber-heron"), "x")),
        "ask 1 is answered already"
    );
    assert_eq!(f.store.ask(ask.id).unwrap().holder, None);
}

#[test]
fn the_first_answer_closes_the_ask_and_a_later_one_changes_nothing() {
    let f = board();
    let ask = f.store.open_ask(&verdict("why not B?")).unwrap();
    f.store
        .hold_ask(ask.id, Some("amber-heron"), "first")
        .unwrap();
    f.advance(120.0);
    let answered = f
        .store
        .answer_ask(ask.id, Some("brisk-finch"), "the card")
        .unwrap();
    assert_eq!(
        (
            answered.open,
            answered.closed,
            answered.answered,
            answered.answered_by.as_deref(),
            answered.answer.as_deref(),
            answered.holder.as_deref()
        ),
        (
            false,
            false,
            Some(NOW + 120.0),
            Some("brisk-finch"),
            Some("the card"),
            Some("amber-heron")
        ),
        "whoever answers first, holder or not, closes it"
    );
    f.advance(60.0);
    let later = f
        .store
        .answer_ask(ask.id, Some("amber-heron"), "mine too")
        .unwrap();
    assert_eq!(later, answered);
    assert_eq!(refused(f.store.answer_ask(4, None, "x")), "no ask 4");
    assert_eq!(refused(f.store.ask(4)), "no ask 4");
}

#[test]
fn an_answer_about_a_thing_closes_its_open_ask_or_finds_none() {
    let f = board();
    let card = f.store.open_ask(&verdict("why not B?")).unwrap();
    let talk = f.store.open_ask(&chat("7", "hello?")).unwrap();
    f.advance(30.0);
    let message = NewAsk {
        kind: "message".into(),
        words: String::new(),
        ..verdict("")
    };
    let answered = f
        .store
        .answer_about(&message, Some("amber-heron"), "on the card")
        .unwrap()
        .expect("the card's ask was open");
    assert_eq!(
        (answered.id, answered.answered, answered.answer.as_deref()),
        (card.id, Some(NOW + 30.0), Some("on the card"))
    );
    assert_eq!(
        f.store
            .answer_about(&message, Some("brisk-finch"), "again")
            .unwrap(),
        None
    );
    assert!(
        f.store.ask(talk.id).unwrap().open,
        "another thing stays open"
    );
    assert_eq!(
        refused(f.store.answer_about(
            &NewAsk {
                kind: "chat".into(),
                ..NewAsk::default()
            },
            None,
            "x"
        )),
        "an ask of kind 'chat' names its thread"
    );
}

#[test]
fn a_close_by_hand_needs_a_known_session_and_a_reason_and_an_open_ask() {
    let f = board();
    let ask = f.store.open_ask(&chat("7", "thanks")).unwrap();
    assert_eq!(
        refused(f.store.close_ask(ask.id, "amber-heron", "  ")),
        "say why ask 1 needs no answer: `just tasks asks close 1 \"WHY\"`"
    );
    assert_eq!(
        refused(f.store.close_ask(ask.id, "nobody-here", "x")),
        f.no_session("nobody-here")
    );
    assert_eq!(
        refused(f.store.close_ask(5, "amber-heron", "x")),
        "no ask 5"
    );
    f.advance(45.0);
    let closed = f
        .store
        .close_ask(ask.id, "amber-heron", " a thank-you needs no answer ")
        .unwrap();
    assert_eq!(
        (
            closed.open,
            closed.closed,
            closed.answered,
            closed.answered_by.as_deref(),
            closed.answer.as_deref()
        ),
        (
            false,
            true,
            Some(NOW + 45.0),
            Some("amber-heron"),
            Some("a thank-you needs no answer")
        )
    );
    assert_eq!(
        refused(f.store.close_ask(ask.id, "amber-heron", "x")),
        "ask 1 is answered already"
    );
}

#[test]
fn a_gone_mark_keeps_the_ask_open_keeps_its_first_time_and_comes_off_and_never_marks_an_answer() {
    let f = board();
    let ask = f.store.open_ask(&verdict("why not B?")).unwrap();
    f.advance(30.0);
    let marked = f
        .store
        .mark_gone(ask.id, Some("the worktree of tools/x is gone"))
        .unwrap();
    assert_eq!(
        (
            marked.open,
            marked.gone.as_deref(),
            marked.gone_since,
            marked.answered
        ),
        (
            true,
            Some("the worktree of tools/x is gone"),
            Some(NOW + 30.0),
            None
        )
    );
    // Listed with the open ones, never with the answered.
    assert_eq!(ids(&f.store.asks(Some(0.0)).unwrap()), [ask.id]);
    assert!(f.store.asks(Some(0.0)).unwrap()[0].open);
    // Reworded, it keeps the time it was first read gone; the same words change nothing.
    f.advance(30.0);
    let reworded = f.store.mark_gone(ask.id, Some("its card is gone")).unwrap();
    assert_eq!(
        (reworded.gone.as_deref(), reworded.gone_since),
        (Some("its card is gone"), Some(NOW + 30.0))
    );
    assert_eq!(
        f.store.mark_gone(ask.id, Some("its card is gone")).unwrap(),
        reworded
    );
    // Back where it was: the mark comes off.
    let back = f.store.mark_gone(ask.id, None).unwrap();
    assert_eq!((back.gone, back.gone_since, back.open), (None, None, true));
    // An answered ask is never marked.
    f.store.answer_ask(ask.id, None, "the card").unwrap();
    let answered = f.store.mark_gone(ask.id, Some("its card is gone")).unwrap();
    assert_eq!((answered.open, answered.gone), (false, None));
    assert_eq!(refused(f.store.mark_gone(9, Some("x"))), "no ask 9");
}

#[test]
fn the_listing_is_the_open_asks_oldest_first_then_those_answered_since_newest_first() {
    let f = board();
    let a = f.store.open_ask(&chat("1", "a")).unwrap();
    f.advance(10.0);
    let b = f.store.open_ask(&chat("2", "b")).unwrap();
    // Said before b, though opened after it.
    let c = f
        .store
        .open_ask(&NewAsk {
            at: Some(NOW + 5.0),
            ..chat("3", "c")
        })
        .unwrap();
    let d = f.store.open_ask(&chat("4", "d")).unwrap();
    f.advance(10.0);
    f.store.answer_ask(a.id, None, "x").unwrap();
    f.advance(10.0);
    f.store.close_ask(d.id, "amber-heron", "y").unwrap();
    assert_eq!(ids(&f.store.asks(None).unwrap()), [c.id, b.id]);
    assert_eq!(
        ids(&f.store.asks(Some(NOW)).unwrap()),
        [c.id, b.id, d.id, a.id]
    );
    assert_eq!(
        ids(&f.store.asks(Some(NOW + 25.0)).unwrap()),
        [c.id, b.id, d.id],
        "an ask answered before `since` is left out"
    );
}

#[test]
fn a_copied_board_numbers_its_next_ask_after_the_copys() {
    let f = Fixture::new(Seed {
        dump: crate::copy::everything(),
    });
    let next = f.store.open_ask(&chat("9", "next")).unwrap();
    assert_eq!(next.id, 4);
    let joined = f.store.open_ask(&chat("7", "still on?")).unwrap();
    assert_eq!(
        (joined.id, joined.lines),
        (2, 2),
        "the copy's open ask is joined"
    );
}

#[test]
fn an_ask_moves_the_boards_marker() {
    let f = board();
    let marker = || f.store.board_mark().unwrap();
    let before = marker();
    let ask = f.store.open_ask(&verdict("why?")).unwrap();
    let opened = marker();
    assert!(opened > before, "{before} -> {opened}");
    f.store.answer_ask(ask.id, None, "x").unwrap();
    assert!(marker() > opened);
}

/// The chat event `key` as the board holds it: its words and whether it was delivered.
fn chat_event(f: &Fixture, key: &str) -> serde_json::Value {
    (f.store.raw().row("chat_event", key))
        .fields(&["text", "delivered"])
        .unwrap()
}

#[test]
fn an_owner_save_queues_its_chat_and_opens_its_ask_in_one_write_once_per_key() {
    let f = board();
    let said = "OWNER /api/decision why not B?";
    f.store
        .owner_save("journal-1", said, Some(&verdict("why not B?")))
        .unwrap();
    assert_eq!(
        chat_event(&f, "journal-1"),
        json!({"text": said, "delivered": false})
    );
    let [ask] = <[Ask; 1]>::try_from(f.store.asks(None).unwrap()).unwrap();
    assert_eq!(
        (ask.id, ask.lines, ask.words.as_str()),
        (1, 1, "why not B?")
    );
    // The same save handed over again changes nothing.
    f.store
        .owner_save("journal-1", said, Some(&verdict("why not B?")))
        .unwrap();
    assert_eq!(f.store.ask(1).unwrap().lines, 1);
    // The next save about the card joins it; a save with no words to deliver keeps its key only.
    f.advance(30.0);
    f.store
        .owner_save("journal-2", "", Some(&verdict("rejected")))
        .unwrap();
    assert_eq!(
        chat_event(&f, "journal-2"),
        json!({"text": "", "delivered": true})
    );
    let joined = f.store.ask(1).unwrap();
    assert_eq!((joined.lines, joined.last), (2, NOW + 30.0));
    // A save that opens no ask queues its chat as before.
    f.store.owner_callback("journal-3", "just chat").unwrap();
    assert_eq!(
        chat_event(&f, "journal-3"),
        json!({"text": "just chat", "delivered": false})
    );
    assert_eq!(ids(&f.store.asks(None).unwrap()), [1]);
    // An ask the board would refuse refuses the save, chat and all.
    let nameless = NewAsk {
        thread: None,
        ..chat("x", "x")
    };
    assert_eq!(
        refused(f.store.owner_save("journal-4", "x", Some(&nameless))),
        "an ask of kind 'chat' names its thread"
    );
    assert_eq!(chat_event(&f, "journal-4"), serde_json::Value::Null);
}

#[test]
fn the_owners_line_on_a_task_opens_its_ask_and_a_line_by_one_holding_a_role_answers_it() {
    let f = board();
    f.store.claim("sim/jump", "amber-heron").unwrap();
    // The owner's line that asks nothing (a board action's record) opens no ask.
    f.store
        .say("sim/jump", "owner", "approved the stamps", None)
        .unwrap();
    assert_eq!(f.store.asks(None).unwrap(), []);
    f.store
        .say_asking("sim/jump", "how far along?", None)
        .unwrap();
    f.advance(20.0);
    f.store
        .say_asking("sim/jump", "and the dock?", None)
        .unwrap();
    let [ask] = <[Ask; 1]>::try_from(f.store.asks(None).unwrap()).unwrap();
    assert_eq!(
        (
            ask.kind.as_str(),
            ask.task.as_deref(),
            ask.words.as_str(),
            ask.lines,
            ask.asked,
            ask.last
        ),
        (
            "task",
            Some("sim/jump"),
            "how far along?",
            2,
            NOW,
            NOW + 20.0
        )
    );
    // A session with no role there, and the owner's line not sent to it, answers nothing.
    f.store
        .say("sim/jump", "brisk-finch", "I can look", None)
        .unwrap();
    assert!(f.store.ask(ask.id).unwrap().open);
    f.advance(40.0);
    let reply = f
        .store
        .say("sim/jump", "amber-heron", "halfway; the dock is next", None)
        .unwrap();
    let answered = f.store.ask(ask.id).unwrap();
    assert_eq!(
        (
            answered.open,
            answered.answered,
            answered.answered_by.as_deref(),
            answered.answer
        ),
        (
            false,
            Some(NOW + 60.0),
            Some("amber-heron"),
            Some(format!("#{}", reply.id))
        )
    );
    // The owner's next line opens a new ask; the session it is sent to answers it, role or not.
    f.store
        .say_asking("sim/jump", "brisk, can you check?", Some("brisk-finch"))
        .unwrap();
    let second = f.store.asks(None).unwrap()[0].clone();
    assert_eq!((second.id, second.lines), (2, 1));
    f.store
        .say("sim/jump", "brisk-finch", "checked", None)
        .unwrap();
    assert_eq!(
        f.store.ask(2).unwrap().answered_by.as_deref(),
        Some("brisk-finch")
    );
}

#[test]
fn concurrent_asks_join_once_and_duplicate_owner_saves_count_once() {
    let f = board();
    let stores = [f.other(), f.other()];
    let barrier = std::sync::Barrier::new(2);
    let opened = std::thread::scope(|scope| {
        let joins: Vec<_> = stores
            .iter()
            .map(|store| {
                scope.spawn(|| {
                    barrier.wait();
                    store.open_ask(&verdict("why?"))
                })
            })
            .collect();
        joins
            .into_iter()
            .map(|join| join.join().unwrap().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(opened[0].id, opened[1].id);
    assert_eq!(f.store.ask(opened[0].id).unwrap().lines, 2);
    std::thread::scope(|scope| {
        let joins: Vec<_> = stores
            .iter()
            .map(|store| {
                scope.spawn(|| {
                    barrier.wait();
                    store.owner_save("same-event", "owner's question", Some(&verdict("again?")))
                })
            })
            .collect();
        for join in joins {
            join.join().unwrap().unwrap();
        }
    });
    assert_eq!(f.store.ask(opened[0].id).unwrap().lines, 3);
    assert_eq!(f.store.raw().table("chat_event").count().unwrap(), 1);
}
