//! `close`: one agent's terminal on its orchestrator's word, whatever it holds, recorded with its
//! restore line; refused for the caller's own, for one the owner may be looking at, and for one
//! running a verify, a test run, a land or the Symphony.

use crate::common::refused;
use crate::{Hooked, World};
use serde_json::json;
use std::cell::Cell;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::screen::ON_SCREEN;
use symphony_board::launch::{Closure, GONE, LaunchError, close, restore};
use symphony_board::rows::TaskSessionRow;
use symphony_board::{Error, NewLaunch};

impl World {
    /// A Claude reviewer launched on sim/jump, which an executor of its own has in review, its agent
    /// idle as it waits for a new tip: its record.
    fn waiting_reviewer(&self) -> TaskSessionRow {
        let executor = self.f.session("worker");
        self.store().claim("sim/jump", &executor).unwrap();
        self.store()
            .move_to("sim/jump", "review", &executor)
            .unwrap();
        let s = self.review("sim/jump").row;
        self.edit_terminal(&s.pane, |t| {
            t.insert("status".into(), json!("idle"));
        });
        s
    }

    /// The orchestrator closes `handle`'s terminal on sim/jump through `hc`.
    fn close_through(&self, hc: &dyn Hc, handle: &str) -> Result<Closure, Error> {
        let m = self.machine();
        close(
            self.store(),
            hc,
            &*m.processes,
            &self.orch,
            "sim/jump",
            handle,
        )
    }

    fn close(&self, handle: &str) -> Result<Closure, Error> {
        self.close_through(&self.hc(), handle)
    }

    /// Nothing was killed or removed, and no record of sim/jump closed.
    fn untouched(&self) {
        assert_eq!(
            self.calls_of(&["kill"]).len() + self.calls_of(&["rm"]).len(),
            0
        );
        let records = self.store().task_sessions("sim/jump").unwrap();
        assert!(records.iter().all(|s| s.closed.is_none()), "{records:?}");
    }
}

fn restore_line(handle: &str) -> String {
    format!("just tasks restore sim/jump {handle}")
}

#[test]
fn an_idle_reviewer_waiting_for_a_new_tip_is_closed_and_recorded_with_its_restore_line() {
    let w = World::new();
    let s = w.waiting_reviewer();
    let h = &s.handle;
    w.store()
        .callback_in(h, Some("codex:0abc-12"), false, false, Some(&s.pane))
        .unwrap();
    assert_eq!(w.clean(false, &[]), [], "cleanup keeps a live role");
    let why = format!("closed by {}", w.orch);
    assert_eq!(
        w.close(h).unwrap(),
        Closure {
            pane: s.pane.clone(),
            handle: Some(h.clone()),
            why: why.clone(),
            restore: restore_line(h),
            closed: true,
            kept: false,
        }
    );
    assert!(!w.has_terminal(&s.pane));
    let closing: Vec<[String; 2]> = w
        .changes()
        .into_iter()
        .filter(|c| c[0] == "kill" || c[0] == "rm")
        .map(|c| [c[0].clone(), c[1].clone()])
        .collect();
    assert_eq!(
        closing,
        [
            ["kill".to_string(), s.pane.clone()],
            ["rm".to_string(), s.pane.clone()]
        ]
    );
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert!(read.closed.is_some());
    assert_eq!(
        (read.closed_why.as_str(), read.restore.as_str()),
        (why.as_str(), restore_line(h).as_str())
    );
    assert_eq!(
        w.f.said("sim/jump").last().unwrap(),
        &format!(
            "closed the pane of {h} ({why}); restore: {}",
            restore_line(h)
        )
    );
    // Its channel is retired with it; its role stays, and restore brings it back.
    assert!(!w.store().callbacks().unwrap().contains_key(h));
    assert!(
        w.store()
            .get("sim/jump")
            .unwrap()
            .holders("reviewer")
            .contains(&h.as_str())
    );
    let [again] = <[_; 1]>::try_from(
        restore(w.store(), &w.hc(), &w.machine(), "sim/jump", Some(h), None).unwrap(),
    )
    .unwrap();
    assert!(w.live().contains(&again.row.pane));
    assert_eq!(again.row.claude_session, s.claude_session);
}

