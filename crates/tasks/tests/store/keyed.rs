//! Posts under a key (`Store::say_keyed`): made once per key on a thread, whatever a later post
//! under it says, and once when two come at once.

use crate::common::Fixture;

/// A post under a key is made once on a thread: every later one under the key answers the line the
/// first made, whatever it says, and writes nothing; another key or another thread is another post.
#[test]
fn a_keyed_post_is_made_once_per_key_on_a_thread() {
    let f = Fixture::empty();
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
    // repeat under its key says nothing of it again.
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
    let f = Fixture::empty();
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
