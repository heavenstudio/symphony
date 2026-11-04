//! `StoreTest`: the store's writes and reads, on a clock the test moves.

use std::collections::BTreeMap;
use std::path::PathBuf;

use symphony::asks::{Answer, AskError, AskStore, MAX_TEXT};

use super::common::Tree;
use super::{Clock, DAY, store_on};

struct Fixture {
    tree: Tree,
    clock: Clock,
    store: AskStore,
}

fn fixture() -> Fixture {
    let tree = Tree::new();
    let clock = Clock::new();
    let store = store_on(&tree.path("asks/asks.db"), &clock);
    Fixture { tree, clock, store }
}

fn refused<T: std::fmt::Debug>(result: Result<T, AskError>) -> bool {
    matches!(result, Err(AskError::Refused(_)))
}

fn ids(threads: &[symphony::asks::Thread]) -> Vec<i64> {
    threads.iter().map(|t| t.id).collect()
}

#[test]
fn the_round_trip_from_question_to_answer() {
    let f = fixture();
    let ask_id = f
        .store
        .ask(
            "docs/A.md",
            "why five bands?",
            "stations",
            "five size bands",
        )
        .unwrap();
    let thread = f.store.thread(ask_id).unwrap();
    assert_eq!(
        (
            thread.id,
            thread.path.as_str(),
            thread.anchor.as_str(),
            thread.quote.as_str()
        ),
        (ask_id, "docs/A.md", "stations", "five size bands")
    );
    assert_eq!(
        (
            thread.question.as_str(),
            thread.asked,
            thread.status.as_str(),
            thread.answers.len()
        ),
        ("why five bands?", f.clock.now(), "open", 0)
    );
    f.clock.add(30.0);
    f.store
        .answer(ask_id, "Three would do; see [[B#Bands]].", "amber-heron")
        .unwrap();
    let thread = f.store.thread(ask_id).unwrap();
    assert_eq!(thread.status, "answered");
    assert_eq!(
        thread.answers,
        vec![Answer {
            text: "Three would do; see [[B#Bands]].".into(),
            by: "amber-heron".into(),
            at: f.clock.now(),
        }]
    );
    let kinds: Vec<&str> = thread.messages.iter().map(|m| m.kind.as_str()).collect();
    assert_eq!(kinds, ["question", "answer"]);
}

#[test]
fn anchor_and_quote_are_optional() {
    let f = fixture();
    let thread = f
        .store
        .thread(
            f.store
                .ask("docs/A.md", "what is this doc for?", "", "")
                .unwrap(),
        )
        .unwrap();
    assert_eq!((thread.anchor.as_str(), thread.quote.as_str()), ("", ""));
}

#[test]
fn a_follow_up_reopens_the_ask_in_the_same_thread() {
    let f = fixture();
    let ask_id = f.store.ask("docs/A.md", "why?", "", "").unwrap();
    f.store.answer(ask_id, "because", "agent").unwrap();
    f.clock.add(5.0);
    f.store.reply(ask_id, "and the carriers?").unwrap();
    let thread = f.store.thread(ask_id).unwrap();
    assert_eq!(thread.status, "open");
    let said: Vec<(&str, &str)> = thread
        .messages
        .iter()
        .map(|m| (m.kind.as_str(), m.text.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("question", "why?"),
            ("answer", "because"),
            ("question", "and the carriers?")
        ]
    );
    // the thread keeps its first question as its title
    assert_eq!(thread.question, "why?");
    assert_eq!(ids(&f.store.open_asks().unwrap()), [ask_id]);
    f.store.answer(ask_id, "those too", "agent").unwrap();
    assert!(f.store.open_asks().unwrap().is_empty());
}

#[test]
fn open_asks_are_oldest_first_and_for_doc_is_newest_first() {
    let f = fixture();
    let first = f.store.ask("docs/A.md", "one", "", "").unwrap();
    f.clock.add(1.0);
    let second = f.store.ask("docs/A.md", "two", "", "").unwrap();
    f.clock.add(1.0);
    let other = f.store.ask("docs/B.md", "elsewhere", "", "").unwrap();
    assert_eq!(ids(&f.store.open_asks().unwrap()), [first, second, other]);
    assert_eq!(ids(&f.store.for_doc("docs/A.md").unwrap()), [second, first]);
    assert_eq!(ids(&f.store.all().unwrap()), [other, second, first]);
}

