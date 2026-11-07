//! `close` against what changes while it runs: another program, work or another owner in the
//! terminal by the last look before the kill; a restore, or a second close, of the same session; a
//! close stopped part way, which the next one finishes; and a terminal already gone.

use crate::close::{closed_line, restore_line};
use crate::common::refused;
use crate::{Hooked, World};
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell};
use symphony_board::launch::{Closure, GONE, restore};
use symphony_board::rows::TaskSessionRow;
use symphony_board::{Error, NewLaunch};

/// The fake hc, running `then` right after its `nth` `call` (`get`, `rm`…) answered.
fn after<'hook>(
    world: &World,
    call: &'hook str,
    nth: usize,
    then: impl Fn() + 'hook,
) -> Hooked<'hook> {
    let seen = Cell::new(0);
    let mut hc = Hooked::new(world);
    hc.after = Box::new(move |args| {
        if args[0] == call {
            seen.set(seen.get() + 1);
            if seen.get() == nth {
                then();
            }
        }
    });
    hc
}

/// The closure lines of sim/jump's thread from the `from`th on.
fn closures(world: &World, from: usize) -> Vec<String> {
    world
        .said_since(from)
        .into_iter()
        .filter(|line| line.starts_with("closed the pane"))
        .collect()
}

/// The terminals hc was asked to kill or remove.
fn killed(world: &World) -> Vec<Vec<String>> {
    [world.calls_of(&["kill"]), world.calls_of(&["rm"])].concat()
}

fn record(world: &World) -> TaskSessionRow {
    let [record] = <[_; 1]>::try_from(world.store().task_sessions("sim/jump").unwrap()).unwrap();
    record
}

/// The close of `session` (`closed`, after the `before`th line of the thread) was refused at its last
/// look, saying `why`: nothing killed, and the closure it recorded first is undone on the thread.
fn kept_at_the_last_look(
    world: &World,
    session: &TaskSessionRow,
    before: usize,
    closed: Result<Closure, Error>,
    why: &str,
) {
    assert_eq!(
        refused(closed),
        format!("terminal {} stays: {why}", session.pane)
    );
    world.untouched();
    assert_eq!(
        world.said_since(before),
        [
            closed_line(&session.handle, &world.orch),
            format!("kept the pane of {} ({why})", session.handle)
        ],
        "{why}"
    );
}

/// An edit of a terminal as hc keeps it, given the pid of its program.
type Replace = fn(&mut Map<String, Value>, i64);

#[test]
fn another_program_by_the_last_look_keeps_the_terminal_even_under_the_same_pid() {
    // A shell in the agent's place under its pid; a job and no agent under its pid; another pid.
    let replacements: [Replace; 3] = [
        |terminal, pid| {
            terminal.insert(
                "foreground".into(),
                json!({"pid": pid, "name": "zsh", "argv": ["zsh", "-l"]}),
            );
        },
        |terminal, pid| {
            terminal.remove("agent");
            terminal.insert("status".into(), json!(null));
            terminal.insert(
                "foreground".into(),
                json!({"pid": pid, "name": "node", "argv": ["node", "active-job.js"]}),
            );
        },
        |terminal, pid| {
            terminal.insert("pid".into(), json!(pid + 1000));
        },
    ];
    for replace in replacements {
        let world = World::new();
        let session = world.waiting_reviewer();
        let pid = world.pid_of(&session.pane);
        let before = world.f.said("sim/jump").len();
        let replaced = after(&world, "get", 1, || {
            world.edit_terminal(&session.pane, |terminal| replace(terminal, pid));
        });
        let closed = world.close_through(&replaced, &session.handle);
        kept_at_the_last_look(
            &world,
            &session,
            before,
            closed,
            "another program runs in it now",
        );
    }
}

#[test]
fn work_started_by_the_last_look_keeps_the_terminal() {
    let wrapped = "tools/rs-bin.sh sf-slots cargo test --manifest-path symphony/Cargo.toml -p \
                   symphony-board --test launch";
    for (command, why) in [
        ("just verify", "it runs a verify (just verify)".to_string()),
        (
            wrapped,
            format!("it runs {}", wrapped.chars().take(60).collect::<String>()),
        ),
        ("just test-rs", "it runs just test-rs".into()),
    ] {
        let world = World::new();
        let session = world.waiting_reviewer();
        let pid = world.pid_of(&session.pane);
        let before = world.f.said("sim/jump").len();
        let started = after(&world, "get", 2, || world.run_under(pid, &[command]));
        let closed = world.close_through(&started, &session.handle);
        kept_at_the_last_look(&world, &session, before, closed, &why);
    }
}

