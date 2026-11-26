//! The runner with a fake hand: who a look tells and how often, the hands again after a `75` on
//! their backoff and bound, a restart mid-retry and one mid-hand, a notice whose way or address
//! moved since it was made, a board that names nobody, and no address in the record, the log or a
//! printed runner.

use super::*;
use crate::testing::Tree;
use serde_json::json;
use std::cell::RefCell;
use symphony::notices::Key;
use symphony_board::people::ownership::{self, Map};
use symphony_board::people::{Group, Member, Profile};

const ANA_SLACK: &str = "U024BE7LH";
const BO_EMAIL: &str = "bo@example.com";
const ORIGIN: &str = "http://studio.tailnet.ts.net:8765";

/// A board naming ana and bo (design), cy (engineering) and dee (design, no way chosen).
fn people() -> People {
    let member = |name: &str, group: &str| Member {
        name: name.into(),
        groups: vec![group.into()],
        ..Member::default()
    };
    People {
        members: vec![
            member("ana", "design"),
            member("bo", "design"),
            member("cy", "engineering"),
            member("dee", "design"),
        ],
        groups: ["design", "engineering"]
            .map(|name| Group {
                name: name.into(),
                ..Group::default()
            })
            .into(),
        revision: 1,
    }
}

/// Ana's every kind by Slack, bo's by email, cy's by WhatsApp, dee's by none.
fn profiles() -> Profiles {
    let every = |way: &str| {
        let mut preferences = serde_json::Map::new();
        for kind in ["card", "question", "verdict"] {
            preferences.insert(kind.into(), json!(way));
        }
        preferences
    };
    let chat = |way: &str, address: &str| {
        let mut chat = serde_json::Map::new();
        chat.insert(way.into(), json!(address));
        chat
    };
    Profiles {
        people: vec![
            Profile {
                name: "ana".into(),
                chat: chat("slack", ANA_SLACK),
                preferences: every("slack"),
                ..Profile::default()
            },
            Profile {
                name: "bo".into(),
                emails: vec![BO_EMAIL.into()],
                preferences: every("email"),
                ..Profile::default()
            },
            Profile {
                name: "cy".into(),
                chat: chat("whatsapp", "+15551234567"),
                preferences: every("whatsapp"),
                ..Profile::default()
            },
            Profile {
                name: "dee".into(),
                emails: vec!["dee@example.com".into()],
                ..Profile::default()
            },
        ],
    }
}

/// Programs for email and Slack; none for WhatsApp.
fn programs() -> Programs {
    Programs::parse("email = \"/fake/email\"\nslack = \"/fake/slack\"\n").unwrap()
}

fn owned_by(people: &People, name: &str) -> Owning {
    ownership::owning(people, &Map::default(), Some(name), &[])
}

fn card(people: &People, slug: &str, group: &str) -> Waiting {
    Waiting::card(
        format!("docs/design/palette.md#{slug}"),
        format!("/docs/design/palette.md#{slug}"),
        owned_by(people, group),
    )
}

fn task_line(people: &People, id: i64, kind: Kind, group: &str) -> Line {
    Line {
        id,
        kind,
        page: "/task/app/x".into(),
        told: ownership::told(people, &owned_by(people, group)),
    }
}

/// The machine's key, kept in `tree`'s machine folder: made at the first call, the same after.
fn key(tree: &Tree) -> Key {
    Key::of(&tree.path("machine")).unwrap()
}

/// The runner of the project `symphony` in `tree`, as a start opens it.
fn opened(tree: &Tree) -> (Runner, Option<String>) {
    Runner::open(&tree.path(""), "symphony", key(tree)).unwrap()
}

/// A runner of the project `symphony` in `tree`, its first look taken at line 10 over `cards`.
fn started(tree: &Tree, people: &People, cards: Vec<Waiting>) -> Runner {
    let (mut runner, unread) = opened(tree);
    assert_eq!(unread, None);
    look(&mut runner, people, Some(cards), vec![], 10);
    runner
}

