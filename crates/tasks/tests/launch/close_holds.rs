//! `close` against what else holds its session: a restore, a second close, one stopped part way and
//! the one that takes its lapsed hold over; and the callback the agent registers while it closes.

use crate::close::{closed_line, restore_line};
use crate::close_races::{after, ahead_of, killed, record};
use crate::common::refused;
use crate::{Hooked, World};
use std::cell::RefCell;
use symphony_board::Error;
use symphony_board::launch::{GONE, LaunchError, Started, restore};

/// What a close or a restore of `handle` is told while `doing` holds its session, just taken.
fn held(doing: &str, handle: &str) -> String {
    format!("{doing} of {handle} is under way: its hold lapses in 300 s")
}

/// `handle`'s session on sim/jump restored, through the world's own hc.
pub(crate) fn restoring(world: &World, handle: &str) -> Result<Vec<Started>, Error> {
    restore(
        world.store(),
        &world.hc(),
        &world.machine(),
        "sim/jump",
        Some(handle),
        None,
    )
}

/// Where the handle's thread registers from: `Some(None)` from no terminal, `Some(Some(T))` from
/// terminal T; `None`, it does not.
type Registered<'a> = Option<Option<&'a str>>;

#[test]
fn a_restore_while_a_close_holds_the_session_is_refused_and_one_after_it_restores_it() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    // Its thread registered from no terminal: the close retires that registration, and never the
    // one its restored agent makes from its new terminal.
    world
        .store()
        .callback_in(handle, Some("codex:0abc-12"), false, false, None)
        .unwrap();
    let before = world.f.said("sim/jump").len();
    let tried = RefCell::new(None);
    // Its terminal is killed and removed, its closure not written yet.
    let racing = after(&world, "rm", 1, || {
        *tried.borrow_mut() = Some(refused(restoring(&world, handle)));
    });
    world.close_through(&racing, handle).unwrap();
    let closer = format!("a close by {}", world.orch);
    assert_eq!(tried.take(), Some(held(&closer, handle)));
    assert_eq!(world.said_since(before), [closed_line(handle, &world.orch)]);
    let callbacks = world.store().callbacks().unwrap();
    assert!(!callbacks.contains_key(handle), "{:?}", callbacks.keys());
    // The close let the session go: the restore brings it back.
    let [again] = <[_; 1]>::try_from(restoring(&world, handle).unwrap()).unwrap();
    let new = again.row.pane;
    world
        .store()
        .callback_in(handle, Some("codex:0abc-12"), false, false, Some(&new))
        .unwrap();
    let now = record(&world);
    assert_eq!((now.closed, now.pane.as_str()), (None, new.as_str()));
    let callbacks = world.store().callbacks().unwrap();
    assert_eq!(
        callbacks
            .get(handle)
            .and_then(|callback| callback.terminal.clone()),
        Some(new)
    );
}

#[test]
fn a_close_while_a_restore_holds_the_session_is_refused_and_the_restore_keeps_it() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world.close(handle).unwrap();
    let tried = RefCell::new(None);
    // Its new terminal has taken its first message, and its record is not reopened yet.
    let racing = after(&world, "send", 1, || {
        *tried.borrow_mut() = Some(refused(world.close(handle)));
    });
    let restored = restore(
        world.store(),
        &racing,
        &world.machine(),
        "sim/jump",
        Some(handle),
        None,
    );
    let [again] = <[_; 1]>::try_from(restored.unwrap()).unwrap();
    assert_eq!(tried.take(), Some(held("a restore", handle)));
    let new = again.row.pane;
    assert!(world.has_terminal(&new), "{new} in {:?}", world.addresses());
    let now = record(&world);
    assert_eq!((now.closed, now.pane.as_str()), (None, new.as_str()));
}

