//! Comments held on documents (`symphony_board::held::OnDocument`, `Store::hold_on_document`,
//! `Store::pass_on_document`): the comment of a person who may speak and may not direct agents is
//! a row of the held table with no task, keyed apart from every task's held message: no line goes
//! on any thread, no ask is opened, and no read of a task's held messages finds it. The reads of a
//! document's find it by its route and checkout, whole; a pass marks it passed once, and a sweep's
//! move keeps a landed worktree's under main's. A board without the step holds nothing and refuses
//! a hold.
//!
//! And the note beside a verdict (users-and-identity.md, "The note beside a verdict"): a line of
//! a person who may decide the task and may not direct agents that reads as their approve is
//! written with its note, not held.

use super::common::{Fixture, NOW, Seed, refused};
use symphony_board::Store;
use symphony_board::held::{Block, MESSAGE, OnDocument, PassedOn, Place, Said};
use symphony_board::people::ownership::Place as Mapped;
use symphony_board::people::{Holder, Power};
use symphony_board::schema::{ADDED, HELD};
use symphony_board::stamps::read::reads_as_approve;

/// The words every case holds, which no agent's read may carry.
const WORDS: &str = "141 lamps\nFar too many to validate.";

/// The session that adds people.
const BY: &str = "amber-heron";

/// A board with `tools/x`, `ana`, who may speak and not direct agents, and `stefano`, who may do
/// both.
fn board() -> Fixture {
    let f = Fixture::new(Seed::default().session(BY));
    f.task("tools/x");
    f.store.add_person("ana", &[], &[Power::Speak], BY).unwrap();
    f.store
        .add_person("stefano", &[], &[Power::Speak, Power::DirectAgents], BY)
        .unwrap();
    f
}

/// A store on the board writing for `name`, as the Symphony makes for a person it admitted.
fn person(f: &Fixture, name: &str) -> Store {
    f.other().for_person(name)
}

/// A callout left after the block `key` of `path`, in the checkout `scope`.
fn callout(path: &str, scope: Option<&str>, key: &str) -> Place {
    Place {
        path: path.into(),
        scope: scope.map(String::from),
        kind: "question".into(),
        card: None,
        block: Some(Block {
            key: key.into(),
            line: 4,
            text: "Some prose.".into(),
        }),
    }
}

/// A message on the card `card` of `path`.
fn message(path: &str, card: &str) -> Place {
    Place {
        path: path.into(),
        scope: None,
        kind: MESSAGE.into(),
        card: Some(card.into()),
        block: None,
    }
}

/// Every text an agent can read of `slug`'s thread.
fn thread(f: &Fixture, slug: &str) -> Vec<String> {
    let thread = f.store.thread(slug).unwrap();
    thread.into_iter().map(|m| m.text).collect()
}

#[test]
fn a_held_comment_is_a_row_of_its_own_no_thread_ask_or_tasks_read_carries() {
    let f = board();
    let lines = f.store.thread_rows("tools/x").unwrap().len();
    let ana = person(&f, "ana");
    let place = callout("docs/a.md", None, "section:topic:1");
    let held = ana.hold_on_document("ana", WORDS, &place).unwrap();
    assert_eq!(
        held,
        OnDocument {
            id: held.id,
            document: place.clone(),
            author: "ana".into(),
            text: WORDS.into(),
            at: NOW,
            passed: None,
        }
    );
    let on_card = ana
        .hold_on_document("ana", "  And why?  ", &message("docs/a.md", "one"))
        .unwrap();
    assert_eq!(on_card.id, held.id + 1, "numbered by a counter of its own");
    assert_eq!(on_card.text, "And why?", "kept trimmed");

    // Nothing goes on a thread, nothing is asked, and a task's reads find none of it.
    assert_eq!(f.store.thread_rows("tools/x").unwrap().len(), lines);
    assert!(thread(&f, "tools/x").iter().all(|t| !t.contains("lamps")));
    assert!(f.store.asks(None).unwrap().is_empty(), "no ask is opened");
    assert_eq!(f.store.held_on("tools/x").unwrap(), []);
    assert_eq!(f.store.held_count("tools/x").unwrap(), 0);

    // A document's read finds its own, by route and checkout; the others find none.
    let both = vec![held.clone(), on_card.clone()];
    assert_eq!(f.store.held_on_document("docs/a.md", None).unwrap(), both);
    assert_eq!(f.store.held_on_document("docs/b.md", None).unwrap(), []);
    assert_eq!(
        f.store
            .held_on_document("docs/a.md", Some("feat~x"))
            .unwrap(),
        []
    );
    let branch = ana
        .hold_on_document("ana", WORDS, &callout("docs/a.md", Some("feat~x"), "k"))
        .unwrap();
    assert_eq!(
        f.store
            .held_on_document("docs/a.md", Some("feat~x"))
            .unwrap(),
        std::slice::from_ref(&branch)
    );
    assert_eq!(f.store.held_on_document("docs/a.md", None).unwrap(), both);
    assert_eq!(f.store.held_comment(held.id).unwrap(), Some(held.clone()));
    assert_eq!(f.store.held_comment(branch.id + 7).unwrap(), None);
    assert_eq!(
        f.store.held_on_documents().unwrap(),
        [held, on_card, branch]
    );
}

