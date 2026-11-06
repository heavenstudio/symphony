//! `cleanup`: which terminals close, which stay, and what the board records of each closure.

use crate::World;
use serde_json::json;
use symphony_board::launch::{Ask, Closure, GONE, LaunchError, cleanup};
use symphony_board::{Error, NewLaunch};

pub const SESSION_ID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

impl World {
    /// A launched executor of `slug`, its task `status` if one is given, its agent idle with nothing
    /// running under it: (handle, terminal).
    pub fn worker(&self, slug: &str, status: Option<&str>) -> (String, String) {
        let s = self.launch(slug).row;
        self.edit_terminal(&s.pane, |t| {
            t.insert("status".into(), "idle".into());
        });
        if let Some(status) = status {
            self.finish_task(slug, &s.handle, status);
        }
        (s.handle, s.pane)
    }

    /// `handle` claims `slug` and takes it to `status`: cancelled, else reviewed and done.
    pub fn finish_task(&self, slug: &str, handle: &str, status: &str) {
        let store = self.store();
        store.claim(slug, handle).unwrap();
        if status == "cancelled" {
            store.move_to(slug, "cancelled", handle).unwrap();
        } else {
            store.move_to(slug, "review", handle).unwrap();
            let reviewer = self.f.session("reviewer");
            store.take_role(slug, &reviewer, "reviewer").unwrap();
            store.verdict(slug, &reviewer, "approve", "ok").unwrap();
        }
    }

    pub fn clean(&self, dry_run: bool, only: &[&str]) -> Vec<Closure> {
        let only: Vec<String> = only.iter().map(|s| s.to_string()).collect();
        let m = self.machine();
        cleanup(self.store(), &self.hc(), &*m.processes, dry_run, &only).unwrap()
    }
}

#[test]
fn an_idle_agent_of_a_finished_task_is_killed_removed_and_its_closure_recorded() {
    for status in ["done", "cancelled"] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some(status));
        let restore = format!("just tasks restore sim/jump {handle}");
        assert_eq!(
            w.clean(false, &[]),
            [Closure {
                pane: terminal.clone(),
                handle: Some(handle.clone()),
                why: "its tasks are finished".into(),
                restore: restore.clone(),
                closed: true,
                kept: false,
            }],
            "{status}"
        );
        assert!(!w.has_terminal(&terminal));
        let closing: Vec<String> = w
            .changes()
            .into_iter()
            .filter(|c| c[0] != "run" && c[0] != "app")
            .map(|c| c[0].clone())
            .collect();
        assert_eq!(closing, ["kill", "rm"]);
        let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert!(read.closed.is_some());
        assert_eq!(
            (read.closed_why.as_str(), read.restore.as_str()),
            ("its tasks are finished", restore.as_str())
        );
        assert!(w.f.said("sim/jump").iter().any(|m| m.contains(&restore)));
    }
}

#[test]
fn a_terminal_whose_task_is_still_held_is_kept() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", None);
    w.store().claim("sim/jump", &handle).unwrap();
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));
}

#[test]
fn an_idle_agent_holding_nothing_live_is_closed() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", None);
    w.store().leave(&handle).unwrap();
    let [closure] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert_eq!(
        (closure.why.as_str(), closure.closed),
        ("it holds nothing live", true)
    );
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn a_working_blocked_or_silent_agent_is_never_closed_and_a_done_one_is() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    for status in [json!("working"), json!("blocked"), json!(null)] {
        w.edit_terminal(&terminal, |t| {
            t.insert("status".into(), status.clone());
        });
        assert_eq!(w.clean(false, &[]), [], "{status}");
    }
    w.edit_terminal(&terminal, |t| {
        t.insert("status".into(), "done".into());
    });
    assert_eq!(w.clean(false, &[]).len(), 1);
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn a_dry_run_names_the_closures_and_changes_nothing() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    let before = w.changes();
    let [closure] = <[_; 1]>::try_from(w.clean(true, &[])).unwrap();
    assert!(!closure.closed);
    assert!(w.has_terminal(&terminal));
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
    assert_eq!(w.changes(), before);
}

#[test]
fn an_idle_shell_of_the_board_is_closed_too_but_not_one_running_a_command() {
    let w = World::new();
    let terminal = w.add_terminal(false, None, json!({}));
    assert_eq!(
        w.clean(false, &[]),
        [Closure {
            pane: terminal.clone(),
            handle: None,
            why: "an idle shell".into(),
            restore: String::new(),
            closed: true,
            kept: false,
        }]
    );
    assert!(!w.has_terminal(&terminal));
    let busy = w.add_terminal(false, None, json!({}));
    w.run_under(w.pid_of(&busy), &["sleep 100"]);
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&busy));
}

