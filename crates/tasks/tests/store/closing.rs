//! A close's writes to a task session's record (`close`, `launch::close`), each bound to the record
//! the close read: the launch and the terminal it names. A record changed since (restored into
//! another terminal, launched again, closed by another close) is left as it is, and nothing is said.
//! And the session's hold, which a close and a restore take for their whole run, and which each of
//! their writes carries.

use crate::common::{Fixture, refused};
use crate::launches::launch;
use symphony_board::NewLaunch;
use symphony_board::rows::TaskSessionRow;

/// The closure lines of sim/jump's thread from the `from`th on.
fn closures(fixture: &Fixture, from: usize) -> Vec<String> {
    fixture.said("sim/jump")[from..]
        .iter()
        .filter(|line| line.starts_with("closed the pane"))
        .cloned()
        .collect()
}

/// sim/jump's one record, as it is now.
fn current(fixture: &Fixture) -> TaskSessionRow {
    let [record] = <[_; 1]>::try_from(fixture.store.task_sessions("sim/jump").unwrap()).unwrap();
    record
}

#[test]
fn a_bound_close_closes_only_the_launch_in_the_terminal_it_read_and_only_once() {
    let fixture = Fixture::new(crate::common::Seed::default());
    fixture.task("sim/jump");
    let read = launch(&fixture, "sim/jump", "executor");
    let handle = &read.handle;
    let restore = format!("just tasks restore sim/jump {handle}");
    let before = fixture.said("sim/jump").len();
    let hold = fixture.store.take_session(handle, "a close by o").unwrap();
    let close = |record, why| {
        fixture
            .store
            .close_task_session_as(record, why, &restore, hold)
            .unwrap()
    };
    // Restored into another terminal since: not the session read.
    fixture
        .store
        .reopen_task_session("sim/jump", handle, "t10")
        .unwrap();
    let closed = close(&read, "closed by o");
    assert_eq!((closed, current(&fixture).closed), (false, None));
    // Launched again since, in the same terminal: not the launch read either.
    fixture.advance(5.0);
    let again = fixture
        .store
        .record_launch(&NewLaunch {
            pane: "t9".into(),
            ..NewLaunch::new("sim/jump", handle, "executor")
        })
        .unwrap();
    let closed = close(&read, "closed by o");
    assert_eq!((closed, current(&fixture).closed), (false, None));
    assert_eq!(closures(&fixture, before), Vec::<String>::new());
    // The launch read now closes, once.
    let (first, second) = (close(&again, "closed by o"), close(&again, "closed by p"));
    let now = current(&fixture);
    assert_eq!(
        (first, second, now.closed, now.closed_why.as_str()),
        (true, false, Some(fixture.now()), "closed by o")
    );
    assert_eq!(
        closures(&fixture, before),
        [format!(
            "closed the pane of {handle} (closed by o); restore: {restore}"
        )]
    );
}

#[test]
fn a_session_is_held_by_one_close_or_restore_until_it_is_released_or_its_hold_lapses() {
    let fixture = Fixture::new(crate::common::Seed::default());
    let store = &fixture.store;
    let handle = fixture.session("worker");
    let other = fixture.session("worker");
    let held_by = |doing: &str, left: u32| {
        format!("{doing} of {handle} is under way: its hold lapses in {left} s")
    };
    let close = store.take_session(&handle, "a close by orch-1").unwrap();
    assert_eq!(
        refused(store.take_session(&handle, "a restore")),
        held_by("a close by orch-1", 300)
    );
    // Another session is another hold.
    store.take_session(&other, "a restore").unwrap();
    // Renewed, it holds another 300 s from then.
    fixture.advance(100.0);
    let renewed = store.renew_session(&handle, close).unwrap();
    fixture.advance(250.0);
    assert_eq!(
        (renewed, refused(store.take_session(&handle, "a restore"))),
        (true, held_by("a close by orch-1", 50))
    );
    store.release_session(&handle, close).unwrap();
    let restore = store.take_session(&handle, "a restore").unwrap();
    // A hold that ended renews and releases nothing: the restore holds the session on.
    assert_eq!(
        (
            restore > close,
            store.renew_session(&handle, close).unwrap()
        ),
        (true, false)
    );
    store.release_session(&handle, close).unwrap();
    assert_eq!(
        refused(store.take_session(&handle, "a close by orch-2")),
        held_by("a restore", 300)
    );
    // A hold stopped part way lapses: the next takes the session over, and the lapsed one's
    // holder can no longer renew it.
    fixture.advance(300.0);
    let taken = store.take_session(&handle, "a close by orch-2").unwrap();
    assert_eq!(
        (
            taken > restore,
            store.renew_session(&handle, restore).unwrap()
        ),
        (true, false)
    );
}

