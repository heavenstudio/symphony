//! A task's gates: every stamp or check it needs to land, each met, missing, waived, held or
//! unknown, with who, when and why. `GET` [`TASK_GATES_PATH`]`?slug=SLUG` answers a [`TaskGates`]
//! for the task's recorded branch at its tip. One gate is the plugin kit's [`Gate`]
//! (`symphony-blocks`), re-exported here.
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

pub use symphony_blocks::{Gate, GatePast, GateState};

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

    /// How many gates there are, how many the task waits on and how many say no one and no time.
    pub fn seen(&self) -> GatesSeen {
        GatesSeen {
            count: self.gates.len(),
            waiting: self.waiting().count(),
            unsigned: self.gates.iter().filter(|gate| !gate.signed()).count(),
        }
    }
}

/// How many gates a task's answer had, how many of them it waited on, and how many had no who or
/// when to draw ([`Gate::signed`]): what a page holds room for before it reads the gates again
/// ([`TaskGates::seen`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatesSeen {
    pub count: usize,
    #[serde(default)]
    pub waiting: usize,
    #[serde(default)]
    pub unsigned: usize,
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
                waiting: 3,
                unsigned: 7
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

    /// The gates `symphony_blocks`' own test finds unsigned and signed (`Gate::signed`), counted in
    /// one answer.
    #[test]
    fn a_tasks_gates_count_those_with_no_who_or_when_to_draw() {
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
        let signed = [
            with(|g| g.by = Some("lucid-heron".into())),
            with(|g| g.at = Some(1_794_398_700.0)),
            with(|g| g.commit = Some("4be91c2aa10".into())),
            with(|g| {
                g.commit = Some("4be91c2".into());
                g.detail = "Green on main a71f0d3".into();
            }),
        ];
        let answer = TaskGates {
            gates: unsigned.iter().chain(&signed).cloned().collect(),
            ..TaskGates::default()
        };
        assert_eq!(
            answer.seen(),
            GatesSeen {
                count: 9,
                waiting: 9,
                unsigned: 5
            }
        );
        // A page of a server one build older says nothing of its unsigned gates.
        let read: GatesSeen = serde_json::from_str(r#"{"count":4,"waiting":1}"#).unwrap();
        assert_eq!(read.unsigned, 0);
    }

    #[test]
    fn a_tasks_gates_with_only_its_slug_and_branch_read_with_no_tip_and_no_gates() {
        let answer: TaskGates =
            serde_json::from_str(r#"{"slug":"tools/x","branch":"tools/x"}"#).unwrap();
        assert_eq!(answer.tip, None);
        assert_eq!(answer.gates, Vec::<Gate>::new());
    }
}
