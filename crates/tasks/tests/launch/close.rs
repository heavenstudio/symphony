//! `close`: one agent's terminal on its orchestrator's word, whatever role it holds, recorded with
//! its restore line; refused for the caller's own, for one the owner may be looking at, and for one
//! whose program is not at rest (work under it, as the idle sweep reads it, or another program in
//! front of it). What changes while a close runs is `close_races.rs`'s.

use crate::World;
use crate::common::refused;
use serde_json::json;
use symphony_board::Error;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::screen::ON_SCREEN;
use symphony_board::launch::{Closure, LaunchError, close, restore};
use symphony_board::rows::TaskSessionRow;

impl World {
    /// A Claude reviewer launched on sim/jump, which an executor of its own has in review, its agent
    /// idle as it waits for a new tip: its record.
    pub(crate) fn waiting_reviewer(&self) -> TaskSessionRow {
        let executor = self.f.session("worker");
        self.store().claim("sim/jump", &executor).unwrap();
        self.store()
            .move_to("sim/jump", "review", &executor)
            .unwrap();
        let session = self.review("sim/jump").row;
        self.edit_terminal(&session.pane, |terminal| {
            terminal.insert("status".into(), json!("idle"));
        });
        session
    }

    /// The orchestrator closes `handle`'s terminal on sim/jump through `hc`.
    pub(crate) fn close_through(&self, hc: &dyn Hc, handle: &str) -> Result<Closure, Error> {
        let machine = self.machine();
        close(
            self.store(),
            hc,
            &*machine.processes,
            &self.orch,
            "sim/jump",
            handle,
        )
    }

    pub(crate) fn close(&self, handle: &str) -> Result<Closure, Error> {
        self.close_through(&self.hc(), handle)
    }

    /// Nothing was killed or removed, and no record of sim/jump is closed.
    pub(crate) fn untouched(&self) {
        let changes = [self.calls_of(&["kill"]), self.calls_of(&["rm"])].concat();
        assert_eq!(changes, Vec::<Vec<String>>::new());
        let records = self.store().task_sessions("sim/jump").unwrap();
        assert!(
            records.iter().all(|record| record.closed.is_none()),
            "{records:?}"
        );
    }

    /// The lines of sim/jump's thread from the `from`th on.
    pub(crate) fn said_since(&self, from: usize) -> Vec<String> {
        self.f.said("sim/jump")[from..].to_vec()
    }
}

pub(crate) fn restore_line(handle: &str) -> String {
    format!("just tasks restore sim/jump {handle}")
}

/// The line a close by `actor` of `handle`'s pane writes.
pub(crate) fn closed_line(handle: &str, actor: &str) -> String {
    format!(
        "closed the pane of {handle} (closed by {actor}); restore: {}",
        restore_line(handle)
    )
}

#[test]
fn an_idle_reviewer_waiting_for_a_new_tip_is_closed_and_recorded_with_its_restore_line() {
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
    assert_eq!(world.clean(false, &[]), [], "cleanup keeps a live role");
    let why = format!("closed by {}", world.orch);
    assert_eq!(
        world.close(handle).unwrap(),
        Closure {
            pane: session.pane.clone(),
            handle: Some(handle.clone()),
            why: why.clone(),
            restore: restore_line(handle),
            closed: true,
            kept: false,
        }
    );
    assert!(
        !world.has_terminal(&session.pane),
        "{:?}",
        world.addresses()
    );
    let closing: Vec<[String; 2]> = world
        .changes()
        .into_iter()
        .filter(|change| change[0] == "kill" || change[0] == "rm")
        .map(|change| [change[0].clone(), change[1].clone()])
        .collect();
    assert_eq!(
        closing,
        [
            ["kill".to_string(), session.pane.clone()],
            ["rm".to_string(), session.pane.clone()]
        ]
    );
    let [read] = <[_; 1]>::try_from(world.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (
            read.closed.is_some(),
            read.closed_why.as_str(),
            read.restore.as_str()
        ),
        (true, why.as_str(), restore_line(handle).as_str())
    );
    assert_eq!(
        world.f.said("sim/jump").last().unwrap(),
        &closed_line(handle, &world.orch)
    );
    // Its channel is retired with it; its role stays, and restore brings it back.
    let callbacks = world.store().callbacks().unwrap();
    assert!(!callbacks.contains_key(handle), "{:?}", callbacks.keys());
    let reviewers = world.store().get("sim/jump").unwrap();
    assert_eq!(reviewers.holders("reviewer"), [handle.as_str()]);
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
    assert!(
        world.live().contains(&again.row.pane),
        "{} not in {:?}",
        again.row.pane,
        world.live()
    );
    assert_eq!(again.row.claude_session, session.claude_session);
}

