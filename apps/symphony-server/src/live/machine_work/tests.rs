//! Private native probe, receiver-clock and cache boundaries; no live board or machine identity.

use super::*;
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository,
};
use symphony_board::cli::machine_work::Freshness;

mod recorder;

/// Where the project at `root` names its machine-work reader, its folder made: the program is the
/// case's to write.
fn prepared_reader(root: &Path) -> PathBuf {
    std::fs::write(
        root.join(symphony::project::FILE),
        "[board]\nmachine_work_reader = \".target/bin/reader\"\n",
    )
    .unwrap();
    let reader = root.join(".target/bin/reader");
    std::fs::create_dir_all(reader.parent().unwrap()).unwrap();
    reader
}

/// The sampler of the project at `root` (`supplied` its named reader) whose reader is given a
/// stall's backstop to answer, not the tick's 2 s: these cases are of what a reader answers, never
/// of how soon, which a loaded machine would decide.
fn reading(root: &Path, supplied: Option<&Path>) -> Sampler {
    let mut sampler = Sampler::new(root, supplied);
    sampler.wait = test_time::backstop(symphony_board::cli::probes::NATIVE_READER_WAIT);
    sampler
}

fn input(state: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"version":1,"complete":false,"uncertain":false,"truncated":false,"records":[{
        "evidence":if state == "finished" { "finished" } else { "active" },
        "operation_id":"11111111111111111111111111111111","parent_id":null,"timing_run_id":"timing",
        "handle":"worker","checkout":"/private/checkout","recipe":"cargo build","kind":"build","state":state,
        "resource":"cargo-room","reason":"fixture","phase_since_epoch_ms":999999999999_u64,
        "owner_pid":123,"child_pid":null,"exit_code":if state == "finished" { Some(0) } else { None },
    }]})).unwrap()
}

fn sampler() -> Sampler {
    let mut sampler = Sampler::new(Path::new("/private/fixture"), None);
    sampler.identity = Some(Ok("machine-a".into()));
    sampler
}

fn record(task: &str) -> TaskSessionRow {
    serde_json::from_value(
        json!({"slug":task,"handle":"worker","role":"executor","claude_session":"",
        "worktree":"/private/checkout","started":1.0,"closed":null}),
    )
    .unwrap()
}

#[test]
fn record_uncertainty_keeps_future_verified_phases_visible_without_renewing_old_publications() {
    let mut sampler = sampler();
    sampler.accept(&input("running"), &[], 10, "host").unwrap();
    let mut next: Value = serde_json::from_slice(&input("queued")).unwrap();
    next["uncertain"] = json!(true);
    next["records"][0]["operation_id"] = json!("22222222222222222222222222222222");
    sampler
        .accept(&serde_json::to_vec(&next).unwrap(), &[], 20, "host")
        .unwrap();
    let view = sampler.snapshot(20).machines.remove(0);
    assert_eq!(view.operations.len(), 2);
    assert_eq!(view.freshness, Freshness::Unknown);
    assert_eq!(view.counts, None);
    let queued = view
        .operations
        .iter()
        .find(|operation| {
            operation.operation_id.as_deref() == Some("22222222222222222222222222222222")
        })
        .expect("new verified operation");
    assert_eq!(
        queued.state,
        symphony_board::cli::machine_work::State::Queued
    );
    next["records"] = json!([]);
    sampler
        .accept(&serde_json::to_vec(&next).unwrap(), &[], 30, "host")
        .unwrap();
    let view = sampler.snapshot(30).machines.remove(0);
    assert_eq!(view.operations.len(), 2);
    assert_eq!(view.age_ms, Some(20));
    assert_eq!(view.sequence, 2);
}

