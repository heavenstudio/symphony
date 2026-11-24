//! Posts under a key (`Store::say_keyed`): made once per key on a thread, whatever a later post
//! under it says, once when two come at once, said once to be unread by any executor, whichever
//! post under the key finds that, and taking the thread's line made under no key when asked to
//! (`Store::adopt_or_say`), with the board's notice of it said under no key.

use crate::common::{Fixture, Seed};
use serde_json::json;
use symphony_board::health::LIVE_SECONDS;

/// A post under a key is made once on a thread: every later one under the key answers the line the
/// first made, whatever it says, and writes nothing; another key or another thread is another post.
#[test]
fn a_keyed_post_is_made_once_per_key_on_a_thread() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    f.task("sim/dock");
    let first = f
        .store
        .say_keyed(
            "sim/jump",
            &a,
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    f.advance(5.0);
    let again = f
        .store
        .say_keyed(
            "sim/jump",
            &a,
            "main is red, said again",
            None,
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(again, first);
    let other_key = f
        .store
        .say_keyed("sim/jump", &a, "main is red", None, "main-red:5a1d")
        .unwrap();
    let other_thread = f
        .store
        .say_keyed("sim/dock", &a, "main is red", None, "main-red:3e0c")
        .unwrap();
    let unkeyed = f.store.say("sim/jump", &a, "main is red", None).unwrap();
    assert!(first.id < other_key.id && other_key.id < other_thread.id);
    assert!(other_thread.id < unkeyed.id);
    // The sender's own lines; the board's one post says no executor read the first, and the
    // repeat under its key finds that said and adds nothing.
    let lines = |slug: &str| -> Vec<(i64, String, Option<String>)> {
        f.store
            .thread(slug)
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == "msg" && m.sender == a)
            .map(|m| (m.id, m.text, m.recipient))
            .collect()
    };
    let by_board = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .filter(|m| m.sender == "board")
        .count();
    assert_eq!(by_board, 1);
    assert_eq!(
        lines("sim/jump"),
        vec![
            (first.id, "main is red".into(), Some("@executor".into())),
            (other_key.id, "main is red".into(), None),
            (unkeyed.id, "main is red".into(), None),
        ]
    );
    assert_eq!(
        lines("sim/dock"),
        vec![(other_thread.id, "main is red".into(), None)]
    );
}

/// Two posts under one key at once make one line: the one that commits second finds the first's.
#[test]
fn two_posts_under_one_key_at_once_make_one_line() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    let barrier = std::sync::Barrier::new(2);
    let (first, second) = (f.other(), f.other());
    let said = std::thread::scope(|scope| {
        let barrier = &barrier;
        let x = scope.spawn(|| {
            barrier.wait();
            first.say_keyed("sim/jump", &a, "one", None, "main-red:3e0c")
        });
        let y = scope.spawn(|| {
            barrier.wait();
            second.say_keyed("sim/jump", &a, "two", None, "main-red:3e0c")
        });
        [x.join().unwrap().unwrap(), y.join().unwrap().unwrap()]
    });
    assert_eq!(said[0], said[1]);
    let lines: Vec<String> = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "msg")
        .map(|m| m.text)
        .collect();
    assert_eq!(lines, vec![said[0].text.clone()]);
}

