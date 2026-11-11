//! Priority edits use the real store and a scratch database for each case.

use super::common::{Fixture, Seed, refused};
use symphony_board::NewTask;

#[test]
fn every_priority_is_recorded_with_its_actor_and_previous_value_only() {
    let f = Fixture::new(
        Seed::default()
            .session("brisk-finch")
            .session("amber-heron"),
    );
    f.task("sim/parent");
    f.task("sim/dependency");
    f.task_with(NewTask {
        body: "Keep this brief".into(),
        parent: Some("sim/parent".into()),
        instruction: true,
        links: vec!["docs/workflow/TASKS.md".into()],
        depends: vec!["sim/dependency".into()],
        ..NewTask::new("sim/jump", "Jump departure", "owner")
    });
    f.store
        .take_role("sim/jump", "amber-heron", "orchestrator")
        .unwrap();
    let mut expected = f.store.get("sim/jump").unwrap();
    let parent = f.store.get("sim/parent").unwrap();
    let dependency = f.store.get("sim/dependency").unwrap();
    let mut history_len = f.store.thread("sim/jump").unwrap().len();
    for priority in [0, 4, 1, 3, 2, 2] {
        f.advance(1.0);
        let old = expected.priority;
        expected.priority = priority;
        assert_eq!(
            f.store
                .set_priority("sim/jump", priority, "brisk-finch")
                .unwrap(),
            expected
        );
        assert_eq!(f.store.get("sim/jump").unwrap(), expected);
        let history = f.store.thread("sim/jump").unwrap();
        history_len += 1;
        assert_eq!(history.len(), history_len);
        let line = history.last().unwrap();
        assert_eq!(line.sender, "brisk-finch");
        assert_eq!(line.kind, "system");
        assert_eq!(line.recipient, None);
        assert_eq!(
            line.text,
            format!("brisk-finch changed priority p{old} → p{priority}")
        );
    }
    assert_eq!(f.store.get("sim/parent").unwrap(), parent);
    assert_eq!(f.store.get("sim/dependency").unwrap(), dependency);
}

#[test]
fn invalid_priorities_missing_tasks_and_invalid_actors_leave_the_board_unchanged() {
    let f = Fixture::new(
        Seed::default()
            .session("brisk-finch")
            .session("amber-heron"),
    );
    f.task("sim/jump");
    f.store.leave("amber-heron").unwrap();
    let before = f.store.dump().unwrap();
    f.advance(10.0);
    for priority in [i64::MIN, -1, 5, i64::MAX] {
        assert_eq!(
            refused(f.store.set_priority("sim/jump", priority, "brisk-finch")),
            "priority runs 0 (most urgent) to 4"
        );
        assert_eq!(f.store.dump().unwrap(), before);
    }
    for (slug, actor, why) in [
        ("sim/missing", "brisk-finch", "no task 'sim/missing'"),
        (
            "sim/jump",
            "ghost-owl",
            "no session 'ghost-owl'; register first (`just tasks register`)",
        ),
        (
            "sim/jump",
            "amber-heron",
            "session amber-heron has left; attach again",
        ),
    ] {
        assert_eq!(refused(f.store.set_priority(slug, 0, actor)), why);
        assert_eq!(f.store.dump().unwrap(), before);
    }
}

#[test]
fn the_owner_changes_priority_without_a_session_and_refusals_still_change_nothing() {
    let f = Fixture::empty();
    let mut expected = f.task("sim/jump");
    f.advance(1.0);
    expected.priority = 0;
    assert_eq!(
        f.store.set_priority("sim/jump", 0, "owner").unwrap(),
        expected
    );
    assert!(f.store.dump().unwrap().sessions.is_empty());
    let history = f.store.thread("sim/jump").unwrap();
    let line = history.last().unwrap();
    assert_eq!(line.sender, "owner");
    assert_eq!(line.kind, "system");
    assert_eq!(line.text, "owner changed priority p2 → p0");
    let before = f.store.dump().unwrap();
    for priority in [-1, 5] {
        assert_eq!(
            refused(f.store.set_priority("sim/jump", priority, "owner")),
            "priority runs 0 (most urgent) to 4"
        );
        assert_eq!(f.store.dump().unwrap(), before);
    }
    assert_eq!(
        refused(f.store.set_priority("sim/missing", 0, "owner")),
        "no task 'sim/missing'"
    );
    assert_eq!(f.store.dump().unwrap(), before);
}