#[test]
fn a_new_or_changed_sibling_never_renews_an_absent_or_byte_unchanged_native_phase() {
    for include_unchanged in [false, true] {
        let mut sampler = sampler();
        sampler
            .accept(&input("running"), &[record("task-a")], 10, "host")
            .unwrap();
        let original = sampler.snapshot(10).machines[0].operations[0].clone();
        assert_eq!(
            sampler.snapshot(10_011).machines[0].freshness,
            Freshness::Stale
        );
        let mut next: Value = serde_json::from_slice(&input("queued")).unwrap();
        next["records"][0]["operation_id"] = json!("22222222222222222222222222222222");
        if include_unchanged {
            let old: Value = serde_json::from_slice(&input("running")).unwrap();
            next["records"]
                .as_array_mut()
                .unwrap()
                .push(old["records"][0].clone());
        }
        for (now, state, sequence) in [(10_012, "queued", 2), (10_013, "running", 3)] {
            next["records"][0]["state"] = json!(state);
            sampler
                .accept(
                    &serde_json::to_vec(&next).unwrap(),
                    &[record("task-b")],
                    now,
                    "host",
                )
                .unwrap();
            let snapshot = sampler.snapshot(now);
            let view = &snapshot.machines[0];
            assert_eq!(
                (view.sequence, view.age_ms, &view.freshness),
                (sequence, Some(now - 10), &Freshness::Stale),
                "unchanged={include_unchanged}: {view:?}"
            );
            assert_eq!(view.operations.len(), 2, "{view:?}");
            assert!(view.operations.contains(&original), "{view:?}");
            let sibling = view
                .operations
                .iter()
                .find(|operation| {
                    operation.operation_id.as_deref() == Some("22222222222222222222222222222222")
                })
                .expect("accepted independent sibling");
            assert_eq!(sibling.task.as_deref(), Some("task-b"));
            assert_eq!(
                sibling.state,
                if state == "queued" {
                    symphony_board::cli::machine_work::State::Queued
                } else {
                    symphony_board::cli::machine_work::State::Running
                }
            );
            assert_eq!(view.counts, None);
            assert!(!view.complete);
            // The same conservative mixed-age evidence reaches every cached API consumer.
            let wire: symphony_api::MachineWork =
                serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap();
            assert_eq!(
                wire.machines[0].freshness,
                symphony_api::WorkFreshness::Stale
            );
            assert_eq!(wire.machines[0].age_ms, Some(now - 10));
            assert_eq!(wire.machines[0].operations.len(), 2);
            assert_eq!(wire.machines[0].counts, None);
        }
    }
}

#[test]
fn unchanged_native_bytes_keep_original_receipt_and_frozen_explicit_task_context() {
    let mut sampler = sampler();
    sampler
        .accept(&input("queued"), &[record("task-a")], 10, "label")
        .unwrap();
    sampler
        .accept(&input("running"), &[record("task-b")], 20, "label")
        .unwrap();
    assert_eq!(
        sampler.snapshot(20).machines[0].operations[0]
            .task
            .as_deref(),
        Some("task-a")
    );
    sampler
        .accept(&input("running"), &[record("task-b")], 5000, "label")
        .unwrap();
    let mut reordered: Value = serde_json::from_slice(&input("running")).unwrap();
    let row = reordered["records"][0].as_object_mut().unwrap();
    let id = row.remove("operation_id").unwrap();
    row.insert("operation_id".into(), id);
    sampler
        .accept(&serde_json::to_vec(&reordered).unwrap(), &[], 5000, "label")
        .unwrap();
    let empty = serde_json::to_vec(
        &json!({"version":1,"complete":false,"uncertain":false,"truncated":false,"records":[]}),
    )
    .unwrap();
    sampler.accept(&empty, &[], 5000, "label").unwrap();
    let snapshot = sampler.snapshot(5000);
    assert_eq!(snapshot.machines[0].age_ms, Some(4980));
    assert_eq!(snapshot.machines[0].sequence, 2);
    sampler.unavailable("unreadable source".into());
    assert_eq!(
        sampler.snapshot(5000).machines[0].freshness,
        Freshness::Unknown
    );
    sampler
        .accept(&input("running"), &[], 6000, "label")
        .unwrap();
    assert_eq!(sampler.snapshot(6000).machines[0].age_ms, Some(5980));
    assert_eq!(
        sampler.snapshot(10021).machines[0].freshness,
        Freshness::Stale
    );
    sampler
        .accept(&input("finished"), &[], 10022, "label")
        .unwrap();
    let snapshot = sampler.snapshot(10022);
    assert!(snapshot.machines[0].operations.is_empty());
    assert!(!snapshot.machines[0].complete);
    assert_eq!(snapshot.machines[0].counts, None);
    assert!(
        sampler
            .accept(&input("running"), &[], 10023, "label")
            .is_err()
    );
    assert!(sampler.snapshot(10023).machines[0].operations.is_empty());
}

