//! Agents API contracts for /api/agents and just tasks agents --json.
//! The board is the record; each test supplies hc terminals and slot-lock probes
//! to describe current work. The JSON key set is pinned as a wire contract.

use crate::common::{Fixture, NOW};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use symphony_board::cli::agents::{
    self, Agent, Detail, MESSAGE_CHARS, Pane, Probes, Run, View, describe, running_tests,
};
use symphony_board::health::{LIVE_SECONDS, RESERVE_SECONDS, STALE_SECONDS};
use symphony_board::{Beat, Health, NewLaunch, NewSession, NewTask};

mod fixtures;
use fixtures::{AGENT_KEYS, local_session};

#[test]
fn idle_agent_projection_uses_the_supplied_board_snapshot() {
    let c = Case::new();
    let worker = c.worker();
    let snapshot = c.f.store.board().unwrap();
    c.f.store.move_to("sim/jump", "review", &worker).unwrap();
    let view =
        agents::describe_board(&c.f.store, &c.probes(), c.f.now(), false, &snapshot).unwrap();
    assert_eq!(found(&view, &worker).tasks[0].status, "in_progress");
    assert_eq!(c.agent(&worker).tasks[0].status, "review");
}

/// A board with an orchestrator and one ready task, `sim/jump`, and probes the test sets: hc.s
/// terminals and the slot runs (None: the probe cannot be asked), and the branch checked out in a folder.
struct Case {
    f: Fixture,
    panes: RefCell<Option<Vec<Value>>>,
    runs: RefCell<Option<Vec<Run>>>,
    branches: RefCell<BTreeMap<String, String>>,
    orch: String,
}

impl Case {
    fn new() -> Case {
        let f = Fixture::empty();
        let orch = f.session_with(local_session("orchestrator"));
        f.store
            .create(&NewTask::new("sim/jump", "Jump", "owner"))
            .unwrap();
        Case {
            f,
            panes: RefCell::new(Some(Vec::new())),
            runs: RefCell::new(Some(Vec::new())),
            branches: RefCell::new(BTreeMap::new()),
            orch,
        }
    }

    fn probes(&self) -> Probes<'_> {
        Probes {
            panes: Box::new(|| {
                let panes = self.panes.borrow().clone().ok_or("hc is down")?;
                Ok(panes
                    .iter()
                    .filter_map(|t| t.as_object().and_then(Pane::from_hc))
                    .collect())
            }),
            runs: Box::new(|| Ok(self.runs.borrow().clone().ok_or("no slots")?)),
            branch_of: Box::new(|cwd| {
                Ok(self.branches.borrow().get(cwd).cloned().unwrap_or_default())
            }),
        }
    }

    fn view(&self) -> View {
        describe(&self.f.store, &self.probes(), self.f.now(), false).unwrap()
    }

    fn view_all(&self) -> View {
        describe(&self.f.store, &self.probes(), self.f.now(), true).unwrap()
    }

    fn agent(&self, handle: &str) -> Agent {
        found(&self.view(), handle)
    }

    fn handles(&self, view: &View) -> Vec<String> {
        view.agents.iter().map(|a| a.handle.clone()).collect()
    }

    /// A worker the orchestrator launched on `slug` in `pane`, working in `worktree`: it registered in
    /// the main checkout, reserved and claimed the task, and its launch is recorded.
    fn worker_on(&self, slug: &str, pane: &str, worktree: &str) -> String {
        let handle = self.f.session_with(NewSession {
            parent: Some(self.orch.clone()),
            worktree: "/main".into(),
            model: "sonnet".into(),
            ..local_session("worker")
        });
        self.f
            .store
            .reserve(slug, &handle, RESERVE_SECONDS)
            .unwrap();
        self.f.store.claim(slug, &handle).unwrap();
        self.f
            .store
            .record_launch(&NewLaunch {
                claude_session: format!("sess-{handle}"),
                worktree: worktree.into(),
                branch: "sim/jump".into(),
                pane: pane.into(),
                model: "sonnet".into(),
                ..NewLaunch::new(slug, &handle, "executor")
            })
            .unwrap();
        handle
    }

    fn worker(&self) -> String {
        self.worker_on("sim/jump", "t5", "/wt/sim-jump")
    }

    fn worker_in(&self, worktree: &str) -> String {
        self.worker_on("sim/jump", "t5", worktree)
    }

    /// A session registered in the main checkout and nothing more.
    fn main_session(&self) -> String {
        self.f.session_with(NewSession {
            worktree: "/main".into(),
            ..local_session("worker")
        })
    }

    fn add_run(&self, run: Run) {
        self.runs
            .borrow_mut()
            .get_or_insert_with(Vec::new)
            .push(run);
    }

    /// The executor goes quiet with its task in review: the board calls it left, as nothing it holds
    /// could be claimed.
    fn quiet_executor_with_task_in_review(&self) -> String {
        let handle = self.worker();
        self.f.store.move_to("sim/jump", "review", &handle).unwrap();
        self.f.advance(LIVE_SECONDS + STALE_SECONDS + 5.0);
        assert_eq!(self.f.health(&handle), Health::Left);
        handle
    }
}

