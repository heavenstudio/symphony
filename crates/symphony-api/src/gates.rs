//! A task's gates: every stamp or check it needs to land, each met, missing, waived, held or
//! unknown, with who, when and why. `GET` [`TASK_GATES_PATH`]`?slug=SLUG` answers a [`TaskGates`]
//! for the task's recorded branch at its tip.
//!
//! Symphony answers three gates of its own, [`REVIEW`], [`CHECK`] and [`OWNER`]; a project's
//! plugin answers its own through the gates seam (`symphony_plugin::Gates`). A plugin's gate with
//! the same name as one of Symphony's takes its place: the more specific level's answer replaces
//! the general one's. A plugin that errs or does not answer in time gives one gate, named after its
//! answerer, in [`GateState::Unknown`], its error as the detail, and the answer still arrives.
//!
//! The gates come in [`GateState::rank`] order (missing, held, unknown, waived, met: what the task
//! waits on first), each state's gates in the order they were answered: Symphony's, each replaced
//! in place by a plugin's of its name, then the plugins' others in the project's order.
//!
//! Every time is server time, in seconds since the epoch, as the board's are. A field added later
//! is `#[serde(default)]`, and a state a client does not know yet reads as
//! [`GateState::Unknown`], so the server and an open client may be one build apart.

use serde::{Deserialize, Serialize};

/// The route that answers a [`TaskGates`]: `?slug=SLUG`.
pub const TASK_GATES_PATH: &str = "/api/task-gates";

/// Symphony's review gate: the board's reviewer verdicts on the task, a round each.
pub const REVIEW: &str = "review";
/// Symphony's check gate: the project's `[verdicts] check` run at the branch's tip, from the run
/// records the board's recorder keeps. There is none for a project with no check.
pub const CHECK: &str = "check";
/// Symphony's owner gate: a card the task posted that waits on the owner. There is none while no
/// card waits.
pub const OWNER: &str = "owner";

/// `GET /api/task-gates?slug=SLUG`: what the task needs to land, at its branch's tip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskGates {
    pub slug: String,
    /// The branch the gates were answered for: the task's recorded branch.
    pub branch: String,
    /// The branch's tip, a full commit id; `None` when the project's repository has no such branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    /// In [`GateState::rank`] order. Empty when nothing gates the task.
    #[serde(default)]
    pub gates: Vec<Gate>,
}

impl TaskGates {
    /// The gates in `state`.
    pub fn count(&self, state: GateState) -> usize {
        self.gates.iter().filter(|gate| gate.state == state).count()
    }

    /// The gates the task waits on: missing or held.
    pub fn waiting(&self) -> impl Iterator<Item = &Gate> {
        self.gates.iter().filter(|gate| gate.state.waits())
    }

    /// How many gates there are and how many the task waits on.
    pub fn seen(&self) -> GatesSeen {
        GatesSeen {
            count: self.gates.len(),
            waiting: self.waiting().count(),
        }
    }
}

/// How many gates a task's answer had and how many of them it waited on: what a page holds room
/// for before it reads the gates again ([`TaskGates::seen`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatesSeen {
    pub count: usize,
    #[serde(default)]
    pub waiting: usize,
}

/// One gate.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// `http(s)` URL. Any other ([`crate::linkable`]) is dropped before a page sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The states it took before this one, oldest first, for the timeline's gates lane; empty
    /// when its answerer keeps none. Each lasted until the next began, the last one until `at`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub past: Vec<GatePast>,
}

/// A state a gate took before its current one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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

/// `gates` in [`GateState::rank`] order, each state's in the order given.
pub fn ranked(mut gates: Vec<Gate>) -> Vec<Gate> {
    gates.sort_by_key(|gate| gate.state.rank());
    gates
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
    fn gates_rank_what_the_task_waits_on_first_and_keep_each_states_order() {
        let gates = ranked(vec![
            gate("nightly", GateState::Met),
            gate("dependency", GateState::Waived),
            gate("review", GateState::Met),
            gate("stamps", GateState::Unknown),
            gate("hold", GateState::Held),
            gate("certificate", GateState::Missing),
            gate("check", GateState::Missing),
        ]);
        let names: Vec<&str> = gates.iter().map(|gate| gate.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "certificate",
                "check",
                "hold",
                "stamps",
                "dependency",
                "nightly",
                "review"
            ]
        );
        let answer = TaskGates {
            gates,
            ..TaskGates::default()
        };
        assert_eq!(answer.count(GateState::Met), 2);
        assert_eq!(answer.count(GateState::Waived), 1);
        assert_eq!(
            answer.seen(),
            GatesSeen {
                count: 7,
                waiting: 3
            }
        );
        assert_eq!(TaskGates::default().seen(), GatesSeen::default());
        assert_eq!(
            answer
                .waiting()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>(),
            ["certificate", "check", "hold"]
        );
        assert_eq!(ranked(Vec::new()), Vec::<Gate>::new());
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
    fn a_gate_with_only_its_name_and_state_reads_with_every_other_field_empty() {
        let read: Gate = serde_json::from_str(r#"{"name":"review","state":"met"}"#).unwrap();
        assert_eq!(read, gate("review", GateState::Met));
        assert_eq!(
            serde_json::to_value(&read).unwrap(),
            serde_json::json!({"name":"review","state":"met","detail":""})
        );
        let answer: TaskGates =
            serde_json::from_str(r#"{"slug":"tools/x","branch":"tools/x"}"#).unwrap();
        assert_eq!(answer.tip, None);
        assert_eq!(answer.gates, Vec::<Gate>::new());
    }
}