fn look(
    runner: &mut Runner,
    people: &People,
    cards: Option<Vec<Waiting>>,
    lines: Vec<Line>,
    latest: i64,
) {
    let look = Look {
        people,
        cards,
        held_read: true,
        lines,
        latest,
    };
    let mut origin = || ORIGIN.to_string();
    (runner.look(&look, &profiles(), &programs(), 0.0, &mut origin)).unwrap();
}

/// What a fake hand was given: each program and message, in order.
#[derive(Default)]
struct Hands {
    given: RefCell<Vec<(PathBuf, String)>>,
    log: RefCell<Vec<String>>,
}

impl Hands {
    /// Hands what is due at `now` on `runner`, each answered `answer`.
    fn at(&self, runner: &mut Runner, now: f64, answer: &dyn Fn() -> Handed) {
        self.with(runner, now, &profiles(), &programs(), answer);
    }

    fn with(
        &self,
        runner: &mut Runner,
        now: f64,
        profiles: &Profiles,
        programs: &Programs,
        answer: &dyn Fn() -> Handed,
    ) {
        let clock = || now;
        let mut hand = |program: &Path, message: &str, _: &[&str]| {
            (self.given.borrow_mut()).push((program.to_path_buf(), message.to_string()));
            answer()
        };
        let mut log = |line: String| self.log.borrow_mut().push(line);
        (runner.hand_due(profiles, programs, &clock, &mut hand, &mut log)).unwrap();
    }

    /// [`Hands::at`], what the hand answers.
    fn tried(&self, runner: &mut Runner, now: f64) -> Result<(), String> {
        let clock = || now;
        let mut hand = |program: &Path, message: &str, _: &[&str]| {
            (self.given.borrow_mut()).push((program.to_path_buf(), message.to_string()));
            Handed::Sent
        };
        let mut log = |line: String| self.log.borrow_mut().push(line);
        runner.hand_due(&profiles(), &programs(), &clock, &mut hand, &mut log)
    }

    /// Each message given, read as the notice crate reads it: id, kind, way and address.
    fn read(&self) -> Vec<(String, String, String, String)> {
        (self.given.borrow().iter())
            .map(|(_, message)| {
                let notice = symphony_notice::Notice::read(message).unwrap();
                (
                    notice.id().to_string(),
                    notice.kind().word().to_string(),
                    notice.to().way().to_string(),
                    notice.to().address().to_string(),
                )
            })
            .collect()
    }
}

fn record(runner: &Runner) -> &Record {
    runner.record.as_ref().unwrap()
}

#[test]
fn the_first_look_takes_what_stands_as_told_and_waits_for_the_cards() {
    let tree = Tree::new();
    let people = people();
    let (mut runner, _) = opened(&tree);
    let hands = Hands::default();
    hands.at(&mut runner, 0.0, &|| Handed::Sent);
    assert!(
        hands.given.borrow().is_empty(),
        "nothing to hand before a look"
    );
    look(&mut runner, &people, None, vec![], 7);
    assert_eq!(runner.cursor(), None, "no look before the cards are read");
    assert!(!tree.path("artifacts/notices.json").exists());
    drop(runner);
    let runner = started(&tree, &people, vec![card(&people, "one", "design")]);
    assert_eq!(runner.cursor(), Some(10));
    let record = record(&runner);
    assert!(record.pending.is_empty(), "nothing of the past is told");
    assert_eq!(
        record.cards["docs/design/palette.md#one"],
        ["ana", "bo", "dee"].map(str::to_string).into()
    );
    assert_eq!(runner.next_due(), None);
    let kept = Record::read(&tree.path("artifacts/notices.json")).unwrap();
    assert_eq!(
        kept.as_ref(),
        Some(record),
        "written under the artifacts folder"
    );
}

/// How many times a look of `cards` and `lines` through `latest` asks its origin.
fn origins_asked(
    runner: &mut Runner,
    people: &People,
    cards: Vec<Waiting>,
    lines: Vec<Line>,
    latest: i64,
) -> usize {
    let look = Look {
        people,
        cards: Some(cards),
        held_read: true,
        lines,
        latest,
    };
    let mut asked = 0;
    let mut origin = || {
        asked += 1;
        ORIGIN.to_string()
    };
    (runner.look(&look, &profiles(), &programs(), 0.0, &mut origin)).unwrap();
    asked
}