#[test]
fn one_native_probe_feeds_cached_agents_task_pages_and_listings_then_preserves_failed_reads() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let reader = prepared_reader(&root);
    crate::fixture_script::write(
        &reader,
        "#!/bin/sh\nprintf 'called\\n' >> \"$3/probe-calls\"\n/bin/cat \"$3/native.json\"\n",
        true,
    );
    std::fs::write(common.join("native.json"), input("running")).unwrap();
    let hub = Hub::new(&root, &root.join("client"));
    let mut sampler = reading(&root, None);
    sampler.identity = Some(Ok("machine-a".into()));
    *hub.machine_work.lock().unwrap() = sampler;
    let evidence = hub
        .machine_work
        .lock()
        .unwrap()
        .scan(&root, Some(&common), &[]);
    let work: symphony_api::MachineWork =
        serde_json::from_value(serde_json::to_value(evidence.snapshot).unwrap()).unwrap();
    assert_eq!(work.machines.len(), 1);
    assert_eq!(work.machines[0].operations.len(), 1);
    let f = Fixture::new(Seed::default());
    let agents = symphony_api::Agents {
        machine_work: work.clone(),
        generated: 1.0,
        probes: symphony_api::AgentProbes {
            panes: "unknown".into(),
            slots: "unknown".into(),
        },
        agents: Vec::new(),
    };
    hub.update_board(&f.store, f.store.board().unwrap(), agents)
        .unwrap();
    let page: symphony_api::TaskPage = serde_json::from_value(json!({
        "task":{"slug":"fixture/task","title":"Fixture","body":"","status":"review","priority":1,"parent":null,
            "created_by":"fixture","created":1.0,"updated":1.0,"links":[],"depends":[],"roles":[],"instruction":false},
        "blocks":[],"children":[],"children_count":0,
    })).unwrap();
    for _ in 0..5 {
        assert_eq!(hub.machine_snapshot(), work);
        assert_eq!(hub.agent_snapshot().unwrap().machine_work, work);
        let shown = hub.observed_page(page.clone());
        assert_eq!(shown.machine_work, work);
        assert_eq!(shown.task.status, "review");
    }
    assert_eq!(
        std::fs::read_to_string(common.join("probe-calls")).unwrap(),
        "called\n"
    );
    std::fs::write(common.join("native.json"), "{").unwrap();
    let failed = hub
        .machine_work
        .lock()
        .unwrap()
        .scan(&root, Some(&common), &[])
        .snapshot;
    assert_eq!(failed.machines[0].operations.len(), 1);
    assert_eq!(failed.machines[0].freshness, Freshness::Unknown);
    assert_eq!(failed.machines[0].sequence, 1);
    assert_eq!(failed.machines[0].counts, None);
    assert!(!failed.machines[0].complete);
    assert_eq!(
        std::fs::read_to_string(common.join("probe-calls")).unwrap(),
        "called\ncalled\n"
    );
}

