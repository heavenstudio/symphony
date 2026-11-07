//! A close's writes to a task session's record (`close`, `launch::close`), each bound to the record
//! the close read: the launch and the terminal it names. A record changed since (restored into
//! another terminal, launched again, closed by another close) is left as it is, and nothing is said.

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
    let fixture = Fixture::empty();
    fixture.task("sim/jump");
    let read = launch(&fixture, "sim/jump", "executor");
    let handle = &read.handle;
    let restore = format!("just tasks restore sim/jump {handle}");
    let before = fixture.said("sim/jump").len();
    let close = |record, why| {
        fixture
            .store
            .close_task_session_as(record, why, &restore)
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
fn a_kept_close_reopens_only_the_closure_it_wrote() {
    let fixture = Fixture::empty();
    fixture.task("sim/jump");
    let read = launch(&fixture, "sim/jump", "executor");
    let handle = &read.handle;
    let restore = format!("just tasks restore sim/jump {handle}");
    fixture
        .store
        .close_task_session_as(&read, "closed by o", &restore)
        .unwrap();
    let before = fixture.said("sim/jump").len();
    let keep = |why| {
        fixture
            .store
            .keep_task_session(&read, why, "on screen")
            .unwrap()
    };
    // Another close's closure is not this one's.
    let kept = keep("closed by p");
    assert_eq!((kept, current(&fixture).closed.is_some()), (false, true));
    let kept = keep("closed by o");
    let open = current(&fixture);
    assert_eq!(
        (
            kept,
            open.closed,
            open.closed_why.as_str(),
            open.pane.as_str()
        ),
        (true, None, "", "t9")
    );
    // Open already: nothing to undo.
    let kept = keep("closed by o");
    assert_eq!((kept, current(&fixture).closed), (false, None));
    assert_eq!(
        fixture.said("sim/jump")[before..],
        [format!("kept the pane of {handle} (on screen)")]
    );
}

#[test]
fn a_session_found_running_is_recorded_closed_in_one_line_once_per_terminal() {
    let fixture = Fixture::empty();
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
    let before = fixture.said("sim/jump").len();
    let written = fixture
        .store
        .record_found_closed(&found, "closed by o", &restore)
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
            .record_found_closed(&found, "closed by p", &restore)
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
        .record_found_closed(&elsewhere, "closed by o", &restore)
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
                .record_found_closed(&nowhere, "closed by o", &restore)
        ),
        "no task 'sim/none'"
    );
}

#[test]
fn a_bound_reopen_never_takes_back_a_session_closed_in_another_terminal_since() {
    let fixture = Fixture::empty();
    fixture.task("sim/jump");
    let read = launch(&fixture, "sim/jump", "executor");
    let handle = &read.handle;
    let restore = format!("just tasks restore sim/jump {handle}");
    let reopen = |record, pane| fixture.store.reopen_task_session_as(record, pane).unwrap();
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
        .record_found_closed(&found, "closed by o", &restore)
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
