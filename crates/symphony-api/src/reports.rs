//! A project's reports: how its work flows, where defects are caught, what waits on the owner and
//! what reached its default branch, hour by hour. The machine's one server rolls each project it
//! serves up once an hour, and once after midnight for the day that ended, into one [`ReportDay`]
//! per UTC day ([`report_file`]), in `symphony/reports/` in the project's git common dir; the
//! Reports page reads them (docs/product/analysis/reports-and-monitoring.md, *The day files*).
//!
//! A day file under way is written again each hour with what its day held up to then; one whose
//! day had ended when it was read is [`ReportDay::closed`] and never written again. Each figure
//! that adds up is kept per hour ([`ReportHour`]), so a page sums the hours of any span (a day, a
//! week, the hours of a local day); a spread, a list of tasks or cards, and what holds at the end
//! of the day are kept once per day. Every time is seconds since the epoch; every hour is a UTC
//! hour, named by its first second ([`ReportHour::start`]), the key the other day files of the
//! `symphony/` folder name their hours by too. A field added later is `#[serde(default)]`, so a
//! reader built before it reads a newer file and one built after it an older file. The board
//! writes the file by hand, as serde would (`crates/tasks/src/reports/json.rs`), so a field added
//! here is written there too; a test there holds the two texts equal byte for byte.

use serde::{Deserialize, Serialize};

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
    /// Each red task filed, closed or open in the day: the board's main and release reds.
    #[serde(default)]
    pub reds: Vec<Red>,
    /// The decision cards of the project's registers over the day, a live branch's posted cards
    /// with main's; none where its documents could not be read.
    #[serde(default)]
    pub decisions: Option<Decisions>,
    /// What git was read for the hours' [`ReportHour::git`]; none where it could not be read.
    #[serde(default)]
    pub git: Option<GitRead>,
    /// Every token the board's `usage` table held at `read`, summed by role, model and speed
    /// (`round` 0): what the next roll-up takes from its own read to tell what the hours since
    /// gained ([`ReportHour::tokens`]). None in a file whose roll-up read none.
    #[serde(default)]
    pub usage: Option<Vec<TokenSum>>,
    /// Each top-level task landed in the day, once at its last landing in it, with the tokens it
    /// and its subtasks took, oldest first.
    #[serde(default)]
    pub landed: Vec<Landed>,
}

/// Tokens of the four kinds a price tells apart and the model calls they came from (the board's
/// `usage::Tokens`), of one role on one model at one speed. Tokens are kept, never dollars: a
/// reader prices them at the table its build embeds, so an edit of a price reprices every day.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSum {
    /// The role they were spent in: a task's (`executor`, `reviewer`, `reserved`), or the
    /// session's kind for calls made while it worked none (`orchestrator`, `worker`).
    pub role: String,
    /// The review round, 1 for the work up to a task's first review, in a task's
    /// [`Landed::spent`]; 0 where rounds are not told apart.
    #[serde(default)]
    pub round: u32,
    pub model: String,
    pub speed: String,
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub calls: u64,
}

/// A top-level task landed in a day and what it took.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Landed {
    pub slug: String,
    /// When its landing wrote its `landed` verdict.
    pub at: f64,
    /// The tokens the board recorded on it and on its subtasks up to the file's read, by role,
    /// round, model and speed; empty where it recorded none.
    #[serde(default)]
    pub spent: Vec<TokenSum>,
    /// The kind it was given (`symphony_board::kind`: feature, fix, red, tests, docs); left out
    /// for one given none, and in a file written before kinds were kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The lines its landing brought the default branch, read by the git roll-up from the commit
    /// on the branch's line of first parents that landed it ([`GitRead::excluded`] left out, as
    /// [`GitCounts`] leaves them): the squash its land's receipt names, or the merge its subject
    /// says. Left out where that commit was not read, and in a file written before they were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<Lines>,
}

/// Lines added and removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lines {
    pub added: u64,
    pub removed: u64,
}

impl Lines {
    /// Added and removed together: the lines changed.
    pub fn changed(&self) -> u64 {
        self.added + self.removed
    }
}

/// One hour of a day: what adds up, so the hours of any span sum to that span's figure.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportHour {
    /// Its first second: the hour's key, as the other day files of the project name an hour.
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
    /// The tokens the board's `usage` table gained in the hour, by role, model and speed: what it
    /// held at a roll-up less what it held at the one before ([`ReportDay::usage`]), shared among
    /// the hours between the two by the time of each between them, so each token is counted once.
    /// None for an hour no pair of roll-ups spans: unknown, never none spent.
    #[serde(default)]
    pub tokens: Option<Vec<TokenSum>>,
    /// Where a custom workflow's tasks spent the hour, by delivery phase and by coordination
    /// state; left out of the file of a board that runs no workflow of its own, which stays as it
    /// was written before this was counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<Placed>,
}