#[test]
fn a_look_asks_its_origin_once_where_it_makes_notices_and_never_where_it_makes_none() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    // A card and a verdict, each for ana and bo: four notices, one origin.
    let cards = vec![card(&people, "one", "design")];
    let verdict = vec![task_line(&people, 11, Kind::Verdict, "design")];
    assert_eq!(
        origins_asked(&mut runner, &people, cards.clone(), verdict, 11),
        1
    );
    let pending = &record(&runner).pending;
    assert_eq!(pending.len(), 4);
    assert!(
        (pending.iter()).all(|notice| notice.link.starts_with(&format!("{ORIGIN}/symphony/"))),
        "{pending:?}"
    );
    // Nothing new, and a card for cy alone, whose way no program is named for: none asked.
    assert_eq!(
        origins_asked(&mut runner, &people, cards.clone(), vec![], 11),
        0
    );
    let cy = [cards, vec![card(&people, "two", "cy")]].concat();
    assert_eq!(origins_asked(&mut runner, &people, cy, vec![], 11), 0);
    assert_eq!(record(&runner).pending.len(), 4);
}

#[test]
fn a_card_waiting_on_a_group_tells_each_member_once_by_their_way_and_nobody_without_one() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    let waits = || vec![card(&people, "one", "design"), card(&people, "two", "cy")];
    look(&mut runner, &people, Some(waits()), vec![], 10);
    let hands = Hands::default();
    hands.at(&mut runner, 0.0, &|| Handed::Sent);
    let told: Vec<_> = (hands.read().into_iter())
        .map(|(_, kind, way, address)| (kind, way, address))
        .collect();
    // dee chose no way; cy's WhatsApp has no program on this machine.
    assert_eq!(
        told,
        [
            ("card".into(), "slack".into(), ANA_SLACK.into()),
            ("card".into(), "email".into(), BO_EMAIL.into()),
        ]
    );
    let given = hands.given.borrow().clone();
    assert_eq!(given[0].0, Path::new("/fake/slack"));
    assert_eq!(given[1].0, Path::new("/fake/email"));
    let link = symphony_notice::Notice::read(&given[0].1).unwrap();
    assert_eq!(
        link.link(),
        "http://studio.tailnet.ts.net:8765/symphony/docs/design/palette.md#one"
    );
    assert_eq!(link.project(), "symphony");
    assert_eq!(
        hands
            .log
            .borrow()
            .iter()
            .filter(|l| l.ends_with(": sent"))
            .count(),
        2
    );
    assert!(record(&runner).pending.is_empty());
    // Still waiting: nobody is told again.
    look(&mut runner, &people, Some(waits()), vec![], 10);
    hands.at(&mut runner, 1.0, &|| Handed::Sent);
    assert_eq!(hands.given.borrow().len(), 2);
    // A look before the documents are read keeps the spells.
    look(&mut runner, &people, None, vec![], 10);
    look(&mut runner, &people, Some(waits()), vec![], 10);
    assert!(record(&runner).pending.is_empty());
    // Answered, then waiting again: told again.
    look(&mut runner, &people, Some(vec![]), vec![], 10);
    look(&mut runner, &people, Some(waits()), vec![], 10);
    hands.at(&mut runner, 2.0, &|| Handed::Sent);
    assert_eq!(hands.given.borrow().len(), 4);
    // Handed to engineering while it waits: cy is told, by none here; design is not told again.
    let handed = vec![card(&people, "one", "engineering")];
    look(&mut runner, &people, Some(handed), vec![], 10);
    assert!(record(&runner).pending.is_empty());
    let ids: std::collections::BTreeSet<_> = hands.read().into_iter().map(|n| n.0).collect();
    assert_eq!(ids.len(), 4, "every notice its own id");
}