#[test]
fn a_terminal_whose_program_ended_is_only_removed() {
    let w = World::new();
    let s = w.waiting_reviewer();
    w.edit_terminal(&s.pane, |t| {
        t.insert("state".into(), json!("finished"));
    });
    assert!(w.close(&s.handle).unwrap().closed);
    assert!(w.calls_of(&["kill"]).is_empty());
    assert_eq!(w.calls_of(&["rm"]).len(), 1);
    assert!(!w.has_terminal(&s.pane));
}

#[test]
fn the_callers_own_session_or_terminal_is_refused() {
    let mut w = World::new();
    let s = w.waiting_reviewer();
    let m = w.machine();
    let own = close(
        w.store(),
        &w.hc(),
        &*m.processes,
        &s.handle,
        "sim/jump",
        &s.handle,
    );
    assert_eq!(
        refused(own),
        format!(
            "{} is you: a session never closes its own terminal",
            s.handle
        )
    );
    w.set("HC_TERMINAL_ID", &s.pane);
    assert_eq!(
        refused(w.close(&s.handle)),
        format!(
            "terminal {} is the one you run in: a session never closes its own terminal",
            s.pane
        )
    );
    assert!(w.has_terminal(&s.pane));
    w.untouched();
}

#[test]
fn a_terminal_the_owner_may_be_looking_at_is_refused() {
    let mut w = World::new();
    let s = w.waiting_reviewer();
    let tab = w.tab_of(&s.pane);
    for shown in [
        json!({"tab": tab}),
        json!({"agent": s.pane}),
        json!({"overlays": [{"terminal": s.pane}]}),
        json!({"floats": {"f1": {"terminal": s.pane}}}),
    ] {
        w.shows(shown.clone());
        assert_eq!(
            refused(w.close(&s.handle)),
            format!("terminal {} stays: {ON_SCREEN}", s.pane),
            "{shown}"
        );
    }
    w.shows(json!({}));
    w.set("FAKE_HC_FAIL", "app status");
    assert_eq!(
        refused(w.close(&s.handle)),
        format!(
            "terminal {} stays: never closed on no information: herdr-app status: the window did \
             not answer",
            s.pane
        )
    );
    assert!(w.has_terminal(&s.pane));
    w.untouched();
}

#[test]
fn a_terminal_running_a_verify_a_test_run_a_land_or_the_symphony_is_refused() {
    let w = World::new();
    let s = w.waiting_reviewer();
    let pid = w.pid_of(&s.pane);
    for (command, what) in [
        ("just verify", "a verify"),
        ("just test-sim SUITE=jump", "a test run"),
        ("just land tools/x", "a land"),
        ("just symphony", "the Symphony"),
    ] {
        w.run_under(pid, &["/bin/zsh -c x", command]);
        assert_eq!(
            refused(w.close(&s.handle)),
            format!("terminal {} stays: it runs {what} ({command})", s.pane)
        );
    }
    // The program the terminal started is read too.
    w.run_under(pid, &[]);
    w.edit_terminal(&s.pane, |t| {
        t.insert("argv".into(), json!(["env", "A=1", "just", "symphony"]));
    });
    assert_eq!(
        refused(w.close(&s.handle)),
        format!(
            "terminal {} stays: it runs the Symphony (just symphony)",
            s.pane
        )
    );
    assert!(w.has_terminal(&s.pane));
    w.untouched();
}

#[test]
fn a_shell_or_a_monitor_under_the_agent_is_no_reason_to_keep_it() {
    let w = World::new();
    let s = w.waiting_reviewer();
    w.run_under(
        w.pid_of(&s.pane),
        &["/bin/zsh -c tail -f log", "tail -f log"],
    );
    assert!(w.close(&s.handle).unwrap().closed);
    assert!(!w.has_terminal(&s.pane));
}

