//! What a task went through, for its page's timeline: `GET` [`TASK_TIMELINE_PATH`]`?slug=SLUG`
//! answers a [`TaskTimeline`].
//!
//! Its lanes are what the server reads for this task, never the whole board: the stages the
//! board's replay of its thread keeps ([`Stage`]), the executors' time in progress ([`Claim`]),
//! each review round ([`Round`]), the builds and tests the board's recorder kept for it ([`Run`]),
//! each gate's states ([`GateLane`], from the gates answer's past states) and, for a task a custom
//! workflow held, each place it put the task at ([`PlaceSpan`], from its recorded transitions). A client lays them
//! out: a list with a bar per step on a phone, lanes over the same hours wide. Optional [`History`]
//! adds bounded role and address metadata, including recorded ancestor orchestration.
//!
//! Every time is server time in seconds since the epoch, as the board's are;
//! [`TaskTimeline::offset_at`] turns one into the server's local clock, which the thread's own
//! times are written in. A field added later is `#[serde(default)]`, and a word a client does not
//! know reads as the variant marked `#[serde(other)]`, so the server and an open client may be one
//! build apart.

use serde::{Deserialize, Serialize};

use crate::GateState;

mod history;
pub use history::*;

/// The route that answers a [`TaskTimeline`]: `?slug=SLUG`.
pub const TASK_TIMELINE_PATH: &str = "/api/task-timeline";

/// How often a client reads an open task's timeline again, in seconds, when the answer does not
/// say ([`TaskTimeline::every`]).
pub const TIMELINE_EVERY: u32 = 60;

/// `GET /api/task-timeline?slug=SLUG`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskTimeline {
    pub slug: String,
    /// When the server answered: where whatever still goes on ends. The answer's validator leaves
    /// it out, so a read that finds nothing else changed answers `304`: a client that keeps the
    /// answer counts on from this by its own clock.
    pub now: f64,
    /// The server's local clock over the timeline's hours: its offset from UTC at `created`, then
    /// each change of it (summer time), oldest first.
    #[serde(default)]
    pub zone: Vec<Offset>,
    /// How often, in seconds, a client reads the timeline again while the task is open;
    /// [`TIMELINE_EVERY`] when the answer does not say.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub every: u32,
    /// The oldest run the board's recorder still keeps, where the task began before it: the task's
    /// runs before then may have fallen out of the recorder's history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_from: Option<f64>,
    /// When the task was made.
    pub created: f64,
    /// When it closed, done or cancelled; none while it is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<f64>,
    /// When an executor first claimed it; the time before it, in ready, is time unclaimed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_claimed: Option<f64>,
    /// How often review sent it back to in progress.
    #[serde(default)]
    pub bounces: u32,
    /// Each stretch in one status, oldest first, one after another from `created`.
    #[serde(default)]
    pub stages: Vec<Stage>,
    /// Who held the executor role while it was in progress, oldest first.
    #[serde(default)]
    pub claims: Vec<Claim>,
    /// Each review round, oldest first.
    #[serde(default)]
    pub rounds: Vec<Round>,
    /// Its builds and tests, by when they were queued. Empty until the board's recorder keeps one
    /// for the task: then a client shows no builds or tests at all, never a zero.
    #[serde(default)]
    pub runs: Vec<Run>,
    /// Each gate's states, by the gate's name; empty when the task has none, or its gates could
    /// not be read.
    #[serde(default)]
    pub gates: Vec<GateLane>,
    /// Each place a custom workflow put the task at, oldest first, from its recorded transitions:
    /// each lasting until the next began, the last until the timeline's end. Time at a place is
    /// the clock's, never the machine's time on runs ([`Self::machine_seconds`]). Empty, and left
    /// out, for a task no workflow held, or whose transitions could not be read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<PlaceSpan>,
    /// Recorded roles and message addresses. Absent on servers without this projection; never
    /// interpret absence, a partial source or a failed source as an empty history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<History>,
}

impl TaskTimeline {
    /// Seconds to add to `at` for the server's local clock then: the offset of the last change of
    /// [`zone`](Self::zone) at or before it, the first one before them all; none without a zone.
    pub fn offset_at(&self, at: f64) -> i64 {
        self.zone
            .iter()
            .take_while(|change| change.from <= at)
            .last()
            .or(self.zone.first())
            .map_or(0, |change| change.seconds)
    }

    /// How often to read it again while the task is open, in seconds.
    pub fn every_seconds(&self) -> u32 {
        if self.every == 0 {
            TIMELINE_EVERY
        } else {
            self.every
        }
    }

    /// Where the timeline ends: its close, or now, or later where a run went on past it.
    pub fn end(&self) -> f64 {
        let close = self.closed.unwrap_or(self.now);
        self.runs
            .iter()
            .map(|run| run.ended.unwrap_or(self.now))
            .fold(close, f64::max)
            .max(self.created)
    }

    /// The machine's time on the task's runs: each one's run time, a running one up to now.
    pub fn machine_seconds(&self) -> f64 {
        self.runs.iter().map(|run| run.ran(self.now)).sum()
    }

