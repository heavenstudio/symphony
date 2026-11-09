//! Exercise production board projection with private records, never live host publishers.
use crate::common::{Fixture, HOST};
use std::cell::RefCell;
use symphony_board::cli::agents::{self, Pane, Probes, Run};
use symphony_board::cli::machine_work::{
    Evidence, Freshness, Kind, Operation, Projection, Sample, State,
};
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::{Health, NewLaunch, NewSession, NewTask};

fn launch(f: &Fixture, slug: &str, host: &str, checkout: &str) -> String {
    f.store.create(&NewTask::new(slug, slug, "owner")).unwrap();
    let handle = f.session_with(NewSession {
        host: host.into(),
        worktree: checkout.into(),
        ..NewSession::new("worker")
    });
    f.store.claim(slug, &handle).unwrap();
    f.store
        .record_launch(&NewLaunch {
            claude_session: format!("conversation-{handle}"),
            worktree: checkout.into(),
            pane: "t1".into(),
            ..NewLaunch::new(slug, &handle, "executor")
        })
        .unwrap();
    handle
}

fn owned_pane(handle: &str) -> Pane {
    Pane {
        id: "t1".into(),
        agent: Some("codex".into()),
        status: Some("blocked".into()),
        session: None,
        cwd: "/same/path".into(),
        title: None,
        name: Some(handle.into()),
    }
}

fn reservation(checkout: &str) -> Run {
    Run {
        pid: 1,
        worktree: checkout.into(),
        recipe: "verify".into(),
        phase: "holding".into(),
        since: None,
        pool: "run".into(),
        handle: None,
    }
}

fn observed(f: &Fixture, panes: Vec<Pane>, runs: Vec<Run>) -> agents::View {
    let probes = Probes {
        panes: Box::new(|| Ok(panes.clone())),
        runs: Box::new(|| Ok(runs.clone())),
        branch_of: Box::new(|_| Ok(String::new())),
    };
    agents::describe_board_observed(
        &f.store,
        &probes,
        f.now(),
        false,
        &f.store.board().unwrap(),
        Evidence {
            local_host: HOST.into(),
            snapshot: Default::default(),
        },
    )
    .unwrap()
}