#[test]
fn unavailable_reader_or_identity_never_claims_a_machine_or_retires_unassigned_evidence() {
    let tree = Tree::new();
    let root = repository(&tree);
    std::fs::create_dir_all(root.join("tools")).unwrap();
    crate::fixture_script::write(
        &root.join("tools/rs-bin.sh"),
        "#!/bin/sh\nprintf invoked > launcher-called\nexit 99\n",
        true,
    );
    // A project that names no reader, then one whose named reader is not there yet: neither runs
    // anything, and each says why.
    let mut reader = None;
    for named in [false, true] {
        if named {
            reader = Some(prepared_reader(&root));
        }
        let mut sampler = Sampler::new(&root, None);
        let evidence = sampler.scan(&root, Some(&tree.path("missing-common")), &[]);
        assert!(sampler.identity.is_none());
        assert_eq!(evidence.snapshot.freshness, Freshness::Unknown);
        assert!(evidence.snapshot.machines.is_empty());
        assert!(
            !root.join("launcher-called").exists(),
            "a missing prepared reader invoked a source launcher"
        );
        let why = reader.as_ref().map_or_else(
            || "[board] machine_work_reader".to_string(),
            |reader| reader.display().to_string(),
        );
        assert!(sampler.reason.contains(&why), "{}", sampler.reason);
    }
    let reader = reader.unwrap();
    let mut sampler = reading(&root, None);
    crate::fixture_script::write(&reader, "#!/bin/sh\n/bin/cat \"$3/native.json\"\n", true);
    let common = root.join(".git");
    sampler.identity = Some(Err("corrupt installation identity".into()));
    std::fs::write(common.join("native.json"), input("running")).unwrap();
    let first = sampler.scan(&root, Some(&common), &[]).snapshot;
    assert!(first.machines.is_empty());
    assert_eq!(first.unassigned.len(), 1);
    assert_eq!(first.unassigned[0].machine_id, None);
    assert_eq!(first.unassigned[0].task, None);
    for bytes in [input("finished"), b"{".to_vec()] {
        std::fs::write(common.join("native.json"), bytes).unwrap();
        let retained = sampler.scan(&root, Some(&common), &[]).snapshot;
        assert_eq!(retained.freshness, Freshness::Unknown);
        assert!(retained.machines.is_empty());
        assert_eq!(
            retained.unassigned, first.unassigned,
            "unassigned evidence was retired without machine identity"
        );
    }
    assert_eq!(sampler.sequence, 0);
}

#[test]
fn explicit_or_prepared_reader_paths_are_retained_without_a_launcher_or_cache_search() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let explicit = tree.path("private-reader");
    assert_eq!(Sampler::reader(&root, None).unwrap(), None);
    assert_eq!(
        Sampler::reader(&root, Some(&explicit)).unwrap(),
        Some(explicit.clone())
    );
    let prepared = prepared_reader(&root);
    assert_eq!(Sampler::reader(&root, None).unwrap(), Some(prepared));
    assert_eq!(
        Sampler::reader(&root, Some(&explicit)).unwrap(),
        Some(explicit.clone()),
        "a named reader wins over the project's"
    );
    assert!(Sampler::reader(&root, Some(Path::new("reader"))).is_err());
    let script =
        "#!/bin/sh\nprintf 'called\\n' >> \"$3/explicit-calls\"\n/bin/cat \"$3/native.json\"\n";
    crate::fixture_script::write(&explicit, script, true);
    std::fs::write(common.join("native.json"), "{").unwrap();
    let mut sampler = reading(&root, Some(&explicit));
    sampler.scan(&root, Some(&common), &[]);
    assert!(
        sampler.identity.is_none(),
        "malformed output initialized identity"
    );
    sampler.identity = Some(Ok("machine-a".into()));
    std::fs::write(common.join("native.json"), input("running")).unwrap();
    assert_eq!(
        sampler.scan(&root, Some(&common), &[]).snapshot.machines[0]
            .operations
            .len(),
        1
    );
    let mut unsupported: Value = serde_json::from_slice(&input("running")).unwrap();
    unsupported["version"] = json!(2);
    std::fs::write(
        common.join("native.json"),
        serde_json::to_vec(&unsupported).unwrap(),
    )
    .unwrap();
    for fault in ["unsupported", "removed", "not executable"] {
        if fault == "removed" {
            std::fs::remove_file(&explicit).unwrap();
        }
        if fault == "not executable" {
            use std::os::unix::fs::PermissionsExt;
            // The script helper shares executable inodes; this permission case owns a real file.
            assert!(!explicit.exists());
            std::fs::write(&explicit, script).unwrap();
            std::fs::set_permissions(&explicit, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(
                std::fs::metadata(&explicit).unwrap().permissions().mode() & 0o111,
                0
            );
        }
        let view = sampler
            .scan(&root, Some(&common), &[])
            .snapshot
            .machines
            .remove(0);
        assert_eq!(view.freshness, Freshness::Unknown, "{fault}");
        assert_eq!(view.operations.len(), 1, "{fault}");
        assert_eq!(view.sequence, 1, "{fault}");
        assert_eq!(view.counts, None, "{fault}");
        assert_eq!(sampler.reader.as_ref().unwrap().as_ref(), Some(&explicit));
    }
    crate::fixture_script::write(&explicit, script, true);
    std::fs::write(common.join("native.json"), input("running")).unwrap();
    let view = sampler
        .scan(&root, Some(&common), &[])
        .snapshot
        .machines
        .remove(0);
    assert_eq!(view.freshness, Freshness::Incomplete);
    assert_eq!(
        view.sequence, 1,
        "restored unchanged bytes renewed publication"
    );
    assert_eq!(
        std::fs::read_to_string(common.join("explicit-calls")).unwrap(),
        "called\ncalled\ncalled\ncalled\n"
    );
}

