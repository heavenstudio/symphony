//! Where a task's time went, read back from what the board already keeps: the task's `created`,
//! `updated` and `status`, and the lines its status changes left on its thread. Nothing here reads
//! the board or the clock: [`stages`] is given the lines and the time to count an open task up to.
//!
//! The lines that change a status. The store builds its move and claim lines with [`moved_line`]
//! and [`claimed_line`] and the others from the constants here, but for the owner's move, whose
//! line the database joins from [`MOVED`] and [`ARROW`]; the verdict words (`changes`, `approve`,
//! `done`) and the `(was STATUS)` and `(released)`/`(left)` endings are written out on both sides,
//! not shared.
//!
//! - `HANDLE moved it FROM → TO` ([`moved_line`]): a move, the owner's too.
//! - `HANDLE claimed it[, handed over from OTHER (HEALTH), …]` ([`claimed_line`]): ready work
//!   starts; a claim of work in any other status changes no status.
//! - `HANDLE released ROLE (released)` and `HANDLE released every role (left)` ([`RELEASED`]): work
//!   in progress whose last executor let go is ready again.
//! - `blocked: cancelled dependencies: … (was STATUS)` ([`BLOCK_LINE`]) and
//!   `unblocked: WHY; back to STATUS` ([`unblocked_line`]): the end of a block, its WHY no
//!   cancelled dependency left ([`UNBLOCK_LINE`]) or the owner's word that ended it.
//! - a `verdict` message: `changes…` sends a task in review back to in progress; any other
//!   (`approve…`, a subtask's `done: …`, a landing) closes it.
//!
//! A thread is replayed in the order of its message ids, the order the lines were written in: a
//! line's time is its writer's clock, so a line dated before the one written ahead of it is taken
//! at that one's time. A line that names the status the task already holds changes nothing (a
//! duplicate). After the line that closed the task, only a move out of done or cancelled is read:
//! it reopens the task, and the lines after it count as before. The statuses' times add up to the
//! time from creation to close (or to now), the time closed left out; the wait before the first claim is a part
//! of them, never a status of its own. Where the replay ends in another status than the task's own, a change the thread
//! does not record (a line lost, a board copied without its thread) is dated at the task's
//! `updated`.
//!
//! Beside the totals the replay keeps each [`Segment`]: one stretch in one status, from when the
//! task took it to when it left it, and who moved it there as the line says (the mover, the
//! claimer, the releaser, a verdict's sender, the creator for the status it was made in). The
//! segments of a status add up to its [`Stage`]'s time and are as many as its visits.

use crate::{BEFORE_WORK, STATUSES};
use std::collections::{BTreeSet, HashSet};

pub mod roles;
pub mod stats;
#[cfg(test)]
mod stats_tests;
#[cfg(test)]
mod tests;

/// What starts the line a task's creation leaves.
pub const CREATED: &str = "created by ";
/// Between the mover and the two statuses of a move's line.
pub const MOVED: &str = " moved it ";
/// Between the two statuses of a move's line.
pub const ARROW: &str = " → ";
/// After the claimer on a claim's line.
pub const CLAIMED: &str = " claimed it";
/// After [`CLAIMED`], before the executors a claim took over from.
pub const HANDED: &str = ", handed over from ";
/// Between the session and the role on a release's line.
pub const RELEASED: &str = " released ";
/// Between the session and the role it took on a join's line (`plum joined as reviewer`).
pub const JOINED: &str = " joined as ";
/// What a release of every role names in place of one.
pub const EVERY_ROLE: &str = "every role";
/// What starts the line a cancelled prerequisite's block puts on its dependent's thread.
pub const BLOCK_LINE: &str = "blocked: cancelled dependencies: ";
/// What starts the line that gives a task so blocked its status back, before that status.
pub const UNBLOCK_LINE: &str = "unblocked: no cancelled dependency left; back to ";
/// What starts every line that ends a block ([`unblocked_line`]).
pub const UNBLOCKED: &str = "unblocked: ";
/// Between why a block ended and the status its line gives the task back ([`unblocked_line`]).
pub const BACK_TO: &str = "; back to ";