#[test]
fn a_terminal_whose_program_ended_is_only_removed() {
    let world = World::new();
    let session = world.waiting_reviewer();
    world.edit_terminal(&session.pane, |terminal| {
        terminal.insert("state".into(), json!("finished"));
        terminal.insert("status".into(), json!("working"));
    });
    let closed = world.close(&session.handle).unwrap();
    assert!(closed.closed, "{closed:?}");
    assert_eq!(
        world
            .changes()
            .into_iter()
            .filter(|change| change[0] == "kill" || change[0] == "rm")
            .collect::<Vec<_>>(),
        [["rm", &session.pane, "--json"].map(String::from).to_vec()]
    );
    assert!(
        !world.has_terminal(&session.pane),
        "{:?}",
        world.addresses()
    );
}

#[test]
fn the_callers_own_session_or_terminal_is_refused() {
    let mut world = World::new();
    let session = world.waiting_reviewer();
    let machine = world.machine();
    let own = close(
        world.store(),
        &world.hc(),
        &*machine.processes,
        &session.handle,
        "sim/jump",
        &session.handle,
    );
    assert_eq!(
        refused(own),
        format!(
            "{} is you: a session never closes its own terminal",
            session.handle
        )
    );
    world.set("HC_TERMINAL_ID", &session.pane);
    assert_eq!(
        refused(world.close(&session.handle)),
        format!(
            "terminal {} is the one you run in: a session never closes its own terminal",
            session.pane
        )
    );
    world.untouched();
}

#[test]
fn a_terminal_the_owner_may_be_looking_at_is_refused() {
    let mut world = World::new();
    let session = world.waiting_reviewer();
    let tab = world.tab_of(&session.pane);
    for shown in [
        json!({"tab": tab}),
        json!({"agent": session.pane}),
        json!({"overlays": [{"terminal": session.pane}]}),
        json!({"floats": {"f1": {"terminal": session.pane}}}),
    ] {
        world.shows(shown.clone());
        assert_eq!(
            refused(world.close(&session.handle)),
            format!("terminal {} stays: {ON_SCREEN}", session.pane),
            "{shown}"
        );
    }
    world.shows(json!({}));
    world.set("FAKE_HC_FAIL", "app status");
    assert_eq!(
        refused(world.close(&session.handle)),
        format!(
            "terminal {} stays: never closed on no information: herdr-app status: the window did \
             not answer",
            session.pane
        )
    );
    world.untouched();
}

#[test]
fn work_under_the_agent_keeps_its_terminal_as_the_idle_sweep_reads_it() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let pid = world.pid_of(&session.pane);
    let wrapped = "tools/rs-bin.sh sf-slots cargo test --manifest-path symphony/Cargo.toml -p \
                   symphony-board --test launch";
    for (command, why) in [
        ("just verify", "a verify (just verify)".to_string()),
        (
            "just test-sim SUITE=jump",
            "a test run (just test-sim SUITE=jump)".into(),
        ),
        ("just land tools/x", "a land (just land tools/x)".into()),
        ("just symphony", "the Symphony (just symphony)".into()),
        // A native test run, through the cargo wrapper, bare, or by its recipe; and anything else.
        (wrapped, wrapped.chars().take(60).collect()),
        (
            "cargo test -p symphony-board",
            "cargo test -p symphony-board".into(),
        ),
        ("just test-rs", "just test-rs".into()),
        ("/bin/zsh -c sleep 600", "/bin/zsh -c sleep 600".into()),
        ("tail -f build.log", "tail -f build.log".into()),
    ] {
        world.run_under(pid, &["caffeinate -i", command]);
        assert_eq!(
            refused(world.close(&session.handle)),
            format!("terminal {} stays: it runs {why}", session.pane),
            "{command}"
        );
    }
    world.untouched();
}