#[test]
fn a_reader_with_no_answer_within_its_wait_is_given_up_and_the_machine_unknown_until_one_answers() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let reader = prepared_reader(&root);
    let answers = "#!/bin/sh\n/bin/cat \"$3/native.json\"\n";
    crate::fixture_script::write(&reader, answers, true);
    std::fs::write(common.join("native.json"), input("running")).unwrap();
    let mut sampler = reading(&root, None);
    sampler.identity = Some(Ok("machine-a".into()));
    let known = sampler.scan(&root, Some(&common), &[]).snapshot;
    assert_eq!(known.machines[0].freshness, Freshness::Incomplete);
    // A reader that never answers, its sleep the very process the kill past the wait ends: a
    // wait of a second is one it cannot meet, whatever the machine's load.
    crate::fixture_script::write(&reader, "#!/bin/sh\nexec /bin/sleep 600\n", true);
    sampler.wait = Duration::from_secs(1);
    let view = sampler
        .scan(&root, Some(&common), &[])
        .snapshot
        .machines
        .remove(0);
    assert_eq!(
        view.reason,
        format!(
            "native observation unknown: native inventory reader {}: no answer in 1 s",
            reader.display()
        )
    );
    assert_eq!(view.freshness, Freshness::Unknown);
    assert_eq!(view.operations.len(), 1, "the last evidence is kept");
    assert_eq!((view.sequence, view.counts), (1, None));
    // The next scan asks again, and a reader that answers makes the machine known again.
    crate::fixture_script::write(&reader, answers, true);
    sampler.wait = test_time::backstop(symphony_board::cli::probes::NATIVE_READER_WAIT);
    let view = sampler
        .scan(&root, Some(&common), &[])
        .snapshot
        .machines
        .remove(0);
    assert_eq!(view.freshness, Freshness::Incomplete, "{view:?}");
    assert_eq!(view.reason, known.machines[0].reason);
}

