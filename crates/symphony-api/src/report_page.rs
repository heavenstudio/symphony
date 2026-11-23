//! The Reports page's one read: `GET` [`REPORTS_PATH`]`?grain=hour|day|week&days=N` answers a
//! [`ReportsPage`], the project's figures over a window of whole periods, summed by the server out of
//! the three day folders of `symphony/` in the project's git common dir and nothing else: the hourly
//! roll-up's ([`crate::ReportDay`]), the agents' hour lines ([`crate::AgentHour`]) and the board's
//! health hours ([`crate::HealthHour`]), all keyed by the UTC hour they name. It never reads the
//! board.
//!
//! The periods ([`ReportBucket`]) are UTC hours, UTC days or weeks from Monday 00:00 UTC, the last
//! one the period the server's clock is in, cut at the end of its current hour ([`ReportsPage::to`]).
//! Each source's part of a period is none when no file holds any hour of it, which the page shows
//! as missing, never as a zero, and says how much of the period it covers when it holds part
//! ([`ReportBucket::rolled`], [`BucketAgents::observed_s`], [`BucketHealth::covered_s`]).
//!
//! Bounds: a window of at most [`REPORTS_MAX_DAYS`] days, [`REPORTS_HOUR_MAX_DAYS`] by the hour (so
//! at most 168 periods); each day file read whole only up to [`REPORTS_FILE_BYTES`], and at most
//! [`REPORTS_READ_BYTES`] read for one answer, newest days first: a file past either is named in
//! [`ReportsPage::unread`] and its hours are missing.

use crate::agent_activity::{Gauge, KindLevel, Level, RoleLevel};
use crate::health::HourPasses;
use crate::reports::{ByStatus, DecisionCounts, GitCounts, PlacedHours, Spread};
use serde::{Deserialize, Serialize};

/// The Reports page's read.
pub const REPORTS_PATH: &str = "/api/reports";
/// The most days a window reaches back.
pub const REPORTS_MAX_DAYS: u32 = 90;
/// The most days a window by the hour reaches back: 168 periods.
pub const REPORTS_HOUR_MAX_DAYS: u32 = 7;
/// The most bytes of one day file read; a larger file is not read, and named.
pub const REPORTS_FILE_BYTES: u64 = 4 << 20;
/// The most bytes read for one answer, newest days first; the files past it are not read, and named.
pub const REPORTS_READ_BYTES: u64 = 64 << 20;

/// How long each period of a window is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Grain {
    Hour,
    #[default]
    Day,
    Week,
}

impl Grain {
    /// The grain `word` names (`hour`, `day`, `week`); none for another word.
    pub fn parse(word: &str) -> Option<Grain> {
        match word {
            "hour" => Some(Grain::Hour),
            "day" => Some(Grain::Day),
            "week" => Some(Grain::Week),
            _ => None,
        }
    }

    /// Its word, as [`Grain::parse`] reads it.
    pub fn word(self) -> &'static str {
        match self {
            Grain::Hour => "hour",
            Grain::Day => "day",
            Grain::Week => "week",
        }
    }

    /// The days a window reaches back when none is asked: two by the hour, fourteen by the day,
    /// twelve weeks by the week.
    pub fn default_days(self) -> u32 {
        match self {
            Grain::Hour => 2,
            Grain::Day => 14,
            Grain::Week => 84,
        }
    }

    /// The most days a window of this grain reaches back.
    pub fn max_days(self) -> u32 {
        match self {
            Grain::Hour => REPORTS_HOUR_MAX_DAYS,
            Grain::Day | Grain::Week => REPORTS_MAX_DAYS,
        }
    }

    /// `days` asked, none for the default, as a window of this grain takes it: at least one, at
    /// most [`Grain::max_days`].
    pub fn days(self, days: Option<u32>) -> u32 {
        days.unwrap_or(self.default_days())
            .clamp(1, self.max_days())
    }
}