#[test]
fn the_agents_helpers_and_board_commands_under_it_are_no_reason_to_keep_it() {
    let world = World::new();
    let session = world.waiting_reviewer();
    world.run_under(
        world.pid_of(&session.pane),
        &[
            "caffeinate -i -t 300",
            "node /x/mcp-server.js",
            "just tasks inbox --as quiet-vole",
        ],
    );
    let closed = world.close(&session.handle).unwrap();
    assert!(closed.closed, "{closed:?}");
    assert!(
        !world.has_terminal(&session.pane),
        "{:?}",
        world.addresses()
    );
}

#[test]
fn a_program_other_than_its_agent_or_a_shell_in_front_is_refused() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let pid = world.pid_of(&session.pane);
    // A job a shell runs in front of the agent's terminal, another pid than its program's.
    world.edit_terminal(&session.pane, |terminal| {
        terminal.insert(
            "foreground".into(),
            json!({"pid": pid + 1, "name": "node", "argv": ["node", "job.js"]}),
        );
    });
    let in_front = "node holds its foreground, not its agent or a shell at rest";
    assert_eq!(
        refused(world.close(&session.handle)),
        format!("terminal {} stays: {in_front}", session.pane)
    );
    // Nothing hc shows there.
    world.edit_terminal(&session.pane, |terminal| {
        terminal.insert("foreground".into(), json!(null));
    });
    assert_eq!(
        refused(world.close(&session.handle)),
        format!(
            "terminal {} stays: hc shows nothing in its foreground, so it is not known to be at rest",
            session.pane
        )
    );
    // A terminal whose program is the Symphony, no agent in it.
    world.edit_terminal(&session.pane, |terminal| {
        terminal.remove("foreground");
        terminal.remove("agent");
        terminal.insert("status".into(), json!(null));
        terminal.insert("argv".into(), json!(["env", "A=1", "just", "symphony"]));
    });
    assert_eq!(
        refused(world.close(&session.handle)),
        format!(
            "terminal {} stays: just holds its foreground, not its agent or a shell at rest",
            session.pane
        )
    );
    world.untouched();
}

#[test]
fn a_terminal_whose_processes_cannot_be_read_is_refused() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let unreadable = |_: i64| Err(LaunchError("ps printed nothing".into()));
    let closed = close(
        world.store(),
        &world.hc(),
        &unreadable,
        &world.orch,
        "sim/jump",
        &session.handle,
    );
    assert_eq!(
        refused(closed),
        format!(
            "terminal {} stays: never closed on no information: ps printed nothing",
            session.pane
        )
    );
    world.edit_terminal(&session.pane, |terminal| {
        terminal.insert("pid".into(), json!(null));
    });
    assert_eq!(
        refused(world.close(&session.handle)),
        format!(
            "terminal {} stays: hc knows no pid for it, so what runs there cannot be read",
            session.pane
        )
    );
    world.untouched();
}

#[test]
fn a_kill_hc_refuses_leaves_the_session_open_writes_nothing_and_holds_it_no_longer() {
    let mut world = World::new();
    let session = world.waiting_reviewer();
    let before = world.f.said("sim/jump").len();
    world.set("FAKE_HC_FAIL", "kill");
    let failed = format!("hc kill {}: fake_failure: kill failed", session.pane);
    assert_eq!(refused(world.close(&session.handle)), failed);
    assert_eq!(world.said_since(before), Vec::<String>::new());
    let [read] = <[_; 1]>::try_from(world.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (read.closed, read.closed_why.as_str(), read.pane.as_str()),
        (None, "", session.pane.as_str())
    );
    // The refused close let the session go: the next one closes it.
    world.set("FAKE_HC_FAIL", "");
    assert_eq!(world.close(&session.handle).unwrap().pane, session.pane);
    assert_eq!(
        world.said_since(before),
        [closed_line(&session.handle, &world.orch)]
    );
}

