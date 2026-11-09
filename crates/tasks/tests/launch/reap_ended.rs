//! Terminals whose program ended: the sweep removes each at the first sweep that finds it, out of
//! the owner's sight, the open records naming it closed first with their restore line; one whose
//! records are closed already goes without a line. A kill that leaves its terminal live a while
//! is still removed once it finishes. Another board's or project's terminal, ended or live, is
//! never touched.

use super::{Agent, How, restore};
use crate::{Hooked, World};
use serde_json::{Value, json};
use std::cell::Cell;
use symphony_board::NewLaunch;
use symphony_board::launch::GONE;
use symphony_board::launch::reap::{Closed, ENDED, Kept, Sweep};
use symphony_board::launch::screen::ON_SCREEN;

impl World {
    /// The terminal of an agent `handle` launched on `slug`, whose program ended.
    fn ended_agent(&self, handle: &str, slug: &str) -> String {
        let terminal = self.agent(Agent::named(handle));
        self.store()
            .record_launch(&NewLaunch {
                slug: slug.into(),
                handle: handle.into(),
                role: "executor".into(),
                claude_session: format!("sess-{terminal}"),
                pane: terminal.clone(),
                log: false,
                ..NewLaunch::default()
            })
            .unwrap();
        self.edit_terminal(&terminal, |t| {
            t.insert("state".into(), json!("finished"));
            t.insert("status".into(), Value::Null);
        });
        terminal
    }
}

#[test]
fn an_ended_terminal_is_removed_at_the_first_sweep_its_open_record_closed_with_its_restore_line() {
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    let before = w.f.said("sim/jump").len();
    let line = restore("sim/jump", &handle);
    assert_eq!(
        w.sweep(),
        Sweep {
            closed: vec![Closed {
                pane: terminal.clone(),
                handle: handle.clone(),
                why: ENDED.into(),
                restore: line.clone(),
            }],
            ..Sweep::default()
        }
    );
    assert!(!w.has_terminal(&terminal));
    // Nothing runs in it: removed, never killed.
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
    assert_eq!(w.calls_of(&["rm", &terminal]).len(), 1);
    let [s] = <[_; 1]>::try_from(w.store().sessions_of(&handle).unwrap()).unwrap();
    assert!(s.closed.is_some());
    assert_eq!(
        (s.closed_why.as_str(), s.restore.as_str()),
        (ENDED, line.as_str())
    );
    assert_eq!(
        w.f.said("sim/jump")[before..],
        [format!(
            "closed the pane of {handle} ({ENDED}); restore: {line}"
        )]
    );
}

#[test]
fn an_ended_terminal_whose_session_closed_already_is_removed_without_a_post() {
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    let line = restore("sim/jump", &handle);
    w.store()
        .close_task_session("sim/jump", &handle, "closed by hand", &line)
        .unwrap();
    // and one that names no session at all
    let shell = w.add_terminal(false, None, json!({"state": "finished"}));
    let before = w.f.said("sim/jump").len();
    let swept = w.sweep();
    let ended = |pane: &str, handle: &str| Closed {
        pane: pane.into(),
        handle: handle.into(),
        why: ENDED.into(),
        restore: String::new(),
    };
    assert_eq!(swept.closed, [ended(&terminal, &handle), ended(&shell, "")]);
    assert!(w.addresses().is_empty());
    assert_eq!(w.f.said("sim/jump").len(), before);
    let [s] = <[_; 1]>::try_from(w.store().sessions_of(&handle).unwrap()).unwrap();
    assert_eq!(s.closed_why, "closed by hand");
}

#[test]
fn a_dry_run_names_the_ended_terminal_it_would_remove_and_changes_nothing() {
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    let swept = w.sweep_as(How {
        dry: true,
        ..How::default()
    });
    let closed = Closed {
        pane: terminal.clone(),
        handle: handle.clone(),
        why: ENDED.into(),
        restore: restore("sim/jump", &handle),
    };
    assert_eq!(swept.closed, [closed]);
    assert!(w.has_terminal(&terminal));
    assert_eq!(w.changes(), Vec::<Vec<String>>::new());
    assert!(w.store().sessions_of(&handle).unwrap()[0].closed.is_none());
}

