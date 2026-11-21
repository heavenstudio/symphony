//! The board's agents view on the Symphony's wire.

use super::page_tests::store_fixture::Fixture;
use super::{agents, wire_agents};
use crate::test_tree::Tree;
use serde::{Deserialize, Serialize};
use serde_json::json;
use symphony_board::cli::agents::View;
use symphony_board::sys::Command;

/// The probes as a client from before the panes probe's rename reads them: symphony-api's type
/// then (c2b3d598), its fields private here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct AgentProbes {
    herdr: String,
    slots: String,
}

#[derive(Deserialize)]
struct LegacyAgent {
    handle: String,
    state: String,
    state_detail: Option<symphony_api::AgentRun>,
}

#[derive(Deserialize)]
struct LegacyAgents {
    agents: Vec<LegacyAgent>,
}

#[test]
fn the_wire_names_the_boards_panes_probe_panes_and_herdr_with_its_answer() {
    for (asked, said) in [(true, "ok"), (false, "unavailable")] {
        let view = View {
            machine_work: Default::default(),
            generated: 1.5,
            herdr: asked,
            slots: true,
            agents: Vec::new(),
        };
        let wire = serde_json::to_value(wire_agents(&view).unwrap()).unwrap();
        assert_eq!(
            wire,
            json!({
                "generated": 1.5,
                "probes": {"panes": said, "herdr": said, "slots": "ok"},
                "agents": [],
                "machine_work": {"freshness": "unknown", "machines": [], "unassigned": [], "guidance": {}},
            })
        );
        // An open tab's client from before the rename reads the probe as it did.
        let before: AgentProbes = serde_json::from_value(wire["probes"].clone()).unwrap();
        assert_eq!(
            before,
            AgentProbes {
                herdr: said.into(),
                slots: "ok".into(),
            }
        );
    }
}

#[test]
fn machine_work_board_projection_serializes_on_the_typed_wire_without_changing_legacy_state() {
    use symphony_board::cli::{agents, machine_work as work};
    let f = Fixture::empty();
    let handle = f.session("reviewer");
    let mut projection = work::Projection::new(2, 8, 10);
    projection
        .observe(
            work::Sample {
                machine_id: "machine-a".into(),
                label: "builder".into(),
                producer_id: "publisher".into(),
                generation: 1,
                sequence: u64::MAX,
                complete: true,
                operations: vec![work::Operation {
                    machine_id: Some("machine-a".into()),
                    operation_id: Some("native-test-1".into()),
                    parent_id: None,
                    handle: Some(handle.clone()),
                    task: None,
                    checkout: "/foreign/path".into(),
                    recipe: "native test".into(),
                    kind: work::Kind::Test,
                    state: work::State::Running,
                    resource: "process".into(),
                    reason: "executable observed".into(),
                    phase_since: None,
                }],
            },
            0,
        )
        .unwrap();
    let probes = agents::Probes {
        panes: Box::new(|| Ok(vec![])),
        runs: Box::new(|| {
            Ok(vec![agents::Run {
                pid: 1,
                handle: Some(handle.clone()),
                worktree: "/foreign/path".into(),
                recipe: "verify".into(),
                phase: "holding".into(),
                since: Some(7.0),
                pool: "run".into(),
            }])
        }),
        branch_of: Box::new(|_| Ok(String::new())),
        processes: Box::new(|| Err("no process probe in this fixture".into())),
    };
    let view = agents::describe_board_observed(
        &f.store,
        &probes,
        f.now(),
        false,
        &f.store.board().unwrap(),
        work::Evidence {
            local_host: "local".into(),
            snapshot: projection.snapshot(0),
        },
    )
    .unwrap();
    let typed = wire_agents(&view).unwrap();
    for value in [0, u64::try_from(i64::MAX).unwrap() + 1, u64::MAX] {
        let mut boundary = view.clone();
        boundary.machine_work = projection.snapshot(value);
        boundary.machine_work.machines[0].sequence = value;
        let sent = wire_agents(&boundary).unwrap();
        assert_eq!(sent.machine_work.machines[0].sequence, value);
        assert_eq!(sent.machine_work.machines[0].age_ms, Some(value));
    }
    assert_eq!(typed.agents[0].kind, "reviewer");
    assert_eq!(typed.machine_work.machines[0].sequence, u64::MAX);
    assert_eq!(
        typed.machine_work.machines[0]
            .counts
            .as_ref()
            .unwrap()
            .running_tests,
        1
    );
    assert_eq!(
        typed.machine_work.machines[0].operations[0]
            .handle
            .as_deref(),
        Some(handle.as_str())
    );
    let legacy: LegacyAgents = serde_json::from_value(view.json()).unwrap();
    assert_eq!(legacy.agents[0].handle, handle);
    assert_eq!(legacy.agents[0].state, "running_test");
    assert_eq!(
        legacy.agents[0].state_detail.as_ref().unwrap().recipe,
        "verify"
    );
    assert_eq!(legacy.agents[0].state, typed.agents[0].state);
    assert_eq!(legacy.agents[0].state_detail, typed.agents[0].state_detail);
}

