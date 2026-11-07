//! The machine probes of the agents view. The panes probe went out as `herdr` before it was
//! `panes`, and an open tab keeps its client across a server's redeploy while a draft is engaged:
//! the probe goes out under both names and is read from either, `panes` first, so neither a
//! client nor a server from before the rename fails on the other.

use serde::{Deserialize, Serialize};

/// Whether each machine probe could be asked: `ok` or `unavailable`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ProbesWire", into = "ProbesWire")]
pub struct AgentProbes {
    /// The terminals agents run in (hc's, shown in the Herdr app).
    pub panes: String,
    /// The test-slot locks.
    pub slots: String,
}

/// [`AgentProbes`] as the wire carries it: `herdr` is the panes probe's earlier name. A field
/// serde reads by either name would refuse a payload holding both as a duplicate.
#[derive(Serialize, Deserialize)]
struct ProbesWire {
    panes: Option<String>,
    herdr: Option<String>,
    slots: String,
}

impl TryFrom<ProbesWire> for AgentProbes {
    type Error = &'static str;

    fn try_from(wire: ProbesWire) -> Result<Self, Self::Error> {
        let panes = wire.panes.or(wire.herdr).ok_or("missing field `panes`")?;
        let slots = wire.slots;
        Ok(AgentProbes { panes, slots })
    }
}

impl From<AgentProbes> for ProbesWire {
    fn from(probes: AgentProbes) -> Self {
        ProbesWire {
            herdr: Some(probes.panes.clone()),
            panes: Some(probes.panes),
            slots: probes.slots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AgentProbes;
    use serde_json::{from_value, json, to_value};

    fn probes(panes: &str) -> AgentProbes {
        AgentProbes {
            panes: panes.into(),
            slots: "ok".into(),
        }
    }

    #[test]
    fn the_panes_probe_goes_out_under_both_its_names() {
        assert_eq!(
            to_value(probes("unavailable")).unwrap(),
            json!({"panes": "unavailable", "herdr": "unavailable", "slots": "ok"})
        );
    }

    #[test]
    fn the_panes_probe_reads_from_either_name_panes_first() {
        for (wire, panes) in [
            (
                json!({"herdr": "unavailable", "slots": "ok"}),
                "unavailable",
            ),
            (
                json!({"panes": "unavailable", "slots": "ok"}),
                "unavailable",
            ),
            (
                json!({"panes": "ok", "herdr": "unavailable", "slots": "ok"}),
                "ok",
            ),
        ] {
            assert_eq!(from_value::<AgentProbes>(wire).unwrap(), probes(panes));
        }
    }

    #[test]
    fn probes_naming_no_panes_probe_are_refused_as_before() {
        let refused = from_value::<AgentProbes>(json!({"slots": "ok"})).unwrap_err();
        assert_eq!(refused.to_string(), "missing field `panes`");
        let refused = from_value::<AgentProbes>(json!({"panes": "ok"})).unwrap_err();
        assert_eq!(refused.to_string(), "missing field `slots`");
    }
}
