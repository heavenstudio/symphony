//! A project's reports: how its work flows, where defects are caught, what waits on the owner and
//! what reached its default branch, hour by hour. The machine's one server rolls each project it
//! serves up once an hour, and once after midnight for the day that ended, into one [`ReportDay`]
//! per UTC day ([`report_file`]), in `symphony/reports/` in the project's git common dir; the
//! Reports page reads them (docs/analysis/reports-and-monitoring.md, *Pipeline and defects*).
//!
//! A day file under way is written again each hour with what its day held up to then; one whose
//! day had ended when it was read is [`ReportDay::closed`] and never written again. Each figure
//! that adds up is kept per hour ([`ReportHour`]), so a page sums the hours of any span (a day, a
//! week, the hours of a local day); a spread, a list of tasks or cards, and what holds at the end
//! of the day are kept once per day. Every time is seconds since the epoch; every hour is a UTC
//! hour. A field added later is `#[serde(default)]`, so a reader built before it reads a newer
//! file and one built after it an older file.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// [`ReportDay::schema`] as written now.
pub const REPORT_SCHEMA: u32 = 1;

/// The name of the day file of `day` (`YYYY-MM-DD`, [`crate::run_day`]): `YYYY-MM-DD.json`.
pub fn report_file(day: &str) -> String {
    format!("{day}.json")
}

/// One UTC day of a project, as the roll-up last read it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportDay {
    /// [`REPORT_SCHEMA`].
    pub schema: u32,
    /// The project's id.
    pub project: String,
    /// The UTC day, `YYYY-MM-DD`.
    pub day: String,
    /// Its first second: UTC midnight.
    pub start: f64,
    /// When the roll-up read the figures, by the server's clock.
    pub read: f64,
    /// What the figures run to: `read` for a day under way, the day's end for one that had ended.
    pub until: f64,
    /// Whether the day had ended when it was read: its file is final, never written again.
    pub closed: bool,
    /// Each hour of the day from its first up to the one `until` falls in, oldest first.
    #[serde(default)]
    pub hours: Vec<ReportHour>,
    /// The top-level tasks over the day.
    #[serde(default)]
    pub top: DayFlow,
    /// The subtasks over the day, counted apart from the top-level tasks.
    #[serde(default)]
    pub subtasks: DayFlow,
    /// The `changes` and `approve` verdicts given in the day, by the review round each ended, the
    /// first round first.
    #[serde(default)]
    pub rounds: Vec<Round>,
    /// Each red task filed, closed or open in the day: the board's main and nightly reds.
    #[serde(default)]
    pub reds: Vec<Red>,
    /// The sessions that registered in the day, by kind and model, the most first.
    #[serde(default)]
    pub sessions: Vec<Started>,
    /// The decision cards of the project's registers over the day; none where its documents
    /// could not be read.
    #[serde(default)]
    pub decisions: Option<Decisions>,
    /// What git was read for the hours' [`ReportHour::git`]; none where it could not be read.
    #[serde(default)]
    pub git: Option<GitRead>,
}

/// One hour of a day: what adds up, so the hours of any span sum to that span's figure.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportHour {
    /// The UTC hour, 0 to 23.
    pub hour: u32,
    /// Its first second.
    pub start: f64,
    /// The top-level tasks.
    #[serde(default)]
    pub top: Flow,
    /// The subtasks, counted apart.
    #[serde(default)]
    pub subtasks: Flow,
    /// Landings: the `landed` verdicts the project's landing wrote in the hour.
    #[serde(default)]
    pub landed: u32,
    /// Red tasks filed in the hour.
    #[serde(default)]
    pub reds_filed: u32,
    /// Red tasks closed in the hour, done or cancelled.
    #[serde(default)]
    pub reds_closed: u32,
    /// The waits that ended in the hour.
    #[serde(default)]
    pub waits: Waits,
    /// The decision cards; none where the documents could not be read.
    #[serde(default)]
    pub decisions: Option<DecisionCounts>,
    /// What reached the default branch; none where git could not be read.
    #[serde(default)]
    pub git: Option<GitCounts>,
}

/// How tasks moved in one hour. A status no task held, took or spent time in is left out of its
/// map.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    /// How many tasks held each open status at the hour's end, or at [`ReportDay::until`] for the
    /// hour under way: backlog, concept, design, ready, `in_progress`, review, blocked.
    #[serde(default)]
    pub held: BTreeMap<String, u32>,
    /// How many times a task took each status in the hour, done and cancelled included: a task
    /// made counts once in the status it was made in, a claim of ready work once in `in_progress`,
    /// and a task sent back from review takes `in_progress` again.
    #[serde(default)]
    pub entered: BTreeMap<String, u32>,
    /// The task-hours spent in each open status in the hour.
    #[serde(default)]
    pub task_hours: BTreeMap<String, f64>,
    /// The part of those task-hours spent before a task's first claim: unclaimed.
    #[serde(default)]
    pub unclaimed_hours: f64,
    /// Moves from review back to in progress, a `changes` verdict's included.
    #[serde(default)]
    pub bounces: u32,
}