#[test]
fn a_question_and_a_verdict_tell_once_for_each_line_after_the_cursor() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    let lines = || {
        vec![
            task_line(&people, 9, Kind::Question, "design"),
            task_line(&people, 11, Kind::Question, "ana"),
            task_line(&people, 12, Kind::Verdict, "design"),
        ]
    };
    look(&mut runner, &people, None, lines(), 12);
    assert_eq!(runner.cursor(), Some(12));
    look(&mut runner, &people, None, lines(), 12);
    let hands = Hands::default();
    hands.at(&mut runner, 0.0, &|| Handed::Sent);
    let told: Vec<_> = (hands.read().into_iter())
        .map(|(_, kind, way, _)| (kind, way))
        .collect();
    assert_eq!(
        told,
        [
            ("question".into(), "slack".into()),
            ("verdict".into(), "slack".into()),
            ("verdict".into(), "email".into()),
        ]
    );
    let message = &hands.given.borrow()[0].1;
    let read = symphony_notice::Notice::read(message).unwrap();
    assert_eq!(read.link(), format!("{ORIGIN}/symphony/task/app/x"));
}

#[test]
fn a_75_is_handed_again_with_the_same_id_on_its_backoff_until_its_last_hand() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    look(
        &mut runner,
        &people,
        None,
        vec![task_line(&people, 11, Kind::Verdict, "ana")],
        11,
    );
    let hands = Hands::default();
    let later = || Handed::Later("ratelimited".into());
    let mut times = Vec::new();
    while let Some(due) = runner.next_due() {
        let before = hands.given.borrow().len();
        hands.at(&mut runner, due - 1.0, &later);
        assert_eq!(hands.given.borrow().len(), before, "nothing before {due}");
        hands.at(&mut runner, due, &later);
        times.push(due);
    }
    assert_eq!(
        times,
        [0.0, 60.0, 180.0, 420.0, 900.0, 1860.0, 3780.0, 7380.0]
    );
    let ids: std::collections::BTreeSet<_> = hands.read().into_iter().map(|n| n.0).collect();
    assert_eq!(
        (hands.given.borrow().len(), ids.len()),
        (8, 1),
        "eight hands of one notice"
    );
    let log = hands.log.borrow();
    assert!(log[0].ends_with("not sent this time (hand 1 of 8), again in 60 s: ratelimited"));
    assert!(log[7].ends_with("not sent after 8 hands, so dropped: ratelimited"));
    assert!(record(&runner).pending.is_empty());
    assert_eq!(again_after(1), 60.0);
    assert_eq!(again_after(7), 3600.0);
    assert_eq!(again_after(40), 3600.0);
}

#[test]
fn any_other_status_is_said_once_and_dropped() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    look(
        &mut runner,
        &people,
        None,
        vec![task_line(&people, 11, Kind::Question, "bo")],
        11,
    );
    let hands = Hands::default();
    hands.at(&mut runner, 0.0, &|| {
        Handed::Never("exit 1: refused".into())
    });
    hands.at(&mut runner, 1e9, &|| Handed::Sent);
    assert_eq!(hands.given.borrow().len(), 1);
    assert_eq!(hands.log.borrow().len(), 1);
    assert!(hands.log.borrow()[0].ends_with("for bo by email: not sent: exit 1: refused"));
    assert_eq!(runner.next_due(), None);
}

#[test]
fn a_restart_mid_retry_hands_the_notice_once_when_due_and_tells_nothing_again() {
    let tree = Tree::new();
    let people = people();
    let waits = || Some(vec![card(&people, "one", "ana")]);
    let lines = || vec![task_line(&people, 11, Kind::Verdict, "ana")];
    let mut first = started(&tree, &people, vec![]);
    look(&mut first, &people, waits(), lines(), 11);
    let hands = Hands::default();
    hands.at(&mut first, 0.0, &|| Handed::Later("down".into()));
    assert_eq!(first.next_due(), Some(60.0));
    drop(first);

    let (mut again, unread) = opened(&tree);
    assert_eq!(unread, None);
    look(&mut again, &people, waits(), lines(), 11);
    assert_eq!(
        record(&again).pending.len(),
        2,
        "the same two, none made again"
    );
    hands.at(&mut again, 30.0, &|| Handed::Sent);
    assert_eq!(hands.given.borrow().len(), 2, "not due yet");
    hands.at(&mut again, 60.0, &|| Handed::Sent);
    let read = hands.read();
    assert_eq!(read.len(), 4);
    assert_eq!(
        (&read[0].0, &read[1].0),
        (&read[2].0, &read[3].0),
        "the same ids"
    );
    assert_eq!(again.next_due(), None);
}