#[test]
fn first_native_scan_uses_full_recorded_task_context_without_trusting_phase_clocks() {
    let mut prior = record("task-a");
    prior.started = 1.0;
    prior.closed = Some(2.0);
    let mut current = record("task-b");
    current.started = 3.0;
    let mut same_task = prior.clone();
    same_task.slug = "task-b".into();
    let mut other_owner = prior.clone();
    other_owner.handle = "other-worker".into();
    let mut sibling_checkout = prior.clone();
    sibling_checkout.worktree = "/private/checkout-other".into();
    let mut parent_checkout = prior.clone();
    parent_checkout.worktree = "/private".into();
    let mut unknown_checkout = prior.clone();
    unknown_checkout.worktree.clear();
    let mut relative_checkout = prior.clone();
    relative_checkout.worktree = "private/checkout".into();
    for (name, records, expected) in [
        ("one supported role", vec![current.clone()], Some("task-b")),
        ("reused launch", vec![prior.clone(), current.clone()], None),
        (
            "reverse history",
            vec![current.clone(), prior.clone()],
            None,
        ),
        ("closed only", vec![prior], None),
        (
            "concurrent tasks",
            vec![record("task-a"), current.clone()],
            None,
        ),
        (
            "same task history",
            vec![same_task, current.clone()],
            Some("task-b"),
        ),
        (
            "other owner",
            vec![other_owner, current.clone()],
            Some("task-b"),
        ),
        (
            "sibling checkout",
            vec![sibling_checkout, current.clone()],
            None,
        ),
        (
            "parent checkout",
            vec![parent_checkout, current.clone()],
            None,
        ),
        (
            "unknown history",
            vec![unknown_checkout, current.clone()],
            None,
        ),
        (
            "relative history",
            vec![relative_checkout, current.clone()],
            None,
        ),
        ("missing context", vec![], None),
    ] {
        for epoch in [1500, 9_999_999_999_999_u64] {
            let mut sampler = sampler();
            let publication = |state| {
                let mut value: Value = serde_json::from_slice(&input(state)).unwrap();
                value["records"][0]["phase_since_epoch_ms"] = json!(epoch);
                serde_json::to_vec(&value).unwrap()
            };
            sampler
                .accept(&publication("running"), &records, 10, "host")
                .unwrap();
            let view = sampler.snapshot(10).machines.remove(0);
            assert_eq!(view.operations.len(), 1, "{name}: {view:?}");
            let operation = &view.operations[0];
            assert_eq!(operation.task.as_deref(), expected, "{name}, epoch={epoch}");
            assert_eq!(operation.handle.as_deref(), Some("worker"));
            assert_eq!(operation.machine_id.as_deref(), Some("machine-a"));
            assert_eq!(operation.phase_since, Some(epoch as f64 / 1000.0));
            assert_eq!(view.counts, None);
            assert!(!view.complete);
            // A first unknown remains unknown even if a later read loses historical rows.
            // A supported initial binding remains exactly the same through later phases.
            sampler
                .accept(&publication("queued"), &[current.clone()], 20, "host")
                .unwrap();
            assert_eq!(
                sampler.snapshot(20).machines[0].operations[0]
                    .task
                    .as_deref(),
                expected,
                "{name}: binding changed"
            );
            sampler
                .accept(&publication("finished"), &[current.clone()], 21, "host")
                .unwrap();
            let finished = sampler.snapshot(21).machines.remove(0);
            assert!(finished.operations.is_empty(), "{name}: {finished:?}");
            assert_eq!(finished.counts, None);
            assert!(!finished.complete);
        }
    }
}

#[test]
fn a_native_phase_without_a_handle_stays_visible_without_acquiring_the_checkout_task() {
    let mut sampler = sampler();
    let mut value: Value = serde_json::from_slice(&input("running")).unwrap();
    value["records"][0]["handle"] = Value::Null;
    sampler
        .accept(
            &serde_json::to_vec(&value).unwrap(),
            &[record("task-a")],
            10,
            "host",
        )
        .unwrap();
    let view = sampler.snapshot(10).machines.remove(0);
    assert_eq!(view.operations.len(), 1, "{view:?}");
    assert_eq!(view.operations[0].handle, None);
    assert_eq!(view.operations[0].task, None);
    assert_eq!(view.operations[0].machine_id.as_deref(), Some("machine-a"));
    assert_eq!(view.counts, None);
    assert!(!view.complete);
}

#[test]
fn ambiguous_or_later_task_roles_do_not_supply_an_initial_task_binding() {
    let mut sampler = sampler();
    sampler
        .accept(&input("queued"), &[record("a"), record("b")], 1, "host")
        .unwrap();
    sampler
        .accept(&input("running"), &[record("a")], 2, "host")
        .unwrap();
    assert_eq!(sampler.snapshot(2).machines[0].operations[0].task, None);
}