fn found(view: &View, handle: &str) -> Agent {
    let mut found: Vec<&Agent> = view.agents.iter().filter(|a| a.handle == handle).collect();
    assert_eq!(found.len(), 1, "{handle} is listed once");
    found.remove(0).clone()
}

fn detail(recipe: &str, phase: &str, since: f64) -> Option<Detail> {
    Some(Detail {
        recipe: recipe.into(),
        phase: phase.into(),
        since: Some(since),
    })
}

fn parsed(view: &View) -> Value {
    serde_json::from_str(&symphony_board::pyjson::dumps(&view.json())).expect("the view is JSON")
}

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

// -- the shape -------------------------------------------------------------------------

#[test]
fn the_contract_keys_are_stable() {
    let c = Case::new();
    c.worker();
    let view = parsed(&c.view());
    assert_eq!(
        keys(&view),
        ["agents", "generated", "machine_work", "probes"]
            .map(String::from)
            .into()
    );
    assert_eq!(
        keys(&view["agents"][0]),
        AGENT_KEYS.map(String::from).into()
    );
    assert_eq!(
        keys(&view["probes"]),
        ["herdr", "slots"].map(String::from).into()
    );
}

#[test]
fn under_a_cap_an_orchestrator_shows_its_open_tasks_against_it_and_a_worker_shows_none() {
    let c = Case::new();
    c.f.store
        .take_role("sim/jump", &c.orch, "orchestrator")
        .unwrap();
    let worker = c.worker();
    let uncapped = c.agent(&c.orch);
    assert_eq!(uncapped.slots, None, "no cap, nothing shown");
    let capped = c.f.other().capping(Some(8));
    let view = describe(&capped, &c.probes(), c.f.now(), false).unwrap();
    assert_eq!(
        found(&view, &c.orch).slots,
        Some(agents::Slots { open: 1, cap: 8 })
    );
    assert_eq!(found(&view, &worker).slots, None);
    let me = parsed(&view)["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["handle"] == c.orch.as_str())
        .cloned()
        .unwrap();
    assert_eq!(me["slots"], json!({"open": 1, "cap": 8}));
}

#[test]
fn the_json_is_written_in_the_contracts_key_order_as_python_writes_it() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.f.store.say("sim/jump", &handle, "on it", None).unwrap();
    let text = symphony_board::pyjson::dumps(&c.view().json());
    let me = text.find(&format!("\"handle\": \"{handle}\"")).unwrap();
    let start = text[..me].rfind("  {").unwrap();
    let end = start + text[start..].find("\n  }").unwrap() + 4;
    assert_eq!(
        &text[start..end],
        format!(
            r#"  {{
   "handle": "{handle}",
   "kind": "worker",
   "model": "sonnet",
   "parent": "{orch}",
   "task": "sim/jump",
   "role": "executor",
   "tasks": [
    {{
     "slug": "sim/jump",
     "role": "executor",
     "status": "in_progress"
    }}
   ],
   "worktree": "/wt/sim-jump",
   "branch": "sim/jump",
   "agent": "claude",
   "session_id": "sess-{handle}",
   "pane": "t5",
   "callback": null,
   "chat": false,
   "state": "working",
   "state_detail": null,
   "health": "live",
   "last_tool_call": null,
   "last_message": {{
    "slug": "sim/jump",
    "ts": 1800000000.0,
    "text": "on it"
   }},
   "slots": null
  }}"#,
            orch = c.orch
        )
    );
    assert!(text.starts_with("{\n \"generated\": 1800000000.0,\n \"probes\": {\n  \"herdr\": \"ok\",\n  \"slots\": \"ok\"\n },\n \"agents\": [\n"), "{text}");
}

#[test]
fn a_launched_agent_is_described_from_its_task_session() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    let a = c.agent(&handle);
    assert_eq!(
        (
            a.kind.as_str(),
            a.model.as_str(),
            a.parent.as_deref(),
            a.task.as_deref(),
            a.role.as_deref()
        ),
        (
            "worker",
            "sonnet",
            Some(c.orch.as_str()),
            Some("sim/jump"),
            Some("executor")
        )
    );
    assert_eq!(
        (
            a.worktree.as_str(),
            a.branch.as_str(),
            a.session_id.clone(),
            a.pane.as_deref()
        ),
        (
            "/wt/sim-jump",
            "sim/jump",
            Some(format!("sess-{handle}")),
            Some("t5")
        )
    );
    let held: Vec<(String, String, String)> = a
        .tasks
        .iter()
        .map(|t| (t.slug.clone(), t.role.clone(), t.status.clone()))
        .collect();
    assert_eq!(
        held,
        [("sim/jump".into(), "executor".into(), "in_progress".into())]
    );
}

