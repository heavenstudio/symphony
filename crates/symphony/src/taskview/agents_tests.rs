//! The board's agents view on the Symphony's wire.

use super::wire_agents;
use serde::{Deserialize, Serialize};
use serde_json::json;
use symphony_board::cli::agents::View;

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