/// The line that ends a block for `why` (a few words with no `; back to` in them), giving the task
/// `to` back: `unblocked: WHY; back to TO`.
pub fn unblocked_line(why: &str, to: &str) -> String {
    format!("{UNBLOCKED}{why}{BACK_TO}{to}")
}

/// The status a line that ends a block gives the task back ([`unblocked_line`]), as written: the
/// text after its last `; back to`, up to what follows it (` (its block line recorded …)`). None
/// for any other line, and for one whose status is no word of a status (checked by the caller).
pub fn unblocked_to(text: &str) -> Option<&str> {
    let (_, rest) = text.strip_prefix(UNBLOCKED)?.rsplit_once(BACK_TO)?;
    Some(rest.split_once(' ').map_or(rest, |(to, _)| to))
}

/// The statuses nothing leaves.
pub const CLOSED: [&str; 2] = ["done", "cancelled"];
/// The line a move leaves on the thread.
pub fn moved_line(handle: &str, from: &str, to: &str) -> String {
    format!("{handle}{MOVED}{from}{ARROW}{to}")
}

/// The line a claim leaves on the thread; `handed` is each executor it took over from, as
/// `HANDLE (HEALTH)`.
pub fn claimed_line(handle: &str, handed: &[String]) -> String {
    if handed.is_empty() {
        format!("{handle}{CLAIMED}")
    } else {
        format!("{handle}{CLAIMED}{HANDED}{}", handed.join(", "))
    }
}

/// One thread line as the replay reads it: a `system` or `verdict` message.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub ts: f64,
    /// Who wrote it; empty where the reader did not read it (`tasks stats`), which only the
    /// segments' `by` of a verdict uses.
    pub sender: String,
}

impl From<&crate::Message> for Line {
    fn from(m: &crate::Message) -> Line {
        Line {
            id: m.id,
            kind: m.kind.clone(),
            text: m.text.clone(),
            ts: m.ts,
            sender: m.sender.clone(),
        }
    }
}

/// What the task row says of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facts<'a> {
    pub status: &'a str,
    pub created: f64,
    pub updated: f64,
}

/// One status a task held.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub status: String,
    /// When it first held it.
    pub first_entered: f64,
    /// Every visit together; none for a status that closed it.
    pub seconds: f64,
    pub visits: u32,
}

/// One stretch of a task in one status.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub status: String,
    /// When it took the status.
    pub from: f64,
    /// When it left it; none for the status it holds, a closing one included.
    pub to: Option<f64>,
    /// Who moved it there, as the line that did names them; none where no line says (a change
    /// the thread does not record, a block or unblock the board made).
    pub by: Option<String>,
}

impl Segment {
    /// How long it held the status, an open one up to `now`, as its [`Stage`] counts it: a
    /// closing status holds none, though a task reopened by hand left it later.
    pub fn seconds(&self, now: f64) -> f64 {
        if CLOSED.contains(&self.status.as_str()) {
            return 0.0;
        }
        (self.to.unwrap_or(now) - self.from).max(0.0)
    }
}

/// Where a task's time went.
#[derive(Debug, Clone, PartialEq)]
pub struct Stages {
    /// Each status it held, in the order it first held them.
    pub stages: Vec<Stage>,
    /// Each stretch in one status, oldest first: the same time as `stages`, in the order it went.
    pub segments: Vec<Segment>,
    /// From its creation to its first claim; with none, to its close, else to `now`. A part of
    /// the statuses' time, not beside it.
    pub unclaimed_seconds: f64,
    pub first_claimed: Option<f64>,
    /// How often review sent it back to in progress.
    pub bounces: u32,
    pub current: String,
    /// When it took the status it holds.
    pub current_since: f64,
    /// When it was closed, if it is.
    pub closed: Option<f64>,
}