#[test]
fn a_hand_a_restart_cut_is_handed_again_at_once_with_the_same_id() {
    let tree = Tree::new();
    let people = people();
    let mut first = started(&tree, &people, vec![]);
    look(
        &mut first,
        &people,
        None,
        vec![task_line(&people, 11, Kind::Question, "ana")],
        11,
    );
    let hands = Hands::default();
    hands.at(&mut first, 0.0, &|| Handed::Cut);
    let kept = record(&first).pending[0].clone();
    assert!(kept.handing);
    assert_eq!(kept.hands, 1);
    drop(first);
    let (mut again, _) = opened(&tree);
    assert_eq!(again.next_due(), Some(0.0), "due at once");
    hands.at(&mut again, -5.0, &|| Handed::Sent);
    let read = hands.read();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].0, read[1].0);
    assert!(record(&again).pending.is_empty());
}

#[test]
fn a_notice_whose_way_address_or_program_moved_since_it_was_made_is_dropped_unhanded() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    let lines = vec![
        task_line(&people, 11, Kind::Question, "ana"),
        task_line(&people, 12, Kind::Question, "bo"),
    ];
    look(&mut runner, &people, None, lines, 12);
    let mut moved = profiles();
    moved.people[0]
        .chat
        .insert("slack".into(), json!("U0NEWNEW1"));
    let hands = Hands::default();
    let email = Programs::parse("email = \"/fake/email\"\n").unwrap();
    // bo's goes as made, by email; ana's address moved.
    hands.with(&mut runner, 0.0, &moved, &email, &|| Handed::Sent);
    assert_eq!(hands.given.borrow().len(), 1);
    let log = hands.log.borrow().clone();
    assert!(
        log[0].ends_with("for ana by slack: dropped: its person's way or address for it changed since it was made"),
        "{log:?}"
    );
    assert!(!log[0].contains("U0NEWNEW1") && !log[0].contains(ANA_SLACK));

    let second = Tree::new();
    let mut runner = started(&second, &people, vec![]);
    look(
        &mut runner,
        &people,
        None,
        vec![task_line(&people, 11, Kind::Question, "ana")],
        11,
    );
    let mut by_email = profiles();
    by_email.people[0]
        .preferences
        .insert("question".into(), json!("email"));
    let hands = Hands::default();
    hands.with(&mut runner, 0.0, &by_email, &programs(), &|| Handed::Sent);
    assert!(hands.given.borrow().is_empty(), "her way moved");
    let third = Tree::new();
    let mut runner = started(&third, &people, vec![]);
    look(
        &mut runner,
        &people,
        None,
        vec![task_line(&people, 11, Kind::Question, "ana")],
        11,
    );
    let hands = Hands::default();
    hands.with(&mut runner, 0.0, &profiles(), &email, &|| Handed::Sent);
    assert!(hands.given.borrow().is_empty());
    assert!(hands.log.borrow()[0].ends_with("dropped: no program is named for its way now"));
}