#[test]
fn an_ended_terminal_on_screen_stays_until_the_owner_looks_away_and_goes_without_a_second_line() {
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    w.shows(json!({"agent": terminal}));
    let before = w.f.said("sim/jump").len();
    let kept = Kept {
        pane: terminal.clone(),
        handle: handle.clone(),
        why: ON_SCREEN.into(),
        idle: false,
        told: false,
    };
    assert_eq!(
        w.sweep(),
        Sweep {
            kept: vec![kept],
            ..Sweep::default()
        }
    );
    assert!(w.has_terminal(&terminal));
    // Its session ended with its program: closed, with its line, though the terminal stays.
    assert_eq!(w.f.said("sim/jump").len(), before + 1);
    w.shows(json!({}));
    assert_eq!(super::terminals(&w.sweep()), [terminal.as_str()]);
    assert!(!w.has_terminal(&terminal));
    assert_eq!(w.f.said("sim/jump").len(), before + 1);
}

#[test]
fn an_ended_terminal_hc_refuses_to_remove_stays_its_session_closed_and_goes_at_a_later_sweep() {
    let mut w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    let before = w.f.said("sim/jump").len();
    w.set("FAKE_HC_FAIL", "rm");
    let swept = w.sweep();
    assert_eq!(swept.closed, []);
    let [kept] = <[_; 1]>::try_from(swept.kept).unwrap();
    assert_eq!(
        (kept.pane.as_str(), kept.handle.as_str(), kept.idle),
        (terminal.as_str(), handle.as_str(), false)
    );
    assert!(
        kept.why.starts_with("hc refused to remove it: "),
        "{kept:?}"
    );
    assert!(w.has_terminal(&terminal));
    // Its program did end: its record closed, with its one line.
    assert_eq!(w.f.said("sim/jump").len(), before + 1);
    assert!(w.store().sessions_of(&handle).unwrap()[0].closed.is_some());
    w.set("FAKE_HC_FAIL", "");
    assert_eq!(super::terminals(&w.sweep()), [terminal.as_str()]);
    assert!(!w.has_terminal(&terminal));
    assert_eq!(w.f.said("sim/jump").len(), before + 1);
}

#[test]
fn an_ended_terminal_whose_program_runs_again_by_the_last_look_stays_its_session_open() {
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    // The sweep's own look finds it ended; the close's, right before the remove, live again.
    let gets = Cell::new(0);
    let mut hooked = Hooked::new(&w);
    hooked.after = Box::new(|args| {
        if args[0] == "get" {
            gets.set(gets.get() + 1);
            if gets.get() == 1 {
                w.edit_terminal(&terminal, |t| {
                    t.insert("state".into(), json!("live"));
                });
            }
        }
    });
    let (swept, warned) = w.sweep_with(&hooked, How::default());
    assert_eq!(warned, "");
    assert_eq!(gets.get(), 2);
    assert_eq!(swept.closed, []);
    let [kept] = <[_; 1]>::try_from(swept.kept).unwrap();
    assert_eq!(
        (kept.pane.as_str(), kept.why.as_str()),
        (terminal.as_str(), "its program runs again")
    );
    assert!(w.has_terminal(&terminal));
    let [s] = <[_; 1]>::try_from(w.store().sessions_of(&handle).unwrap()).unwrap();
    assert_eq!((s.closed, s.pane.as_str()), (None, terminal.as_str()));
}

#[test]
fn a_kill_that_leaves_its_terminal_live_a_while_is_removed_once_it_finishes() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent::named(&handle));
    // Its program takes three looks to exit after the kill, during which hc refuses to remove it.
    w.edit_terminal(&terminal, |t| {
        t.insert("ending".into(), json!(3));
    });
    w.sweep();
    assert_eq!(super::terminals(&w.sweep()), [terminal.as_str()]);
    assert!(!w.has_terminal(&terminal));
    let after_kill: Vec<String> = w
        .calls()
        .into_iter()
        .skip_while(|c| c[0] != "kill")
        .map(|c| c[..2].join(" "))
        .collect();
    let looked = format!("get {terminal}");
    assert_eq!(
        after_kill,
        [
            format!("kill {terminal}"),
            looked.clone(),
            looked.clone(),
            looked,
            format!("rm {terminal}")
        ]
    );
}