#[test]
fn a_restore_whose_hold_lapsed_as_its_terminal_started_gives_way_to_the_close_that_took_it_over() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world.close(handle).unwrap();
    let before = world.f.said("sim/jump").len();
    let closing = RefCell::new(None);
    // Its new terminal finished its first turn, its record is not reopened yet, and its hold
    // has lapsed: a close takes the session over and closes that idle terminal.
    let racing = after(&world, "send", 1, || {
        world.f.advance(300.0);
        let [pane] = <[_; 1]>::try_from(world.addresses()).unwrap();
        world.edit_terminal(&pane, |t| {
            t.insert("status".into(), "idle".into());
        });
        *closing.borrow_mut() = Some(world.close(handle));
    });
    let restored = restore(
        world.store(),
        &racing,
        &world.machine(),
        "sim/jump",
        Some(handle),
        None,
    );
    let new = closing.take().unwrap().unwrap().pane;
    assert_eq!(
        refused(restored),
        format!(
            "another close or a restore of {handle} took its session over while it was restored, \
             so the restore records nothing and terminal {new}, which it started, is killed"
        )
    );
    assert!(
        !world.has_terminal(&new),
        "{new} in {:?}",
        world.addresses()
    );
    let now = record(&world);
    assert_eq!(
        (now.closed.is_some(), now.pane.as_str()),
        (true, new.as_str()),
        "{now:?}"
    );
    assert_eq!(world.said_since(before), [closed_line(handle, &world.orch)]);
}

#[test]
fn a_second_close_while_one_runs_is_refused_and_writes_nothing() {
    // Before the first's last look, then between its kill and its closure.
    for (call, nth) in [("get", 1), ("rm", 1)] {
        let world = World::new();
        let session = world.waiting_reviewer();
        let handle = &session.handle;
        let before = world.f.said("sim/jump").len();
        let second = RefCell::new(None);
        let racing = after(&world, call, nth, || {
            *second.borrow_mut() = Some(refused(world.close(handle)));
        });
        let first = world.close_through(&racing, handle).unwrap();
        let closer = format!("a close by {}", world.orch);
        assert_eq!(
            (first.closed, second.take()),
            (true, Some(held(&closer, handle))),
            "{call}"
        );
        assert_eq!(
            world.said_since(before),
            [closed_line(handle, &world.orch)],
            "{call}"
        );
    }
}

#[test]
fn a_close_whose_kill_fails_undoes_nothing_of_the_close_that_took_its_lapsed_hold_over() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    let before = world.f.said("sim/jump").len();
    let other = RefCell::new(None);
    // As this close kills, its hold has lapsed: another close takes the session over and closes the
    // terminal, then this one's kill fails.
    let mut failing = Hooked::new(&world);
    failing.instead = Box::new(|args| {
        if args[0] != "kill" {
            return None;
        }
        world.f.advance(300.0);
        *other.borrow_mut() = Some(world.close(handle));
        Some(Err(LaunchError("hc kill: connection lost".into())))
    });
    let failed = world.close_through(&failing, handle);
    assert_eq!(refused(failed), "hc kill: connection lost");
    let other = other.take().unwrap().unwrap();
    let why = format!("closed by {}", world.orch);
    assert_eq!((other.closed, other.why.as_str()), (true, why.as_str()));
    assert!(
        !world.has_terminal(&session.pane),
        "{:?}",
        world.addresses()
    );
    let now = record(&world);
    assert_eq!(
        (now.closed.is_some(), now.closed_why.as_str()),
        (true, why.as_str())
    );
    assert_eq!(world.said_since(before), [closed_line(handle, &world.orch)]);
}

#[test]
fn a_close_stopped_before_its_kill_leaves_the_session_open_and_held_until_its_hold_lapses() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    let before = world.f.said("sim/jump").len();
    // A close took the session and stopped there.
    world
        .store()
        .take_session(handle, "a close by amber-heron")
        .unwrap();
    assert_eq!(
        refused(world.close(handle)),
        held("a close by amber-heron", handle)
    );
    world.untouched();
    assert_eq!(world.said_since(before), Vec::<String>::new());
    world.f.advance(300.0);
    world.close(handle).unwrap();
    assert!(
        !world.has_terminal(&session.pane),
        "{:?}",
        world.addresses()
    );
    assert_eq!(world.said_since(before), [closed_line(handle, &world.orch)]);
}