/// The project's figures over a window, a period at a time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportsPage {
    pub grain: Grain,
    /// The days the window reaches back, as [`Grain::days`] took the ask.
    pub days: u32,
    /// The first second of its first period.
    pub from: f64,
    /// Where it ends: the end of the hour the server's clock is in, so an answer stays the same
    /// through an hour no file changed in.
    pub to: f64,
    /// Each period from `from` to `to`, oldest first.
    #[serde(default)]
    pub buckets: Vec<ReportBucket>,
    /// The first hour any of the project's roll-up files holds, whatever the window: before it there
    /// was no roll-up yet; none when the roll-up has written no file.
    #[serde(default)]
    pub first: Option<f64>,
    /// The commits on the default branch's line of first parents in the window that changed the
    /// project's settings file, where its gates and limits are set, or the instructions its agents
    /// work under (a skill, the rules), oldest first.
    #[serde(default)]
    pub marks: Vec<ReportMark>,
    /// What was not read, each in a sentence naming its file or folder and why: an hour it held is
    /// missing from the periods.
    #[serde(default)]
    pub unread: Vec<String>,
    /// The day the price table's prices were read (`YYYY-MM-DD`), which every estimate of the
    /// answer is at; empty from a server that prices none.
    #[serde(default)]
    pub prices: String,
}

/// Tokens and what they cost, estimated at API list prices: never money spent, as a subscription
/// is not billed by the token.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Spend {
    pub tokens: crate::AgentTokens,
    pub cost: crate::AgentCost,
}

/// A role's tokens in a period, and their estimated cost: a [`Spend`]'s two fields beside its
/// role, never `#[serde(flatten)]`, which reads no number back where `serde_json` keeps a number
/// as its text (`arbitrary_precision`, as the server's build links it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoleSpend {
    /// A task's role (`executor`, `reviewer`, `reserved`), or the kind of a session working no
    /// task (`orchestrator`, `worker`).
    pub role: String,
    pub tokens: crate::AgentTokens,
    pub cost: crate::AgentCost,
}

/// The Landed tasks tab's read: `GET` [`REPORTS_LANDED_PATH`]`?grain=hour|day|week&days=N`
/// answers a [`LandedPage`], the top-level tasks landed in the same window as [`REPORTS_PATH`]'s,
/// each once at its last landing in it, priced with its subtasks at the table the server embeds,
/// out of the roll-up's day files alone: by task, not by period. Its own read, as a window of
/// weeks lands a few thousand tasks, each a row.
pub const REPORTS_LANDED_PATH: &str = "/api/reports/landed";

/// The top-level tasks landed in a window and what they took.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct LandedPage {
    pub grain: Grain,
    /// The days the window reaches back, as [`Grain::days`] took the ask.
    pub days: u32,
    /// The first second of its first period and where it ends, as [`ReportsPage`]'s.
    pub from: f64,
    pub to: f64,
    /// Each one the board recorded tokens for, the dearest first, then by slug: the first ten are
    /// the dearest, and all of them the dots and the table.
    #[serde(default)]
    pub tasks: Vec<LandedRow>,
    /// Those it recorded none for: in no figure.
    #[serde(default)]
    pub without: u32,
    /// The window's figures, of the tasks with a priced cost.
    #[serde(default)]
    pub figures: LandedFigures,
    /// By the kind each was given, those that cost the most in all first, the tasks given none
    /// last.
    #[serde(default)]
    pub kinds: Vec<KindRow>,
    /// By how many reviews each took, the fewest first, the last one of four or more.
    #[serde(default)]
    pub reviews: Vec<ReviewRow>,
    /// When the first landing of the window that names a kind landed; none when none does, as in
    /// a window from before kinds were kept.
    #[serde(default)]
    pub kinds_from: Option<f64>,
    /// The day the price table's prices were read (`YYYY-MM-DD`).
    #[serde(default)]
    pub prices: String,
    /// What was not read, each in a sentence naming its file and why: a task it held is missing.
    #[serde(default)]
    pub unread: Vec<String>,
}

/// A landed task: what it and its subtasks took, in US dollars estimated at list prices.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct LandedRow {
    pub slug: String,
    /// When it landed.
    pub at: f64,
    /// Its kind; none for one given none.
    #[serde(default)]
    pub kind: Option<String>,
    /// What its work up to its first review cost, and every round after it.
    pub first: f64,
    pub later: f64,
    /// Its tokens, all told.
    pub tokens: u64,
    /// Whether some of its tokens had no price, and are in no dollar here; one none of whose had
    /// one has no cost to rank, and is in no figure.
    #[serde(default)]
    pub unpriced: bool,
    /// How many reviews it took: the last round it took tokens in, less the first.
    #[serde(default)]
    pub reviews: u32,
    /// The lines its landing changed, added and removed; none where they were not read.
    #[serde(default)]
    pub lines: Option<u64>,
}