#[test]
fn a_terminal_whose_processes_cannot_be_read_is_refused() {
    let w = World::new();
    let s = w.waiting_reviewer();
    let unreadable = |_: i64| Err(LaunchError("ps printed nothing".into()));
    let closed = close(
        w.store(),
        &w.hc(),
        &unreadable,
        &w.orch,
        "sim/jump",
        &s.handle,
    );
    assert_eq!(
        refused(closed),
        format!(
            "terminal {} stays: never closed on no information: ps printed nothing",
            s.pane
        )
    );
    w.edit_terminal(&s.pane, |t| {
        t.insert("pid".into(), json!(null));
    });
    assert_eq!(
        refused(w.close(&s.handle)),
        format!(
            "terminal {} stays: hc knows no pid for it, so what runs there cannot be read",
            s.pane
        )
    );
    w.untouched();
}

#[test]
fn work_started_or_another_program_by_the_last_look_keeps_the_terminal() {
    let w = World::new();
    let s = w.waiting_reviewer();
    let pid = w.pid_of(&s.pane);
    // A verify started between the judgement and the look right before the kill.
    let gets = Cell::new(0);
    let mut started = Hooked::new(&w);
    started.after = Box::new(|args| {
        if args[0] == "get" {
            gets.set(gets.get() + 1);
            if gets.get() == 2 {
                w.run_under(pid, &["just verify"]);
            }
        }
    });
    assert_eq!(
        refused(w.close_through(&started, &s.handle)),
        format!("terminal {} stays: it runs a verify (just verify)", s.pane)
    );
    w.untouched();
    // Another program took the terminal by then.
    w.run_under(pid, &[]);
    let gets = Cell::new(0);
    let mut replaced = Hooked::new(&w);
    replaced.after = Box::new(|args| {
        if args[0] == "get" {
            gets.set(gets.get() + 1);
            if gets.get() == 1 {
                w.edit_terminal(&s.pane, |t| {
                    t.insert("pid".into(), json!(pid + 1000));
                });
            }
        }
    });
    assert_eq!(
        refused(w.close_through(&replaced, &s.handle)),
        format!("terminal {} stays: another program runs in it now", s.pane)
    );
    assert!(w.has_terminal(&s.pane));
    w.untouched();
}

#[test]
fn a_kill_hc_refuses_records_nothing() {
    let mut w = World::new();
    let s = w.waiting_reviewer();
    let before = w.f.said("sim/jump");
    w.set("FAKE_HC_FAIL", "kill");
    assert_eq!(
        refused(w.close(&s.handle)),
        format!("hc kill {}: fake_failure: kill failed", s.pane)
    );
    assert_eq!(w.f.said("sim/jump"), before);
    assert!(
        w.store()
            .task_sessions("sim/jump")
            .unwrap()
            .iter()
            .all(|r| r.closed.is_none())
    );
}

#[test]
fn a_terminal_of_another_board_or_another_handle_is_refused() {
    let w = World::new();
    let s = w.waiting_reviewer();
    let board = w.board();
    for (labelled, said) in [
        (json!("web-bench/tasks"), "web-bench/tasks"),
        (json!(null), "no board"),
    ] {
        w.edit_terminal(&s.pane, |t| {
            t["labels"]["sf.board"] = labelled.clone();
        });
        assert_eq!(
            refused(w.close(&s.handle)),
            format!(
                "terminal {} is not this board's ({said}): the board closes only its own",
                s.pane
            )
        );
    }
    w.edit_terminal(&s.pane, |t| {
        t["labels"]["sf.board"] = json!(board);
        t["labels"]["sf.handle"] = json!("other-owl");
    });
    assert_eq!(
        refused(w.close(&s.handle)),
        format!("terminal {} runs other-owl, not {}", s.pane, s.handle)
    );
    assert!(w.has_terminal(&s.pane));
    w.untouched();
}

