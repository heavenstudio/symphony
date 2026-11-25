//! Native project reads retain their wire shape without machine-wide observations or probes.

/// Empty/unknown evidence is the existing wire representation for unavailable machine work.
/// Never infer even a task's display stage from machine operations in another checkout.
pub(crate) fn project_agents(view: &mut symphony_api::Agents) {
    view.machine_work = symphony_api::MachineWork::default();
    view.probes = symphony_api::AgentProbes {
        panes: "unavailable".into(),
        slots: "unavailable".into(),
    };
}

/// The first hosted profile has no native pane/screen controls or machine inventory.
pub(crate) fn project_page(mut page: symphony_api::TaskPage) -> symphony_api::TaskPage {
    page.machine_work = symphony_api::MachineWork::default();
    page.sessions.clear();
    page.local = false;
    page
}

/// Keep ordinary project deltas and every native version, without diagnostics. A diagnostics-only
/// event keeps its empty changes array so section reads at that cursor can still catch up.
/// Malformed envelopes are withheld, never passed through as an authentication fallback.
pub(crate) fn project_feed(json: &str) -> Option<String> {
    let mut event: serde_json::Value = serde_json::from_slice(json.as_bytes()).ok()?;
    let changes = event.get_mut("changes")?.as_array_mut()?;
    changes.retain(|change| {
        !matches!(
            change.get("region").and_then(serde_json::Value::as_str),
            Some("machine-work" | "agent-probes")
        )
    });
    serde_json::to_string(&event).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn feed_projection_keeps_ordinary_values_and_versions_without_machine_payloads() {
        let ordinary = json!({"region":"task", "op":"upsert", "key":"tools/visible",
            "data":{"title":"Ordinary Ω", "number":3.5, "nested":[null,true]}});
        let source = json!({"version":17,"changes":[
            {"region":"machine-work","data":crate::hosted::tests::diagnostic_work()},
            ordinary,
            {"region":"agent-probes","data":{"panes":"private-reader"}}
        ]});
        let projected = project_feed(&source.to_string()).unwrap();
        let projected: serde_json::Value = serde_json::from_str(&projected).unwrap();
        assert_eq!(projected, json!({"version":17,"changes":[ordinary]}));
    }

    #[test]
    fn empty_and_diagnostics_only_envelopes_keep_their_cursor_without_payloads() {
        for source in [
            json!({"version":1,"changes":[]}),
            json!({"version":2,"changes":[{"region":"machine-work","data":{"reason":"secret"}}]}),
            json!({"version":3,"changes":[{"region":"agent-probes","data":"secret"}]}),
        ] {
            let projected = project_feed(&source.to_string()).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&projected).unwrap(),
                json!({"version":source["version"],"changes":[]})
            );
        }
    }

    #[test]
    fn malformed_envelopes_are_withheld() {
        for json in ["invalid", "{}", r#"{"changes":null}"#, r#"{"changes":{}}"#] {
            assert_eq!(project_feed(json), None, "{json}");
        }
    }
}