#[test]
fn the_typed_wire_carries_an_agents_tokens_and_cost_and_none_for_one_that_took_nothing() {
    use symphony_board::cli::{agents, machine_work as work};
    use symphony_board::usage::{Spent, Tokens};
    let f = Fixture::empty();
    let (spender, quiet) = (f.session("worker"), f.session("worker"));
    assert!(f.store.bind_agent("agent-1", &spender, false).unwrap());
    let spent = Spent {
        model: "claude-opus-5-5".into(),
        speed: "standard".into(),
        tokens: Tokens {
            input: 12,
            cache_write_5m: 3_400,
            cache_write_1h: 560,
            cache_read: 78_900,
            output: 1_200,
            calls: 7,
        },
    };
    let beat = symphony_board::Beat::default();
    f.store
        .agent_beat_spending("agent-1", &beat, None, None, &[spent])
        .unwrap();
    let probes = agents::Probes {
        panes: Box::new(|| Ok(vec![])),
        runs: Box::new(|| Ok(vec![])),
        branch_of: Box::new(|_| Ok(String::new())),
        processes: Box::new(|| Err("no process probe in this fixture".into())),
    };
    let view = agents::describe_board_observed(
        &f.store,
        &probes,
        f.now(),
        true,
        &f.store.board().unwrap(),
        work::Evidence {
            local_host: "local".into(),
            snapshot: work::Projection::new(2, 8, 10).snapshot(0),
        },
    )
    .unwrap();
    let typed = wire_agents(&view).unwrap();
    let tokens = |handle: &str| {
        (typed.agents.iter())
            .find(|agent| agent.handle == handle)
            .map(|agent| agent.tokens)
    };
    assert_eq!(
        tokens(&spender),
        Some(Some(symphony_api::AgentTokens {
            input: 12,
            cache_write_5m: 3_400,
            cache_write_1h: 560,
            cache_read: 78_900,
            output: 1_200,
            calls: 7,
        }))
    );
    assert_eq!(tokens(&quiet), Some(None));
    // Their cost reaches the wire as the board priced it, with the day its prices were read.
    let prices = symphony_board::usage::Prices::embedded();
    let priced = prices.cost(
        "claude-opus-5-5",
        "standard",
        &Tokens {
            input: 12,
            cache_write_5m: 3_400,
            cache_write_1h: 560,
            cache_read: 78_900,
            output: 1_200,
            calls: 7,
        },
    );
    let cost = |handle: &str| {
        (typed.agents.iter())
            .find(|agent| agent.handle == handle)
            .and_then(|agent| agent.cost.clone())
    };
    let wired = cost(&spender).unwrap();
    let board: symphony_api::AgentCost = serde_json::from_value(priced.json(&prices.read)).unwrap();
    assert_eq!(wired, board);
    assert!(wired.dollars > 0.0 && wired.unpriced.is_empty());
    assert_eq!(wired.prices, prices.read);
    assert_eq!(cost(&quiet), None);
}