    /// The runs' time waiting to start: queued up to their start, or up to now.
    pub fn queued_seconds(&self) -> f64 {
        self.runs.iter().map(|run| run.waited(self.now)).sum()
    }
}

/// The server's local clock from `from` on: `seconds` ahead of UTC.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Offset {
    pub from: f64,
    pub seconds: i64,
}

/// One stretch of the task in one status.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Stage {
    /// A board status: `ready`, `in_progress`, `review`, `blocked`, `done`…
    pub status: String,
    pub from: f64,
    /// When it left it; none for the status it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
    /// Who moved it there, where its line says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// For a stretch in review, its round, from 1; 0 otherwise.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub round: u32,
    /// A stretch in review after its round was approved: the task waits to land.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub approved: bool,
}

impl Stage {
    /// How long it held the status, an open one up to `now`.
    pub fn seconds(&self, now: f64) -> f64 {
        (self.to.unwrap_or(now) - self.from).max(0.0)
    }
}

/// An executor's time on the task while it was in progress.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Claim {
    pub by: String,
    pub from: f64,
    /// When it let go, or the task left in progress; none while it works on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
}

/// One review round: from the task going to review to its verdict.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Round {
    /// Its number, from 1 (`round` on the wire).
    #[serde(rename = "round")]
    pub number: u32,
    /// When the task went to review.
    pub from: f64,
    /// When a reviewer took it up; the time before is the wait for one. Its start when a reviewer
    /// already held the role; none while nobody has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken: Option<f64>,
    /// Its verdict's time, or when the task left review without one; none while it goes on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
    /// The reviewer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// None while it goes on, or when it ended without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
}

/// A review round's verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(rename = "RoundVerdict"))]
pub enum Verdict {
    Approved,
    Changes,
    /// A verdict word this client does not know.
    #[serde(other)]
    Other,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Changes => "changes",
            Self::Other => "verdict",
        }
    }
}

/// A build or test the board's recorder kept for the task.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Run {
    /// `build` or `test`.
    pub phase: String,
    /// The recipe or command line (`just test-rs -p symphony-board`).
    pub label: String,
    /// Who ran it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The commit it ran at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The machine it ran on.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host: String,
    pub queued: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<f64>,
    pub result: RunResult,
}

impl Run {
    /// How long it waited to start, up to `now` while it still waits.
    pub fn waited(&self, now: f64) -> f64 {
        (self.started.or(self.ended).unwrap_or(now) - self.queued).max(0.0)
    }

    /// How long it ran, up to `now` while it runs.
    pub fn ran(&self, now: f64) -> f64 {
        self.started.map_or(0.0, |started| {
            (self.ended.unwrap_or(now) - started).max(0.0)
        })
    }
}

/// Where a run is, or how it ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum RunResult {
    /// Recorded, its command not started yet.
    #[default]
    Queued,
    Running,
    /// Its command exited 0.
    Green,
    /// Its command exited otherwise, or a signal ended it.
    Red,
    /// Its recorder ended before it could say, or a word this client does not know.
    #[serde(other)]
    Unknown,
}

impl RunResult {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Green => "green",
            Self::Red => "red",
            Self::Unknown => "ended",
        }
    }

    /// Whether it still goes on.
    pub const fn open(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

/// A place a custom workflow put the task at, and when (docs/workflow/WORKFLOW.md, "The app").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct PlaceSpan {
    /// Its phase's ID, then its label in the definition the move was made under: the ID where
    /// that definition is not recorded or does not name it.
    pub phase: String,
    pub phase_label: String,
    /// Its coordination state's ID and label, named as its phase is.
    pub state: String,
    pub state_label: String,
    /// The board status the place projects to, which a page colours it by, as it colours a
    /// status.
    pub status: String,
    /// When the task was put there.
    pub from: f64,
    /// The edge that moved it there: none for a move an activation or a filing made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    /// Who moved it: an agent session's handle, or the owner.
    pub by: String,
}

/// One gate's states over time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct GateLane {
    pub name: String,
    /// Oldest first, each lasting until the next began, the last one until the timeline's end.
    #[serde(default)]
    pub states: Vec<GateSpan>,
}

