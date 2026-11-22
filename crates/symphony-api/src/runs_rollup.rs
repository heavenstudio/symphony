//! A project's runs rolled up by UTC day ([`RunsDay`]), from the day files of its run lines
//! ([`crate::RunLine`], docs/workflow/RUNS.md). The machine's one server writes them once an hour, and once
//! after midnight for the day that ended, as it writes the board's ([`crate::reports`]). They go in
//! [`RUNS_ROLLUP`] beside the run lines, `symphony/runs/rollup/YYYY-MM-DD.json` in the git common
//! dir ([`crate::report_file`] names each), and are kept for good. The Runs page reads them for its
//! trends.
//!
//! A day's figures are taken from the runs that ended in it, each run once ([`crate::fold`]): its
//! pipelines and its cost from the runs, never their steps, as a run's CPU already holds its
//! steps'. Its suites come from each line that carries them, except a run whose steps carry them
//! too, so a suite is counted once. Its flaky tests look back over [`FLAKY_DAYS`] days. A field
//! added later is `#[serde(default)]`.

use crate::reports::Spread;
use serde::{Deserialize, Serialize};

/// [`RunsDay::schema`] as written now.
pub const RUNS_DAY_SCHEMA: u32 = 1;

/// The folder under the runs' day files that holds the roll-ups.
pub const RUNS_ROLLUP: &str = "rollup";

/// How many days a day's flaky tests look back over: that day and the 13 before it.
pub const FLAKY_DAYS: u32 = 14;

/// How many of the costliest tasks and of the slowest suites a day names.
pub const RUNS_TOP: usize = 10;

/// One UTC day of a project's runs, as the roll-up last read it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunsDay {
    /// [`RUNS_DAY_SCHEMA`].
    pub schema: u32,
    /// The project's id.
    pub project: String,
    /// The UTC day, `YYYY-MM-DD`.
    pub day: String,
    /// Its first second: UTC midnight.
    pub start: f64,
    /// When the roll-up read the day files, by the server's clock.
    pub read: f64,
    /// What the figures run to: `read` for a day under way, the day's end for one that had ended.
    pub until: f64,
    /// Whether the day had ended when it was read: its file is final, never written again.
    pub closed: bool,
    /// Each pipeline a run of which ended in the day, by name.
    #[serde(default)]
    pub pipelines: Vec<PipelineDay>,
    /// Each test binary a run that ended in the day reported, by name.
    #[serde(default)]
    pub suites: Vec<SuiteDay>,
    /// The [`RUNS_TOP`] suites of [`RunsDay::suites`] with the longest median time, the slowest
    /// first. Each one's trend is its line in the other days' files.
    #[serde(default)]
    pub slowest: Vec<String>,
    /// The tests that failed and passed on the same tree in the [`FLAKY_DAYS`] days up to
    /// `until`: the most flaky first.
    #[serde(default)]
    pub flaky: Vec<FlakyTest>,
    /// What the project's runs that ended in the day cost.
    #[serde(default)]
    pub cost: Cost,
    /// The [`RUNS_TOP`] tasks whose runs took the most CPU in the day, the costliest first.
    #[serde(default)]
    pub tasks: Vec<TaskCost>,
}

/// One pipeline's runs that ended in a day.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineDay {
    pub pipeline: String,
    /// Every run that ended, however it ended.
    pub runs: u32,
    pub green: u32,
    pub red: u32,
    pub timed_out: u32,
    pub interrupted: u32,
    pub incomplete: u32,
    /// Green over green, red and timed out; none when there was none of the three. An interrupted
    /// or incomplete run says nothing of what it ran.
    pub pass_rate: Option<f64>,
    /// From start to end, of the runs that started; none when none did.
    pub duration: Option<Spread>,
}

/// One test binary's runs that ended in a day.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SuiteDay {
    pub name: String,
    /// The runs that reported it.
    pub runs: u32,
    /// Those in which a test of it failed, or it ended with no result line ([`crate::Suite::error`]).
    pub failed: u32,
    /// The runs in which none failed, over all.
    pub pass_rate: f64,
    /// Its time in each run.
    pub time: Spread,
}

/// A test that failed and passed on the same tree.
///
/// A pass is not listed in a run's line, only a failure ([`crate::FailedTest`]), so a test passed in
/// a run that ended green, or that ended red with the test's binary reported and the test not among
/// its failures. Two runs are of the same tree when they share their pipeline, step, label and tree,
/// both clean: what a rerun repeats.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlakyTest {
    /// Its test binary, when its failures named one.
    pub suite: Option<String>,
    pub name: String,
    /// The runs it failed in, on a tree it passed on too.
    pub flaky: u32,
    /// The runs it failed in, on a tree it never passed on.
    pub red: u32,
}

/// What runs cost: the CPU and the time they took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub runs: u32,
    /// The CPU seconds the runs took: user and system, their steps' among them.
    pub cpu_s: f64,
    /// The seconds they ran, from start to end.
    pub wall_s: f64,
    /// Whether a run's CPU is known to be short, or not known: `cpu_s` is then at least that.
    pub cpu_partial: bool,
}

/// One task's runs that ended in a day.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskCost {
    pub task: String,
    pub cost: Cost,
}