/// The agents view asks hc for its terminals with this process's environment as the board reads
/// it. Started from an agent's hc terminal, that environment names the agent's session and the
/// terminal and core it runs in; hc must see none of them, or it would answer for that core alone.
#[test]
fn the_agents_views_hc_probe_runs_without_the_starters_session() {
    const ROOT: &str = "SYMPHONY_AGENTS_PROBE_ROOT";
    if let Some(root) = std::env::var_os(ROOT) {
        let board = Fixture::empty();
        agents(&board.store, std::path::Path::new(&root)).unwrap();
        return;
    }
    let tree = Tree::new();
    // hc's stand-in: what it holds of the session, then no terminal.
    let hc = tree.write(
        "hc",
        "#!/bin/sh\nprintf '%s|%s|%s|%s|%s\\n' \"${TASKS_SESSION-unset}\" \"${HC_TERMINAL_ID-unset}\" \
         \"${HC_TERMINAL_SOCKET-unset}\" \"${HC_SESSION-unset}\" \"${HC_SCOPE-unset}\" \
         >> \"$SYMPHONY_AGENTS_PROBE_ROOT/held\"\nprintf '[]\\n'\n",
    );
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hc, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "taskview::agents_tests::the_agents_views_hc_probe_runs_without_the_starters_session",
            ])
            .env(ROOT, &tree.root)
            .env("HC_BIN", &hc)
            .env("TASKS_SESSION", "fixture-session")
            .env("HC_TERMINAL_ID", "t7")
            .env("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock")
            .env("HC_SESSION", "fixture-core")
            .env("HC_SCOPE", "fixture-scope"),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let held = tree.read("held");
    assert!(!held.is_empty(), "hc was never asked");
    for line in held.lines() {
        assert_eq!(line, "unset|unset|unset|unset|unset");
    }
}

#[test]
fn a_tasks_tokens_are_its_and_its_subtasks_tally_and_one_with_none_has_none() {
    use symphony_board::NewTask;
    use symphony_board::usage::{Prices, Spent, Tally, Tokens};
    let f = Fixture::empty();
    let (orch, exe, sub) = (
        f.session("orchestrator"),
        f.session("worker"),
        f.session("worker"),
    );
    f.task_with(NewTask::new("sim/jump", "Jump", &orch));
    f.task_with(NewTask {
        parent: Some("sim/jump".into()),
        ..NewTask::new("sim/jump/fix", "Fix", &orch)
    });
    f.task_with(NewTask::new("sim/quiet", "Quiet", &orch));
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.claim("sim/jump/fix", &sub).unwrap();
    let spent = |model: &str, n: u64| Spent {
        model: model.into(),
        speed: "standard".into(),
        tokens: Tokens {
            input: n,
            output: n,
            calls: 1,
            ..Tokens::default()
        },
    };
    for (key, handle, model) in [("a", &exe, "claude-opus-5-5"), ("b", &sub, "gpt-x")] {
        assert!(f.store.bind_agent(key, handle, false).unwrap());
        let beat = symphony_board::Beat::default();
        f.store
            .agent_beat_spending(key, &beat, None, None, &[spent(model, 100)])
            .unwrap();
    }
    let tokens = super::tokens(&f.store, "sim/jump").unwrap();
    let rows = f.store.spent_on(&["sim/jump", "sim/jump/fix"]).unwrap();
    let prices = Prices::embedded();
    let tally: symphony_api::TaskTokens =
        serde_json::from_value(Tally::at(&rows, prices).json(&[], prices)).unwrap();
    assert_eq!(tokens, tally);
    assert_eq!(tokens.total.calls, 2, "its subtask's calls with its own");
    assert_eq!(tokens.cost.unpriced.len(), 1);
    let quiet = super::tokens(&f.store, "sim/quiet").unwrap();
    assert_eq!(quiet.total, symphony_api::AgentTokens::default());
    assert!(quiet.by_role.is_empty() && quiet.by_round.is_empty());
    assert_eq!(quiet.cost.prices, prices.read);
    assert!(super::tokens(&f.store, "sim/none").is_err(), "no such task");
    // A card's figure is its page's, all told; a task that took none, or none at all, has none.
    let asked = ["sim/jump", "sim/quiet", "sim/none"].map(String::from);
    let costs = super::costs(&f.store, &asked).unwrap();
    assert_eq!(costs.prices, prices.read);
    assert_eq!(
        costs.costs,
        [symphony_api::TaskCost {
            slug: "sim/jump".into(),
            dollars: tokens.cost.dollars,
            unpriced: true,
        }]
    );
}