/// A look whose held comments did not read keeps what was told of them as it stands, so none is
/// told again once they read, while the cards it did read are told as ever; a first look without
/// them waits for one that has them.
#[test]
fn a_look_whose_held_comments_did_not_read_keeps_what_was_told_of_them() {
    let tree = Tree::new();
    let people = people();
    let (mut runner, _) = opened(&tree);
    let held = Waiting {
        key: format!("{HELD}4"),
        page: "/docs/design/palette.md#held-4".into(),
        owning: owned_by(&people, "design"),
        kind: Kind::Held,
        told: Some(vec!["bo".into()]),
    };
    let without = |runner: &mut Runner, cards: Vec<Waiting>| {
        let look = Look {
            people: &people,
            cards: Some(cards),
            held_read: false,
            lines: vec![],
            latest: 10,
        };
        let mut origin = || ORIGIN.to_string();
        (runner.look(&look, &profiles(), &programs(), 0.0, &mut origin)).unwrap();
    };
    without(&mut runner, vec![card(&people, "a", "design")]);
    assert_eq!(runner.cursor(), None, "a first look without them waits");
    look(&mut runner, &people, Some(vec![held.clone()]), vec![], 10);
    assert_eq!(runner.cursor(), Some(10), "the first look that has them");
    without(&mut runner, vec![card(&people, "a", "design")]);
    let told: Vec<(&str, &str)> = (record(&runner).pending.iter())
        .map(|notice| (notice.kind.as_str(), notice.person.as_str()))
        .collect();
    assert_eq!(told, [("card", "ana"), ("card", "bo")]);
    assert_eq!(
        record(&runner).cards.get(&held.key),
        Some(&["bo".to_string()].into()),
        "its spell stands"
    );
    look(
        &mut runner,
        &people,
        Some(vec![card(&people, "a", "design"), held]),
        vec![],
        10,
    );
    assert_eq!(record(&runner).pending.len(), 2, "nobody told again");
}

/// A held message's notice goes by the way each person chose for questions, as no profile chooses
/// one for it: made by it, handed as `held`, and dropped once that way moved, whatever the way
/// chosen for another kind. Who it tells is the line's alone, here ana and not bo.
#[test]
fn a_held_messages_notice_goes_by_the_way_chosen_for_questions_to_whom_its_line_names() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    let held = Line {
        id: 11,
        kind: Kind::Held,
        page: "/task/app/x#message-11".into(),
        told: vec!["ana".into()],
    };
    let mut by_kind = profiles();
    for (kind, way) in [("card", "email"), ("verdict", "email")] {
        by_kind.people[0]
            .preferences
            .insert(kind.into(), json!(way));
    }
    by_kind.people[0].emails = vec!["ana@example.com".into()];
    let look_at = Look {
        people: &people,
        cards: None,
        held_read: true,
        lines: vec![held.clone()],
        latest: 11,
    };
    let mut origin = || ORIGIN.to_string();
    (runner.look(&look_at, &by_kind, &programs(), 0.0, &mut origin)).unwrap();
    let pending = &record(&runner).pending;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        (
            pending[0].kind.as_str(),
            pending[0].person.as_str(),
            pending[0].way.as_str()
        ),
        ("held", "ana", "slack")
    );
    assert_eq!(
        pending[0].link,
        format!("{ORIGIN}/symphony/task/app/x#message-11")
    );
    let hands = Hands::default();
    hands.with(&mut runner, 0.0, &by_kind, &programs(), &|| Handed::Sent);
    let read = hands.read();
    assert_eq!(
        (read[0].1.as_str(), read[0].2.as_str(), read[0].3.as_str()),
        ("held", "slack", ANA_SLACK)
    );

    // Its question way moved after it was made: dropped, though no other kind's way moved.
    let second = Tree::new();
    let mut runner = started(&second, &people, vec![]);
    look(&mut runner, &people, None, vec![held], 11);
    let mut moved = profiles();
    (moved.people[0].preferences).insert("question".into(), json!("email"));
    let hands = Hands::default();
    hands.with(&mut runner, 0.0, &moved, &programs(), &|| Handed::Sent);
    assert!(hands.given.borrow().is_empty());
    assert!(
        hands.log.borrow()[0].contains("held for ana by slack: dropped"),
        "{:?}",
        hands.log.borrow()
    );
}

#[test]
fn a_board_that_names_nobody_sends_nothing() {
    let tree = Tree::new();
    let nobody = People::default();
    let owning = ownership::owning(&nobody, &Map::default(), Some("design"), &[]);
    let waiting = |slug: &str| {
        Waiting::card(
            format!("docs/a.md#{slug}"),
            format!("/docs/a.md#{slug}"),
            owning.clone(),
        )
    };
    let mut runner = started(&tree, &nobody, vec![waiting("one")]);
    let line = Line {
        id: 11,
        kind: Kind::Question,
        page: "/task/app/x".into(),
        told: ownership::told(&nobody, &owning),
    };
    look(
        &mut runner,
        &nobody,
        Some(vec![waiting("one"), waiting("two")]),
        vec![line],
        11,
    );
    assert!(record(&runner).pending.is_empty());
    assert_eq!(runner.next_due(), None);
}

