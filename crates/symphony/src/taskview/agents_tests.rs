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

#[test]
fn the_wire_names_the_boards_panes_probe_panes_and_herdr_with_its_answer() {
    for (asked, said) in [(true, "ok"), (false, "unavailable")] {
        let view = View {
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

/// The agents view asks hc for its terminals with this process's environment as the board reads
/// it. Started from an agent's hc terminal, that environment names the agent's session and the
/// terminal and core it runs in; hc must see none of them, or it would answer for that core alone.
#[test]
fn the_agents_views_hc_probe_runs_without_the_starters_session() {
    const ROOT: &str = "SF_SYMPHONY_AGENTS_PROBE_ROOT";
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
         >> \"$SF_SYMPHONY_AGENTS_PROBE_ROOT/held\"\nprintf '[]\\n'\n",
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
