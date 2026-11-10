//! `close` against what changes in the terminal while it runs: another program, work or another
//! owner by the last look before the kill; and a terminal already gone. What holds the session
//! meanwhile (a restore, another close, one stopped part way) is `close_holds.rs`'s.

use crate::close::restore_line;
use crate::common::refused;
use crate::{Hooked, World};
use serde_json::{Map, Value, json};
use std::cell::Cell;
use symphony_board::launch::hc::Hc;
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

/// The fake's terminal and process responses, with either native status encoding on each get.
/// `after_get` changes the fixture only after that response has been captured.
fn native_hc<'w>(w: &'w World, detailed: bool, after_get: impl Fn(usize) + 'w) -> Hooked<'w> {
    let mut hc = Hooked::new(w);
    let gets = Cell::new(0);
    hc.instead = Box::new(move |args| {
        if args[0] != "get" {
            return None;
        }
        let answer = w.hc().run(args).map(|out| {
            let mut found: Value = serde_json::from_str(&out).unwrap();
            let state = w.terminal(args[1])["status"].clone();
            found["status"] = if detailed {
                json!({"app": found["agent"], "state": state})
            } else {
                state
            };
            found.to_string()
        });
        gets.set(gets.get() + 1);
        after_get(gets.get());
        Some(answer)
    });
    hc
}

/// Registration identity remains present and bound to the same terminal after a refused close.
fn callbacks(w: &World) -> Vec<Value> {
    w.store()
        .callbacks()
        .unwrap()
        .into_iter()
        .map(|(handle, c)| {
            json!({"key": handle, "handle": c.handle, "address": c.address,
               "host": c.host, "chat": c.chat, "terminal": c.terminal})
        })
        .collect()
}

#[test]
fn only_a_known_idle_or_done_native_agent_closes_even_with_no_descendants() {
    for agent in ["claude", "codex"] {
        for detailed in [false, true] {
            for ready in ["idle", "done"] {
                let w = World::new();
                let s = w.waiting_reviewer();
                let pid = w.pid_of(&s.pane);
                w.run_under(pid, &[]);
                let hc = native_hc(&w, detailed, |_| {});
                w.store()
                    .callback_in(&s.handle, Some("codex:0abc"), false, false, Some(&s.pane))
                    .unwrap();
                let registered = callbacks(&w);
                assert_eq!(registered.len(), 1);
                let records = w.store().sessions_of(&s.handle).unwrap();
                let before = w.f.said("sim/jump").len();
                for state in [
                    json!("working"),
                    json!("blocked"),
                    json!("waiting"),
                    json!("unknown"),
                    json!(null),
                    json!(42),
                ] {
                    w.edit_terminal(&s.pane, |t| {
                        t.insert("agent".into(), json!(agent));
                        t.insert("argv".into(), json!([agent]));
                        t.insert(
                            "foreground".into(),
                            json!({"pid": pid, "name": agent, "argv": [agent]}),
                        );
                        t.insert("status".into(), state.clone());
                    });
                    // No child can provide the independent busy-work refusal.
                    assert!(w.procs.borrow()[&w.pid_of(&s.pane)].is_empty());
                    let status = state.as_str().unwrap_or("silent");
                    assert_eq!(
                        refused(w.close_through(&hc, &s.handle)),
                        format!(
                            "terminal {} stays: its agent is {status}, not idle or done",
                            s.pane
                        )
                    );
                    w.untouched();
                    assert_eq!(w.store().sessions_of(&s.handle).unwrap(), records);
                    assert_eq!(callbacks(&w), registered);
                    assert!(w.said_since(before).is_empty());
                }
                w.edit_terminal(&s.pane, |t| {
                    t.insert("status".into(), json!(ready));
                });
                let closed = w.close_through(&hc, &s.handle).unwrap();
                assert!(closed.closed);
                assert_eq!(closed.restore, restore_line(&s.handle));
                assert!(!w.has_terminal(&s.pane));
                assert!(!w.store().callbacks().unwrap().contains_key(&s.handle));
                let [read] = <[_; 1]>::try_from(w.store().sessions_of(&s.handle).unwrap()).unwrap();
                assert!(read.closed.is_some());
                assert_eq!(read.restore, restore_line(&s.handle));
                assert_eq!(
                    w.store().get("sim/jump").unwrap().holders("reviewer"),
                    [s.handle.as_str()]
                );
            }
        }
    }
}

#[test]
fn a_native_state_change_alone_at_the_final_look_keeps_the_agent_and_its_callback() {
    for agent in ["claude", "codex"] {
        for detailed in [false, true] {
            for state in [
                json!("working"),
                json!("blocked"),
                json!("waiting"),
                json!("unknown"),
                json!(null),
            ] {
                let w = World::new();
                let s = w.waiting_reviewer();
                let pid = w.pid_of(&s.pane);
                w.run_under(pid, &[]);
                w.edit_terminal(&s.pane, |t| {
                    t.insert("agent".into(), json!(agent));
                    t.insert("argv".into(), json!([agent]));
                    t.insert(
                        "foreground".into(),
                        json!({"pid": pid, "name": agent, "argv": [agent]}),
                    );
                    t.insert("status".into(), json!("idle"));
                });
                w.store()
                    .callback_in(&s.handle, Some("codex:0abc"), false, false, Some(&s.pane))
                    .unwrap();
                let registered = callbacks(&w);
                assert_eq!(registered.len(), 1);
                let records = w.store().sessions_of(&s.handle).unwrap();
                let terminal = w.terminal(&s.pane);
                let before = w.f.said("sim/jump").len();
                let changed = native_hc(&w, detailed, |n| {
                    if n == 1 {
                        w.edit_terminal(&s.pane, |t| {
                            t.insert("status".into(), state.clone());
                        });
                    }
                });
                let status = state.as_str().unwrap_or("silent");
                kept_at_the_last_look(
                    &w,
                    &s,
                    before,
                    w.close_through(&changed, &s.handle),
                    &format!("its agent is {status}, not idle or done"),
                );
                assert_eq!(w.terminal(&s.pane)["pid"], terminal["pid"]);
                assert!(terminal["foreground"].is_object());
                assert_eq!(w.terminal(&s.pane)["foreground"], terminal["foreground"]);
                assert!(w.procs.borrow()[&w.pid_of(&s.pane)].is_empty());
                assert_eq!(w.store().sessions_of(&s.handle).unwrap(), records);
                assert_eq!(callbacks(&w), registered);
            }
        }
    }
}