impl Stages {
    pub fn stage(&self, status: &str) -> Option<&Stage> {
        self.stages.iter().find(|s| s.status == status)
    }

    /// From creation to close, for a closed task.
    pub fn lead_seconds(&self, created: f64) -> Option<f64> {
        self.closed.map(|closed| (closed - created).max(0.0))
    }

    /// When the task counts as done: its last close, while it holds done; none for a task open,
    /// cancelled (after done too) or reopened since. A task approved, sent back to review by its
    /// landing and approved again is done once, at the second approve. The one rule of a task done:
    /// `tasks stats` counts its tasks done per day by it, and the roll-up its hours' `done`
    /// ([`crate::reports`]), so the two agree on one board read at one time.
    pub fn done_at(&self) -> Option<f64> {
        self.closed.filter(|_| self.current == "done")
    }
}

/// What a line did.
#[derive(Debug, PartialEq)]
enum Event<'a> {
    Move {
        from: &'a str,
        to: &'a str,
    },
    Claim {
        by: &'a str,
        handed: Vec<&'a str>,
    },
    /// An executor let go.
    Release {
        by: &'a str,
    },
    Block {
        was: Option<&'a str>,
    },
    Unblock {
        to: &'a str,
    },
    /// A `changes` verdict.
    Changes,
    /// Any other verdict; `from` where its text says what it closed.
    Close {
        from: Option<&'static str>,
    },
}

fn status(text: &str) -> Option<&str> {
    STATUSES.contains(&text).then_some(text)
}

/// Whether a verdict's text is this verdict, alone or before its `: text`, as `just tasks verdict`
/// writes it: the one reader of that rule, for the replay here, the reports and the store.
pub fn verdict_is(text: &str, verdict: &str) -> bool {
    text.strip_prefix(verdict)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
}

fn event(line: &Line) -> Option<Event<'_>> {
    let text = line.text.as_str();
    if line.kind == "verdict" {
        return Some(if verdict_is(text, "changes") {
            Event::Changes
        } else if verdict_is(text, "approve") {
            Event::Close {
                from: Some("review"),
            }
        } else if verdict_is(text, "done") {
            Event::Close {
                from: Some("in_progress"),
            }
        } else {
            Event::Close { from: None }
        });
    }
    if line.kind != "system" {
        return None;
    }
    if let Some(rest) = text.strip_prefix(BLOCK_LINE) {
        let was = rest
            .strip_suffix(')')
            .and_then(|rest| rest.rsplit_once(" (was "))
            .and_then(|(_, was)| status(was));
        return Some(Event::Block { was });
    }
    if let Some(to) = unblocked_to(text) {
        return status(to).map(|to| Event::Unblock { to });
    }
    if let Some((_, rest)) = text.split_once(MOVED) {
        let (from, to) = rest.split_once(ARROW)?;
        return Some(Event::Move {
            from: status(from)?,
            to: status(to)?,
        });
    }
    if let Some((by, rest)) = text.split_once(CLAIMED) {
        let handed = match rest.strip_prefix(HANDED) {
            Some(others) => others
                .split(", ")
                .map(|other| other.split_once(" (").map_or(other, |(handle, _)| handle))
                .collect(),
            None if rest.is_empty() => Vec::new(),
            None => return None,
        };
        return Some(Event::Claim { by, handed });
    }
    if let Some((by, rest)) = text.split_once(RELEASED) {
        let role = rest.split_once(" (").map_or(rest, |(role, _)| role);
        return (role == "executor" || role == EVERY_ROLE).then_some(Event::Release { by });
    }
    None
}