#[test]
fn a_recorded_terminal_closed_outside_the_board_is_recorded_gone_with_nothing_killed() {
    let w = World::new();
    let s = w.waiting_reviewer();
    w.edit_terminals(|all| all.clear());
    let restore = restore_line(&s.handle);
    assert_eq!(
        w.close(&s.handle).unwrap(),
        Closure {
            pane: s.pane.clone(),
            handle: Some(s.handle.clone()),
            why: GONE.into(),
            restore: restore.clone(),
            closed: true,
            kept: false,
        }
    );
    assert_eq!(w.calls_of(&["kill"]).len() + w.calls_of(&["rm"]).len(), 0);
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (read.closed.is_some(), read.closed_why.as_str()),
        (true, GONE)
    );
    assert_eq!(
        w.f.said("sim/jump").last().unwrap(),
        &format!(
            "closed the pane of {} ({GONE}); restore: {restore}",
            s.handle
        )
    );
}

#[test]
fn a_herdr_pane_of_the_record_is_gone_without_asking_hc() {
    let w = World::new();
    let h = w.f.session("reviewer");
    w.store()
        .record_launch(&NewLaunch {
            claude_session: "c-1".into(),
            pane: "w1:p3".into(),
            ..NewLaunch::new("sim/jump", &h, "reviewer")
        })
        .unwrap();
    assert_eq!(w.close(&h).unwrap().why, GONE);
    assert!(w.calls().is_empty(), "{:?}", w.calls());
    assert!(w.store().open_task_sessions().unwrap().is_empty());
}

#[test]
fn an_agent_with_a_role_and_no_record_is_found_by_its_label_and_recorded_where_closed() {
    let w = World::new();
    let executor = w.f.session("worker");
    w.store().claim("sim/jump", &executor).unwrap();
    w.store().move_to("sim/jump", "review", &executor).unwrap();
    let h = w.f.session("reviewer");
    w.store().take_role("sim/jump", &h, "reviewer").unwrap();
    let terminal = w.add_terminal(true, Some(&h), json!({"name": null}));
    let before = w.f.said("sim/jump").len();
    assert!(w.close(&h).unwrap().closed);
    assert!(!w.has_terminal(&terminal));
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (
            read.role.as_str(),
            read.pane.as_str(),
            read.claude_session.as_str(),
            read.worktree.as_str()
        ),
        (
            "reviewer",
            terminal.as_str(),
            format!("sess-{terminal}").as_str(),
            "/somewhere"
        )
    );
    assert!(read.closed.is_some());
    // One line: the closure, with its restore line; no line of its being found.
    let said = w.f.said("sim/jump");
    assert_eq!(
        said[before..],
        [format!(
            "closed the pane of {h} (closed by {}); restore: {}",
            w.orch,
            restore_line(&h)
        )]
    );
}

#[test]
fn an_agent_found_by_its_name_alone_is_closed_too() {
    let w = World::new();
    let h = w.f.session("worker");
    w.store().claim("sim/jump", &h).unwrap();
    let terminal = w.add_terminal(true, None, json!({"name": h}));
    assert!(w.close(&h).unwrap().closed);
    assert!(!w.has_terminal(&terminal));
    assert_eq!(
        w.store().task_sessions("sim/jump").unwrap()[0].role,
        "executor"
    );
}

#[test]
fn a_handle_with_nothing_on_the_task_or_no_terminal_or_an_unknown_task_is_refused() {
    let w = World::new();
    let stranger = w.f.session("worker");
    assert_eq!(
        refused(w.close(&stranger)),
        format!("{stranger} has no recorded session or role on sim/jump")
    );
    w.store().claim("sim/jump", &stranger).unwrap();
    assert_eq!(
        refused(w.close(&stranger)),
        format!("{stranger} has no open session on sim/jump and runs in no terminal of this board")
    );
    assert_eq!(refused(w.close("ghost-owl")), "no session 'ghost-owl'");
    let m = w.machine();
    let elsewhere = close(
        w.store(),
        &w.hc(),
        &*m.processes,
        &w.orch,
        "sim/none",
        &stranger,
    );
    assert_eq!(refused(elsewhere), "no task 'sim/none'");
}