#[test]
fn the_registered_worktree_is_not_taken_for_the_real_one() {
    let c = Case::new();
    let handle = c.worker_in("/wt/real");
    assert_eq!(c.agent(&handle).worktree, "/wt/real"); // register ran in /main
}

#[test]
fn left_sessions_are_left_out_unless_asked() {
    let c = Case::new();
    let handle = c.worker();
    c.f.store.leave(&handle).unwrap();
    assert!(!c.handles(&c.view()).contains(&handle));
    assert!(c.handles(&c.view_all()).contains(&handle));
}

#[test]
fn the_open_task_comes_first_and_a_finished_one_stays_listed() {
    let c = Case::new();
    let handle = c.worker();
    c.f.store
        .create(&NewTask::new("sim/dock", "Dock", "owner"))
        .unwrap();
    c.f.store.claim("sim/dock", &handle).unwrap();
    c.f.store.move_to("sim/jump", "review", &handle).unwrap();
    let reviewer = c.f.session_with(local_session("reviewer"));
    c.f.store
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    c.f.store
        .verdict("sim/jump", &reviewer, "approve", "ok")
        .unwrap();
    let a = c.agent(&handle);
    assert_eq!(
        (a.task.as_deref(), a.role.as_deref()),
        (Some("sim/dock"), Some("executor"))
    );
    let statuses: BTreeMap<String, String> = a
        .tasks
        .iter()
        .map(|t| (t.slug.clone(), t.status.clone()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("sim/jump".into(), "done".into()),
            ("sim/dock".into(), "in_progress".into())
        ]
        .into()
    );
}

#[test]
fn the_primary_of_finished_work_alone_is_the_last_updated() {
    let c = Case::new();
    let handle = c.worker();
    let reviewer = c.f.session_with(local_session("reviewer"));
    c.f.store
        .create(&NewTask::new("sim/dock", "Dock", "owner"))
        .unwrap();
    c.f.store.claim("sim/dock", &handle).unwrap();
    for slug in ["sim/dock", "sim/jump"] {
        c.f.store.move_to(slug, "review", &handle).unwrap();
        c.f.store.take_role(slug, &reviewer, "reviewer").unwrap();
        c.f.store.verdict(slug, &reviewer, "approve", "ok").unwrap();
        c.f.advance(5.0);
    }
    let a = c.agent(&handle);
    assert_eq!(
        (a.task.as_deref(), a.role.as_deref()),
        (Some("sim/jump"), Some("executor"))
    );
}

#[test]
fn last_tool_call_and_last_board_message() {
    let c = Case::new();
    let handle = c.worker();
    c.f.store.bind_agent("agent-1", &handle, false).unwrap();
    c.f.advance(30.0);
    c.f.store
        .say("sim/jump", &handle, "the ring is at 900", None)
        .unwrap();
    c.f.advance(10.0);
    c.f.store.agent_beat("agent-1", &Beat::default()).unwrap();
    let a = c.agent(&handle);
    assert_eq!(a.last_tool_call, Some(c.f.now()));
    let said = a.last_message.unwrap();
    assert_eq!(
        (said.slug.as_str(), said.ts, said.text.as_str()),
        ("sim/jump", NOW + 30.0, "the ring is at 900")
    );
}

#[test]
fn an_agent_that_never_spoke_has_no_last_message() {
    let c = Case::new();
    let handle = c.worker();
    assert!(c.agent(&handle).last_message.is_none());
}

#[test]
fn a_long_message_is_cut() {
    let c = Case::new();
    let handle = c.worker();
    c.f.store
        .say("sim/jump", &handle, &"é".repeat(500), None)
        .unwrap();
    let text = c.agent(&handle).last_message.unwrap().text;
    assert_eq!(text, format!("{}…", "é".repeat(MESSAGE_CHARS)));
}

#[test]
fn a_message_is_one_line_with_its_spaces_collapsed_as_python_splits_them() {
    let c = Case::new();
    let handle = c.worker();
    c.f.store
        .say(
            "sim/jump",
            &handle,
            "a\n\n b\u{1f}c\u{a0} d\u{2003}e\u{200b}f",
            None,
        )
        .unwrap();
    assert_eq!(
        c.agent(&handle).last_message.unwrap().text,
        "a b c d e\u{200b}f"
    );
}