#[test]
fn another_boards_or_projects_terminals_and_unlabelled_ones_are_never_touched() {
    let w = World::new();
    let theirs = [
        w.add_terminal(
            false,
            None,
            json!({"labels": {"sf.board": "web-bench/tasks"}}),
        ),
        w.add_terminal(
            false,
            None,
            json!({"labels": {"sf.board": "lamp-works/scratch"}}),
        ),
        w.add_terminal(
            true,
            Some(&w.f.session("worker")),
            json!({"labels": {"sf.board": null}}),
        ),
    ];
    assert_eq!(w.clean(false, &[]), []);
    for t in &theirs {
        assert!(w.has_terminal(t), "{t}");
    }
    assert!(w.calls_of(&["get"]).is_empty());
    assert_eq!(w.changes(), Vec::<Vec<String>>::new());
}

#[test]
fn an_agent_terminal_with_a_shell_or_monitor_running_is_kept() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    let pid = w.pid_of(&terminal);
    for command in [
        "tail -f log",
        "/repo/.git/sf-cargo/bin/tasks-0123456789abcdef attach --kind worker",
        "/bin/zsh -c sleep 5",
    ] {
        w.run_under(pid, &["claude --model x", command]);
        assert_eq!(w.clean(false, &[]), [], "{command}");
    }
}

#[test]
fn a_running_verify_test_run_land_or_symphony_app_is_never_interrupted() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    let pid = w.pid_of(&terminal);
    for command in [
        "just verify",
        "python3 tools/run_sim_tests.py",
        "godot --headless -s tests/run_sim_tests.gd",
        "just test-sim SUITE=a",
        "just land tools/x",
        "python3 tools/land.py tools/x",
        "just symphony",
        "/app/bin/symphony-server serve",
        "just verify-full",
    ] {
        w.run_under(pid, &[command]);
        assert_eq!(w.clean(true, &[]), [], "{command}");
    }
    assert!(w.has_terminal(&terminal));
}

#[test]
fn the_agent_itself_and_its_helpers_are_not_a_running_command() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.run_under(
        w.pid_of(&terminal),
        &["claude --model sonnet -n x", "node /x/mcp-server.js"],
    );
    assert_eq!(w.clean(false, &[]).len(), 1);
}

#[test]
fn codex_interactive_arguments_can_close_but_exec_jobs_stay() {
    for (command, closures) in [
        ("codex --profile e", 1),
        ("codex resume thread Explain exec behavior", 1),
        ("codex -- exec", 1),
        ("codex -- e", 1),
        ("codex exec draw", 0),
        ("codex e draw", 0),
    ] {
        let w = World::new();
        let (_, terminal) = w.worker("sim/jump", Some("done"));
        w.run_under(w.pid_of(&terminal), &[command]);
        assert_eq!(w.clean(true, &[]).len(), closures, "{command}");
    }
}

#[test]
fn the_orchestrators_and_the_callers_terminal_are_never_closed() {
    let w = World::new();
    let terminal = w.add_terminal(true, Some(&w.orch.clone()), json!({}));
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));

    let mut w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.set("HC_TERMINAL_ID", &terminal);
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));
}

#[test]
fn an_agent_terminal_that_maps_to_no_session_is_kept() {
    let w = World::new();
    let terminal = w.add_terminal(true, None, json!({}));
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));
}

#[test]
fn a_terminal_is_matched_by_its_record_label_conversation_bound_agent_name_flag_or_name() {
    let w = World::new();
    let ask = Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/jump")
    };
    let s = w.try_launch(&ask).unwrap().row;
    let (done, launched) = (s.handle, s.pane);
    w.edit_terminal(&launched, |t| {
        t.insert("status".into(), "idle".into());
    });
    w.finish_task("sim/jump", &done, "done");
    // Its record and its label say the launched one; drop both and the conversation tells.
    w.edit_terminal(&launched, |t| {
        t.insert("labels".into(), json!({"sf.board": "lamp-works/tasks"}));
        t.insert("name".into(), json!("someone"));
    });
    w.store()
        .reopen_task_session("sim/jump", &done, "t-elsewhere")
        .unwrap();
    let [gone, by_conversation] = <[_; 2]>::try_from(w.clean(true, &[])).unwrap();
    assert_eq!(
        (gone.pane.as_str(), gone.why.as_str()),
        ("t-elsewhere", GONE)
    );
    assert_eq!(
        (by_conversation.pane, by_conversation.handle),
        (launched, Some(done.clone()))
    );

    let w = World::new();
    let named = w.f.session("worker");
    w.task("sim/dock", None);
    w.finish_task("sim/dock", &named, "done");
    for (how, extra) in [
        ("label", json!({"labels": {"sf.handle": named}})),
        (
            "name flag",
            json!({"labels": {"sf.handle": ""}, "argv": ["claude", "-n", named]}),
        ),
        ("name", json!({"name": named, "argv": ["claude"]})),
    ] {
        let t = w.add_terminal(true, None, extra);
        let [closure] = <[_; 1]>::try_from(w.clean(true, &[])).unwrap();
        assert_eq!(
            (closure.pane, closure.handle),
            (t.clone(), Some(named.clone())),
            "{how}"
        );
        w.edit_terminals(|all| all.retain(|x| x["address"] != t.as_str()));
    }

    let w = World::new();
    let bound = w.f.session("worker");
    w.task("sim/dock", None);
    w.finish_task("sim/dock", &bound, "done");
    w.store().bind_agent(SESSION_ID, &bound, true).unwrap();
    let t = w.add_terminal(
        true,
        None,
        json!({"argv": ["claude", "--resume", SESSION_ID]}),
    );
    let [closure] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert_eq!((closure.pane, closure.handle), (t, Some(bound)));
}

