//! One gate of a task: a stamp or check it needs to land, met, missing, waived, held or unknown,
//! with who, when and why. A project's plugin answers its own through the gates seam
//! (`symphony_plugin::Gates`); a task's whole answer, Symphony's gates and the plugins', is the app's
//! `symphony_api::gates::TaskGates`.
//!
//! Every time is server time, in seconds since the epoch, as the board's are. A field added later
//! is `#[serde(default)]`, and a state a client does not know yet reads as
//! [`GateState::Unknown`], so the server and an open client may be one build apart.

use serde::{Deserialize, Serialize};

/// One gate.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Gate {
    /// Its name, unique among a task's gates (`review`, `certificate`).
    pub name: String,
    pub state: GateState,
    /// Who met, waived or holds it, or whose run or verdict it reads: a handle, `owner`, or a
    /// pipeline (`nightly`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// When it took this state; for a run still going, when the run started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
    /// The commit it holds for, a full or short id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Why, for a waiver or a hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// One line saying what it is in this state.
    #[serde(default)]
    pub detail: String,
    /// Where its evidence is: an app path (`/task/SLUG#message-ID`, `/docs/X.md#card`) or an
    /// `http(s)` URL. Any other (`symphony_api::linkable`) is dropped before a page sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The states it took before this one, oldest first, for the timeline's gates lane; empty
    /// when its answerer keeps none. Each lasted until the next began, the last one until `at`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub past: Vec<GatePast>,
}

impl Gate {
    /// The gate's commit, its first seven characters, when its words (its detail and why) do not
    /// already say it: what its who and when add.
    pub fn unsaid_commit(&self) -> Option<String> {
        let commit = self.commit.as_deref().filter(|commit| !commit.is_empty())?;
        let short: String = commit.chars().take(7).collect();
        let said = |text: &str| text.contains(&short);
        (!said(&self.detail) && !self.why.as_deref().is_some_and(said)).then_some(short)
    }

    /// Whether a page has a who and when to draw for the gate: who, when, or a commit its words
    /// do not say. The Symphony's review gate before any verdict has none.
    pub fn signed(&self) -> bool {
        self.by.as_deref().is_some_and(|by| !by.is_empty())
            || self.at.is_some()
            || self.unsaid_commit().is_some()
    }
}

/// A state a gate took before its current one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct GatePast {
    pub state: GateState,
    /// When it took that state.
    pub at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub detail: String,
}

/// Where a gate stands. A task's gates come in [`GateState::rank`] order, not this one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[non_exhaustive]
pub enum GateState {
    /// Not met, and something can meet it: a run, a verdict, a stamp.
    Missing,
    /// Waiting on the owner, or on a hold.
    Held,
    /// Not needed: someone waived it, saying why.
    Waived,
    Met,
    /// Its answerer could not say; a state this client does not know reads as this too (serde
    /// reads an unknown word as the last variant only).
    #[default]
    #[serde(other)]
    Unknown,
}

impl GateState {
    /// Its place in a task's gates: what the task waits on first, then what nobody knows, then
    /// what no longer stands in its way.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Missing => 0,
            Self::Held => 1,
            Self::Unknown => 2,
            Self::Waived => 3,
            Self::Met => 4,
        }
    }

    /// Whether the task waits on a gate in this state.
    pub const fn waits(self) -> bool {
        matches!(self, Self::Missing | Self::Held)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Held => "held",
            Self::Unknown => "unknown",
            Self::Waived => "waived",
            Self::Met => "met",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(name: &str, state: GateState) -> Gate {
        Gate {
            name: name.into(),
            state,
            ..Gate::default()
        }
    }

    #[test]
    fn a_state_word_written_by_a_newer_server_reads_as_unknown() {
        for (word, state) in [
            ("missing", GateState::Missing),
            ("held", GateState::Held),
            ("unknown", GateState::Unknown),
            ("waived", GateState::Waived),
            ("met", GateState::Met),
            ("expired", GateState::Unknown),
        ] {
            let read: GateState = serde_json::from_str(&format!("\"{word}\"")).unwrap();
            assert_eq!(read, state, "{word}");
            if state != GateState::Unknown || word == "unknown" {
                assert_eq!(read.as_str(), word);
                assert_eq!(serde_json::to_string(&read).unwrap(), format!("\"{word}\""));
            }
        }
        assert!(GateState::Missing.waits() && GateState::Held.waits());
        assert!(!GateState::Unknown.waits() && !GateState::Waived.waits());
        assert!(!GateState::Met.waits());
    }

    #[test]
    fn a_gate_is_signed_by_who_when_or_a_commit_its_words_do_not_say() {
        let with = |edit: fn(&mut Gate)| {
            let mut gate = gate("review", GateState::Missing);
            edit(&mut gate);
            gate
        };
        let unsigned = [
            with(|_| ()),
            with(|g| g.by = Some(String::new())),
            with(|g| g.commit = Some(String::new())),
            with(|g| {
                g.commit = Some("4be91c2aa10".into());
                g.detail = "Verify on 4be91c2 running".into();
            }),
            with(|g| {
                g.commit = Some("4be91c2".into());
                g.why = Some("4be91c2 is the tree it read".into());
            }),
        ];
        for gate in &unsigned {
            assert!(!gate.signed(), "{gate:?}");
        }
        let signed = [
            with(|g| g.by = Some("lucid-heron".into())),
            with(|g| g.at = Some(1_794_398_700.0)),
            with(|g| g.commit = Some("4be91c2aa10".into())),
            with(|g| {
                g.commit = Some("4be91c2".into());
                g.detail = "Green on main a71f0d3".into();
            }),
        ];
        for gate in &signed {
            assert!(gate.signed(), "{gate:?}");
        }
        assert_eq!(signed[2].unsaid_commit().as_deref(), Some("4be91c2"));
        assert_eq!(unsigned[3].unsaid_commit(), None);
        // A commit shorter than seven characters is said as it is.
        assert_eq!(
            with(|g| g.commit = Some("4be9".into()))
                .unsaid_commit()
                .as_deref(),
            Some("4be9")
        );
    }

    #[test]
    fn a_gate_with_only_its_name_and_state_reads_with_every_other_field_empty() {
        let read: Gate = serde_json::from_str(r#"{"name":"review","state":"met"}"#).unwrap();
        assert_eq!(read, gate("review", GateState::Met));
        assert_eq!(
            serde_json::to_value(&read).unwrap(),
            serde_json::json!({"name":"review","state":"met","detail":""})
        );
    }
}