#[test]
fn no_address_is_in_the_record_the_log_or_a_printed_runner() {
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    look(
        &mut runner,
        &people,
        Some(vec![card(&people, "one", "design")]),
        vec![],
        10,
    );
    let hands = Hands::default();
    hands.at(&mut runner, 0.0, &|| Handed::Later("busy".into()));
    let kept = std::fs::read_to_string(tree.path("artifacts/notices.json")).unwrap();
    let printed = format!("{runner:?}");
    let log = hands.log.borrow().join("\n");
    assert_eq!(
        hands.given.borrow().len(),
        2,
        "both were handed their address"
    );
    for address in [ANA_SLACK, BO_EMAIL] {
        assert!(!kept.contains(address), "the record: {kept}");
        assert!(!printed.contains(address), "a printed runner: {printed}");
        assert!(!log.contains(address), "the log: {log}");
    }
    assert!(
        kept.contains(&key(&tree).digest(ANA_SLACK)),
        "its digest is kept"
    );
    // A digest with no key is read back by trying every address: it is not what is kept.
    let unkeyed: String = (<sha2::Sha256 as sha2::Digest>::digest(ANA_SLACK.as_bytes()).iter())
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(!kept.contains(&unkeyed));
}

#[test]
fn a_record_that_does_not_read_is_said_and_made_anew_at_the_first_look() {
    let tree = Tree::new();
    tree.write("artifacts/notices.json", "{ not json");
    let (mut runner, unread) = opened(&tree);
    let why = unread.unwrap();
    assert!(why.contains("notices.json"), "{why}");
    let people = people();
    look(
        &mut runner,
        &people,
        Some(vec![card(&people, "one", "design")]),
        vec![],
        4,
    );
    assert_eq!(runner.cursor(), Some(4));
    assert!(
        record(&runner).pending.is_empty(),
        "made anew as at a first look"
    );
}

#[test]
fn a_record_that_cannot_be_written_starts_no_program_and_uses_up_no_hand() {
    use std::os::unix::fs::PermissionsExt as _;
    let tree = Tree::new();
    let people = people();
    let mut runner = started(&tree, &people, vec![]);
    look(
        &mut runner,
        &people,
        None,
        vec![task_line(&people, 11, Kind::Question, "ana")],
        11,
    );
    let artifacts = tree.path("artifacts");
    let mode = |bits: u32| {
        std::fs::set_permissions(&artifacts, std::fs::Permissions::from_mode(bits)).unwrap();
    };
    mode(0o500);
    let hands = Hands::default();
    for now in [0.0, 1.0, 2.0] {
        let why = hands.tried(&mut runner, now).unwrap_err();
        assert!(why.contains("notices.json"), "{why}");
    }
    mode(0o700);
    assert!(hands.given.borrow().is_empty(), "no program started");
    let kept = &record(&runner).pending[0];
    assert_eq!((kept.hands, kept.handing), (0, false), "no hand used up");
    hands.tried(&mut runner, 3.0).unwrap();
    assert_eq!(hands.given.borrow().len(), 1);
    assert!(record(&runner).pending.is_empty());
}

#[test]
fn one_runner_at_a_time_holds_a_projects_record() {
    let tree = Tree::new();
    let people = people();
    let first = started(&tree, &people, vec![]);
    let Err(why) = Runner::open(&tree.path(""), "symphony", key(&tree)) else {
        panic!("a second runner opened the record the first holds");
    };
    assert!(
        why.contains("notices.lock is held by another server's notices"),
        "{why}"
    );
    drop(first);
    let (again, _) = opened(&tree);
    assert_eq!(again.cursor(), Some(10), "the record as the first left it");
}