/// The status the task was made in: what the first line that names the status it changed says it
/// was, a claim before that line having started ready work; with no such line, the task's own
/// status where a task can be made in it, else ready.
fn made_in<'a>(now_is: &'a str, events: &[Timed<'a>]) -> &'a str {
    let mut claimed = false;
    for timed in events {
        let from = match &timed.event {
            Event::Move { from, .. } => *from,
            Event::Unblock { .. } => "blocked",
            Event::Changes => "review",
            Event::Block { was: Some(from) } | Event::Close { from: Some(from) } => *from,
            // A change from a status its line does not name: no later line says more.
            Event::Block { was: None } | Event::Close { from: None } => break,
            Event::Claim { .. } => {
                claimed = true;
                continue;
            }
            Event::Release { .. } => continue,
        };
        return if claimed && from == "in_progress" {
            "ready"
        } else {
            from
        };
    }
    if BEFORE_WORK.contains(&now_is) {
        now_is
    } else {
        "ready"
    }
}

/// An event, when it counts as having happened, and who its line names as doing it.
struct Timed<'a> {
    at: f64,
    by: Option<&'a str>,
    event: Event<'a>,
}

/// Who `line`, which did `event`, says did it: the mover, the claimer, the releaser or a verdict's
/// sender; nobody for a block or an unblock, which the board writes.
fn actor<'a>(line: &'a Line, event: &Event<'a>) -> Option<&'a str> {
    match event {
        Event::Move { .. } => line.text.split_once(MOVED).map(|(by, _)| by),
        Event::Claim { by, .. } | Event::Release { by } => Some(by),
        Event::Changes | Event::Close { .. } => {
            (!line.sender.is_empty()).then_some(line.sender.as_str())
        }
        Event::Block { .. } | Event::Unblock { .. } => None,
    }
}

struct Replay {
    stages: Vec<Stage>,
    segments: Vec<Segment>,
    current: String,
    since: f64,
    closed: Option<f64>,
}

impl Replay {
    fn visit(&mut self, status: &str, at: f64, by: Option<&str>) {
        match self.stages.iter_mut().find(|s| s.status == status) {
            Some(stage) => stage.visits += 1,
            None => self.stages.push(Stage {
                status: status.into(),
                first_entered: at,
                seconds: 0.0,
                visits: 1,
            }),
        }
        self.segments.push(Segment {
            status: status.into(),
            from: at,
            to: None,
            by: by.map(str::to_string),
        });
    }

    /// The time since the current status was taken, up to `at`, is its own.
    fn spend(&mut self, at: f64) {
        let spent = (at - self.since).max(0.0);
        if let Some(stage) = self.stages.iter_mut().find(|s| s.status == self.current) {
            stage.seconds += spent;
        }
    }

    /// Take `to` at `at`, `by` whom; whether that changed anything.
    fn enter(&mut self, to: &str, at: f64, by: Option<&str>) -> bool {
        if self.closed.is_some() || to == self.current {
            return false;
        }
        self.take(to, at, by);
        true
    }

    fn take(&mut self, to: &str, at: f64, by: Option<&str>) {
        let at = at.max(self.since);
        if self.closed.is_none() {
            self.spend(at);
        }
        if let Some(left) = self.segments.last_mut() {
            left.to = Some(at);
        }
        self.current = to.into();
        self.since = at;
        self.visit(to, at, by);
        self.closed = CLOSED.contains(&to).then_some(at);
    }
}