#[test]
fn concurrent_edits_record_the_values_they_actually_replace() {
    let f = Fixture::new(
        Seed::default()
            .session("brisk-finch")
            .session("amber-heron"),
    );
    f.task("sim/jump");
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let first = f.other();
        let second = f.other();
        let barrier = &barrier;
        let a = scope.spawn(move || {
            barrier.wait();
            first.set_priority("sim/jump", 0, "brisk-finch").unwrap()
        });
        let b = scope.spawn(move || {
            barrier.wait();
            second.set_priority("sim/jump", 4, "amber-heron").unwrap()
        });
        assert_eq!(a.join().unwrap().priority, 0);
        assert_eq!(b.join().unwrap().priority, 4);
    });
    let history = f.store.thread("sim/jump").unwrap();
    assert_eq!(history.len(), 3);
    let final_priority = f.store.get("sim/jump").unwrap().priority;
    let (first, last) = match final_priority {
        0 => (("amber-heron", 4), ("brisk-finch", 0)),
        4 => (("brisk-finch", 0), ("amber-heron", 4)),
        other => panic!("neither edit survived: p{other}"),
    };
    assert_eq!(
        history[1].text,
        format!("{} changed priority p2 → p{}", first.0, first.1)
    );
    assert_eq!(
        history[2].text,
        format!("{} changed priority p{} → p{}", last.0, first.1, last.1)
    );
}

#[test]
fn many_rounds_of_four_writers_keep_the_entire_history_chain() {
    const WRITERS: [(&str, i64); 4] = [
        ("brisk-finch", 0),
        ("amber-heron", 1),
        ("calm-owl", 3),
        ("dusky-kestrel", 4),
    ];
    let mut seed = Seed::default();
    for (actor, _) in WRITERS {
        seed = seed.session(actor);
    }
    let f = Fixture::new(seed);
    f.task("sim/jump");
    for _ in 0..20 {
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            for (actor, priority) in WRITERS {
                let store = f.other();
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    assert_eq!(
                        store
                            .set_priority("sim/jump", priority, actor)
                            .unwrap()
                            .priority,
                        priority
                    );
                });
            }
        });
    }
    let history = f.store.thread("sim/jump").unwrap();
    assert_eq!(history.len(), 81);
    let mut previous = 2;
    for line in &history[1..] {
        assert_eq!(line.kind, "system");
        let prefix = format!("{} changed priority p", line.sender);
        let (old, new) = line
            .text
            .strip_prefix(&prefix)
            .unwrap()
            .split_once(" → p")
            .unwrap();
        assert_eq!(old.parse::<i64>().unwrap(), previous, "{}", line.text);
        let expected = WRITERS
            .iter()
            .find(|(actor, _)| *actor == line.sender)
            .unwrap()
            .1;
        previous = new.parse().unwrap();
        assert_eq!(previous, expected, "{}", line.text);
    }
    assert_eq!(f.store.get("sim/jump").unwrap().priority, previous);
}

#[test]
fn a_priority_edit_sends_no_callback_to_a_listening_orchestrator() {
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    struct Socket {
        listener: UnixListener,
        path: std::path::PathBuf,
    }

    impl Drop for Socket {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn envelopes(listener: &UnixListener) -> String {
        let mut all = String::new();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Dispatch has returned and closed every probe/delivery connection.
                    stream.set_nonblocking(true).unwrap();
                    stream.read_to_string(&mut all).unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return all,
                Err(error) => panic!("callback listener: {error}"),
            }
        }
    }

    let f = Fixture::empty();
    let actor = f.session("worker");
    let watcher = f.session("orchestrator");
    f.task("sim/jump");
    f.store
        .take_role("sim/jump", &watcher, "orchestrator")
        .unwrap();
    let path = std::path::PathBuf::from("/tmp").join(format!(
        "sf-priority-{}-{}.sock",
        std::process::id(),
        f.other_client().database()
    ));
    let socket = Socket {
        listener: UnixListener::bind(&path).unwrap(),
        path,
    };
    socket.listener.set_nonblocking(true).unwrap();
    f.store
        .callback(
            &watcher,
            Some(&format!("uds:{}", socket.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store
        .say("sim/jump", &actor, "listener is receiving", Some(&watcher))
        .unwrap();
    assert!(envelopes(&socket.listener).contains("listener is receiving"));
    f.store.set_priority("sim/jump", 0, &actor).unwrap();
    assert!(envelopes(&socket.listener).is_empty());
    assert!(f.store.callbacks().unwrap().contains_key(&watcher));
    assert!(f.store.callback_inbox(&watcher).unwrap().is_empty());
}