#[test]
fn a_terminal_that_was_not_launched_is_recorded_as_found_from_what_hc_knows() {
    let w = World::new();
    let handle = w.f.session("worker");
    w.task("sim/dock", None);
    w.finish_task("sim/dock", &handle, "done");
    let terminal = w.add_terminal(
        true,
        Some(&handle),
        json!({"cwd": "/some/wt", "argv": ["claude", "--session-id", SESSION_ID]}),
    );
    let [closure] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert_eq!(closure.handle.as_deref(), Some(handle.as_str()));
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/dock").unwrap()).unwrap();
    assert_eq!(
        (
            read.claude_session.as_str(),
            read.worktree.as_str(),
            read.pane.as_str(),
            read.role.as_str()
        ),
        (SESSION_ID, "/some/wt", terminal.as_str(), "executor")
    );
    assert!(read.closed.is_some());
    let said = w.f.said("sim/dock");
    assert!(
        !said
            .iter()
            .any(|t| t.starts_with(&format!("launched {handle}"))),
        "{said:?}"
    );
    assert!(
        said.iter()
            .any(|t| t.contains(&format!("found {handle} running"))),
        "{said:?}"
    );
}

#[test]
fn a_codex_terminal_found_records_its_thread_once_known_else_codex_alone() {
    let w = World::new();
    let handle = w.f.session("worker");
    w.task("sim/dock", None);
    w.finish_task("sim/dock", &handle, "done");
    w.add_terminal(
        true,
        Some(&handle),
        json!({"agent": "codex", "argv": ["codex", "--no-daemon"]}),
    );
    w.clean(false, &[]);
    assert_eq!(
        w.store().task_sessions("sim/dock").unwrap()[0].claude_session,
        "codex:"
    );
}

#[test]
fn an_agent_terminal_with_a_handle_but_no_task_to_record_it_on_is_kept() {
    let w = World::new();
    let handle = w.f.session("worker");
    let terminal = w.add_terminal(true, Some(&handle), json!({}));
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));
}

#[test]
fn only_the_named_tasks_sessions_are_cleaned_when_asked() {
    let w = World::new();
    w.task("sim/dock", None);
    let (_, a) = w.worker("sim/jump", Some("done"));
    let (_, b) = w.worker("sim/dock", Some("done"));
    let [closure] = <[_; 1]>::try_from(w.clean(false, &["sim/dock"])).unwrap();
    assert_eq!(closure.pane, b);
    assert!(w.has_terminal(&a));
}

#[test]
fn a_terminal_hc_cannot_describe_or_whose_processes_cannot_be_read_is_kept_and_says_why() {
    let mut w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    let kept = |handle: Option<&str>, why: &str| Closure {
        pane: terminal.clone(),
        handle: handle.map(String::from),
        why: format!("never closed on no information: {why}"),
        restore: String::new(),
        closed: false,
        kept: true,
    };
    // `hc get` fails before the terminal's session is looked for: of no known session.
    w.set("FAKE_HC_FAIL", "get");
    for dry_run in [true, false] {
        assert_eq!(
            w.clean(dry_run, &[]),
            [kept(
                None,
                &format!("hc get {terminal}: fake_failure: get failed")
            )]
        );
    }
    w.set("FAKE_HC_FAIL", "");
    // The process table fails for a terminal the board would close: its session is named.
    let broken =
        |_: i64| -> Result<Vec<String>, LaunchError> { Err(LaunchError("ps failed".into())) };
    assert_eq!(
        cleanup(w.store(), &w.hc(), &broken, false, &[]).unwrap(),
        [kept(Some(&handle), "ps failed")]
    );
    assert!(w.calls_of(&["kill"]).is_empty() && w.calls_of(&["rm"]).is_empty());
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
    assert!(w.has_terminal(&terminal));
}