#[test]
fn a_task_message_and_a_document_comment_held_side_by_side_never_meet() {
    let f = board();
    let ana = person(&f, "ana");
    let Ok(Said::Held(on_task)) = ana.say_or_hold_as("tools/x", "ana", "on the task", None) else {
        panic!("not held")
    };
    // However their numbers fall, the keys differ: both rows stand.
    let mut comments = Vec::new();
    for _ in 0..=on_task.id {
        comments.push(
            ana.hold_on_document("ana", WORDS, &message("docs/a.md", "one"))
                .unwrap(),
        );
    }
    assert!(comments.iter().any(|comment| comment.id == on_task.id));
    assert_eq!(
        f.store.held_on("tools/x").unwrap(),
        std::slice::from_ref(&on_task)
    );
    assert_eq!(
        f.store.held_on_documents().unwrap().len(),
        comments.len(),
        "{comments:?}"
    );
    // A pass of one is no pass of the other.
    let same = on_task.id;
    f.store.pass_on_document(same, "owner").unwrap();
    assert_eq!(f.store.held_on("tools/x").unwrap()[0].passed, None);
}

#[test]
fn a_pass_marks_the_comment_passed_once_and_names_who_passed_it() {
    let f = board();
    let held = person(&f, "ana")
        .hold_on_document("ana", WORDS, &message("docs/a.md", "one"))
        .unwrap();
    let passed = person(&f, "stefano")
        .pass_on_document(held.id, "stefano")
        .unwrap();
    assert_eq!(
        passed.passed,
        Some(PassedOn {
            by: "stefano".into(),
            at: NOW
        })
    );
    assert_eq!(f.store.held_comment(held.id).unwrap(), Some(passed.clone()));
    assert_eq!(f.store.held_on_documents().unwrap(), []);
    assert_eq!(
        f.store.held_on_document("docs/a.md", None).unwrap(),
        std::slice::from_ref(&passed)
    );
    assert_eq!(
        refused(f.store.pass_on_document(held.id, "owner")),
        format!(
            "comment #{} was passed on already, by stefano: nothing was written",
            held.id
        )
    );
    assert_eq!(
        refused(f.store.pass_on_document(held.id + 1, "owner")),
        format!(
            "no comment #{} is held on a document: nothing was written",
            held.id + 1
        )
    );
    assert_eq!(f.store.held_comment(held.id).unwrap(), Some(passed));
}

/// A pass into a checkout keeps its row under that checkout, where the reads of its document find
/// it: a branch's message written on main's card is main's, a page's own stays its page's, and a
/// pass that names none keeps its row where it was left. Its refusals move nothing.
#[test]
fn a_pass_into_a_checkout_keeps_its_row_under_the_checkout_it_wrote_to() {
    let f = board();
    let ana = person(&f, "ana");
    let on_branch = |card: &str| Place {
        scope: Some("feat~x".into()),
        ..message("docs/a.md", card)
    };
    let hold = |place: &Place| ana.hold_on_document("ana", WORDS, place).unwrap();
    let (to_main, kept, unnamed) = (
        hold(&on_branch("one")),
        hold(&on_branch("two")),
        hold(&on_branch("three")),
    );
    let into_branch = hold(&message("docs/a.md", "one"));
    let stefano = person(&f, "stefano");
    let passed = stefano
        .pass_on_document_into(to_main.id, "stefano", None)
        .unwrap();
    assert_eq!(passed.document, message("docs/a.md", "one"));
    assert_eq!(
        passed.passed,
        Some(PassedOn {
            by: "stefano".into(),
            at: NOW
        })
    );
    let stays = stefano
        .pass_on_document_into(kept.id, "stefano", Some("feat~x"))
        .unwrap();
    assert_eq!(stays.document, kept.document);
    let marked = stefano.pass_on_document(unnamed.id, "stefano").unwrap();
    assert_eq!(marked.document, unnamed.document);
    let moved = stefano
        .pass_on_document_into(into_branch.id, "stefano", Some("feat~y"))
        .unwrap();
    assert_eq!(moved.document.scope.as_deref(), Some("feat~y"));
    let ids = |scope: Option<&str>| -> Vec<i64> {
        (f.store.held_on_document("docs/a.md", scope).unwrap().iter())
            .map(|held| held.id)
            .collect()
    };
    assert_eq!(ids(None), [to_main.id]);
    assert_eq!(ids(Some("feat~x")), [kept.id, unnamed.id]);
    assert_eq!(ids(Some("feat~y")), [into_branch.id]);
    assert_eq!(f.store.held_comment(to_main.id).unwrap(), Some(passed));
    // A second pass is refused, and the row stays where its first pass kept it.
    let again = refused(stefano.pass_on_document_into(to_main.id, "owner", Some("feat~x")));
    assert!(again.contains("passed on already, by stefano"), "{again}");
    let none = refused(stefano.pass_on_document_into(into_branch.id + 9, "owner", None));
    assert!(none.starts_with("no comment #"), "{none}");
    assert_eq!(ids(None), [to_main.id]);
}