/// The board says once that no executor reads a keyed line, whichever post under the key finds
/// that: a repeat after the first post's run stopped before saying so, or after the executor went,
/// says it, and a repeat after it was said adds nothing.
#[test]
fn a_repeated_keyed_post_says_once_that_no_executor_reads_it() {
    let f = Fixture::new(
        Seed::default()
            .session("ochre-owl")
            .session("amber-heron")
            .task("sim/jump", "in_progress")
            .role("sim/jump", "ochre-owl", "orchestrator")
            .role("sim/jump", "amber-heron", "executor"),
    );
    let told = || -> Vec<String> {
        f.store
            .inbox("ochre-owl")
            .unwrap()
            .into_iter()
            .filter(|m| m.sender == "board")
            .map(|m| m.text)
            .collect()
    };
    // The executor is live at the first post and stale with no callback by the repeat.
    let first = f
        .store
        .say_keyed(
            "sim/jump",
            "ochre-owl",
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(told(), Vec::<String>::new());
    f.advance(LIVE_SECONDS + 1.0);
    let again = f
        .store
        .say_keyed(
            "sim/jump",
            "ochre-owl",
            "main is red",
            None,
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(again, first);
    let notice = format!(
        "#{} from ochre-owl to @executor reaches no live executor: amber-heron is stale with no callback",
        first.id
    );
    assert_eq!(told(), vec![notice.clone()]);
    f.store
        .say_keyed(
            "sim/jump",
            "ochre-owl",
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(told(), vec![notice]);
}

/// Asked to adopt, a key naming no line yet takes the thread's line reading its text to its
/// recipient, one made under no key, and the board's notice of that line: nothing is written, a
/// later post under the key answers the line, and a key not asked to, or asked with a text the
/// thread lacks, makes its own line.
#[test]
fn a_post_asked_to_adopt_takes_the_threads_line_reading_it_and_the_notice_of_it() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    let thread = || -> (Vec<i64>, usize) {
        let lines = f.store.thread("sim/jump").unwrap();
        (
            lines
                .iter()
                .filter(|m| m.sender == a)
                .map(|m| m.id)
                .collect(),
            lines.iter().filter(|m| m.sender == "board").count(),
        )
    };
    // Under no key, to a task nobody executes: the board says once, under the line's own key
    // (`unread:<id>`), that no executor reads it.
    let old = f
        .store
        .say("sim/jump", &a, "main is red", Some("@executor"))
        .unwrap();
    assert_eq!(thread(), (vec![old.id], 1));
    let taken = f
        .store
        .adopt_or_say(
            "sim/jump",
            &a,
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(taken, old);
    let again = f
        .store
        .say_keyed("sim/jump", &a, "main is red, again", None, "main-red:3e0c")
        .unwrap();
    assert_eq!(again, old);
    assert_eq!(thread(), (vec![old.id], 1));
    let own = f
        .store
        .say_keyed(
            "sim/jump",
            &a,
            "main is red",
            Some("@executor"),
            "main-red:5a1d",
        )
        .unwrap();
    let other = f
        .store
        .adopt_or_say(
            "sim/jump",
            &a,
            "main is red again",
            Some("@executor"),
            "main-red:7c2e",
        )
        .unwrap();
    assert!(old.id < own.id && own.id < other.id);
    assert_eq!(thread(), (vec![old.id, own.id, other.id], 3));
}

/// A post that took a line made under no key says once that no executor reads it when nothing
/// said so yet: the executor was live at the line's post and is stale by the taking.
#[test]
fn a_post_that_takes_a_line_says_once_that_no_executor_reads_it() {
    let f = Fixture::new(
        Seed::default()
            .session("ochre-owl")
            .session("amber-heron")
            .task("sim/jump", "in_progress")
            .role("sim/jump", "ochre-owl", "orchestrator")
            .role("sim/jump", "amber-heron", "executor"),
    );
    let told = || -> Vec<String> {
        f.store
            .inbox("ochre-owl")
            .unwrap()
            .into_iter()
            .filter(|m| m.sender == "board")
            .map(|m| m.text)
            .collect()
    };
    let old = f
        .store
        .say("sim/jump", "ochre-owl", "main is red", Some("@executor"))
        .unwrap();
    assert_eq!(told(), Vec::<String>::new());
    f.advance(LIVE_SECONDS + 1.0);
    let taken = f
        .store
        .adopt_or_say(
            "sim/jump",
            "ochre-owl",
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(taken, old);
    let notice = format!(
        "#{} from ochre-owl to @executor reaches no live executor: amber-heron is stale with no callback",
        old.id
    );
    assert_eq!(told(), vec![notice.clone()]);
    f.store
        .say_keyed(
            "sim/jump",
            "ochre-owl",
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(told(), vec![notice]);
}

/// A post that takes a line made under no key takes the board's notice of it said under no key
/// too: the key naming the line's notice (`unread:<id>`) takes the thread's earliest line of the
/// board saying the line reaches no live executor, so neither the taking nor a repeat under the
/// key says it again.
#[test]
fn a_post_that_takes_a_line_takes_the_notice_said_of_it_under_no_key() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    let by = |sender: &str| -> Vec<(i64, String)> {
        f.store
            .thread("sim/jump")
            .unwrap()
            .into_iter()
            .filter(|m| m.sender == sender)
            .map(|m| (m.id, m.text))
            .collect()
    };
    // The line to the task's executor and the board's notice that nobody reads it, both made
    // under no key.
    let seed = |sender: &str, to: &str, text: &str| {
        let counter = f.store.raw().row("counter", "message");
        let sid = counter.field("last").unwrap().as_i64().unwrap_or(0) + 1;
        counter.set(json!({"last": sid, "ts": f.now()})).unwrap();
        f.store
            .raw()
            .row("message", json!(["sim/jump", sid]))
            .create(json!({"slug":"sim/jump", "sid":sid, "sender":sender,
                "recipient":to, "kind":"msg", "text":text, "ts":f.now()}))
            .unwrap();
    };
    seed(&a, "@executor", "main is red");
    let old = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .find(|m| m.sender == a)
        .unwrap();
    let notice = format!(
        "#{} from {a} to @executor reaches no live executor: no session holds the role",
        old.id
    );
    seed("board", "@orchestrator", &notice);
    assert_eq!(by("board"), vec![(old.id + 1, notice.clone())]);
    let taken = f
        .store
        .adopt_or_say(
            "sim/jump",
            &a,
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(taken, old);
    let again = f
        .store
        .say_keyed(
            "sim/jump",
            &a,
            "main is red",
            Some("@executor"),
            "main-red:3e0c",
        )
        .unwrap();
    assert_eq!(again, old);
    assert_eq!(by(&a), vec![(old.id, "main is red".into())]);
    assert_eq!(by("board"), vec![(old.id + 1, notice)]);
}