/// The figures of a set of landed tasks with a priced cost, in US dollars.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct LandedFigures {
    /// The tasks with a priced cost.
    pub tasks: u32,
    #[serde(default)]
    pub median: Option<f64>,
    #[serde(default)]
    pub mean: Option<f64>,
    #[serde(default)]
    pub p90: Option<f64>,
    /// What they cost, all told, and what their review rounds cost of it.
    pub cost: f64,
    pub later: f64,
    /// Those back after review: reviewed twice or more, a round past the second.
    pub back: u32,
    /// Those whose landing changed a counted line, what those cost and their lines.
    pub lined: u32,
    pub lined_cost: f64,
    pub lines: u64,
    /// Those whose landing changed no counted line, and those whose lines were not read.
    pub unlined: u32,
    pub unread: u32,
}

/// The landed tasks of one kind, or of none.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct KindRow {
    /// The kind; none for the tasks given none.
    #[serde(default)]
    pub kind: Option<String>,
    pub tasks: u32,
    #[serde(default)]
    pub median: Option<f64>,
    /// What they cost, all told.
    pub cost: f64,
    /// Of them, what those whose landing changed a counted line cost, and those lines.
    pub lined_cost: f64,
    pub lines: u64,
}

/// The landed tasks that took one number of reviews.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ReviewRow {
    /// The reviews; the last row's is four, for four or more.
    pub reviews: u32,
    pub tasks: u32,
    /// The mean cost of one's first round and of its review rounds.
    pub first: f64,
    pub later: f64,
    #[serde(default)]
    pub median: Option<f64>,
}

/// The top-level tasks landed in one period, each once at its last landing in the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BucketLanded {
    /// The tasks landed in it, with tokens recorded or not: what **All in** divides the period's
    /// whole cost by.
    pub tasks: u32,
    /// Of them, those with a priced cost, the median and the p90 of what one cost, in dollars.
    pub priced: u32,
    #[serde(default)]
    pub median: Option<f64>,
    #[serde(default)]
    pub p90: Option<f64>,
    /// Of those, the ones whose landing changed a counted line, what they cost and their lines.
    pub lined: u32,
    pub lined_cost: f64,
    pub lines: u64,
}

/// One period of a window.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportBucket {
    /// Its first second.
    pub start: f64,
    /// Its end: the period's own, or [`ReportsPage::to`] for the one under way.
    pub end: f64,
    /// The hours of the roll-up's files in it, of the whole hours from `start` to `end`.
    #[serde(default)]
    pub rolled: u32,
    /// How the top-level tasks moved; none when no roll-up hour is in it.
    #[serde(default)]
    pub flow: Option<BucketFlow>,
    /// The decision cards; none when no roll-up hour in it read them.
    #[serde(default)]
    pub decisions: Option<DecisionCounts>,
    /// What reached the default branch, summed; none when no roll-up hour in it read git.
    #[serde(default)]
    pub git: Option<GitCounts>,
    /// The waits that ended in it, each spread over the waits of that kind.
    #[serde(default)]
    pub waits: Option<BucketWaits>,
    /// Creation to close of the top-level tasks done in its days, as [`BucketFlow::done`] counts
    /// them: a day's as the roll-up keeps it;
    /// a week's out of its days', its median the mean of theirs weighted by their closes and its
    /// p90 the largest of theirs, so approximate. None by the hour, and with no close.
    #[serde(default)]
    pub lead: Option<Spread>,
    /// The review verdicts given in its days, every round; none by the hour, and with no day file.
    #[serde(default)]
    pub verdicts: Option<BucketVerdicts>,
    /// The time to fix the reds closed done in it: filing to close.
    #[serde(default)]
    pub red_fix: Option<Spread>,
    /// The agents at work; none when no agents' hour line is in it.
    #[serde(default)]
    pub agents: Option<BucketAgents>,
    /// The board's health; none when no health line is in it.
    #[serde(default)]
    pub health: Option<BucketHealth>,
    /// The tokens the board recorded in it, by role, with their estimated cost, out of the hours
    /// of the roll-up's files that tell them ([`crate::ReportHour::tokens`]); none when none does,
    /// which is unknown, never none spent, and empty when those hours gained none.
    #[serde(default)]
    pub spend: Option<Vec<RoleSpend>>,
    /// Where a custom workflow's top-level tasks spent it, by phase and by state
    /// ([`crate::reports::Placed`]), summed over its roll-up hours that record it; left out when
    /// none does, as on a board that runs no workflow of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<PlacedHours>,
    /// The top-level tasks landed in it and what they cost, by the roll-up's day files; none when
    /// none landed, which has no point.
    #[serde(default)]
    pub landed: Option<BucketLanded>,
}

