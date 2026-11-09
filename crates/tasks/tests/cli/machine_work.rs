//! Exercise production board projection with private records, never live host publishers.
use crate::common::{Fixture, HOST};
use symphony_board::cli::agents::{self, Pane, Probes, Run};
use symphony_board::cli::machine_work::{
    Evidence, Freshness, Kind, Operation, Projection, Sample, State,
};
use symphony_board::{NewLaunch, NewSession, NewTask};

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
            worktree: checkout.into(),
            pane: "t1".into(),
            ..NewLaunch::new(slug, &handle, "executor")
        })
        .unwrap();
    handle
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
                name: None,
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
        assert!(operation.machine_id.is_none());
        assert!(operation.operation_id.is_none());
    }
    let remote = view.agents.iter().find(|a| a.handle == foreign).unwrap();
    assert_eq!(remote.state, "working");
    assert!(remote.agent.is_none());
    assert!(remote.pane.is_none());
    assert!(remote.detail.is_none());
    let unknown = view.agents.iter().find(|a| a.handle == unknown).unwrap();
    assert!(unknown.pane.is_none());
    assert!(unknown.detail.is_none());
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
    assert!(view.machine_work.machines[0].counts.is_none());
    assert_eq!(view.machine_work.machines[0].operations.len(), 3);
    assert!(view.machine_work.machines[0].operations[1].handle.is_none());
    assert!(view.machine_work.machines[0].operations[1].task.is_none());
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
    assert!(view.agents[0].detail.is_none());
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
    let probes = Probes {
        panes: Box::new(|| Ok(vec![])),
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
    assert!(closed.agents[0].detail.is_none());
    assert!(closed.agents[0].pane.is_none());
    assert!(closed.machine_work.unassigned[0].handle.is_none());
}