// -- the last tool call ----------------------------------------------------------------

#[test]
fn the_monitors_beat_is_not_a_tool_call() {
    let c = Case::new();
    let handle = c.worker();
    c.f.store.bind_agent("agent-1", &handle, false).unwrap();
    c.f.advance(30.0);
    c.f.store.agent_beat("agent-1", &Beat::default()).unwrap();
    let tool = c.f.now();
    c.f.advance(20.0);
    c.f.store.touch(&handle).unwrap(); // the monitor's 20 s beat
    assert_eq!(c.agent(&handle).last_tool_call, Some(tool));
}

#[test]
fn an_agent_that_never_beat_for_a_tool_call_has_none() {
    let c = Case::new();
    let handle = c.worker();
    assert_eq!(c.agent(&handle).last_tool_call, None);
}

// -- who is listed ---------------------------------------------------------------------

#[test]
fn a_quiet_executor_whose_task_waits_in_review_and_whose_pane_is_idle_is_listed() {
    let c = Case::new();
    let handle = c.quiet_executor_with_task_in_review();
    c.pane(&handle, "t5", "idle", json!({}));
    assert_eq!(c.agent(&handle).state, "idle");
}

#[test]
fn a_quiet_session_the_board_calls_left_with_no_pane_and_no_run_is_not_listed() {
    let c = Case::new();
    let handle = c.quiet_executor_with_task_in_review();
    assert!(!c.handles(&c.view()).contains(&handle));
    assert!(c.handles(&c.view_all()).contains(&handle));
}

#[test]
fn a_quiet_session_the_board_calls_left_is_listed_while_it_runs_a_test() {
    let c = Case::new();
    let handle = c.quiet_executor_with_task_in_review();
    c.add_run(Run {
        pid: 1,
        worktree: "/wt/sim-jump".into(),
        recipe: "verify".into(),
        phase: "holding".into(),
        since: Some(NOW),
        pool: "run".into(),
        handle: Some(handle.clone()),
    });
    assert_eq!(c.agent(&handle).state, "running_test");
}

#[test]
fn the_agents_never_registered_to_work_are_not_listed() {
    let c = Case::new();
    for _ in 0..3 {
        c.main_session();
    }
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 5.0);
    assert!(c.view().agents.is_empty());
    assert_eq!(c.view_all().agents.len(), 4); // and the orchestrator
}

#[test]
fn a_session_that_left_is_listed_only_with_all() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "idle", json!({}));
    c.f.store.leave(&handle).unwrap();
    assert!(!c.handles(&c.view()).contains(&handle));
    assert_eq!(found(&c.view_all(), &handle).handle, handle);
}

// -- which run is whose ----------------------------------------------------------------

/// A verify in a trial merge, or by an agent never launched (its worktree is the shared main checkout).
#[test]
fn a_run_tagged_with_its_handle_is_the_agents_wherever_it_runs() {
    let c = Case::new();
    let (a, b) = (c.main_session(), c.main_session());
    c.tagged(&a, "/wt/trial-merge");
    assert_eq!(c.agent(&a).state, "running_test");
    assert_eq!(c.agent(&b).state, "working");
}

#[test]
fn a_tagged_run_is_not_given_to_another_agent_by_worktree() {
    let c = Case::new();
    let owner = c.main_session();
    let alone = c.worker_in("/wt/alone");
    c.tagged(&owner, "/wt/alone");
    assert_eq!(c.agent(&alone).state, "working");
    assert_eq!(c.agent(&owner).state, "running_test");
}

#[test]
fn a_tagged_lost_agent_reads_live() {
    let c = Case::new();
    let handle = c.main_session();
    c.f.store.claim("sim/jump", &handle).unwrap();
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    c.tagged(&handle, "/wt/rev-trial");
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Live));
}

#[test]
fn a_run_tagged_with_an_unknown_handle_falls_back_to_the_worktree() {
    let c = Case::new();
    let handle = c.worker_in("/wt/alone");
    c.pane(&handle, "t5", "working", json!({}));
    c.tagged("nobody-here", "/wt/alone");
    assert_eq!(c.agent(&handle).state, "running_test");
}

/// Who shares a worktree, and which handles are known, count every session, listed or not.
#[test]
fn a_run_is_attributed_the_same_with_and_without_all() {
    let c = Case::new();
    let a = c.worker_in("/wt/alone");
    c.pane(&a, "t5", "working", json!({}));
    let ghost = c.f.session_with(NewSession {
        worktree: "/wt/alone".into(),
        ..local_session("worker")
    });
    c.f.store.leave(&ghost).unwrap();
    c.run_of("holding", "/wt/alone", NOW, "verify");
    assert_eq!(c.agent(&a).state, "working"); // the left session still shares the worktree
    assert_eq!(found(&c.view_all(), &a).state, "working");
    *c.runs.borrow_mut() = Some(vec![Run {
        pid: 1,
        worktree: "/wt/alone".into(),
        recipe: "verify".into(),
        phase: "holding".into(),
        since: Some(NOW),
        pool: "run".into(),
        handle: Some(ghost),
    }]);
    assert_eq!(c.agent(&a).state, "working"); // a known handle's run is not the worktree's
    assert_eq!(found(&c.view_all(), &a).state, "working");
}