/// The time a custom workflow's tasks spent in one hour, from the board's recorded workflow
/// transitions alone (docs/workflow/WORKFLOW.md, "The app"): the top-level tasks and the subtasks apart, as
/// [`ReportHour::top`] and [`ReportHour::subtasks`] are. It is never the machine's time on a
/// task's builds and tests, which is the runs' own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    #[serde(default)]
    pub top: PlacedHours,
    #[serde(default)]
    pub subtasks: PlacedHours,
}

/// Task-hours by phase and by state, two totals of the same time: each hour a task spent at an
/// open position counts once in its phase and once in its state, so the two lists sum alike and
/// are never added together. Time at a position that projects to done or cancelled is left out, as
/// [`Flow::task_hours`] leaves the closed statuses out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlacedHours {
    /// By delivery phase, ordered by ID and then label.
    #[serde(default)]
    pub phases: Vec<Spent>,
    /// By coordination state, ordered the same way.
    #[serde(default)]
    pub states: Vec<Spent>,
}

/// The task-hours spent in one phase or one state under one label.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spent {
    /// The phase's or state's stable ID.
    pub id: String,
    /// Its label in the definition the move that put the task there was made under: the ID where
    /// that definition is not recorded or does not name it. A label changed later is a second
    /// entry of the same ID from the moves made after it, so it never renames this one.
    pub label: String,
    pub hours: f64,
}

/// How tasks moved in one hour. A status no task held, took or spent time in is left out of its
/// map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    /// How many tasks held each open status at the hour's end, or at [`ReportDay::until`] for the
    /// hour under way: backlog, concept, design, ready, `in_progress`, review, blocked.
    #[serde(default)]
    pub held: ByStatus<u32>,
    /// How many times a task took each status in the hour, done and cancelled included: a task
    /// made counts once in the status it was made in, a claim of ready work once in `in_progress`,
    /// and a task sent back from review takes `in_progress` again. A task approved, sent back to
    /// review by its landing and approved again takes done twice: the tasks done are [`Flow::done`].
    #[serde(default)]
    pub entered: ByStatus<u32>,
    /// The tasks done in the hour, each once: a task that held done when the file was read, in the
    /// hour of its last close, as `tasks stats` counts its tasks done per day
    /// (`symphony_board::stages::Stages::done_at`). One cancelled or reopened since counts in none.
    /// None in a file that does not record it.
    #[serde(default)]
    pub done: Option<u32>,
    /// The task-hours spent in each open status in the hour.
    #[serde(default)]
    pub task_hours: ByStatus<f64>,
    /// The part of those task-hours spent before a task's first claim: unclaimed.
    #[serde(default)]
    pub unclaimed_hours: f64,
    /// Moves from review back to in progress, a `changes` verdict's included.
    #[serde(default)]
    pub bounces: u32,
}

/// A figure for each status a task can take, written as a map of the statuses whose figure is not
/// zero, by name. A field each rather than a map of names: a map costs the server's binary its own
/// tree code for each kind of figure (docs/workflow/CHECKS.md, size budgets), and the board has these nine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    default,
    bound(
        serialize = "T: Serialize + Default + PartialEq",
        deserialize = "T: Deserialize<'de> + Default"
    )
)]
pub struct ByStatus<T> {
    #[serde(skip_serializing_if = "is_zero")]
    pub backlog: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub blocked: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub cancelled: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub concept: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub design: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub done: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub in_progress: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub ready: T,
    #[serde(skip_serializing_if = "is_zero")]
    pub review: T,
}

/// The statuses of a [`ByStatus`], in the order of its fields: the order the file writes them.
const STATUSES: [&str; 9] = [
    "backlog",
    "blocked",
    "cancelled",
    "concept",
    "design",
    "done",
    "in_progress",
    "ready",
    "review",
];

/// Where `status` stands in [`STATUSES`]: one copy of the names' comparison for every kind of
/// figure.
fn status_index(status: &str) -> Option<usize> {
    STATUSES.iter().position(|name| *name == status)
}

impl<T> ByStatus<T> {
    /// The figure of `status`; none for a name that is no status.
    pub fn get_mut(&mut self, status: &str) -> Option<&mut T> {
        Some(match status_index(status)? {
            0 => &mut self.backlog,
            1 => &mut self.blocked,
            2 => &mut self.cancelled,
            3 => &mut self.concept,
            4 => &mut self.design,
            5 => &mut self.done,
            6 => &mut self.in_progress,
            7 => &mut self.ready,
            _ => &mut self.review,
        })
    }