/// Where the time of the task `facts` describes went, by its thread's `lines` (in any order, other
/// kinds of message among them), an open task counted up to `now`.
pub fn stages(facts: Facts, lines: &[Line], now: f64) -> Stages {
    let mut seen = HashSet::new();
    let mut ordered: Vec<&Line> = lines.iter().filter(|l| seen.insert(l.id)).collect();
    ordered.sort_unstable_by_key(|line| line.id);
    let creator = ordered
        .iter()
        .find(|line| line.kind == "system")
        .and_then(|line| line.text.strip_prefix(CREATED));
    // No line is dated before the task, nor before the line written ahead of it.
    let mut latest = facts.created;
    let events: Vec<Timed> = ordered
        .into_iter()
        .filter_map(|line| {
            let event = event(line)?;
            latest = latest.max(line.ts);
            Some(Timed {
                at: latest,
                by: actor(line, &event),
                event,
            })
        })
        .collect();
    let first = made_in(facts.status, &events);
    let mut replay = Replay {
        stages: Vec::new(),
        segments: Vec::new(),
        current: first.into(),
        since: facts.created,
        closed: None,
    };
    replay.visit(first, facts.created, creator);
    let mut executors: BTreeSet<&str> = BTreeSet::new();
    let (mut first_claimed, mut bounces) = (None, 0);
    for Timed { at, by, event } in &events {
        let (at, by) = (*at, *by);
        if replay.closed.is_some() {
            // A closed task takes nothing but a move out of its close: reopened, it goes on, and
            // what follows counts as before it closed.
            if let Event::Move { from, to } = event
                && CLOSED.contains(from)
                && !CLOSED.contains(to)
            {
                replay.take(to, at, by);
            }
            continue;
        }
        match event {
            Event::Move { from, to } => {
                if replay.enter(to, at, by) && *from == "review" && *to == "in_progress" {
                    bounces += 1;
                }
            }
            Event::Claim {
                by: claimer,
                handed,
            } => {
                for other in handed {
                    executors.remove(other);
                }
                executors.insert(*claimer);
                first_claimed.get_or_insert(at);
                if replay.current == "ready" {
                    replay.enter("in_progress", at, by);
                }
            }
            Event::Release { by: releaser } => {
                if executors.remove(releaser)
                    && executors.is_empty()
                    && replay.current == "in_progress"
                {
                    replay.enter("ready", at, by);
                }
            }
            Event::Block { .. } => {
                replay.enter("blocked", at, by);
            }
            Event::Unblock { to } => {
                replay.enter(to, at, by);
            }
            Event::Changes => {
                if replay.enter("in_progress", at, by) {
                    bounces += 1;
                }
            }
            Event::Close { .. } => {
                replay.enter("done", at, by);
            }
        }
    }
    if replay.current != facts.status {
        replay.take(facts.status, facts.updated, None);
    }
    if replay.closed.is_none() {
        replay.spend(now);
    }
    let waited_until = first_claimed.or(replay.closed).unwrap_or(now);
    Stages {
        stages: replay.stages,
        segments: replay.segments,
        unclaimed_seconds: (waited_until - facts.created).max(0.0),
        first_claimed,
        bounces,
        current: replay.current,
        current_since: replay.since,
        closed: replay.closed,
    }
}

/// When the task `facts` describe took the status it holds, by its thread's `lines`: the replay's
/// [`Stages::current_since`], which no clock moves. For a closed task it is its close; where the
/// thread does not record the change to the status it holds, its `updated`.
pub fn in_status_since(facts: Facts, lines: &[Line]) -> f64 {
    in_status(facts, lines).since
}

/// A task's status entry, previous status and review-round count, by the replay
/// ([`in_status`]): what the board's compact cards read of its thread.
#[derive(Debug, Clone, PartialEq)]
pub struct InStatus {
    /// One round per replayed stretch in review, including the current one, as the task timeline.
    pub review_rounds: u32,
    /// [`Stages::current_since`].
    pub since: f64,
    /// The status of the stretch before the one it holds; none for a task in the status it was
    /// made in. For a blocked task, the status it was blocked from.
    pub from: Option<String>,
}

/// [`in_status_since`], with the previous status and count of replayed review stretches.
pub fn in_status(facts: Facts, lines: &[Line]) -> InStatus {
    let replayed = stages(facts, lines, facts.updated);
    let count = replayed.segments.len();
    InStatus {
        review_rounds: u32::try_from(
            replayed
                .segments
                .iter()
                .filter(|s| s.status == "review")
                .count(),
        )
        .unwrap_or(u32::MAX),
        since: replayed.current_since,
        from: (count >= 2).then(|| replayed.segments[count - 2].status.clone()),
    }
}