#[test]
fn every_agent_with_a_run_is_answered_in_one_view() {
    let c = Case::new();
    let a = c.worker_in("/wt/a");
    c.f.store
        .create(&NewTask::new("sim/other", "Other", "owner"))
        .unwrap();
    let b = c.worker_on("sim/other", "t5", "/wt/b");
    c.run_of("waiting", "/wt/a", NOW, "verify");
    c.runs.borrow_mut().as_mut().unwrap()[0].handle = Some(a.clone());
    let asked = RefCell::new(Vec::new());
    let probes = Probes {
        panes: Box::new(|| {
            asked.borrow_mut().push("panes");
            Ok(Vec::new())
        }),
        runs: Box::new(|| {
            asked.borrow_mut().push("runs");
            Ok(c.runs.borrow().clone().unwrap())
        }),
        branch_of: Box::new(|_| Ok(String::new())),
    };
    let found = running_tests(&c.f.store, &probes).unwrap();
    assert_eq!(found.keys().collect::<Vec<_>>(), [&a]);
    assert_eq!(found[&a].phase, "waiting");
    assert_eq!(*asked.borrow(), ["panes", "runs"]);
    assert!(!found.contains_key(&b));
}

// -- probes that fail ------------------------------------------------------------------

#[test]
fn a_branch_lookup_that_fails_does_not_stop_the_view() {
    let c = Case::new();
    let handle = c.main_session();
    c.pane(
        &handle,
        "t9",
        "working",
        json!({"title": handle, "cwd": "/wt/other"}),
    );
    let probes = Probes {
        branch_of: Box::new(|_| Err("git is gone".into())),
        ..c.probes()
    };
    let view = describe(&c.f.store, &probes, NOW, false).unwrap();
    let a = found(&view, &handle);
    assert_eq!((a.branch.as_str(), a.worktree.as_str()), ("", "/wt/other"));
}

#[test]
fn a_terminal_without_an_address_is_ignored() {
    let c = Case::new();
    let handle = c.worker();
    *c.panes.borrow_mut() = Some(vec![
        json!({"agent": "claude", "status": "blocked", "argv": ["claude", "--session-id", format!("sess-{handle}")]}),
        json!({"address": "", "agent": "claude", "status": "blocked", "title": handle}),
        json!("t3"),
    ]);
    assert_eq!(c.agent(&handle).state, "working");
}

#[test]
fn hc_that_cannot_be_asked_is_reported_and_the_beat_decides() {
    let c = Case::new();
    let handle = c.worker();
    *c.panes.borrow_mut() = None;
    let view = c.view();
    assert!(!view.herdr);
    assert_eq!(parsed(&view)["probes"]["herdr"], "unavailable");
    assert_eq!(found(&view, &handle).state, "working");
}

#[test]
fn slots_that_cannot_be_read_are_reported() {
    let c = Case::new();
    c.worker();
    *c.runs.borrow_mut() = None;
    assert_eq!(parsed(&c.view())["probes"]["slots"], "unavailable");
}

#[test]
fn working_probes_say_ok() {
    let c = Case::new();
    assert_eq!(
        parsed(&c.view())["probes"],
        json!({"herdr": "ok", "slots": "ok"})
    );
}

// -- the state -------------------------------------------------------------------------

#[test]
fn a_working_pane_is_working() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    assert_eq!(c.agent(&handle).state, "working");
}

#[test]
fn a_blocked_pane_is_waiting_on_input() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "blocked", json!({}));
    assert_eq!(c.agent(&handle).state, "waiting_on_input");
}

#[test]
fn idle_and_done_panes_are_idle() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "done", json!({}));
    assert_eq!(c.agent(&handle).state, "idle");
}

#[test]
fn a_terminal_in_a_state_hc_did_not_have_falls_back_to_the_beat() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "unknown", json!({}));
    assert_eq!(c.agent(&handle).state, "working");
    c.f.advance(LIVE_SECONDS + 5.0);
    assert_eq!(c.agent(&handle).state, "idle"); // stale
}

#[test]
fn an_agent_with_no_pane_and_a_live_beat_is_working() {
    let c = Case::new();
    let handle = c.worker();
    assert_eq!(c.agent(&handle).state, "working");
}

