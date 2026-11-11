//! Where a task's time went, read back from what the board already keeps: the task's `created`,
//! `updated` and `status`, and the lines its status changes left on its thread. Nothing here reads
//! the board or the clock: [`stages`] is given the lines and the time to count an open task up to.
//!
//! The lines that change a status, each written by the store through the names here so the two
//! cannot drift:
//!
//! - `HANDLE moved it FROM → TO` ([`moved_line`]): a move, the owner's too.
//! - `HANDLE claimed it[, handed over from OTHER (HEALTH), …]` ([`claimed_line`]): ready work
//!   starts; a claim of work in any other status changes no status.
//! - `HANDLE released ROLE (released)` and `HANDLE released every role (left)` ([`RELEASED`]): work
//!   in progress whose last executor let go is ready again.
//! - `blocked: cancelled dependencies: … (was STATUS)` ([`BLOCK_LINE`]) and
//!   `unblocked: no cancelled dependency left; back to STATUS` ([`UNBLOCK_LINE`]).
//! - a `verdict` message: `changes…` sends a task in review back to in progress; any other
//!   (`approve…`, a subtask's `done: …`, a landing) closes it.
//!
//! A thread is replayed in the order of its times, then its ids. A line that names the status the
//! task already holds changes nothing (a duplicate), and nothing after the line that closed the
//! task is read. Where the replay ends in another status than the task's own, a change the thread
//! does not record (a line lost, a board copied without its thread) is dated at the task's
//! `updated`.

use crate::{BEFORE_WORK, STATUSES};
use std::collections::{BTreeSet, HashSet};

pub mod stats;
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
/// What a release of every role names in place of one.
pub const EVERY_ROLE: &str = "every role";
/// What starts the line a cancelled prerequisite's block puts on its dependent's thread.
pub const BLOCK_LINE: &str = "blocked: cancelled dependencies: ";
/// What starts the line that gives a task so blocked its status back, before that status.
pub const UNBLOCK_LINE: &str = "unblocked: no cancelled dependency left; back to ";

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
}

impl From<&crate::Message> for Line {
    fn from(m: &crate::Message) -> Line {
        Line {
            id: m.id,
            kind: m.kind.clone(),
            text: m.text.clone(),
            ts: m.ts,
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

/// Where a task's time went.
#[derive(Debug, Clone, PartialEq)]
pub struct Stages {
    /// Each status it held, in the order it first held them.
    pub stages: Vec<Stage>,
    /// From its creation to its first claim; with none, to its close, else to `now`.
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

/// Whether a verdict's text is this verdict, alone or before its `: text`.
fn verdict_is(text: &str, verdict: &str) -> bool {
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
    if let Some(rest) = text.strip_prefix(UNBLOCK_LINE) {
        let to = rest.split_once(' ').map_or(rest, |(to, _)| to);
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
fn made_in<'a>(now_is: &'a str, events: &[(f64, Event<'a>)]) -> &'a str {
    let mut claimed = false;
    for (_, event) in events {
        let from = match event {
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

struct Replay {
    stages: Vec<Stage>,
    current: String,
    since: f64,
    closed: Option<f64>,
}

impl Replay {
    fn visit(&mut self, status: &str, at: f64) {
        match self.stages.iter_mut().find(|s| s.status == status) {
            Some(stage) => stage.visits += 1,
            None => self.stages.push(Stage {
                status: status.into(),
                first_entered: at,
                seconds: 0.0,
                visits: 1,
            }),
        }
    }

    /// The time since the current status was taken, up to `at`, is its own.
    fn spend(&mut self, at: f64) {
        let spent = (at - self.since).max(0.0);
        if let Some(stage) = self.stages.iter_mut().find(|s| s.status == self.current) {
            stage.seconds += spent;
        }
    }

    /// Take `to` at `at`; whether that changed anything.
    fn enter(&mut self, to: &str, at: f64) -> bool {
        if self.closed.is_some() || to == self.current {
            return false;
        }
        self.take(to, at);
        true
    }

    fn take(&mut self, to: &str, at: f64) {
        let at = at.max(self.since);
        if self.closed.is_none() {
            self.spend(at);
        }
        self.current = to.into();
        self.since = at;
        self.visit(to, at);
        self.closed = CLOSED.contains(&to).then_some(at);
    }
}

/// Where the time of the task `facts` describes went, by its thread's `lines` (in any order, other
/// kinds of message among them), an open task counted up to `now`.
pub fn stages(facts: Facts, lines: &[Line], now: f64) -> Stages {
    let mut seen = HashSet::new();
    let mut ordered: Vec<&Line> = lines.iter().filter(|l| seen.insert(l.id)).collect();
    ordered.sort_by(|a, b| a.ts.total_cmp(&b.ts).then(a.id.cmp(&b.id)));
    let events: Vec<(f64, Event)> = ordered
        .into_iter()
        .filter_map(|line| Some((line.ts.max(facts.created), event(line)?)))
        .collect();
    let first = made_in(facts.status, &events);
    let mut replay = Replay {
        stages: Vec::new(),
        current: first.into(),
        since: facts.created,
        closed: None,
    };
    replay.visit(first, facts.created);
    let mut executors: BTreeSet<&str> = BTreeSet::new();
    let (mut first_claimed, mut bounces) = (None, 0);
    for (at, event) in &events {
        if replay.closed.is_some() {
            break;
        }
        match event {
            Event::Move { from, to } => {
                if replay.enter(to, *at) && *from == "review" && *to == "in_progress" {
                    bounces += 1;
                }
            }
            Event::Claim { by, handed } => {
                for other in handed {
                    executors.remove(other);
                }
                executors.insert(*by);
                first_claimed.get_or_insert(*at);
                if replay.current == "ready" {
                    replay.enter("in_progress", *at);
                }
            }
            Event::Release { by } => {
                if executors.remove(by) && executors.is_empty() && replay.current == "in_progress" {
                    replay.enter("ready", *at);
                }
            }
            Event::Block { .. } => {
                replay.enter("blocked", *at);
            }
            Event::Unblock { to } => {
                replay.enter(to, *at);
            }
            Event::Changes => {
                if replay.enter("in_progress", *at) {
                    bounces += 1;
                }
            }
            Event::Close { .. } => {
                replay.enter("done", *at);
            }
        }
    }
    if replay.current != facts.status {
        replay.take(facts.status, facts.updated);
    }
    if replay.closed.is_none() {
        replay.spend(now);
    }
    let waited_until = first_claimed.or(replay.closed).unwrap_or(now);
    Stages {
        stages: replay.stages,
        unclaimed_seconds: (waited_until - facts.created).max(0.0),
        first_claimed,
        bounces,
        current: replay.current,
        current_since: replay.since,
        closed: replay.closed,
    }
}

/// `seconds` as hours to one decimal: `1.3 h`.
pub fn hours(seconds: f64) -> String {
    format!("{:.1} h", seconds / 3600.0)
}

/// One line of where the time went: the wait before the first claim, each open status held, and
/// the bounces if any.
pub fn summary_line(stages: &Stages) -> String {
    let mut parts = vec![format!("unclaimed {}", hours(stages.unclaimed_seconds))];
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
    format!("Stages: {}", parts.join(" · "))
}