/// A state a gate took, and when.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct GateSpan {
    pub state: GateState,
    /// When it took it; a state taken before the task was made, or at a time its answerer does not
    /// keep, from the task's creation.
    pub from: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default)]
    pub detail: String,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(queued: f64, started: Option<f64>, ended: Option<f64>, result: RunResult) -> Run {
        Run {
            phase: "test".into(),
            label: "just test-rs".into(),
            queued,
            started,
            ended,
            result,
            ..Run::default()
        }
    }

    #[test]
    fn a_runs_wait_and_run_time_count_up_to_now_while_it_goes_on() {
        let now = 100.0;
        let finished = run(10.0, Some(13.0), Some(40.0), RunResult::Green);
        assert_eq!((finished.waited(now), finished.ran(now)), (3.0, 27.0));
        let running = run(50.0, Some(60.0), None, RunResult::Running);
        assert_eq!((running.waited(now), running.ran(now)), (10.0, 40.0));
        let queued = run(90.0, None, None, RunResult::Queued);
        assert_eq!((queued.waited(now), queued.ran(now)), (10.0, 0.0));
        // A recorder lost before its command started: it waited until it ended, and never ran.
        let lost = run(20.0, None, Some(25.0), RunResult::Unknown);
        assert_eq!((lost.waited(now), lost.ran(now)), (5.0, 0.0));
        let timeline = TaskTimeline {
            now,
            created: 0.0,
            runs: vec![finished, running, queued, lost],
            ..TaskTimeline::default()
        };
        assert_eq!(timeline.machine_seconds(), 67.0);
        assert_eq!(timeline.queued_seconds(), 28.0);
    }

    #[test]
    fn a_time_takes_the_offset_of_the_last_change_of_the_zone_before_it() {
        let timeline = TaskTimeline {
            zone: vec![
                Offset {
                    from: 100.0,
                    seconds: 7200,
                },
                Offset {
                    from: 500.0,
                    seconds: 3600,
                },
            ],
            every: 5,
            ..TaskTimeline::default()
        };
        // Before the first change (a time before the task), at it, between and after.
        let offsets = [50.0, 100.0, 499.0, 500.0, 9e9].map(|at| timeline.offset_at(at));
        assert_eq!(offsets, [7200, 7200, 7200, 3600, 3600]);
        assert_eq!(timeline.every_seconds(), 5);
    }

    #[test]
    fn a_timeline_ends_at_its_close_or_now_or_a_run_that_went_on_past_them() {
        let mut timeline = TaskTimeline {
            now: 100.0,
            created: 10.0,
            ..TaskTimeline::default()
        };
        assert_eq!(timeline.end(), 100.0);
        timeline.closed = Some(60.0);
        assert_eq!(timeline.end(), 60.0);
        timeline.runs = vec![run(50.0, Some(55.0), Some(70.0), RunResult::Green)];
        assert_eq!(timeline.end(), 70.0);
        timeline
            .runs
            .push(run(80.0, Some(81.0), None, RunResult::Running));
        assert_eq!(timeline.end(), 100.0);
    }

    #[test]
    fn a_timeline_no_workflow_held_writes_no_places_and_a_held_ones_read_back_whole() {
        let plain = TaskTimeline {
            slug: "tools/x".into(),
            ..TaskTimeline::default()
        };
        let written = serde_json::to_value(&plain).unwrap();
        assert!(written.get("places").is_none(), "{written}");
        let place = PlaceSpan {
            phase: "review".into(),
            phase_label: "Review".into(),
            state: "awaiting".into(),
            state_label: "Awaiting review".into(),
            status: "review".into(),
            from: 2.0,
            edge: None,
            by: "owner".into(),
        };
        assert_eq!(
            serde_json::to_value(&place).unwrap(),
            serde_json::json!({
                "phase": "review", "phase_label": "Review", "state": "awaiting",
                "state_label": "Awaiting review", "status": "review", "from": 2.0, "by": "owner"
            })
        );
        let held = TaskTimeline {
            places: vec![
                place.clone(),
                PlaceSpan {
                    edge: Some("approve".into()),
                    ..place
                },
            ],
            ..plain
        };
        let read: TaskTimeline =
            serde_json::from_str(&serde_json::to_string(&held).unwrap()).unwrap();
        assert_eq!(read, held);
    }

    #[test]
    fn words_a_newer_server_writes_read_as_unknown_and_empty_fields_stay_out() {
        let verdict: Verdict = serde_json::from_str("\"withdrawn\"").unwrap();
        assert_eq!(verdict, Verdict::Other);
        let result: RunResult = serde_json::from_str("\"cancelled\"").unwrap();
        assert_eq!(result, RunResult::Unknown);
        for (result, word) in [
            (RunResult::Queued, "queued"),
            (RunResult::Running, "running"),
            (RunResult::Green, "green"),
            (RunResult::Red, "red"),
        ] {
            assert_eq!(
                serde_json::to_string(&result).unwrap(),
                format!("\"{word}\"")
            );
            assert_eq!(result.as_str(), word);
        }
        assert!(RunResult::Running.open() && RunResult::Queued.open());
        assert!(!RunResult::Green.open() && !RunResult::Unknown.open());
        assert_eq!(
            serde_json::to_value(Stage {
                status: "ready".into(),
                from: 1.0,
                ..Stage::default()
            })
            .unwrap(),
            serde_json::json!({"status": "ready", "from": 1.0})
        );
        let read: TaskTimeline =
            serde_json::from_str(r#"{"slug":"tools/x","now":5.0,"created":1.0}"#).unwrap();
        assert_eq!(
            (read.offset_at(3.0), read.every_seconds()),
            (0, TIMELINE_EVERY)
        );
        assert!(read.stages.is_empty() && read.runs.is_empty() && read.gates.is_empty());
        let stage = Stage {
            status: "review".into(),
            from: 2.0,
            ..Stage::default()
        };
        assert_eq!(stage.seconds(5.0), 3.0);
    }
}