#[test]
fn an_agent_quiet_past_the_stale_line_with_no_pane_is_gone() {
    let c = Case::new();
    let handle = c.worker();
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 5.0);
    assert_eq!(c.agent(&handle).state, "gone");
}

#[test]
fn a_pane_found_by_its_claude_session_when_none_is_recorded() {
    let c = Case::new();
    let handle = c.main_session();
    c.f.store
        .reserve("sim/jump", &handle, RESERVE_SECONDS)
        .unwrap();
    c.pane(
        &handle,
        "t9",
        "blocked",
        json!({"argv": ["claude", "--session-id", "abc"], "cwd": "/wt/elsewhere"}),
    );
    c.f.store.bind_agent("abc", &handle, false).unwrap();
    let a = c.agent(&handle);
    assert_eq!(
        (
            a.state,
            a.pane.as_deref(),
            a.session_id.as_deref(),
            a.worktree.as_str()
        ),
        ("waiting_on_input", Some("t9"), Some("abc"), "/wt/elsewhere")
    );
}

#[test]
fn an_unlaunched_agents_branch_comes_from_its_panes_directory() {
    let c = Case::new();
    let handle = c.main_session();
    c.pane(
        &handle,
        "t9",
        "working",
        json!({"title": handle, "cwd": "/wt/other"}),
    );
    c.branches
        .borrow_mut()
        .insert("/wt/other".into(), "tools/x".into());
    let a = c.agent(&handle);
    assert_eq!(
        (a.worktree.as_str(), a.branch.as_str(), a.pane.as_deref()),
        ("/wt/other", "tools/x", Some("t9"))
    );
}

#[test]
fn a_pane_named_for_the_handle_is_its_and_its_foreground_folder_is_where_it_works() {
    let c = Case::new();
    let handle = c.main_session();
    c.pane(
        &handle,
        "t9",
        "working",
        json!({"name": handle, "foreground": {"cwd": "/wt/fg"}, "cwd": "/wt/shell"}),
    );
    let a = c.agent(&handle);
    assert_eq!(
        (a.pane.as_deref(), a.worktree.as_str()),
        (Some("t9"), "/wt/fg")
    );
}

#[test]
fn the_agent_kind_comes_from_the_pane() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "idle", json!({"agent": "codex"}));
    assert_eq!(c.agent(&handle).agent.as_deref(), Some("codex"));
}

#[test]
fn a_pane_that_is_not_ours_is_not_matched() {
    let c = Case::new();
    let handle = c.worker();
    c.pane("other-worker", "t77", "blocked", json!({}));
    assert_eq!(c.agent(&handle).state, "working"); // no pane found: the live beat says so
}

#[test]
fn a_recorded_terminal_without_observed_local_ownership_is_not_named() {
    let c = Case::new();
    let handle = c.worker();
    let a = c.agent(&handle);
    assert_eq!((a.pane.as_deref(), a.agent), (None, None));
}

// -- running a test --------------------------------------------------------------------

#[test]
fn an_agent_holding_a_slot_is_running_a_test() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "idle", json!({}));
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "verify-full");
    let a = c.agent(&handle);
    assert_eq!(a.state, "running_test");
    assert_eq!(a.detail, detail("verify-full", "holding", NOW - 90.0));
}

#[test]
fn a_running_test_is_its_state_detail_in_the_json() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    let waiting = c.runs.borrow().clone().unwrap().remove(0);
    c.add_run(Run {
        since: None,
        ..waiting
    });
    let view = parsed(&c.view());
    let me = view["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["handle"] == handle.as_str())
        .unwrap();
    assert_eq!(
        me["state_detail"],
        json!({"recipe": "verify", "phase": "waiting", "since": null})
    );
}

#[test]
fn an_agent_waiting_for_a_slot_says_so() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    let a = c.agent(&handle);
    assert_eq!(a.state, "running_test");
    assert_eq!(a.detail, detail("verify", "waiting", NOW - 90.0));
}

#[test]
fn a_waiting_agent_is_never_lost() {
    let c = Case::new();
    let handle = c.worker();
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    assert_eq!(c.agent(&handle).health, Health::Lost);
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("waiting", "/wt/sim-jump", NOW, "verify-full");
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Live));
}

#[test]
fn the_same_agent_without_the_run_reads_gone_and_lost() {
    let c = Case::new();
    let handle = c.worker();
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("gone", Health::Lost));
}

#[test]
fn a_run_in_another_worktree_is_not_this_agents() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "idle", json!({}));
    c.run_of("holding", "/wt/other", NOW - 90.0, "verify-full");
    assert_eq!(c.agent(&handle).state, "idle");
}