#[test]
fn a_terminal_that_changed_hands_by_the_last_look_is_kept() {
    for (label, value, why) in [
        (
            "sf.board",
            "another-project/tasks",
            "it is not this board's (another-project/tasks): the board closes only its own",
        ),
        (
            "sf.handle",
            "another-owl",
            "it runs another-owl, not {handle}",
        ),
    ] {
        let world = World::new();
        let session = world.waiting_reviewer();
        let before = world.f.said("sim/jump").len();
        let taken = after(&world, "get", 1, || {
            world.edit_terminal(&session.pane, |terminal| {
                terminal["labels"][label] = json!(value)
            });
        });
        let closed = world.close_through(&taken, &session.handle);
        kept_at_the_last_look(
            &world,
            &session,
            before,
            closed,
            &why.replace("{handle}", &session.handle),
        );
    }
}

#[test]
fn a_close_while_the_session_is_restored_keeps_it_closed_and_refuses_the_restore() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world.close(handle).unwrap();
    let before = world.f.said("sim/jump").len();
    // Its new terminal has taken its first message, and the restore has not reopened its record yet.
    let closing = RefCell::new(None);
    let racing = after(&world, "send", 1, || {
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
            "{handle} was closed while it was restored: its record changed, so terminal {new} is not kept"
        )
    );
    assert!(
        !world.has_terminal(&new),
        "{new} in {:?}",
        world.addresses()
    );
    let read = record(&world);
    assert_eq!(
        (read.closed.is_some(), read.pane.as_str()),
        (true, new.as_str()),
        "{read:?}"
    );
    assert_eq!(world.said_since(before), [closed_line(handle, &world.orch)]);
}

#[test]
fn a_restore_waits_until_the_close_has_killed_the_terminal() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let tried = RefCell::new(None);
    // Right before the kill, the closure is recorded and the terminal still runs.
    let racing = after(&world, "get", 2, || {
        let early = restore(
            world.store(),
            &world.hc(),
            &world.machine(),
            "sim/jump",
            Some(&session.handle),
            None,
        );
        *tried.borrow_mut() = Some(refused(early));
    });
    let closed = world.close_through(&racing, &session.handle).unwrap();
    assert_eq!(
        (closed.why, closed.closed),
        (format!("closed by {}", world.orch), true)
    );
    assert_eq!(
        tried.take(),
        Some(format!(
            "{} still runs in terminal {}: close it before restoring",
            session.handle, session.pane
        ))
    );
    assert!(record(&world).closed.is_some(), "{:?}", record(&world));
}

#[test]
fn a_session_restored_once_its_terminal_is_removed_stays_open_with_its_new_terminal_and_callback() {
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
    let before = world.f.said("sim/jump").len();
    let new = RefCell::new(String::new());
    // As soon as the old terminal is removed, the session is restored, and its agent registers the
    // same thread again from its new terminal.
    let racing = after(&world, "rm", 1, || {
        let [again] = <[_; 1]>::try_from(
            restore(
                world.store(),
                &world.hc(),
                &world.machine(),
                "sim/jump",
                Some(handle),
                None,
            )
            .unwrap(),
        )
        .unwrap();
        world
            .store()
            .callback_in(
                handle,
                Some("codex:0abc-12"),
                false,
                false,
                Some(&again.row.pane),
            )
            .unwrap();
        *new.borrow_mut() = again.row.pane;
    });
    let closed = world.close_through(&racing, handle).unwrap();
    assert_eq!(
        (closed.pane.as_str(), closed.closed),
        (session.pane.as_str(), true)
    );
    let new = new.take();
    assert!(
        world.live().contains(&new),
        "{new} not in {:?}",
        world.live()
    );
    let now = record(&world);
    assert_eq!(
        (now.closed, now.pane.as_str()),
        (None, new.as_str()),
        "{now:?}"
    );
    assert_eq!(closures(&world, before), [closed_line(handle, &world.orch)]);
    let callbacks = world.store().callbacks().unwrap();
    assert_eq!(
        callbacks.get(handle).and_then(|c| c.terminal.clone()),
        Some(new),
        "{:?}",
        callbacks.keys()
    );
}

#[test]
fn two_overlapping_closes_of_one_terminal_write_one_closure_line() {
    // The second runs while the first is between its kill and its end, then before the first has
    // recorded anything.
    for (call, nth) in [("rm", 1), ("get", 1)] {
        let world = World::new();
        let session = world.waiting_reviewer();
        let before = world.f.said("sim/jump").len();
        let second = RefCell::new(None);
        let racing = after(&world, call, nth, || {
            *second.borrow_mut() = Some(world.close(&session.handle));
        });
        let first = world.close_through(&racing, &session.handle).unwrap();
        let second = second.take().unwrap().unwrap();
        assert_eq!((first.closed, second.closed), (true, true), "{call}");
        assert!(
            !world.has_terminal(&session.pane),
            "{call}: {:?}",
            world.addresses()
        );
        assert_eq!(
            world.said_since(before),
            [closed_line(&session.handle, &world.orch)],
            "{call}"
        );
    }
}