/// The tasks over a whole day: what does not add up across hours.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DayFlow {
    /// Creation to close of each close in the day; none with no close.
    #[serde(default)]
    pub lead: Option<Spread>,
    /// The open tasks that had held their status longest at [`ReportDay::until`], longest first,
    /// at most ten.
    #[serde(default)]
    pub longest: Vec<Sitting>,
}

/// How a set of durations spreads, in seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    /// How many durations.
    pub n: u32,
    pub median: f64,
    pub p90: f64,
}

/// An open task and when it took the status it holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sitting {
    pub slug: String,
    pub status: String,
    /// When it took it: [`ReportDay::until`] less this is how long it had sat there.
    pub since: f64,
}

/// The verdicts that ended one review round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    /// 1 for a task's first time in review, 2 for its second, and on.
    pub number: u32,
    /// `changes` verdicts: the round sent the work back.
    pub changes: u32,
    /// `approve` verdicts.
    pub approved: u32,
}

/// A red task: a project's main line or nightly run gone red.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Red {
    pub slug: String,
    /// `main` or `nightly`, by the series it is filed in; `red` for a red the board marked
    /// outside them.
    pub series: String,
    /// When it was filed.
    pub filed: f64,
    /// When it was closed, done or cancelled; none while it is open.
    #[serde(default)]
    pub closed: Option<f64>,
    /// Its status at [`ReportDay::until`]: its time to fix is `closed` less `filed` once it is
    /// `done`.
    pub status: String,
}

/// Sessions that registered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Started {
    /// `orchestrator`, `worker`, `reviewer`, …, as the session registered.
    pub kind: String,
    /// The model it named; empty when it named none.
    pub model: String,
    pub count: u32,
}

/// The waits that ended in an hour, each in the hour it ended.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Waits {
    /// From the `approve` a landing went by to the landing.
    #[serde(default)]
    pub approved_to_landed: Vec<Wait>,
    /// From a move into review to the first reviewer on it: a session joining the task as its
    /// reviewer, or claiming an instruction subtask of it, while it is in review; none at once
    /// where one already held the task.
    #[serde(default)]
    pub review_to_reviewer: Vec<Wait>,
    /// From a `changes` verdict to its executor's first line after it: a claim of the task, or a
    /// post, a move or a verdict by a session holding its executor role then.
    #[serde(default)]
    pub changes_to_resumed: Vec<Wait>,
    /// From a `changes` verdict to the task's next move into review.
    #[serde(default)]
    pub changes_to_review: Vec<Wait>,
}

/// One wait, by the task it was on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Wait {
    pub slug: String,
    pub seconds: f64,
}

/// The decision cards in one hour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionCounts {
    /// Cards waiting on the owner at the hour's end, or at [`ReportDay::until`] for the hour
    /// under way: those the owner's queue holds, its *Needs your decision* and *Answered: approve
    /// again or comment* groups.
    pub open: u32,
    /// Cards the owner's own line took out of the queue in the hour.
    pub answered: u32,
    /// Cards an agent's line took out of the queue, or posted already answered, in the hour: answered
    /// without asking the owner.
    pub unasked: u32,
}

/// The decision cards over a day.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Decisions {
    /// Each card waiting on the owner at [`ReportDay::until`], longest waiting first.
    #[serde(default)]
    pub open: Vec<OpenCard>,
    /// Each card the owner's line took out of the queue in the day, oldest first.
    #[serde(default)]
    pub answered: Vec<Answer>,
    /// Each card answered without asking the owner in the day, oldest first.
    #[serde(default)]
    pub unasked: Vec<Answer>,
}

/// A card waiting on the owner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenCard {
    /// The document it is in, by its route.
    pub doc: String,
    pub slug: String,
    /// Since when it has waited: the minute of its line that last put it in the owner's queue,
    /// posting it or asking; none for a card with no dated line, which has no history to tell.
    #[serde(default)]
    pub since: Option<f64>,
}

/// A card taken out of the owner's queue.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub doc: String,
    pub slug: String,
    /// When: the minute of the line that did it.
    pub at: f64,
    /// What did it. The owner's verdict word (`approved`, `rejected`, `question`, `dismissed`),
    /// or `commented` for a line with none; for a card answered without asking,
    /// `auto-approved` (an agent's auto-approve line, the triage's judgement or a basis naming the
    /// earlier verdict that decides the card) or `agent` (an agent's plain, `resolved` or
    /// `dismissed` line).
    pub how: String,
}

/// What reached the default branch in one hour, by commit time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCounts {
    /// Commits of any branch the default branch holds that were made in the hour, merges left
    /// out.
    pub commits: u32,
    /// Merge commits on the default branch's line of first parents: its landings by git.
    pub merges: u32,
    /// Lines added by the commits on that line, each a merge by what it brought against its first
    /// parent, the paths [`GitRead::excluded`] names left out.
    pub added: u64,
    /// Lines removed the same way.
    pub removed: u64,
    /// Commits the default branch holds whose subject begins `Revert "`.
    pub reverts: u32,
}

/// What the hours' [`GitCounts`] were read from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRead {
    /// The default branch.
    pub branch: String,
    /// The paths whose lines are not counted: each generator's outputs (`[land] generators`) and
    /// lock files by name, as written in the day file.
    pub excluded: Vec<String>,
}