#[test]
fn a_run_in_a_subdirectory_of_the_worktree_is_not_its_agents() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of(
        "holding",
        "/wt/sim-jump/artifacts/export",
        NOW - 90.0,
        "verify-full",
    );
    assert_eq!(c.agent(&handle).state, "working");
}

/// azure-raven's live case: one run under lamp-works/.worktrees/x made 25 main-checkout agents read
/// running_test.
#[test]
fn a_run_in_a_worktree_nested_under_the_main_checkout_is_not_every_main_agents() {
    let c = Case::new();
    let mains: Vec<String> = (0..3).map(|_| c.main_session()).collect();
    let lost = c.main_session();
    c.f.store.claim("sim/jump", &lost).unwrap(); // work another could take, so it reads lost rather than left
    c.run_of("holding", "/main/.worktrees/x", NOW - 90.0, "verify-full");
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    for h in &mains {
        c.f.store.touch(h).unwrap(); // the main-checkout agents beat; the last one stays quiet and is lost
    }
    let view = c.view();
    for h in &mains {
        assert_ne!(found(&view, h).state, "running_test", "{h}");
    }
    let a = found(&view, &lost);
    assert_eq!((a.state, a.health), ("gone", Health::Lost));
}

#[test]
fn a_run_in_the_main_checkout_is_not_attributed_to_agents_sharing_it() {
    let c = Case::new();
    let (a, b) = (c.main_session(), c.main_session());
    c.pane(&a, "t1", "working", json!({"name": a, "cwd": "/main"}));
    c.pane(&b, "t2", "working", json!({"name": b, "cwd": "/main"}));
    c.run_of("holding", "/main", NOW - 90.0, "verify-full");
    assert_eq!(
        (c.agent(&a).state, c.agent(&b).state),
        ("working", "working")
    );
}

#[test]
fn a_run_is_attributed_when_one_agent_alone_holds_the_worktree() {
    let c = Case::new();
    let handle = c.worker_in("/wt/alone");
    c.pane(&handle, "t5", "working", json!({}));
    c.main_session();
    c.run_of("holding", "/wt/alone", NOW - 90.0, "verify-full");
    assert_eq!(c.agent(&handle).state, "running_test");
}

#[test]
fn a_run_matches_its_worktree_through_a_symbolic_link() {
    let c = Case::new();
    let tmp = std::env::temp_dir().join(format!("agents-link-{}", std::process::id()));
    let real = tmp.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = tmp.join("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let handle = c.worker_in(link.to_str().unwrap());
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("holding", real.to_str().unwrap(), NOW - 90.0, "verify-full");
    let state = c.agent(&handle).state;
    std::fs::remove_dir_all(&tmp).unwrap();
    assert_eq!(state, "running_test");
}

#[test]
fn the_monitor_helper_agrees_with_the_view() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify-full");
    assert_eq!(
        running_tests(&c.f.store, &c.probes()).unwrap()[&handle].phase,
        "waiting"
    );
    let mains: Vec<String> = (0..2).map(|_| c.main_session()).collect();
    c.run_of("holding", "/main", NOW - 90.0, "verify-full");
    assert!(
        !running_tests(&c.f.store, &c.probes())
            .unwrap()
            .contains_key(&mains[0])
    );
}

#[test]
fn a_holding_run_beats_a_waiting_one() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "test-sim");
    assert_eq!(c.agent(&handle).detail.unwrap().phase, "holding");
}

#[test]
fn of_two_runs_the_one_that_began_first_is_shown() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("holding", "/wt/sim-jump", NOW - 10.0, "test-sim");
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "verify");
    assert_eq!(
        c.agent(&handle).detail,
        detail("verify", "holding", NOW - 90.0)
    );
}

// -- the table -------------------------------------------------------------------------

#[test]
fn the_table_is_a_column_per_field() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    let view = c.view();
    let lines = agents::table(&view);
    assert_eq!(lines.len(), 2);
    let row = |cols: [&str; 7]| {
        format!(
            "{:<18} {:<12} {:<17} {:<28} {:<9} {:<24} {}",
            cols[0], cols[1], cols[2], cols[3], cols[4], cols[5], cols[6]
        )
    };
    let line = |handle: &str| {
        lines
            .iter()
            .find(|l| l.starts_with(handle))
            .unwrap()
            .clone()
    };
    assert_eq!(
        line(&c.orch),
        row([&c.orch, "orchestrator", "working", "-", "-", "-", "-"])
    );
    assert_eq!(
        line(&handle),
        row([
            &handle,
            "worker",
            "running_test (waiting verify)",
            "sim/jump",
            "executor",
            "sim/jump",
            "t5"
        ])
    );
}

