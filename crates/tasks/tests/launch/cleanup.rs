//! `cleanup`: which panes close, which stay, and what the board records of each closure.

use crate::World;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use tasks::Error;
use tasks::launch::herdr::Herdr;
use tasks::launch::procs::{busy_reason, descendants_by};
use tasks::launch::{Closure, LaunchError, cleanup};

pub const SESSION_ID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

impl World {
    /// A launched executor of `slug`, its task `status` if one is given, its pane idle with a shell
    /// of its own: (handle, pane).
    pub fn worker(&self, slug: &str, status: Option<&str>) -> (String, String) {
        let s = self.launch(slug).row;
        let pid = self.new_pid();
        self.edit_pane(&s.pane, |p| {
            p.insert("agent_status".into(), "idle".into());
            p.insert("shell_pid".into(), pid.into());
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

    pub fn clean(&self, dry_run: bool, current: Option<&str>, only: &[&str]) -> Vec<Closure> {
        let only: Vec<String> = only.iter().map(|s| s.to_string()).collect();
        let m = self.machine();
        cleanup(
            self.store(),
            &self.herdr(),
            &*m.processes,
            dry_run,
            current,
            &only,
        )
        .unwrap()
    }
}

#[test]
fn an_idle_agent_of_a_finished_task_is_closed_and_the_closure_recorded_with_its_restore_line() {
    for status in ["done", "cancelled"] {
        let w = World::new();
        let (handle, pane) = w.worker("sim/jump", Some(status));
        let restore = format!("just tasks restore sim/jump {handle}");
        assert_eq!(
            w.clean(false, None, &[]),
            [Closure {
                pane: pane.clone(),
                handle: Some(handle.clone()),
                why: "its tasks are finished".into(),
                restore: restore.clone(),
                closed: true,
            }],
            "{status}"
        );
        assert!(!w.has_pane(&pane));
        let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert!(read.closed.is_some());
        assert_eq!(read.restore, restore);
        assert!(w.f.said("sim/jump").iter().any(|m| m.contains(&restore)));
    }
}

#[test]
fn a_pane_whose_task_is_still_held_is_kept() {
    let w = World::new();
    let (handle, pane) = w.worker("sim/jump", None);
    w.store().claim("sim/jump", &handle).unwrap();
    assert_eq!(w.clean(false, None, &[]), []);
    assert!(w.has_pane(&pane));
}

#[test]
fn an_idle_agent_holding_nothing_live_is_closed() {
    let w = World::new();
    let (handle, pane) = w.worker("sim/jump", None);
    w.store().leave(&handle).unwrap();
    let [closure] = <[_; 1]>::try_from(w.clean(false, None, &[])).unwrap();
    assert_eq!(
        (closure.why.as_str(), closure.closed),
        ("it holds nothing live", true)
    );
    assert!(!w.has_pane(&pane));
}

#[test]
fn a_working_blocked_or_unknown_agent_is_never_closed_and_a_done_one_is() {
    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    for status in ["working", "blocked", "unknown"] {
        w.edit_pane(&pane, |p| {
            p.insert("agent_status".into(), status.into());
        });
        assert_eq!(w.clean(false, None, &[]), [], "{status}");
    }
    w.edit_pane(&pane, |p| {
        p.insert("agent_status".into(), "done".into());
    });
    assert_eq!(w.clean(false, None, &[]).len(), 1);
    assert!(!w.has_pane(&pane));
}

#[test]
fn a_dry_run_names_the_closures_and_changes_nothing() {
    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    let [closure] = <[_; 1]>::try_from(w.clean(true, None, &[])).unwrap();
    assert!(!closure.closed);
    assert!(w.has_pane(&pane));
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
    assert_eq!(w.calls_of(&["pane", "close"]), Vec::<Vec<String>>::new());
}

#[test]
fn an_idle_shell_pane_is_closed_too_but_not_one_running_a_command() {
    let w = World::new();
    let pane = w.add_pane(false, None, json!({"agent_status": "unknown"}));
    assert_eq!(
        w.clean(false, None, &[]),
        [Closure {
            pane: pane.clone(),
            handle: None,
            why: "an idle shell".into(),
            restore: String::new(),
            closed: true,
        }]
    );
    assert!(!w.has_pane(&pane));
    let busy = w.add_pane(false, None, json!({"agent_status": "unknown"}));
    w.run_under(w.shell_pid(&busy), &["sleep 100"]);
    assert_eq!(w.clean(false, None, &[]), []);
    assert!(w.has_pane(&busy));
}

#[test]
fn an_agent_pane_with_a_shell_or_monitor_running_is_kept() {
    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    let pid = w.shell_pid(&pane);
    for command in [
        "tail -f log",
        "/repo/.git/sf-cargo/bin/tasks-0123456789abcdef attach --kind worker",
        "/bin/zsh -c sleep 5",
    ] {
        w.run_under(pid, &["claude --model x", command]);
        assert_eq!(w.clean(false, None, &[]), [], "{command}");
    }
}

#[test]
fn a_running_verify_test_run_land_or_symphony_app_is_never_interrupted() {
    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    let pid = w.shell_pid(&pane);
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
        assert_eq!(w.clean(true, None, &[]), [], "{command}");
    }
    assert!(w.has_pane(&pane));
}

#[test]
fn the_agent_itself_and_its_helpers_are_not_a_running_command() {
    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    w.run_under(
        w.shell_pid(&pane),
        &["claude --model sonnet -n x", "node /x/mcp-server.js"],
    );
    assert_eq!(w.clean(false, None, &[]).len(), 1);
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
        let (_, pane) = w.worker("sim/jump", Some("done"));
        w.run_under(w.shell_pid(&pane), &[command]);
        assert_eq!(w.clean(true, None, &[]).len(), closures, "{command}");
    }
}

#[test]
fn the_orchestrators_the_callers_and_the_focused_pane_are_never_closed() {
    let w = World::new();
    let pane = w.add_pane(true, Some(&w.orch.clone()), json!({}));
    assert_eq!(w.clean(false, None, &[]), []);
    assert!(w.has_pane(&pane));

    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    assert_eq!(w.clean(false, Some(&pane), &[]), []);
    w.edit_pane(&pane, |p| {
        p.insert("focused".into(), true.into());
    });
    assert_eq!(w.clean(false, None, &[]), []);
    assert!(w.has_pane(&pane));
}

#[test]
fn an_agent_pane_that_maps_to_no_session_is_kept() {
    let w = World::new();
    let pane = w.add_pane(true, None, json!({}));
    assert_eq!(w.clean(false, None, &[]), []);
    assert!(w.has_pane(&pane));
}

#[test]
fn a_pane_is_matched_to_its_handle_by_the_name_flag_the_title_or_a_bound_agent() {
    let w = World::new();
    let (done, launched) = w.worker("sim/jump", Some("done"));
    w.edit_panes(|panes| panes.retain(|p| p["pane_id"] != launched.as_str()));
    let by_name = w.add_pane(true, Some(&done), json!({}));
    let [closure] = <[_; 1]>::try_from(w.clean(false, None, &[])).unwrap();
    assert_eq!(
        (closure.pane, closure.handle),
        (by_name, Some(done.clone()))
    );

    let by_title = w.add_pane(
        true,
        None,
        json!({"terminal_title_stripped": done, "foreground": []}),
    );
    w.store()
        .reopen_task_session("sim/jump", &done, &by_title)
        .unwrap();
    w.edit_pane(&by_title, |p| {
        p.insert("agent_session".into(), json!({"value": "not-recorded"}));
    });
    // the recorded pane matches it first; with another pane recorded, the title does
    w.store()
        .reopen_task_session("sim/jump", &done, "w1:elsewhere")
        .unwrap();
    let [closure] = <[_; 1]>::try_from(w.clean(false, None, &[])).unwrap();
    assert_eq!((closure.pane, closure.handle), (by_title, Some(done)));

    let w = World::new();
    let bound = w.f.session("worker");
    w.task("sim/dock", None);
    w.finish_task("sim/dock", &bound, "done");
    w.store().bind_agent(SESSION_ID, &bound, true).unwrap();
    let pane = w.add_pane(true, None, json!({"agent_session": {"value": SESSION_ID}}));
    let [closure] = <[_; 1]>::try_from(w.clean(false, None, &[])).unwrap();
    assert_eq!((closure.pane, closure.handle), (pane, Some(bound)));
}

#[test]
fn a_pane_that_was_not_launched_is_recorded_as_found_from_what_herdr_knows() {
    let w = World::new();
    let handle = w.f.session("worker");
    w.task("sim/dock", None);
    w.finish_task("sim/dock", &handle, "done");
    let pane = w.add_pane(
        true,
        Some(&handle),
        json!({"cwd": "/some/wt", "agent_session": {"agent": "claude", "kind": "id", "value": SESSION_ID}}),
    );
    let [closure] = <[_; 1]>::try_from(w.clean(false, None, &[])).unwrap();
    assert_eq!(closure.handle.as_deref(), Some(handle.as_str()));
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/dock").unwrap()).unwrap();
    assert_eq!(
        (
            read.claude_session.as_str(),
            read.worktree.as_str(),
            read.pane.as_str(),
            read.role.as_str()
        ),
        (SESSION_ID, "/some/wt", pane.as_str(), "executor")
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
fn an_agent_pane_with_a_handle_but_no_task_to_record_it_on_is_kept() {
    let w = World::new();
    let handle = w.f.session("worker");
    let pane = w.add_pane(
        true,
        Some(&handle),
        json!({"agent_session": {"value": SESSION_ID}}),
    );
    assert_eq!(w.clean(false, None, &[]), []);
    assert!(w.has_pane(&pane));
}

#[test]
fn only_the_named_tasks_sessions_are_cleaned_when_asked() {
    let w = World::new();
    w.task("sim/dock", None);
    let (_, a) = w.worker("sim/jump", Some("done"));
    let (_, b) = w.worker("sim/dock", Some("done"));
    let [closure] = <[_; 1]>::try_from(w.clean(false, None, &["sim/dock"])).unwrap();
    assert_eq!(closure.pane, b);
    assert!(w.has_pane(&a));
}

#[test]
fn a_pane_herdr_cannot_describe_with_no_shell_or_whose_processes_cannot_be_read_is_kept() {
    let mut w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    w.set("FAKE_HERDR_FAIL", "pane process-info");
    assert_eq!(w.clean(false, None, &[]), []);
    w.set("FAKE_HERDR_FAIL", "");
    let broken =
        |_: i64| -> Result<Vec<String>, LaunchError> { Err(LaunchError("ps failed".into())) };
    assert_eq!(
        cleanup(w.store(), &w.herdr(), &broken, false, None, &[]).unwrap(),
        []
    );
    w.edit_pane(&pane, |p| {
        p.insert("shell_pid".into(), 0.into());
    });
    assert_eq!(w.clean(false, None, &[]), []);
    assert!(w.has_pane(&pane));
}

#[test]
fn a_pane_list_herdr_refuses_is_a_refusal_naming_it() {
    let mut w = World::new();
    w.set("FAKE_HERDR_FAIL", "pane list");
    let m = w.machine();
    match cleanup(w.store(), &w.herdr(), &*m.processes, false, None, &[]) {
        Err(Error::Refused(why)) => assert!(why.starts_with("herdr pane list failed: "), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_close_herdr_refuses_is_said_and_records_nothing() {
    let mut w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    w.set("FAKE_HERDR_FAIL", "pane close");
    let [closure] = <[_; 1]>::try_from(w.clean(false, None, &[])).unwrap();
    assert_eq!(
        (closure.pane.as_str(), closure.closed),
        (pane.as_str(), false)
    );
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
}

#[test]
fn panes_and_tabs_are_listed_in_the_calling_workspace_only() {
    let mut w = World::new();
    w.set("HERDR_WORKSPACE_ID", "w9");
    let herdr = w.herdr();
    herdr.panes().unwrap();
    herdr.tabs().unwrap();
    herdr.agents().unwrap();
    let calls = w.calls();
    assert_eq!(
        calls,
        [
            ["pane", "list", "--workspace", "w9"]
                .map(String::from)
                .to_vec(),
            ["tab", "list", "--workspace", "w9"]
                .map(String::from)
                .to_vec(),
            ["agent", "list"].map(String::from).to_vec(),
        ]
    );
}

#[test]
fn a_command_line_that_is_not_utf8_is_read_and_the_pids_still_parse() {
    let w = World::new();
    let ps = w.dir().join("ps");
    std::fs::write(
        &ps,
        b"#!/bin/sh\nprintf '  10     1 bash\\n  11    10 sleep \\377\\n  12    11 tail\\n  13     1 x\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&ps, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        descendants_by(tasks::sys::Command::new(&ps), 10).unwrap(),
        ["sleep \u{fffd}", "tail"]
    );
    std::fs::write(&ps, b"#!/bin/sh\nexit 0\n").unwrap();
    assert_eq!(
        descendants_by(tasks::sys::Command::new(&ps), 10),
        Err(LaunchError("ps printed nothing".into()))
    );
    let missing = descendants_by(tasks::sys::Command::new(w.dir().join("no-ps")), 10);
    assert!(
        matches!(&missing, Err(LaunchError(why)) if why.starts_with("ps: ")),
        "{missing:?}"
    );
}

#[test]
fn a_shell_started_command_that_mentions_mcp_is_still_work() {
    assert!(busy_reason(&["/bin/zsh -c 'node /x/mcp-server.js'".into()]).is_some());
    assert_eq!(
        busy_reason(&["node /x/mcp-server.js".into(), "claude --model x".into()]),
        None
    );
}
