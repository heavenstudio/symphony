//! The board's health an hour at a time, as the machine's one Symphony server keeps it: one
//! [`HealthHour`] a line in the day files `symphony/health/YYYY-MM-DD.jsonl` in the project's git
//! common dir, appended as each hour ends from what the server held in memory over it, with no
//! board request (docs/workflow/TASKS.md, *Board health*). The Reports page reads them.
//!
//! A line covers what its writer counted from [`HealthHour::from`] to [`HealthHour::to`], whole
//! minutes of the hour: all of it, or the part it recorded ([`HealthHour::partial`]). An hour with
//! no line is one no server recorded (it was down, or the machine slept), one whose minutes the
//! server let go before it wrote them (uncalled for two hours of requests), or one of a project
//! with no board: never a line of zeros. An hour can have more than one, from two writers (a server
//! stopped and another started, or a reload handed over in it) or one that slept in it; their spans
//! never overlap, so their counts add up, the slowest request is the slowest of theirs, and each
//! line's write percentiles are its own. Lines join the agents' hour lines (`symphony/agents/`,
//! `docs/workflow/AGENT_ACTIVITY.md`) and the hourly roll-up's hours on [`HealthHour::start`], the hour's
//! first second in all three. A field added later is `#[serde(default)]`, so a reader built before it
//! reads a newer line and one after it an older one.

use serde::{Deserialize, Serialize};

/// [`HealthHour::schema`] as written now.
pub const HEALTH_SCHEMA: u32 = 1;

/// One hour of the board's health, or the part of it one writer recorded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HealthHour {
    /// [`HEALTH_SCHEMA`].
    pub schema: u32,
    /// When it was written, by the server's clock, in seconds since the epoch.
    pub at: f64,
    /// The server process that wrote it.
    pub pid: u32,
    /// When that server began recording: two servers of one reload's hand-off have different
    /// pids, and a pid used again another day has another generation.
    pub generation: f64,
    /// The hour's first second, a whole UTC hour in seconds since the epoch: the `start` of the
    /// agents' hour line and of the roll-up's hour, on which the files join.
    pub start: f64,
    /// Where what it counts begins: the hour's start, or the first whole minute after its writer
    /// began recording in it.
    pub from: f64,
    /// Where what it counts ends: the hour's end, or where its writer stopped recording in it.
    pub to: f64,
    /// Whether it covers less than the whole hour.
    pub partial: bool,
    /// The server clock's callback dispatch passes.
    #[serde(default)]
    pub dispatch: HourPasses,
    /// The idle sweeps of this board's project: the machine's one server makes them for each
    /// project it serves that has a board, each recorded in that project's own report.
    #[serde(default)]
    pub sweep: HourPasses,
    /// Every request any client in the server's process sent the board.
    #[serde(default)]
    pub requests: HourRequests,
    /// Of them, those that may write.
    #[serde(default)]
    pub writes: HourWrites,
}

impl HealthHour {
    /// The UTC day whose file holds it, its hour's: `YYYY-MM-DD` ([`crate::run_day`]).
    pub fn day(&self) -> String {
        crate::run_day(self.start)
    }
}

/// One kind of pass in a line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HourPasses {
    /// Passes that were ok.
    pub ok: u64,
    /// Passes that failed.
    pub failed: u64,
}

/// The requests of a line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HourRequests {
    /// Requests sent.
    pub count: u64,
    /// Of them, those that took a second or longer.
    pub slow: u64,
    /// Of them, those that got no answer.
    pub unanswered: u64,
    /// The slowest; none when none was sent.
    #[serde(default)]
    pub slowest: Option<HourSlowest>,
}

/// The slowest request of a line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HourSlowest {
    /// How long it took, in milliseconds.
    pub ms: f64,
    /// The file and line that asked for it: its caller.
    pub source: String,
    /// When it ended, in seconds since the epoch.
    pub at: f64,
    /// Whether it got no answer: it took the client's whole wait.
    pub unanswered: bool,
}

/// The requests of a line that may write.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HourWrites {
    /// How many there were.
    pub count: u64,
    /// The median of their latencies in milliseconds, over the newest few thousand the server
    /// held; none when there were none.
    #[serde(default)]
    pub p50_ms: Option<f64>,
    /// Their 90th percentile, over the same.
    #[serde(default)]
    pub p90_ms: Option<f64>,
}