#[test]
fn a_close_stopped_after_its_kill_is_recorded_gone_by_the_next_once_its_hold_lapses() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world
        .store()
        .callback_in(
            handle,
            Some("codex:0abc-12"),
            false,
            false,
            Some(&session.pane),
        )
        .unwrap();
    world
        .store()
        .take_session(handle, "a close by amber-heron")
        .unwrap();
    world.edit_terminals(|all| all.clear());
    world.f.advance(300.0);
    let closed = world.close(handle).unwrap();
    assert_eq!((closed.why.as_str(), closed.closed), (GONE, true));
    let now = record(&world);
    assert_eq!(
        (now.closed.is_some(), now.closed_why.as_str()),
        (true, GONE)
    );
    assert_eq!(
        world.f.said("sim/jump").last().unwrap(),
        &format!(
            "closed the pane of {handle} ({GONE}); restore: {}",
            restore_line(handle)
        )
    );
    let callbacks = world.store().callbacks().unwrap();
    assert!(!callbacks.contains_key(handle), "{:?}", callbacks.keys());
}

#[test]
fn a_close_stopped_after_its_record_has_its_callback_retired_by_the_next_with_no_second_line() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world
        .store()
        .callback_in(
            handle,
            Some("codex:0abc-12"),
            false,
            false,
            Some(&session.pane),
        )
        .unwrap();
    let earlier = "closed by amber-heron";
    let hold = world
        .store()
        .take_session(handle, "a close by amber-heron")
        .unwrap();
    world.edit_terminals(|all| all.clear());
    world
        .store()
        .close_task_session_as(&session, earlier, &restore_line(handle), hold)
        .unwrap();
    let before = world.f.said("sim/jump").len();
    world.f.advance(300.0);
    let closed = world.close(handle).unwrap();
    assert_eq!((closed.why.as_str(), closed.closed), (earlier, true));
    assert_eq!(killed(&world), Vec::<Vec<String>>::new());
    assert_eq!(world.said_since(before), Vec::<String>::new());
    let callbacks = world.store().callbacks().unwrap();
    assert!(!callbacks.contains_key(handle), "{:?}", callbacks.keys());
}

#[test]
fn a_first_callback_registered_right_before_the_kill_completes_the_record_and_is_retired() {
    let world = World::new();
    let session = world.launch("sim/jump").row;
    world.edit_terminal(&session.pane, |t| {
        t.insert("status".into(), "idle".into());
    });
    let handle = &session.handle;
    let before = world.f.said("sim/jump").len();
    let registering = ahead_of(&world, "kill", 1, || {
        world
            .store()
            .callback_in(
                handle,
                Some("codex:0abc-12"),
                false,
                false,
                Some(&session.pane),
            )
            .unwrap();
    });
    let closed = world.close_through(&registering, handle).unwrap();
    let now = record(&world);
    assert_eq!(
        (
            session.claude_session.as_str(),
            closed.closed,
            now.closed.is_some(),
            now.claude_session.as_str()
        ),
        ("codex:", true, true, "codex:0abc-12")
    );
    // The thread arrived, so the closure says nothing of one lost.
    assert_eq!(world.said_since(before), [closed_line(handle, &world.orch)]);
    let callbacks = world.store().callbacks().unwrap();
    assert!(!callbacks.contains_key(handle), "{:?}", callbacks.keys());
}

#[test]
fn a_close_retires_the_registration_from_its_terminal_or_the_very_one_it_read_and_no_other() {
    // Registered before the close, registered right before its kill, and whether it is retired.
    let cases: [(Registered, Registered, bool); 5] = [
        (Some(None), None, true),
        (None, Some(Some("{pane}")), true),
        // From no terminal again since: not the registration read.
        (Some(None), Some(None), false),
        (Some(None), Some(Some("t999")), false),
        (Some(Some("t999")), None, false),
    ];
    for (read, since, retired) in cases {
        let world = World::new();
        let session = world.waiting_reviewer();
        let handle = &session.handle;
        let terminal = |from: Option<&str>| from.map(|from| from.replace("{pane}", &session.pane));
        let register = |from: Option<&str>| {
            world
                .store()
                .callback_in(
                    handle,
                    Some("codex:0abc-12"),
                    false,
                    false,
                    terminal(from).as_deref(),
                )
                .unwrap();
        };
        if let Some(from) = read {
            register(from);
        }
        let racing = ahead_of(&world, "kill", 1, || {
            if let Some(from) = since {
                register(from);
            }
        });
        world.close_through(&racing, handle).unwrap();
        let left = world
            .store()
            .callbacks()
            .unwrap()
            .remove(handle)
            .map(|callback| callback.terminal);
        let last = since.or(read).expect("a case registers");
        let kept = if retired { None } else { Some(terminal(last)) };
        assert_eq!(left, kept, "{read:?}, then {since:?}");
    }
}