#[test]
fn no_write_of_a_hold_another_took_over_lands_and_one_lapsed_but_not_taken_over_still_writes() {
    let fixture = Fixture::new(crate::common::Seed::default());
    fixture.task("sim/jump");
    let store = &fixture.store;
    let read = launch(&fixture, "sim/jump", "executor");
    let handle = &read.handle;
    let restore = format!("just tasks restore sim/jump {handle}");
    let found = NewLaunch {
        pane: "t10".into(),
        how: "found".into(),
        log: false,
        ..NewLaunch::new("sim/jump", handle, "executor")
    };
    let first = store.take_session(handle, "a close by o").unwrap();
    // It lapses, and a restore takes the session over.
    fixture.advance(300.0);
    let second = store.take_session(handle, "a restore").unwrap();
    let before = fixture.said("sim/jump").len();
    let taken = format!("another close or a restore of {handle} took its session over");
    assert_eq!(
        [
            refused(store.close_task_session_as(&read, "closed by o", &restore, first)),
            refused(store.record_found_closed(&found, "closed by o", &restore, first)),
            refused(store.reopen_task_session_as(&read, "t11", first)),
        ],
        [taken.clone(), taken.clone(), taken]
    );
    assert_eq!(
        (current(&fixture), fixture.said("sim/jump").len()),
        (read.clone(), before)
    );
    // The hold that took it over lapses as well, and no other takes it: its writes land.
    fixture.advance(300.0);
    let closed = store
        .close_task_session_as(&read, "closed by o", &restore, second)
        .unwrap();
    let reopened = store
        .reopen_task_session_as(&current(&fixture), "t11", second)
        .unwrap();
    let now = current(&fixture);
    assert_eq!(
        (closed, reopened, now.closed, now.pane.as_str()),
        (true, true, None, "t11")
    );
}

#[test]
fn a_session_found_running_is_recorded_closed_in_one_line_once_per_terminal() {
    let fixture = Fixture::new(crate::common::Seed::default());
    fixture.task("sim/jump");
    let handle = fixture.session("worker");
    let restore = format!("just tasks restore sim/jump {handle}");
    let found = NewLaunch {
        claude_session: "codex:".into(),
        worktree: "/wt/x".into(),
        pane: "t3".into(),
        how: "found".into(),
        log: false,
        ..NewLaunch::new("sim/jump", &handle, "reviewer")
    };
    let hold = fixture.store.take_session(&handle, "a close by o").unwrap();
    let before = fixture.said("sim/jump").len();
    let written = fixture
        .store
        .record_found_closed(&found, "closed by o", &restore, hold)
        .unwrap()
        .unwrap();
    assert_eq!(current(&fixture), written);
    assert_eq!(
        (
            written.role.as_str(),
            written.pane.as_str(),
            written.closed,
            written.closed_why.as_str(),
            written.restore.as_str()
        ),
        (
            "reviewer",
            "t3",
            Some(fixture.now()),
            "closed by o",
            restore.as_str()
        )
    );
    let closed = format!("closed the pane of {handle} (closed by o); restore: {restore}");
    assert_eq!(
        fixture.said("sim/jump")[before..],
        [format!(
            "{closed}. Its Codex thread id never arrived, so restore cannot resume its \
             conversation: start it again with just tasks launch sim/jump"
        )]
    );
    // That terminal again: its closure is recorded already.
    assert_eq!(
        fixture
            .store
            .record_found_closed(&found, "closed by p", &restore, hold)
            .unwrap(),
        None
    );
    assert_eq!(fixture.said("sim/jump").len(), before + 1);
    // Found in another terminal, its thread registered: a record of its own, with the thread.
    fixture
        .store
        .callback(&handle, Some("codex:0abc-12"), false, false)
        .unwrap();
    let elsewhere = NewLaunch {
        pane: "t4".into(),
        ..found.clone()
    };
    let written = fixture
        .store
        .record_found_closed(&elsewhere, "closed by o", &restore, hold)
        .unwrap()
        .unwrap();
    assert_eq!(written.claude_session, "codex:0abc-12");
    assert_eq!(fixture.said("sim/jump").last().unwrap(), &closed);
    let nowhere = NewLaunch {
        slug: "sim/none".into(),
        ..found
    };
    assert_eq!(
        refused(
            fixture
                .store
                .record_found_closed(&nowhere, "closed by o", &restore, hold)
        ),
        "no task 'sim/none'"
    );
}

#[test]
fn a_bound_reopen_never_takes_back_a_session_closed_in_another_terminal_since() {
    let fixture = Fixture::new(crate::common::Seed::default());
    fixture.task("sim/jump");
    let read = launch(&fixture, "sim/jump", "executor");
    let handle = &read.handle;
    let restore = format!("just tasks restore sim/jump {handle}");
    let hold = fixture.store.take_session(handle, "a restore").unwrap();
    let reopen = |record, pane| {
        fixture
            .store
            .reopen_task_session_as(record, pane, hold)
            .unwrap()
    };
    // Open: nothing to reopen.
    let reopened = reopen(&read, "t10");
    assert_eq!(
        (reopened, current(&fixture).pane),
        (false, "t9".to_string())
    );
    fixture
        .store
        .close_task_session("sim/jump", handle, "idle", &restore)
        .unwrap();
    // A close found it in its new terminal and recorded it closed there meanwhile.
    fixture.advance(1.0);
    let found = NewLaunch {
        pane: "t10".into(),
        how: "found".into(),
        log: false,
        ..NewLaunch::new("sim/jump", handle, "executor")
    };
    fixture
        .store
        .record_found_closed(&found, "closed by o", &restore, hold)
        .unwrap()
        .unwrap();
    let before = fixture.said("sim/jump").len();
    let reopened = reopen(&read, "t10");
    let closed = current(&fixture);
    assert_eq!(
        (
            reopened,
            closed.closed.is_some(),
            closed.closed_why.as_str()
        ),
        (false, true, "closed by o")
    );
    assert_eq!(fixture.said("sim/jump").len(), before);
    // The record as it is now reopens, once.
    let (first, second) = (reopen(&closed, "t11"), reopen(&closed, "t11"));
    let open = current(&fixture);
    assert_eq!(
        (first, second, open.closed, open.pane.as_str()),
        (true, false, None, "t11")
    );
    assert_eq!(
        fixture.said("sim/jump")[before..],
        [format!("restored {handle} in pane t11")]
    );
}