#[test]
fn machine_work_projects_every_reservation_and_fences_foreign_path_and_pane_owners() {
    let f = Fixture::empty();
    let local = launch(&f, "tools/local", HOST, "/same/path");
    let foreign = launch(&f, "tools/foreign", "other-host", "/same/path");
    let unknown = launch(&f, "tools/unknown", "", "/same/path");
    let probes = Probes {
        panes: Box::new(|| {
            Ok(vec![Pane {
                id: "t1".into(),
                agent: Some("codex".into()),
                status: Some("blocked".into()),
                session: None,
                cwd: "/same/path".into(),
                title: None,
                name: Some(local.clone()),
            }])
        }),
        runs: Box::new(|| {
            Ok((0..3)
                .map(|pid| Run {
                    pid,
                    worktree: "/same/path".into(),
                    recipe: "cargo test".into(),
                    phase: if pid == 0 { "waiting" } else { "holding" }.into(),
                    since: Some(1.0),
                    pool: "run".into(),
                    handle: None,
                })
                .collect())
        }),
        branch_of: Box::new(|_| Ok(String::new())),
    };
    let view = agents::describe_board_observed(
        &f.store,
        &probes,
        f.now(),
        false,
        &f.store.board().unwrap(),
        Evidence {
            local_host: HOST.into(),
            snapshot: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(view.machine_work.freshness, Freshness::Unknown);
    assert_eq!(view.machine_work.unassigned.len(), 3);
    for operation in &view.machine_work.unassigned {
        assert_eq!(operation.kind, Kind::Reservation);
        assert_eq!(operation.handle.as_deref(), Some(local.as_str()));
        assert_eq!(operation.task.as_deref(), Some("tools/local"));
        assert_eq!(operation.machine_id, None);
        assert_eq!(operation.operation_id, None);
    }
    let remote = view.agents.iter().find(|a| a.handle == foreign).unwrap();
    assert_eq!(remote.state, "working");
    assert_eq!(remote.agent, None);
    assert_eq!(remote.pane, None);
    assert_eq!(remote.detail, None);
    let unknown = view.agents.iter().find(|a| a.handle == unknown).unwrap();
    assert_eq!(unknown.pane, None);
    assert_eq!(unknown.detail, None);
    let local = view.agents.iter().find(|a| a.handle == local).unwrap();
    assert_eq!(local.state, "running_test");
    assert_eq!(local.detail.as_ref().unwrap().phase, "holding");
    let json: serde_json::Value =
        serde_json::from_str(&symphony_board::pyjson::dumps_line(&view.json())).unwrap();
    assert_eq!(
        json["machine_work"]["unassigned"].as_array().unwrap().len(),
        3
    );
    assert_eq!(json["agents"].as_array().unwrap().len(), 3);
}

#[test]
fn machine_work_receiver_snapshot_survives_board_projection_without_foreign_path_attribution() {
    let f = Fixture::empty();
    let handle = launch(&f, "tools/actual", HOST, "/active/path");
    f.store
        .create(&NewTask::new("tools/other", "Other", "owner"))
        .unwrap();
    f.store
        .take_role("tools/other", &handle, "reviewer")
        .unwrap();
    let operation = Operation {
        machine_id: Some("remote-machine".into()),
        operation_id: Some("native-1".into()),
        parent_id: Some("driver".into()),
        handle: Some(handle.clone()),
        task: Some("tools/actual".into()),
        checkout: "/active/path".into(),
        recipe: "native test".into(),
        kind: Kind::Test,
        state: State::Running,
        resource: "process".into(),
        reason: "native executable".into(),
        phase_since: Some(100.0),
    };
    let mut receiver = Projection::new(2, 10, 10);
    let mut foreign_unowned = operation.clone();
    foreign_unowned.operation_id = Some("native-2".into());
    foreign_unowned.handle = None;
    foreign_unowned.task = None;
    let mut previous_task = operation.clone();
    previous_task.operation_id = Some("native-3".into());
    previous_task.task = Some("tools/other".into());
    receiver
        .observe(
            Sample {
                machine_id: "remote-machine".into(),
                label: HOST.into(),
                producer_id: "publisher".into(),
                generation: 1,
                sequence: u64::MAX,
                complete: true,
                operations: vec![operation.clone(), operation, foreign_unowned, previous_task],
            },
            5,
        )
        .unwrap();
    let snapshot = receiver.snapshot(20);
    f.store
        .release("tools/other", &handle, Some("reviewer"))
        .unwrap();
    let probes = Probes {
        panes: Box::new(|| Ok(vec![])),
        runs: Box::new(|| Ok(vec![])),
        branch_of: Box::new(|_| Ok(String::new())),
    };
    let view = agents::describe_board_observed(
        &f.store,
        &probes,
        f.now(),
        false,
        &f.store.board().unwrap(),
        Evidence {
            local_host: HOST.into(),
            snapshot,
        },
    )
    .unwrap();
    assert_eq!(view.machine_work.machines[0].freshness, Freshness::Stale);
    assert_eq!(view.machine_work.machines[0].counts, None);
    assert_eq!(view.machine_work.machines[0].operations.len(), 3);
    assert_eq!(view.machine_work.machines[0].operations[1].handle, None);
    assert_eq!(view.machine_work.machines[0].operations[1].task, None);
    // Current roles do not veto explicit work attribution from before a role handover.
    assert_eq!(
        view.machine_work.machines[0].operations[2]
            .handle
            .as_deref(),
        Some(handle.as_str())
    );
    assert_eq!(
        view.machine_work.machines[0].operations[2].task.as_deref(),
        Some("tools/other")
    );
    let json: serde_json::Value =
        serde_json::from_str(&symphony_board::pyjson::dumps_line(&view.json())).unwrap();
    assert_eq!(
        json["machine_work"]["machines"][0]["operations"][0]["task"],
        "tools/actual"
    );
    assert_eq!(
        json["machine_work"]["machines"][0]["counts"],
        serde_json::Value::Null
    );
    assert_eq!(json["machine_work"]["machines"][0]["sequence"], u64::MAX);
    // A real operation does not overwrite legacy state_detail or task lifecycle.
    assert_eq!(view.agents[0].detail, None);
    assert_eq!(view.agents[0].tasks[0].status, "in_progress");
}

#[test]
fn machine_work_uses_the_active_checkout_even_when_primary_role_names_a_closed_launch() {
    let f = Fixture::empty();
    let handle = launch(&f, "aa/prior", HOST, "/old/path");
    f.store
        .close_task_session("aa/prior", &handle, "closed", "restore prior")
        .unwrap();
    f.store
        .create(&NewTask::new("zz/active", "Active", "owner"))
        .unwrap();
    f.store.claim("zz/active", &handle).unwrap();
    f.store
        .record_launch(&NewLaunch {
            worktree: "/active/path".into(),
            pane: "t2".into(),
            ..NewLaunch::new("zz/active", &handle, "executor")
        })
        .unwrap();
    let panes = RefCell::new(vec![owned_pane(&handle)]);
    let probes = Probes {
        panes: Box::new(|| Ok(panes.borrow().clone())),
        branch_of: Box::new(|_| Ok(String::new())),
        runs: Box::new(|| {
            Ok(vec![Run {
                pid: 1,
                worktree: "/active/path".into(),
                recipe: "check".into(),
                phase: "waiting".into(),
                since: None,
                pool: "run".into(),
                handle: None,
            }])
        }),
    };
    let view = agents::describe_board_observed(
        &f.store,
        &probes,
        f.now(),
        false,
        &f.store.board().unwrap(),
        Evidence {
            local_host: HOST.into(),
            snapshot: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(view.agents[0].task.as_deref(), Some("aa/prior"));
    assert_eq!(view.agents[0].worktree, "/active/path");
    let operation = &view.machine_work.unassigned[0];
    assert_eq!(operation.handle.as_deref(), Some(handle.as_str()));
    assert_eq!(operation.task.as_deref(), Some("zz/active"));
    f.store
        .close_task_session("zz/active", &handle, "closed", "restore active")
        .unwrap();
    panes.borrow_mut().clear();
    let closed = agents::describe_board_observed(
        &f.store,
        &probes,
        f.now(),
        false,
        &f.store.board().unwrap(),
        Evidence {
            local_host: HOST.into(),
            snapshot: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(closed.agents[0].detail, None);
    assert_eq!(closed.agents[0].pane, None);
    assert_eq!(closed.machine_work.unassigned[0].handle, None);
}

#[test]
fn machine_work_reused_checkout_does_not_revive_a_closed_launch_with_the_active_owners_run() {
    for old_pane_still_observed in [false, true] {
        let f = Fixture::empty();
        let closed = launch(&f, "tools/closed", HOST, "/same/path");
        f.store
            .close_task_session("tools/closed", &closed, "closed", "reused checkout")
            .unwrap();
        let active = launch(&f, "tools/active", HOST, "/same/path");
        f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
        let mut panes = vec![owned_pane(&active)];
        if old_pane_still_observed {
            panes.push(owned_pane(&closed));
        }
        let view = observed(&f, panes, vec![reservation("/same/path")]);
        let old = view.agents.iter().find(|a| a.handle == closed).unwrap();
        assert_eq!(old.health, Health::Lost, "{old:?}");
        assert_eq!(old.detail, None, "{old:?}");
        assert_ne!(old.state, "running_test", "{old:?}");
        let current = view.agents.iter().find(|a| a.handle == active).unwrap();
        assert_eq!(current.state, "running_test", "{current:?}");
        assert_eq!(current.health, Health::Live, "{current:?}");
        assert_eq!(
            view.machine_work.unassigned[0].handle.as_deref(),
            Some(active.as_str())
        );
        assert_eq!(
            view.machine_work.unassigned[0].task.as_deref(),
            Some("tools/active")
        );
    }
}

#[test]
fn machine_work_hostname_path_and_pane_id_collisions_require_observed_ownership() {
    let f = Fixture::empty();
    let handle = launch(&f, "tools/foreign", HOST, "/same/path");
    f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    let contradictory = Pane {
        session: Some("local-conversation".into()),
        name: Some("local-owner".into()),
        ..owned_pane(&handle)
    };
    for panes in [vec![], vec![contradictory]] {
        let view = observed(&f, panes, vec![reservation("/same/path")]);
        let agent = &view.agents[0];
        assert_eq!(agent.health, Health::Lost, "{agent:?}");
        assert_eq!(agent.detail, None, "{agent:?}");
        assert_eq!(agent.pane, None, "{agent:?}");
        assert_eq!(agent.agent, None, "{agent:?}");
        assert_eq!(view.machine_work.unassigned[0].handle, None);
        assert_eq!(view.machine_work.unassigned[0].task, None);
    }
    // Either an observed matching conversation or an explicit, uncontradicted handle is evidence.
    let conversation = Pane {
        session: Some(format!("conversation-{handle}")),
        name: Some("unrelated-label".into()),
        ..owned_pane(&handle)
    };
    for pane in [conversation, owned_pane(&handle)] {
        let view = observed(&f, vec![pane], vec![reservation("/same/path")]);
        assert_eq!(view.agents[0].state, "running_test");
        assert_eq!(
            view.machine_work.unassigned[0].handle.as_deref(),
            Some(handle.as_str())
        );
        assert_eq!(
            view.machine_work.unassigned[0].task.as_deref(),
            Some("tools/foreign")
        );
    }
}