/// The ids of the comments held on `path` of the checkout `scope`, passed on or not.
fn ids(f: &Fixture, path: &str, scope: Option<&str>) -> Vec<i64> {
    (f.store.held_on_document(path, scope).unwrap().iter())
        .map(|held| held.id)
        .collect()
}

/// A sweep's move (`Store::held_landed`): the comments passed into the swept worktree's checkout
/// are kept under the project's own from then on, whole but for their scope, where a count of
/// main's file reads them. One still held there, one passed into another checkout and main's own
/// stay where they are; a second move, and one of a checkout nothing was passed into, move none.
#[test]
fn a_sweeps_move_keeps_the_rows_passed_into_a_landed_worktree_under_mains() {
    let f = board();
    let ana = person(&f, "ana");
    let on = |scope: &str, card: &str| Place {
        scope: Some(scope.into()),
        ..message("docs/a.md", card)
    };
    let hold = |place: &Place| ana.hold_on_document("ana", WORDS, place).unwrap();
    let card = hold(&on("feat~x", "one"));
    let after = hold(&callout("docs/b.md", Some("feat~x"), "section:topic:1"));
    let still = hold(&on("feat~x", "two"));
    let other = hold(&on("feat~y", "one"));
    let mains = hold(&message("docs/a.md", "one"));
    let stefano = person(&f, "stefano");
    let pass = |held: &OnDocument, into: Option<&str>| {
        (stefano.pass_on_document_into(held.id, "stefano", into)).unwrap()
    };
    let card = pass(&card, Some("feat~x"));
    pass(&after, Some("feat~x"));
    pass(&other, Some("feat~y"));
    pass(&mains, None);
    assert_eq!(f.store.held_landed("feat~x").unwrap(), 2);
    assert_eq!(ids(&f, "docs/a.md", None), [card.id, mains.id]);
    assert_eq!(ids(&f, "docs/b.md", None), [after.id]);
    assert_eq!(ids(&f, "docs/a.md", Some("feat~x")), [still.id]);
    assert_eq!(ids(&f, "docs/a.md", Some("feat~y")), [other.id]);
    let mains_now = OnDocument {
        document: message("docs/a.md", "one"),
        ..card.clone()
    };
    assert_eq!(f.store.held_comment(card.id).unwrap(), Some(mains_now));
    assert_eq!(f.store.held_landed("feat~x").unwrap(), 0);
    assert_eq!(f.store.held_landed("feat~z").unwrap(), 0);
    assert_eq!(ids(&f, "docs/a.md", Some("feat~x")), [still.id]);
}

/// On a board without the step a sweep's move of a checkout nothing was passed into writes nothing,
/// so nothing refuses it; one with a passed row is refused by the guard, the row where it was.
#[test]
fn a_sweeps_move_on_a_board_without_the_step_writes_only_where_a_row_was_passed() {
    let f = board();
    let ana = person(&f, "ana");
    let place = Place {
        scope: Some("feat~x".into()),
        ..message("docs/a.md", "one")
    };
    let held = ana.hold_on_document("ana", WORDS, &place).unwrap();
    (person(&f, "stefano").pass_on_document_into(held.id, "stefano", Some("feat~x"))).unwrap();
    let step = ADDED.iter().position(|step| *step == HELD).unwrap();
    (f.store.raw().row("meta", "additions"))
        .set(serde_json::json!({"version": step}))
        .unwrap();
    assert_eq!(f.store.held_landed("feat~y").unwrap(), 0);
    let why = refused(f.store.held_landed("feat~x"));
    assert!(why.contains("this write names `held_message`"), "{why}");
    assert_eq!(ids(&f, "docs/a.md", Some("feat~x")), [held.id]);
}

