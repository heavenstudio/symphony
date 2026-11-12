//! What a task went through, for its page's timeline: `GET` [`TASK_TIMELINE_PATH`]`?slug=SLUG`
//! answers a [`TaskTimeline`].
//!
//! Its lanes are what the server reads of the task alone, never the whole board: the stages the
//! board's replay of its thread keeps ([`Stage`]), the executors' time in progress ([`Claim`]),
//! each review round ([`Round`]), the builds and tests the board's recorder kept for it ([`Run`])
//! and each gate's states ([`GateLane`], from the gates answer's past states). A client lays them
//! out: a list with a bar per step on a phone, lanes over the same hours wide.
//!
//! Every time is server time in seconds since the epoch, as the board's are; [`TaskTimeline::offset`]
//! turns one into the server's local clock, which the thread's own times are written in. A field
//! added later is `#[serde(default)]`, and a word a client does not know reads as the variant
//! marked `#[serde(other)]`, so the server and an open client may be one build apart.

use serde::{Deserialize, Serialize};

use crate::GateState;

/// The route that answers a [`TaskTimeline`]: `?slug=SLUG`.
pub const TASK_TIMELINE_PATH: &str = "/api/task-timeline";

/// `GET /api/task-timeline?slug=SLUG`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskTimeline {
    pub slug: String,
    /// When the server answered: where whatever still goes on ends.
    pub now: f64,
    /// Seconds to add to a time for the server's local clock at `now`.
    #[serde(default)]
    pub offset: i64,
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
}

impl TaskTimeline {
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

/// One stretch of the task in one status.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "is_false")]
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
pub struct Claim {
    pub by: String,
    pub from: f64,
    /// When it let go, or the task left in progress; none while it works on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
}

/// One review round: from the task going to review to its verdict.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Round {
    /// From 1.
    pub round: u32,
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

/// One gate's states over time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GateLane {
    pub name: String,
    /// Oldest first, each lasting until the next began, the last one until the timeline's end.
    #[serde(default)]
    pub states: Vec<GateSpan>,
}

/// A state a gate took, and when.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn is_false(b: &bool) -> bool {
    !*b
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
        assert_eq!(read.offset, 0);
        assert!(read.stages.is_empty() && read.runs.is_empty() && read.gates.is_empty());
        let stage = Stage {
            status: "review".into(),
            from: 2.0,
            ..Stage::default()
        };
        assert_eq!(stage.seconds(5.0), 3.0);
    }
}