/// How the top-level tasks moved in a period.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BucketFlow {
    /// How many held each open status at the end of its last roll-up hour.
    #[serde(default)]
    pub held: ByStatus<u32>,
    /// How many times a task took each status in it: a task approved again after its landing sent
    /// it back takes done twice.
    #[serde(default)]
    pub entered: ByStatus<u32>,
    /// The tasks done in it, each once at its last close ([`crate::reports::Flow::done`]), summed
    /// over its roll-up hours; none unless every one of them records them, so a period that spans
    /// a file written before they were counted is a gap, never a part of its count shown whole.
    #[serde(default)]
    pub done: Option<u32>,
    /// The task-hours spent in each open status in it.
    #[serde(default)]
    pub task_hours: ByStatus<f64>,
    /// The part of those spent before a first claim.
    #[serde(default)]
    pub unclaimed_hours: f64,
    /// Moves from review back to in progress.
    #[serde(default)]
    pub bounces: u32,
    /// Landings: the project's landing's `landed` verdicts.
    #[serde(default)]
    pub landed: u32,
    #[serde(default)]
    pub reds_filed: u32,
    #[serde(default)]
    pub reds_closed: u32,
}

/// The waits that ended in a period, a spread of each kind ([`crate::reports::Waits`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BucketWaits {
    #[serde(default)]
    pub review_to_reviewer: Option<Spread>,
    #[serde(default)]
    pub changes_to_resumed: Option<Spread>,
    #[serde(default)]
    pub changes_to_review: Option<Spread>,
    #[serde(default)]
    pub approved_to_landed: Option<Spread>,
}

/// The review verdicts of a period's days.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketVerdicts {
    /// `changes`: work sent back.
    pub changes: u32,
    pub approved: u32,
}

/// The agents at work in a period, out of its agents' hour lines as `docs/workflow/AGENT_ACTIVITY.md` reads
/// an hour: seconds and counts added up, peaks the largest, means weighted by the seconds each line
/// watched (by its snapshots where none watched any).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BucketAgents {
    /// The seconds of it the server watched.
    pub observed_s: f64,
    pub samples: u64,
    /// Live sessions running work, by role, each once for each role it holds.
    #[serde(default)]
    pub running: Vec<RoleLevel>,
    /// The running sessions, each once.
    #[serde(default)]
    pub running_agents: Level,
    /// Sessions holding work, live, stale or unknown, by role.
    #[serde(default)]
    pub holding: Vec<RoleLevel>,
    #[serde(default)]
    pub holding_agents: Level,
    /// Live sessions by kind.
    #[serde(default)]
    pub live: Vec<KindLevel>,
    /// Sessions that connected in it, of every kind and model.
    #[serde(default)]
    pub started: u64,
    #[serde(default)]
    pub went_stale: u64,
    #[serde(default)]
    pub went_lost: u64,
    #[serde(default)]
    pub stale_s: f64,
    #[serde(default)]
    pub lost_s: f64,
    /// The machine's load average over 1 and 15 minutes; none when no line read it.
    #[serde(default)]
    pub load_1m: Option<Gauge>,
    #[serde(default)]
    pub load_15m: Option<Gauge>,
}

/// The board's health in a period, its lines added up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BucketHealth {
    /// The seconds of it its lines cover.
    pub covered_s: f64,
    #[serde(default)]
    pub dispatch: HourPasses,
    #[serde(default)]
    pub sweep: HourPasses,
    /// Requests the server's clients sent the board.
    #[serde(default)]
    pub requests: u64,
    /// Of them, those that took a second or longer.
    #[serde(default)]
    pub slow: u64,
    /// Of them, those that got no answer.
    #[serde(default)]
    pub unanswered: u64,
    /// The slowest request's milliseconds.
    #[serde(default)]
    pub slowest_ms: Option<f64>,
    /// Requests that may write.
    #[serde(default)]
    pub writes: u64,
    /// The largest of its hours' write p90s, in milliseconds.
    #[serde(default)]
    pub write_p90_ms: Option<f64>,
}

/// A commit that changed the project's settings file or the instructions its agents work under.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ReportMark {
    /// Its commit time.
    pub at: f64,
    /// Its hash, abbreviated.
    pub commit: String,
    pub subject: String,
    /// What it changed, each once: `settings`, `rules` and `skill NAME`. A server from before the
    /// instructions were marked names none, and its marks are settings changes.
    #[serde(default)]
    pub changed: Vec<String>,
}