#[test]
fn a_view_of_nobody_is_an_empty_table() {
    let c = Case::new();
    c.f.store.leave(&c.orch).unwrap();
    let view = c.view();
    assert!(agents::table(&view).is_empty());
    assert_eq!(parsed(&view)["agents"], json!([]));
    assert_eq!(view.generated, NOW);
}

/// `just tasks agents` as the command line runs it, hc the crate's `fake-hc` with one blocked
/// terminal named for a worker bound to its conversation.
mod command {
    use crate::{Case, repository, scratch};
    use serde_json::{Value, json};
    use std::collections::BTreeSet;

    const FAKE: &str = env!("CARGO_BIN_EXE_fake-hc");

    struct Agents {
        c: Case,
        orch: String,
        worker: String,
    }

    impl Agents {
        fn new(name: &str) -> Agents {
            let dir = scratch(&format!("agents-{name}"));
            let mut c = Case::new();
            c.env.cwd = repository(&dir.join("repo")); // its slot locks are its own
            let orch = c.register("orchestrator");
            let worker = c.register_with("worker", &["--parent", &orch]);
            c.store().bind_agent("sess-1", &worker, false).unwrap();
            let state = dir.join("hc.json");
            let terminals = json!({"terminals": [
                {"address": "t5", "name": worker, "cwd": "/wt/x", "agent": "claude-code",
                 "state": "live", "status": "blocked", "turn": 2, "pid": 9,
                 "argv": ["claude", "--session-id", "sess-1"]}]});
            std::fs::write(&state, terminals.to_string()).unwrap();
            c.env.vars.insert("HC_BIN".into(), FAKE.into());
            c.env
                .vars
                .insert("FAKE_HC_STATE".into(), state.display().to_string());
            Agents { c, orch, worker }
        }

        fn hc_down(&mut self) {
            self.c.env.vars.insert("FAKE_HC_FAIL".into(), "ls".into());
        }

        fn json(&self, args: &[&str]) -> Value {
            serde_json::from_str(&self.c.ok(&self.orch, args)).unwrap()
        }

        fn handles(&self, args: &[&str]) -> Vec<String> {
            self.json(args)["agents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["handle"].as_str().unwrap().to_string())
                .collect()
        }
    }

    #[test]
    fn its_json_is_the_agents_contract() {
        let a = Agents::new("contract");
        let data = a.json(&["agents", "--json"]);
        let keys: BTreeSet<&str> = data
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from(["generated", "probes", "agents", "machine_work"])
        );
        let worker = data["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["handle"] == a.worker.as_str())
            .unwrap();
        assert_eq!(
            (&worker["state"], &worker["pane"], &worker["session_id"]),
            (&json!("waiting_on_input"), &json!("t5"), &json!("sess-1"))
        );
        assert_eq!(data["probes"]["herdr"], "ok");
    }

    #[test]
    fn the_table_names_the_handle_the_kind_and_the_state() {
        let a = Agents::new("table");
        let out = a.c.ok(&a.orch, &["agents"]);
        let row = out.lines().find(|l| l.contains(&a.worker)).unwrap();
        assert!(
            row.contains("worker") && row.contains("waiting_on_input"),
            "{row}"
        );
    }

    #[test]
    fn all_takes_in_the_sessions_that_left() {
        let a = Agents::new("all");
        let gone = a.c.register("worker");
        a.c.ok(&gone, &["leave"]);
        assert!(!a.handles(&["agents", "--json"]).contains(&gone));
        assert!(a.handles(&["agents", "--json", "--all"]).contains(&gone));
    }

    #[test]
    fn the_table_says_which_probe_could_not_be_asked() {
        let mut a = Agents::new("table-probe");
        a.hc_down();
        let out = a.c.ok(&a.orch, &["agents"]);
        // The rows (registered at one moment: by handle), then the probe that was down.
        let (last, rows) = out
            .lines()
            .collect::<Vec<_>>()
            .split_last()
            .map(|(l, r)| (*l, r.to_vec()))
            .unwrap();
        assert_eq!(rows.len(), 2, "{out}");
        assert!(rows.iter().any(|row| row.starts_with(&a.worker)), "{out}");
        assert_eq!(
            last,
            "(probe unavailable: hc; states come from the board alone)"
        );
    }

    #[test]
    fn it_needs_no_session() {
        let a = Agents::new("no-session");
        assert_eq!(a.c.run_as("", &["agents", "--json"], "").code, 0);
    }

    #[test]
    fn hc_that_is_down_is_a_probe_status_not_an_error() {
        let mut a = Agents::new("hc-down");
        a.hc_down();
        let data = a.json(&["agents", "--json"]);
        assert_eq!(data["probes"]["herdr"], "unavailable");
        assert!(a.handles(&["agents", "--json"]).contains(&a.worker));
    }
}