#[test]
fn an_explicit_role_covers_native_package_cwds_but_never_siblings_or_traversal() {
    for (checkout, expected) in [
        ("/private/checkout/tools/gate", Some("task-a")),
        ("/private/checkout-other/tools/gate", None),
        ("/private/checkout/../elsewhere", None),
        ("private/checkout/tools/gate", None),
    ] {
        let mut sampler = sampler();
        let mut bytes: Value = serde_json::from_slice(&input("running")).unwrap();
        bytes["records"][0]["checkout"] = json!(checkout);
        sampler
            .accept(
                &serde_json::to_vec(&bytes).unwrap(),
                &[record("task-a")],
                1,
                "host",
            )
            .unwrap();
        assert_eq!(
            sampler.snapshot(1).machines[0].operations[0]
                .task
                .as_deref(),
            expected,
            "cwd {checkout}"
        );
    }
}

#[test]
fn native_guidance_map_keeps_optional_zero_and_maximum_values_and_lexical_keys() {
    let tree = Tree::new();
    let root = repository(&tree);
    std::fs::write(
        root.join("symphony.toml"),
        "[board.machine_wip.z]\nbuild=0\n[board.machine_wip.a]\ntest=4294967295\n",
    )
    .unwrap();
    let f = Fixture::new(Seed::default());
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let board = f.other_database();
    hub.connect_reader = Box::new(move |_| Ok(crate::testing::store_on(&board)));
    hub.refresh_page("", &root).unwrap();
    hub.board_read(&f.store).unwrap();
    let expected = std::collections::BTreeMap::from([
        (
            "a".into(),
            symphony_api::WorkGuidance {
                build: None,
                test: Some(u32::MAX),
            },
        ),
        (
            "z".into(),
            symphony_api::WorkGuidance {
                build: Some(0),
                test: None,
            },
        ),
    ]);
    assert_eq!(
        hub.agent_snapshot().unwrap().machine_work.guidance,
        expected
    );
    let (shared, ask) = crate::live::read_paths_tests::served(&root, hub);
    let (status, value) = ask("/api/agents");
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        value["machine_work"]["guidance"],
        serde_json::to_value(&expected).unwrap()
    );
    std::fs::write(root.join("symphony.toml"), "").unwrap();
    let hub = shared.api.get().unwrap();
    hub.board_read(&f.store).unwrap();
    assert!(
        hub.agent_snapshot()
            .unwrap()
            .machine_work
            .guidance
            .is_empty()
    );
    shared.stop();
}

#[test]
fn guidance_and_signoff_map_payloads_keep_last_values_and_report_cost() {
    fn compare<V: Clone + PartialEq + std::fmt::Debug>(name: &str, pairs: Vec<(String, V)>) {
        for count in [0, 1, 32, 512] {
            for reversed in [false, true] {
                let mut input = pairs[..count].to_vec();
                input.sort_by(|a, b| a.0.cmp(&b.0));
                if reversed {
                    input.reverse();
                }
                let old_input = input.clone();
                let (old, old_cpu) =
                    test_time::cpu::spent(|| old_input.into_iter().collect::<BTreeMap<_, _>>());
                let (new, new_cpu) = test_time::cpu::spent(|| {
                    let mut map = BTreeMap::new();
                    for (k, v) in input {
                        map.insert(k, v);
                    }
                    map
                });
                assert_eq!(new, old);
                eprintln!(
                    "{name} n={count},reversed={reversed}: standard_cpu={old_cpu:?},insert_cpu={new_cpu:?};no collection pair Vec/stable scratch in production insertion"
                );
            }
        }
    }
    compare(
        "guidance",
        (0..512)
            .map(|i| {
                (
                    format!("machine-{}", i % 37),
                    symphony_api::WorkGuidance {
                        build: (i % 3 != 0).then_some(i),
                        test: (i % 2 == 0).then_some(u32::MAX - i),
                    },
                )
            })
            .collect(),
    );
    compare(
        "signoff lane",
        (0..512)
            .map(|i| {
                (
                    format!("task-{}", i % 37),
                    vec![format!("card-{i}"), "exact second card".into()],
                )
            })
            .collect(),
    );
}