#[test]
fn a_shell_or_another_boards_or_projects_terminal_is_never_swept() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let shell = w.add_terminal(false, Some(&handle), json!({"status": "idle"}));
    let mut others = vec![shell.clone()];
    for board in [
        json!("web-bench/tasks"),
        json!("lamp-works/scratch"),
        json!(null),
    ] {
        for state in ["live", "finished"] {
            let t = w.agent(Agent::named(&handle));
            w.edit_terminal(&t, |row| {
                row["labels"]["sf.board"] = board.clone();
                row.insert("state".into(), json!(state));
            });
            others.push(t);
        }
    }
    let swept = w.twice(How::default());
    for t in &others {
        assert!(w.has_terminal(t), "{t}");
    }
    assert_eq!(w.closes(), Vec::<Vec<String>>::new());
    let looked: Vec<String> = w
        .calls_of(&["get"])
        .into_iter()
        .map(|c| c[1].clone())
        .collect();
    assert_eq!(
        looked,
        [shell.clone(), shell.clone()],
        "only this board's live ones"
    );
    // The shell is this board's: kept, saying why, as no idle agent's.
    let kept = Kept {
        pane: shell,
        handle,
        why: "no agent runs in it".into(),
        idle: false,
        told: false,
    };
    assert_eq!(
        swept,
        Sweep {
            kept: vec![kept],
            ..Sweep::default()
        }
    );
}

#[test]
fn an_ended_terminal_never_closes_a_record_a_restore_moved_to_another_terminal_since() {
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    // After the sweep read its record, a restore starts its session in a new terminal: as the
    // sweep's own look at the ended one answers.
    let moved = Cell::new(false);
    let mut hooked = Hooked::new(&w);
    hooked.instead = Box::new(|args| {
        if args[0] == "get" && args[1] == terminal && !moved.replace(true) {
            let store = w.store();
            let [record] = <[_; 1]>::try_from(store.sessions_of(&handle).unwrap()).unwrap();
            let hold = store.take_session(&handle, "a restore").unwrap();
            let line = restore("sim/jump", &handle);
            assert!(
                store
                    .close_task_session_as(&record, GONE, &line, hold)
                    .unwrap()
            );
            assert!(
                store
                    .reopen_task_session_as(&record, "t-new", hold)
                    .unwrap()
            );
            store.release_session(&handle, hold).unwrap();
        }
        None
    });
    let (swept, warned) = w.sweep_with(&hooked, How::default());
    assert_eq!(warned, "");
    // The ended terminal goes, with no line: the record is the new terminal's.
    let gone = Closed {
        pane: terminal.clone(),
        handle: handle.clone(),
        why: ENDED.into(),
        restore: String::new(),
    };
    assert_eq!(swept.closed, [gone]);
    assert!(!w.has_terminal(&terminal));
    let [s] = <[_; 1]>::try_from(w.store().sessions_of(&handle).unwrap()).unwrap();
    assert_eq!((s.closed, s.pane.as_str()), (None, "t-new"));
}

#[test]
fn an_ended_terminal_stays_while_a_restore_of_its_session_is_under_way_and_holds_it_while_removed()
{
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    let terminal = w.ended_agent(&handle, "sim/jump");
    let store = w.store();
    let hold = store.take_session(&handle, "a restore").unwrap();
    let why = format!(
        "the board took no record of its end: a restore of {handle} is under way: its hold lapses \
         in 300 s"
    );
    let kept = Kept {
        pane: terminal.clone(),
        handle: handle.clone(),
        why,
        idle: false,
        told: false,
    };
    assert_eq!(w.sweep().kept, [kept]);
    assert!(w.has_terminal(&terminal));
    assert!(store.sessions_of(&handle).unwrap()[0].closed.is_none());
    store.release_session(&handle, hold).unwrap();
    // Once it is done, the next sweep removes the terminal, holding the session meanwhile: a
    // restore asked for while it removes it is refused, and one asked for as the sweep looks at
    // the next terminal takes it.
    let other = w.agent(Agent::named(&w.f.session("worker")));
    let (refused, taken) = (Cell::new(None), Cell::new(false));
    let mut hooked = Hooked::new(&w);
    hooked.instead = Box::new(|args| {
        let restore = || store.take_session(&handle, "a restore");
        if args[0] == "rm" {
            refused.set(restore().err());
        } else if args[0] == "get" && args[1] == other && !taken.get() {
            let hold = restore().unwrap();
            store.release_session(&handle, hold).unwrap();
            taken.set(true);
        }
        None
    });
    let (swept, warned) = w.sweep_with(&hooked, How::default());
    assert_eq!(warned, "");
    assert_eq!(super::terminals(&swept), [terminal.as_str()]);
    assert_eq!(
        refused.take().map(|e| e.to_string()),
        Some(format!(
            "the idle sweep's removal of its ended terminal of {handle} is under way: its hold \
             lapses in 300 s"
        ))
    );
    assert!(taken.get());
    assert!(store.sessions_of(&handle).unwrap()[0].closed.is_some());
}
