//! `close` against what changes in the terminal while it runs: another program, work or another
//! owner by the last look before the kill; and a terminal already gone. What holds the session
//! meanwhile (a restore, another close, one stopped part way) is `close_holds.rs`'s.

use crate::close::restore_line;
use crate::common::refused;
use crate::{Hooked, World};
use serde_json::{Map, Value, json};
use std::cell::Cell;
use symphony_board::launch::{Closure, GONE};
use symphony_board::rows::TaskSessionRow;
use symphony_board::{Error, NewLaunch};

/// The fake hc, running `then` right before its `nth` `call` (`get`, `kill`…) goes to it.
pub(crate) fn ahead_of<'hook>(
    world: &World,
    call: &'hook str,
    nth: usize,
    then: impl Fn() + 'hook,
) -> Hooked<'hook> {
    let seen = Cell::new(0);
    let mut hc = Hooked::new(world);
    hc.instead = Box::new(move |args| {
        if args[0] == call {
            seen.set(seen.get() + 1);
            if seen.get() == nth {
                then();
            }
        }
        None
    });
    hc
}

/// The fake hc, running `then` right after its `nth` `call` (`get`, `rm`…) answered.
pub(crate) fn after<'hook>(
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

/// The terminals hc was asked to kill or remove.
pub(crate) fn killed(world: &World) -> Vec<Vec<String>> {
    [world.calls_of(&["kill"]), world.calls_of(&["rm"])].concat()
}

/// sim/jump's one record, as it is now.
pub(crate) fn record(world: &World) -> TaskSessionRow {
    let [record] = <[_; 1]>::try_from(world.store().task_sessions("sim/jump").unwrap()).unwrap();
    record
}

/// The close of `session` (`closed`, after the `before`th line of the thread) was refused at its last
/// look, saying `why`: nothing killed, nothing written, the session open.
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
    assert_eq!(world.said_since(before), Vec::<String>::new(), "{why}");
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
fn a_close_whose_hold_lapsed_and_was_taken_over_by_its_last_look_keeps_the_terminal() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    let before = world.f.said("sim/jump").len();
    let taken = after(&world, "get", 1, || {
        world.f.advance(300.0);
        world.store().take_session(handle, "a restore").unwrap();
    });
    let closed = world.close_through(&taken, handle);
    let why = "another close or a restore took its session over";
    kept_at_the_last_look(&world, &session, before, closed, why);
    // The close let go of nothing that is not its own: the restore holds the session on.
    assert_eq!(
        refused(world.close(handle)),
        format!("a restore of {handle} is under way: its hold lapses in 300 s")
    );
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
