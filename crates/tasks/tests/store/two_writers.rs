//! Two writers meeting on one task: each one's lines and priorities are all
//! there, every line numbered once and in the order they committed, and no write is lost, each
//! priority line naming the priority the one before it set.

use crate::common::{Fixture, Seed};
use symphony_board::{Message, Store};

/// Each writer's lines and priorities.
const ROUNDS: usize = 12;

/// `rounds` lines said and priorities set on `tools/one` by `actor` through `store`, its lines
/// named `name`.
fn write(store: &Store, actor: &str, name: &str, rounds: usize) {
    for round in 0..rounds {
        let line = format!("{name} {round}");
        store.say("tools/one", actor, &line, None).unwrap();
        let priority = i64::try_from((round + name.len()) % 5).unwrap();
        store.set_priority("tools/one", priority, actor).unwrap();
    }
}

/// The priority lines' `pWAS → pNEW`, as numbers, in the thread's order.
fn priorities(thread: &[Message]) -> Vec<(i64, i64)> {
    (thread.iter())
        .filter_map(|line| line.text.split_once(" changed priority p"))
        .map(|(_, change)| {
            let (was, new) = change.split_once(" → p").unwrap();
            (was.parse().unwrap(), new.parse().unwrap())
        })
        .collect()
}

#[test]
fn two_writers_on_one_task_lose_no_write_and_number_each_line_once() {
    let f = Fixture::new(Seed::default().task("tools/one", "ready"));
    let handle = f.session("worker");
    let before = f.store.thread("tools/one").unwrap().len();
    let (owner, session) = (f.other(), f.other());
    std::thread::scope(|scope| {
        scope.spawn(|| write(&owner, "owner", "first", ROUNDS));
        scope.spawn(|| write(&session, &handle, "second", ROUNDS));
    });

    let thread = f.store.thread("tools/one").unwrap();
    assert_eq!(
        thread.len(),
        before + 4 * ROUNDS,
        "every line is on the thread"
    );
    let numbers: Vec<i64> = thread.iter().map(|line| line.id).collect();
    assert!(
        numbers.windows(2).all(|pair| pair[0] < pair[1]),
        "each line is numbered once, in order: {numbers:?}"
    );
    for name in ["first", "second"] {
        let said: Vec<&str> = (thread.iter())
            .map(|line| line.text.as_str())
            .filter(|text| text.starts_with(&format!("{name} ")))
            .collect();
        let wanted: Vec<String> = (0..ROUNDS).map(|round| format!("{name} {round}")).collect();
        assert_eq!(said, wanted, "{name}'s lines are all there, in its order");
    }
    let changes = priorities(&thread);
    assert_eq!(changes.len(), 2 * ROUNDS);
    for pair in changes.windows(2) {
        assert_eq!(
            pair[1].0, pair[0].1,
            "no priority write is lost: {changes:?}"
        );
    }
    let last = changes.last().unwrap().1;
    assert_eq!(f.store.get("tools/one").unwrap().priority, last);
}