#[test]
fn asks_a_day_old_are_pruned_on_the_next_write() {
    let f = fixture();
    let old = f.store.ask("docs/A.md", "stale", "", "").unwrap();
    f.clock.add(DAY - 1.0);
    let young = f.store.ask("docs/A.md", "fresh", "", "").unwrap();
    let mut both = ids(&f.store.all().unwrap());
    both.sort_unstable();
    assert_eq!(both, [old, young]);
    f.clock.add(2.0);
    f.store.answer(young, "still here", "agent").unwrap();
    assert_eq!(ids(&f.store.all().unwrap()), [young]);
    assert!(refused(f.store.thread(old)));
}

#[test]
fn activity_keeps_a_thread_alive() {
    let f = fixture();
    let ask_id = f.store.ask("docs/A.md", "long talk", "", "").unwrap();
    f.clock.add(DAY - 10.0);
    f.store.reply(ask_id, "still going").unwrap();
    f.clock.add(20.0);
    assert_eq!(f.store.prune(symphony::asks::MAX_AGE).unwrap(), 0);
    assert_eq!(f.store.thread(ask_id).unwrap().status, "open");
}

#[test]
fn prune_takes_a_max_age_and_says_how_many_went() {
    let f = fixture();
    f.store.ask("docs/A.md", "a", "", "").unwrap();
    f.store.ask("docs/A.md", "b", "", "").unwrap();
    f.clock.add(61.0);
    assert_eq!(f.store.prune(60.0).unwrap(), 2);
    assert!(f.store.all().unwrap().is_empty());
}

#[test]
fn a_path_that_is_no_relative_markdown_route_is_refused() {
    let f = fixture();
    for path in [
        "docs/../secret.md",
        "/docs/A.md",
        "docs/A.txt",
        "",
        "docs/sub/../../x.md",
        "docs//A.md",
        "./A.md",
    ] {
        assert!(refused(f.store.ask(path, "hi", "", "")), "{path:?}");
    }
    assert!(f.store.all().unwrap().is_empty());
    // which entrypoint serves a route is the project's to say: at one on `/`, `index.md` is a page
    f.store.ask("index.md", "hi", "", "").unwrap();
    assert_eq!(f.store.all().unwrap().len(), 1);
}

#[test]
fn an_empty_or_overlong_question_is_refused() {
    let f = fixture();
    let long = "x".repeat(MAX_TEXT + 1);
    for question in ["", "   \n  ", long.as_str()] {
        assert!(
            refused(f.store.ask("docs/A.md", question, "", "")),
            "{} characters",
            question.len()
        );
    }
    let ask_id = f.store.ask("docs/A.md", "  x  ", "", "").unwrap();
    assert_eq!(f.store.thread(ask_id).unwrap().question, "x");
    let long = "y".repeat(MAX_TEXT + 1);
    assert!(refused(f.store.reply(ask_id, " ")));
    assert!(refused(f.store.answer(ask_id, "", "agent")));
    assert!(refused(f.store.answer(ask_id, &long, "agent")));
}

#[test]
fn an_unknown_ask_is_refused() {
    let f = fixture();
    assert!(refused(f.store.answer(99, "x", "agent")));
    assert!(refused(f.store.reply(99, "x")));
    assert!(refused(f.store.thread(99)));
}

#[test]
fn questions_since_lists_asks_and_follow_ups_but_not_answers() {
    let f = fixture();
    let ask_id = f.store.ask("docs/A.md", "one", "alpha", "").unwrap();
    f.store.answer(ask_id, "answered", "agent").unwrap();
    f.store.reply(ask_id, "two").unwrap();
    let other = f.store.ask("docs/B.md", "three", "", "").unwrap();
    let questions = f.store.questions_since(0).unwrap();
    let said: Vec<(i64, &str)> = questions.iter().map(|q| (q.ask, q.text.as_str())).collect();
    assert_eq!(said, [(ask_id, "one"), (ask_id, "two"), (other, "three")]);
    assert_eq!(
        f.store.questions_since(questions[1].seq).unwrap(),
        questions[2..]
    );
}

#[test]
fn nothing_under_docs_changes() {
    let f = fixture();
    f.tree.write("docs/A.md", "# A\n");
    let snapshot = || -> BTreeMap<PathBuf, Vec<u8>> {
        std::fs::read_dir(f.tree.path("docs"))
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect()
    };
    let before = snapshot();
    let ask_id = f.store.ask("docs/A.md", "q", "", "").unwrap();
    f.store.answer(ask_id, "a", "agent").unwrap();
    f.store.reply(ask_id, "q2").unwrap();
    assert_eq!(snapshot(), before);
}
