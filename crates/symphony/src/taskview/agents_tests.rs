//! The board's agents view on the Symphony's wire.

use super::wire_agents;
use serde_json::json;
use symphony_board::cli::agents::View;

#[test]
fn the_wire_names_the_boards_panes_probe_panes_with_its_answer() {
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
            json!({"generated": 1.5, "probes": {"panes": said, "slots": "ok"}, "agents": []})
        );
    }
}