#[test]
fn a_terminal_of_another_board_or_another_handle_is_refused() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let board = world.board();
    for (labelled, said) in [
        (json!("web-bench/tasks"), "web-bench/tasks"),
        (json!(null), "no board"),
    ] {
        world.edit_terminal(&session.pane, |terminal| {
            terminal["labels"]["sf.board"] = labelled.clone();
        });
        assert_eq!(
            refused(world.close(&session.handle)),
            format!(
                "terminal {} stays: it is not this board's ({said}): the board closes only its own",
                session.pane
            )
        );
    }
    world.edit_terminal(&session.pane, |terminal| {
        terminal["labels"]["sf.board"] = json!(board);
        terminal["labels"]["sf.handle"] = json!("other-owl");
    });
    assert_eq!(
        refused(world.close(&session.handle)),
        format!(
            "terminal {} stays: it runs other-owl, not {}",
            session.pane, session.handle
        )
    );
    world.untouched();
}

#[test]
fn an_agent_with_a_role_and_no_record_is_found_by_its_label_and_recorded_where_closed() {
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
    let terminal = world.add_terminal(true, Some(&handle), json!({"name": null}));
    let before = world.f.said("sim/jump").len();
    let closed = world.close(&handle).unwrap();
    assert_eq!(
        (closed.pane.as_str(), closed.closed),
        (terminal.as_str(), true)
    );
    assert!(!world.has_terminal(&terminal), "{:?}", world.addresses());
    let [read] = <[_; 1]>::try_from(world.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (
            read.role.as_str(),
            read.pane.as_str(),
            read.claude_session.as_str(),
            read.worktree.as_str(),
            read.closed.is_some()
        ),
        (
            "reviewer",
            terminal.as_str(),
            format!("sess-{terminal}").as_str(),
            "/somewhere",
            true
        )
    );
    // One line: the closure, with its restore line; no line of its being found.
    assert_eq!(
        world.said_since(before),
        [closed_line(&handle, &world.orch)]
    );
}

#[test]
fn an_agent_found_by_its_name_alone_is_closed_too() {
    let world = World::new();
    let handle = world.f.session("worker");
    world.store().claim("sim/jump", &handle).unwrap();
    let terminal = world.add_terminal(true, None, json!({"name": handle}));
    let closed = world.close(&handle).unwrap();
    assert_eq!(
        (closed.pane.as_str(), closed.closed),
        (terminal.as_str(), true)
    );
    assert!(!world.has_terminal(&terminal), "{:?}", world.addresses());
    assert_eq!(
        world.store().task_sessions("sim/jump").unwrap()[0].role,
        "executor"
    );
}

#[test]
fn a_handle_with_nothing_on_the_task_or_no_terminal_or_an_unknown_task_is_refused() {
    let world = World::new();
    let stranger = world.f.session("worker");
    assert_eq!(
        refused(world.close(&stranger)),
        format!("{stranger} has no recorded session or role on sim/jump")
    );
    world.store().claim("sim/jump", &stranger).unwrap();
    assert_eq!(
        refused(world.close(&stranger)),
        format!("{stranger} has no open session on sim/jump and runs in no terminal of this board")
    );
    assert_eq!(refused(world.close("ghost-owl")), "no session 'ghost-owl'");
    let machine = world.machine();
    let elsewhere = close(
        world.store(),
        &world.hc(),
        &*machine.processes,
        &world.orch,
        "sim/none",
        &stranger,
    );
    assert_eq!(refused(elsewhere), "no task 'sim/none'");
}

#[test]
fn a_live_non_agent_shell_keeps_its_existing_close_policy_without_native_status() {
    let w = World::new();
    let s = w.waiting_reviewer();
    let pid = w.pid_of(&s.pane);
    w.edit_terminal(&s.pane, |t| {
        t.remove("agent");
        t.insert("status".into(), json!(null));
        t.insert("argv".into(), json!(["/bin/zsh", "-l"]));
        t.insert(
            "foreground".into(),
            json!({"pid": pid, "name": "zsh", "argv": ["/bin/zsh", "-l"]}),
        );
    });
    let closed = w.close(&s.handle).unwrap();
    assert!(closed.closed);
    assert_eq!(closed.restore, restore_line(&s.handle));
    assert!(!w.has_terminal(&s.pane));
}