    /// Each status's figure, by name, as the file writes them.
    pub fn each(&self) -> [(&'static str, &T); 9] {
        let [
            backlog,
            blocked,
            cancelled,
            concept,
            design,
            done,
            in_progress,
            ready,
            review,
        ] = STATUSES;
        [
            (backlog, &self.backlog),
            (blocked, &self.blocked),
            (cancelled, &self.cancelled),
            (concept, &self.concept),
            (design, &self.design),
            (done, &self.done),
            (in_progress, &self.in_progress),
            (ready, &self.ready),
            (review, &self.review),
        ]
    }
}

fn is_zero<T: Default + PartialEq>(figure: &T) -> bool {
    *figure == T::default()
}

/// The tasks over a whole day: what does not add up across hours.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DayFlow {
    /// Creation to close of the tasks done in the day, each once at its last close, as
    /// [`Flow::done`] counts them; none with none.
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

/// A red task: a project's main line or release run gone red.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Red {
    pub slug: String,
    /// `main` or `release`, by the series it is filed in (a release run's in either of its two,
    /// `tests/release-red-` and the one before its rename); `red` for a red the board marked
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
    /// The commits its filer said it came between, `GOOD..BAD`, the newest it knew green and the
    /// first it found red, as its body's `between:` line names them; none for a red that names
    /// none.
    #[serde(default)]
    pub between: Option<String>,
    /// The tasks that landed in `between`, by slug, newest first: each merge the land made on the
    /// default branch's line of first parents in that range, the landings it may trace back to.
    /// None where it names no range, or git could not read it.
    #[serde(default)]
    pub landed: Option<Vec<String>>,
}

/// The waits that ended in an hour, each in the hour it ended.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Waits {
    /// From the `approve` a landing went by to the landing.
    #[serde(default)]
    pub approved_to_landed: Vec<Wait>,
    /// From a move into review to the first reviewer on it: a session joining the task as its
    /// reviewer, or claiming an instruction subtask of it, while it is in review. A reviewer that
    /// already held the role at the move (a re-review whose reviewer kept it) ends none, as the
    /// board's lines cannot tell when it came back.
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

/// The decision cards in one hour. A card leaves the owner's queue by one line, counted once in
/// the hour of that line, in `answered`, `auto_approved` or `withdrawn` by whose line it is and
/// what it says. Not counted: a decision recorded as answered by a plain agent line on a card the
/// owner approved, which leaves the queue as it was; a card an agent posted already out of the
/// queue, unless auto-approved; and a live branch's card while its task is done and the branch not
/// landed, which the owner's queue leaves out too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionCounts {
    /// Cards waiting on the owner at the hour's end, or at [`ReportDay::until`] for the hour
    /// under way: those the owner's queue holds, its *Needs your decision* and *Answered: approve
    /// again or comment* groups, a live branch's posted cards among them.
    pub open: u32,
    /// Cards the owner's own line took out of the queue in the hour.
    pub answered: u32,
    /// Cards an agent's `auto-approved` line took out of the queue, or posted already
    /// auto-approved, in the hour: decided without asking the owner.
    #[serde(default)]
    pub auto_approved: u32,
    /// Cards another agent's line took out of the queue in the hour, deciding nothing: superseded
    /// by a newer card (`dismissed`), the agent's own question withdrawn, or marked resolved.
    #[serde(default)]
    pub withdrawn: u32,
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
    /// Each card auto-approved in the day ([`DecisionCounts::auto_approved`]), oldest first.
    #[serde(default)]
    pub auto_approved: Vec<Answer>,
    /// Each card withdrawn in the day ([`DecisionCounts::withdrawn`]), oldest first.
    #[serde(default)]
    pub withdrawn: Vec<Answer>,
}

/// A card waiting on the owner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenCard {
    /// The page it is on, by its route: `docs/DECISIONS.md`, a live branch's copy of one of main's
    /// cards included, or `@BRANCH/docs/DECISIONS.md` for a card only a live branch posted.
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
    /// The page it is on, as [`OpenCard::doc`] names it.
    pub doc: String,
    pub slug: String,
    /// When: the minute of the line that did it.
    pub at: f64,
    /// The verdict word of that line (the owner's `approved`, `rejected`, `question`,
    /// `dismissed`; an agent's `auto-approved`, `dismissed`, `resolved`), or `commented` for the
    /// owner's line with none and `replied` for an agent's (never `answered`, the name of the
    /// owner's own leavings, [`Decisions::answered`]).
    pub how: String,
}

/// What reached the default branch in one hour: each commit in the hour the commit on the
/// branch's line of first parents that brought it there was made, itself or the merge that landed
/// it, so commits, merges and lines share one time base.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCounts {
    /// Commits that reached the branch, merges left out, and so is a commit that changed no path
    /// outside [`GitRead::excluded`] (a generator's own commit of its outputs).
    pub commits: u32,
    /// Merge commits on the branch's line of first parents: its landings by git.
    pub merges: u32,
    /// Lines added by the commits on that line, each a merge by what it brought against its first
    /// parent, the paths [`GitRead::excluded`] names left out.
    pub added: u64,
    /// Lines removed the same way.
    pub removed: u64,
    /// Of `commits`, those whose subject begins `Revert "`.
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