#[test]
fn a_process_table_that_fails_is_said_only_of_a_terminal_the_board_would_close() {
    let w = World::new();
    let (handle, held) = w.worker("sim/jump", None);
    w.store().claim("sim/jump", &handle).unwrap(); // live work: kept whatever runs there
    let orchestrator = w.add_terminal(true, Some(&w.orch), json!({}));
    let asked = std::rc::Rc::new(std::cell::Cell::new(0));
    let counted = std::rc::Rc::clone(&asked);
    let broken = move |_: i64| -> Result<Vec<String>, LaunchError> {
        counted.set(counted.get() + 1);
        Err(LaunchError("ps failed".into()))
    };
    assert_eq!(
        cleanup(w.store(), &w.hc(), &broken, false, &[]).unwrap(),
        []
    );
    assert_eq!(asked.get(), 0, "the board said first: no table asked for");
    assert!(w.has_terminal(&held) && w.has_terminal(&orchestrator));
}

#[test]
fn a_terminal_hc_knows_no_pid_of_is_not_known_idle_and_stays_unsaid() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.edit_terminal(&terminal, |t| {
        t.insert("pid".into(), 0.into());
    });
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));
}

#[test]
fn a_list_hc_refuses_is_a_refusal_naming_it() {
    let mut w = World::new();
    w.set("FAKE_HC_FAIL", "ls");
    let m = w.machine();
    match cleanup(w.store(), &w.hc(), &*m.processes, false, &[]) {
        Err(Error::Refused(why)) => assert_eq!(why, "hc ls: fake_failure: ls failed"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_close_hc_refuses_is_said_and_records_nothing() {
    let mut w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.set("FAKE_HC_FAIL", "kill");
    let [closure] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert_eq!(
        (closure.pane.as_str(), closure.closed),
        (terminal.as_str(), false)
    );
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
}

#[test]
fn a_terminal_whose_program_ended_is_removed_and_its_record_closed_with_its_restore_line() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", None);
    w.edit_terminal(&terminal, |t| {
        t.insert("state".into(), "finished".into());
        t.insert("exit_code".into(), 0.into());
    });
    let restore = format!("just tasks restore sim/jump {handle}");
    assert_eq!(
        w.clean(false, &[]),
        [
            Closure {
                pane: terminal.clone(),
                handle: Some(handle.clone()),
                why: GONE.into(),
                restore: restore.clone(),
                closed: true,
                kept: false,
            },
            Closure {
                pane: terminal.clone(),
                handle: Some(handle.clone()),
                why: "its program ended".into(),
                restore: String::new(),
                closed: true,
                kept: false,
            },
        ]
    );
    assert!(!w.has_terminal(&terminal));
    assert!(w.calls_of(&["kill"]).is_empty());
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (read.closed_why.as_str(), read.restore.as_str()),
        (GONE, restore.as_str())
    );
}

#[test]
fn a_record_of_a_herdr_pane_or_a_terminal_closed_outside_the_board_is_recorded_gone() {
    for pane in ["w1:p3", "t-closed-by-hand"] {
        let w = World::new();
        let handle = w.f.session("worker");
        w.store()
            .record_launch(&NewLaunch {
                claude_session: SESSION_ID.into(),
                worktree: "/some/wt".into(),
                pane: pane.into(),
                ..NewLaunch::new("sim/jump", &handle, "executor")
            })
            .unwrap();
        let restore = format!("just tasks restore sim/jump {handle}");
        let gone = Closure {
            pane: pane.into(),
            handle: Some(handle.clone()),
            why: GONE.into(),
            restore: restore.clone(),
            closed: false,
            kept: false,
        };
        assert_eq!(w.clean(true, &[]), std::slice::from_ref(&gone), "{pane}");
        assert!(
            w.store().task_sessions("sim/jump").unwrap()[0]
                .closed
                .is_none()
        );
        assert_eq!(
            w.clean(false, &["sim/jump"]),
            [Closure {
                closed: true,
                ..gone
            }]
        );
        let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert!(read.closed.is_some());
        assert_eq!(
            (read.closed_why.as_str(), read.restore.as_str()),
            (GONE, restore.as_str())
        );
        // Nothing of herdr is asked, and no terminal is touched: the record alone changes.
        assert_eq!(w.changes(), Vec::<Vec<String>>::new());
        assert_eq!(w.clean(false, &[]), []);
    }
}