#[test]
fn a_close_stopped_after_its_record_is_finished_by_the_next_with_no_second_line() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let earlier = "closed by amber-heron";
    // The record a close writes before its kill, the close stopping there.
    world
        .store()
        .close_task_session_as(&session, earlier, &restore_line(&session.handle))
        .unwrap();
    let before = world.f.said("sim/jump").len();
    let closed = world.close(&session.handle).unwrap();
    assert_eq!((closed.why.as_str(), closed.closed), (earlier, true));
    assert!(
        !world.has_terminal(&session.pane),
        "{:?}",
        world.addresses()
    );
    assert_eq!(world.said_since(before), Vec::<String>::new());
    assert_eq!(record(&world).closed_why, earlier);
}

#[test]
fn a_close_stopped_after_its_kill_has_its_callback_retired_by_the_next() {
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
    let earlier = format!("closed by {}", world.orch);
    world
        .store()
        .close_task_session_as(&session, &earlier, &restore_line(handle))
        .unwrap();
    world.edit_terminals(|all| all.clear());
    let before = world.f.said("sim/jump").len();
    let closed = world.close(handle).unwrap();
    assert_eq!((closed.why, closed.closed), (earlier, true));
    assert_eq!(killed(&world), Vec::<Vec<String>>::new());
    assert_eq!(world.said_since(before), Vec::<String>::new());
    let callbacks = world.store().callbacks().unwrap();
    assert!(!callbacks.contains_key(handle), "{:?}", callbacks.keys());
}

#[test]
fn a_terminal_closed_outside_the_board_is_recorded_gone_and_its_callback_retired() {
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
    world.edit_terminals(|all| all.clear());
    let restore = restore_line(handle);
    assert_eq!(
        world.close(handle).unwrap(),
        Closure {
            pane: session.pane.clone(),
            handle: Some(handle.clone()),
            why: GONE.into(),
            restore: restore.clone(),
            closed: true,
            kept: false,
        }
    );
    assert_eq!(killed(&world), Vec::<Vec<String>>::new());
    let read = record(&world);
    assert_eq!(
        (read.closed.is_some(), read.closed_why.as_str()),
        (true, GONE)
    );
    assert_eq!(
        world.f.said("sim/jump").last().unwrap(),
        &format!("closed the pane of {handle} ({GONE}); restore: {restore}")
    );
    let callbacks = world.store().callbacks().unwrap();
    assert!(!callbacks.contains_key(handle), "{:?}", callbacks.keys());
}

#[test]
fn a_gone_terminal_leaves_a_callback_registered_from_another_terminal() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world
        .store()
        .callback_in(handle, Some("codex:0abc-12"), false, false, Some("t999"))
        .unwrap();
    world.edit_terminals(|all| all.clear());
    assert_eq!(world.close(handle).unwrap().why, GONE);
    let callbacks = world.store().callbacks().unwrap();
    assert_eq!(
        callbacks.get(handle).and_then(|c| c.terminal.as_deref()),
        Some("t999")
    );
}

#[test]
fn a_terminal_found_by_its_label_and_gone_before_it_is_read_is_refused_with_nothing_recorded() {
    let world = World::new();
    let executor = world.f.session("worker");
    world.store().claim("sim/jump", &executor).unwrap();
    world
        .store()
        .move_to("sim/jump", "review", &executor)
        .unwrap();
    let handle = world.f.session("reviewer");
    world
        .store()
        .take_role("sim/jump", &handle, "reviewer")
        .unwrap();
    world.add_terminal(true, Some(&handle), json!({"name": null}));
    let vanishing = after(&world, "ls", 1, || world.edit_terminals(|all| all.clear()));
    assert_eq!(
        refused(world.close_through(&vanishing, &handle)),
        format!("{handle} has no open session on sim/jump and runs in no terminal of this board")
    );
    assert_eq!(killed(&world), Vec::<Vec<String>>::new());
    assert_eq!(
        world.store().task_sessions("sim/jump").unwrap(),
        Vec::<TaskSessionRow>::new()
    );
}

#[test]
fn an_ended_program_running_again_by_the_last_look_keeps_the_terminal() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let set_state = |state: &str| {
        world.edit_terminal(&session.pane, |terminal| {
            terminal.insert("state".into(), json!(state));
        });
    };
    set_state("finished");
    let before = world.f.said("sim/jump").len();
    let revived = after(&world, "get", 1, || set_state("live"));
    let closed = world.close_through(&revived, &session.handle);
    kept_at_the_last_look(&world, &session, before, closed, "its program runs again");
}

#[test]
fn a_herdr_pane_of_the_record_is_gone_without_asking_hc() {
    let world = World::new();
    let handle = world.f.session("reviewer");
    world
        .store()
        .record_launch(&NewLaunch {
            claude_session: "c-1".into(),
            pane: "w1:p3".into(),
            ..NewLaunch::new("sim/jump", &handle, "reviewer")
        })
        .unwrap();
    assert_eq!(world.close(&handle).unwrap().why, GONE);
    assert_eq!(world.calls(), Vec::<Vec<String>>::new());
    assert_eq!(
        world.store().open_task_sessions().unwrap(),
        Vec::<TaskSessionRow>::new()
    );
}