/// One session's hold on the executor or reviewer role of a task.
#[derive(Debug, Clone, PartialEq)]
pub struct Hold {
    pub handle: String,
    /// `executor` or `reviewer`.
    pub role: String,
    pub from: f64,
    /// When it let go, or another took over from it; none while it holds the role.
    pub to: Option<f64>,
}

/// Who held the executor and reviewer roles of the task made at `created`, by its thread's
/// `lines`, oldest first: a claim starts an executor's hold and ends those it took over from, a
/// join as reviewer starts a reviewer's, and a release of the role or of every role ends it. Lines
/// are read as [`stages`] reads them: in the order written, none dated before the one ahead of it.
pub fn holds(lines: &[Line], created: f64) -> Vec<Hold> {
    let mut seen = HashSet::new();
    let mut ordered: Vec<&Line> = lines.iter().filter(|l| seen.insert(l.id)).collect();
    ordered.sort_unstable_by_key(|line| line.id);
    let mut found: Vec<Hold> = Vec::new();
    let end = |found: &mut Vec<Hold>, handle: &str, role: Option<&str>, at: f64| {
        for hold in found.iter_mut().filter(|hold| {
            hold.to.is_none() && hold.handle == handle && role.is_none_or(|role| hold.role == role)
        }) {
            hold.to = Some(at);
        }
    };
    let start = |found: &mut Vec<Hold>, handle: &str, role: &str, at: f64| {
        if !found
            .iter()
            .any(|hold| hold.to.is_none() && hold.handle == handle && hold.role == role)
        {
            found.push(Hold {
                handle: handle.into(),
                role: role.into(),
                from: at,
                to: None,
            });
        }
    };
    let mut latest = created;
    for line in ordered {
        let system = (line.kind == "system").then_some(line.text.as_str());
        let joined = system
            .and_then(|text| text.split_once(JOINED))
            .filter(|(_, role)| *role == "reviewer");
        let released = system
            .and_then(|text| text.split_once(RELEASED))
            .map(|(by, rest)| (by, rest.split_once(" (").map_or(rest, |(role, _)| role)));
        let read = event(line);
        if read.is_none() && joined.is_none() && released.is_none() {
            continue;
        }
        // Dated as the replay dates it: no earlier than any line it reads ahead of it.
        latest = latest.max(line.ts);
        let claim = match read {
            Some(Event::Claim { by, handed }) => Some((by, handed)),
            _ => None,
        };
        if let Some((by, handed)) = claim {
            for other in handed {
                end(&mut found, other, Some("executor"), latest);
            }
            start(&mut found, by, "executor", latest);
        } else if let Some((by, role)) = joined {
            start(&mut found, by, role, latest);
        } else if let Some((by, role)) = released {
            match role {
                EVERY_ROLE => end(&mut found, by, None, latest),
                "executor" | "reviewer" => end(&mut found, by, Some(role), latest),
                _ => {}
            }
        }
    }
    found
}

/// `seconds` as hours to one decimal: `1.3 h`.
pub fn hours(seconds: f64) -> String {
    format!("{:.1} h", seconds / 3600.0)
}

/// One line of where the time went: each open status held and the bounces if any, then, apart
/// from them since it is a part of their time, the wait before the first claim.
pub fn summary_line(stages: &Stages) -> String {
    let mut parts = Vec::new();
    for stage in &stages.stages {
        if CLOSED.contains(&stage.status.as_str()) {
            continue;
        }
        let visits = if stage.visits > 1 {
            format!(" ({} visits)", stage.visits)
        } else {
            String::new()
        };
        parts.push(format!("{} {}{visits}", stage.status, hours(stage.seconds)));
    }
    match stages.bounces {
        0 => {}
        1 => parts.push("1 bounce".into()),
        n => parts.push(format!("{n} bounces")),
    }
    let waited = hours(stages.unclaimed_seconds);
    let claim = if stages.first_claimed.is_some() {
        format!("first claimed after {waited}")
    } else {
        format!("never claimed in {waited}")
    };
    format!("Stages: {}; {claim}", parts.join(" · "))
}