#[test]
fn a_hold_that_cannot_be_made_writes_nothing() {
    let f = board();
    let ana = person(&f, "ana");
    let good = message("docs/a.md", "one");
    let nowhere = Place {
        path: " ".into(),
        ..good.clone()
    };
    let no_card = Place {
        card: None,
        ..good.clone()
    };
    let a_block = Place {
        block: callout("docs/a.md", None, "k").block,
        ..good.clone()
    };
    let no_block = Place {
        block: None,
        ..callout("docs/a.md", None, "k")
    };
    let on_card = Place {
        card: Some("one".into()),
        ..callout("docs/a.md", None, "k")
    };
    for (who, text, place, said) in [
        (
            &f.store,
            WORDS,
            &good,
            "only a person's words are held, and 'owner' writes as no person here",
        ),
        (&ana, "  ", &good, "an empty comment says nothing"),
        (&ana, WORDS, &nowhere, "a held comment names its document"),
        (&ana, WORDS, &no_card, "a held message names its card"),
        (&ana, WORDS, &a_block, "a held message names its card"),
        (&ana, WORDS, &no_block, "a held callout names the block"),
        (&ana, WORDS, &on_card, "a held callout names the block"),
    ] {
        let author = if std::ptr::eq(who, &f.store) {
            "owner"
        } else {
            "ana"
        };
        let why = refused(who.hold_on_document(author, text, place));
        assert!(why.starts_with(said), "{place:?}: {why}");
    }
    // A store that writes for nobody holds no one's words either.
    let why = refused(f.store.hold_on_document("ana", WORDS, &good));
    assert!(why.contains("'ana' writes as no person here"), "{why}");
    assert_eq!(f.store.held_on_documents().unwrap(), []);
}

#[test]
fn a_board_without_the_step_takes_no_hold_and_reads_none() {
    let f = board();
    let ana = person(&f, "ana");
    let held = ana
        .hold_on_document("ana", WORDS, &message("docs/a.md", "one"))
        .unwrap();
    let step = ADDED.iter().position(|step| *step == HELD).unwrap();
    (f.store.raw().row("meta", "additions"))
        .set(serde_json::json!({"version": step}))
        .unwrap();
    for why in [
        refused(ana.hold_on_document("ana", WORDS, &message("docs/a.md", "one"))),
        refused(f.store.pass_on_document(held.id, "owner")),
    ] {
        assert!(why.contains("this write names `held_message`"), "{why}");
        assert!(why.contains("nothing was written"), "{why}");
    }
    assert_eq!(f.store.held_comment(held.id).unwrap().unwrap().passed, None);
}

/// `ana`, who may decide and not direct agents: her line that reads as her approve is written
/// with its note where the task is hers to decide, and held for people where it is not; a line
/// that does not read as one is held, as every line of hers is.
#[test]
fn an_approve_with_its_note_by_one_who_may_decide_and_not_direct_agents_is_written() {
    let f = board();
    (f.store)
        .grant(Holder::Person("ana"), &[Power::Decide], BY)
        .unwrap();
    let ana = person(&f, "ana");
    let approve = "approve 1a2b3c4 the header fits on the phone now";
    // Not hers to decide while the board maps no group to it: held, never refused.
    let said = ana.say_or_hold_as("tools/x", "ana", approve, None).unwrap();
    assert!(matches!(said, Said::Held(_)), "{said:?}");
    f.store.add_group("design", &[], BY).unwrap();
    (f.store)
        .map_place(&Mapped::Area("tools".into()), "design", BY)
        .unwrap();
    f.store.join_group("ana", "design", BY).unwrap();
    let said = ana.say_or_hold_as("tools/x", "ana", approve, None).unwrap();
    let Said::Line(line) = said else {
        panic!("held: {said:?}")
    };
    assert_eq!((line.sender.as_str(), line.text.as_str()), ("ana", approve));
    assert!(reads_as_approve(&line.text));
    let asks = f.store.asks(None).unwrap();
    assert_eq!(
        asks.iter()
            .map(|ask| ask.task.as_deref())
            .collect::<Vec<_>>(),
        [Some("tools/x")],
        "her note waits on an agent's answer"
    );
    // Any other line of hers is held, on the task she decides too.
    let said = ana.say_or_hold_as("tools/x", "ana", "and the footer", None);
    assert!(matches!(said, Ok(Said::Held(_))), "{said:?}");
    assert!(!thread(&f, "tools/x").contains(&"and the footer".to_string()));
}
