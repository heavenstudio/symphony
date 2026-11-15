//! The PULL digest: work on the board that no event announces.
//!
//! The orchestrator's monitor (`just tasks-attach --kind orchestrator`) hears messages, verdicts and
//! status changes. It does not hear that a ready task has nobody on it, that an in-progress task's
//! executor is gone, that a review has no reviewer, or that a branch landed under a task still open.
//! This looks at the board for exactly those, and prints one header line and the first six of each:
//!
//! ```text
//! PULL 14:05 executing=3 red_unstaffed=1 ready_unstaffed=2 in_progress_orphaned=1 review_unreviewed=0 landed_open=1 idle=2
//!   red_unstaffed p0 tests/main-red-abc — Main red: static/check
//!   ready_unstaffed p1 sim/jump-departure — Jump departure
//!   in_progress_orphaned p2 view/hud — HUD scale
//!   landed_open p2 sim/x — X   (landed on main, task still review)
//!   idle calm-vole
//! ```
//!
//! A task is **staffed** when it, or any unfinished task under it, has an executor whose session is live
//! or stale (so an agent working a subtask `X/do` staffs `X`). A ready task also needs its dependencies
//! done to be offered at all; cancelled prerequisites are listed as `dependency_blocked` with a
//! reason instead. A red task ([`crate::red`]) nobody executes [`UNSTAFFED_SECONDS`] after it was
//! filed, whatever its status short of review, is `red_unstaffed`, named first and nowhere else, a
//! cancelled dependency of its in its note, and counted in the header only then, as
//! `dependency_blocked` is. The monitor delivers it only for newly actionable entries.
//!
//! While the pipeline alarm stands ([`crate::alarm`]: review holding more than twice the work in
//! progress for 4 hours, by default), the header ends `review_alarm=REVIEW/IN_PROGRESS`, and the
//! lines after it say since when and name the [`ALARM_SHOWN`] tasks longest in review, each with
//! what its landing waits on where the landing tool records it ([`crate::alarm::Waits`]), every
//! orchestrator hearing the whole board's; its raising is an entry of its own, so it is delivered:
//!
//! ```text
//! PULL 14:05 executing=4 ready_unstaffed=2 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0 review_alarm=10/4
//!   review_alarm: review 10, in progress 4, more than 2× since 09:40
//!   review_alarm p1 tools/x — X   (in review since 11-07 21:12; missing certificate, review)
//! ```
//!
//! A task in review whose record says a reviewer asked changes of the branch's tip now is rework:
//! the digest and the alarm count it in progress, its live executor as executing, and one nobody
//! executes as orphaned ([`REWORK_NOTE`]). One whose record judged the tip now, as the repository
//! tells it, and names neither `review` nor `changes` waits on no reviewer: the project's own review
//! gate, as its landing keeps it, is met or not needed there and replaces the Symphony's, so it is
//! not `review_unreviewed`. A record of another tip or of a tip the repository does not tell, or
//! none, leaves the Symphony's: a review with no reviewer is listed.
//!
//! A recipient told a digest before hears the next one as what changed since the last one it
//! received ([`Digest::changes_since`], against what that one said, [`Said`]): the header's counts
//! that moved, the rows added and the rows resolved. `tasks pull --as HANDLE` prints the whole
//! digest ([`Digest::lines`]) on demand.

mod pipeline;

use crate::alarm::{Alarm, Thresholds, Tips};
use crate::health::{EXECUTING, Health};
use crate::orchestration::{Owners, claimable_work};
use crate::red::UNSTAFFED_SECONDS;
use crate::store::unanswered::Pending;
use crate::sys::local_minute;
use crate::{Board, Error, Session, Store, Task};
pub(crate) use pipeline::Observation;
pub use pipeline::digest;
use pipeline::{Fixes, Judged, review_alarm, waiting};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// How many of a category are listed.
pub const SHOWN: usize = 6;
/// The pipeline alarm's name: its header count, its lines and its entry.
pub const ALARM: &str = "review_alarm";
/// How many of the tasks longest in review the alarm names.
pub const ALARM_SHOWN: usize = 5;
/// The note on a task in review whose tip a reviewer asked changes of, counted as in progress
/// ([`crate::alarm::CHANGES`]), when nobody executes it.
pub const REWORK_NOTE: &str = "in review; changes asked of its tip";
/// A count a PULL could not say, as its recipient's next one is measured against.
pub const UNKNOWN: &str = "unknown";
/// One successful canonical observation, retained across scheduling and recipient scoping.
pub(crate) type Holdings = BTreeMap<String, BTreeSet<String>>;
/// The categories, in the order they are listed; `idle` holds handles, the others tasks.
pub const CATEGORIES: [&str; 8] = [
    "red_unstaffed",
    "ready_unstaffed",
    "dependency_blocked",
    "in_progress_orphaned",
    "review_unreviewed",
    "unanswered",
    "landed_open",
    "idle",
];

/// What one PULL said, as its recipient's next one is measured against: every count of its header
/// (those the header leaves out at zero too), the entries it named, shown or only counted, and the
/// sessions whose liveness it could not tell.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Said {
    pub counts: BTreeMap<String, String>,
    pub entries: BTreeSet<(String, String)>,
    pub liveness_unknown: Vec<String>,
}

/// One look at the board.
#[derive(Debug, Clone)]
pub struct Digest {
    pub now: f64,
    /// In-progress tasks with an executor present.
    pub executing: usize,
    /// The slugs `executing` counts, for [`Digest::only_for`].
    running: Vec<String>,
    tasks: BTreeMap<&'static str, Vec<Task>>,
    /// Handles the agents API reports idle.
    pub idle: Vec<String>,
    /// Slug -> a short remark: who reserved it, that it landed.
    pub notes: BTreeMap<String, String>,
    /// Work retained because its shared native presence signal expired.
    pub liveness_unknown: Vec<String>,
    unanswered: Vec<Pending>,
    /// Whether the unanswered-post scan answered: a look whose scan failed cannot say which posts
    /// wait, nor how many ([`Digest::said_after`]).
    unanswered_scanned: bool,
    /// The cap on an orchestrator's open tasks the board holds starts to ([`crate::slots`]).
    pub cap: Option<usize>,
    /// The open slots the orchestrator the digest is for holds, under a cap ([`Digest::only_for`]).
    pub slots: Option<usize>,
    /// An orchestrator below this many open slots should look for disjoint work or owned blockers
    /// (`[limits] low_volume`, [`crate::limits`]); 0 tells none.
    pub low_volume: usize,
    /// The pipeline alarm, while it stands: the whole board's, whoever the digest is for.
    pub alarm: Option<ReviewAlarm>,
    /// Canonical counts for active orchestrators; only the recipient remains after scoping.
    volume: BTreeMap<String, usize>,
    scoped: bool,
}

/// The pipeline alarm as the digest says it ([`crate::alarm`]).
#[derive(Debug, Clone)]
pub struct ReviewAlarm {
    pub alarm: Alarm,
    pub thresholds: Thresholds,
    /// The work longest in review, longest first.
    pub longest: Vec<InReview>,
}

/// A task the alarm names among the longest in review.
#[derive(Debug, Clone)]
pub struct InReview {
    pub task: Task,
    /// When it went in.
    pub since: f64,
    /// What its landing waits on, where that is recorded ([`crate::alarm::Waits`]).
    pub note: Option<String>,
}

impl ReviewAlarm {
    /// The line after the header: the counts and since when review has held more than the ratio
    /// allows, or, in the hour that clears it, no more.
    fn line(&self, now: f64) -> String {
        let Alarm {
            counts,
            over,
            since,
            ..
        } = self.alarm;
        let ratio = self.thresholds.ratio;
        let said = if over {
            format!("more than {ratio}× since {}", when(since, now))
        } else {
            format!(
                "at most {ratio}× since {}; it clears after {} h of that",
                when(since, now),
                self.thresholds.clear_hours
            )
        };
        format!(
            "  {ALARM}: review {}, in progress {}, {said}",
            counts.review, counts.in_progress
        )
    }
}

/// A time on a digest line: its minute, and its day too when it is not `now`'s.
fn when(at: f64, now: f64) -> String {
    let (at, now) = (local_minute(at), local_minute(now));
    if at[..10] == now[..10] {
        at[11..].to_string()
    } else {
        at[5..].to_string()
    }
}

impl Digest {
    /// Actionable identities, independent of clocks, counts or titles.
    pub fn entries(&self) -> BTreeSet<(String, String)> {
        let alarm = (self.alarm.iter()).map(|_| (ALARM.to_string(), "review".to_string()));
        let mut entries: BTreeSet<_> = CATEGORIES
            .iter()
            .flat_map(|name| {
                let entries: Vec<String> = if *name == "idle" {
                    self.idle.clone()
                } else if *name == "unanswered" {
                    self.unanswered.iter().map(pending_key).collect()
                } else {
                    self.items(name).iter().map(|t| t.slug.clone()).collect()
                };
                entries
                    .into_iter()
                    .map(move |entry| (name.to_string(), entry))
            })
            .chain(alarm)
            .collect();
        for (handle, count) in &self.volume {
            if *count < self.low_volume {
                entries.insert(("low_volume".into(), handle.clone()));
            }
        }
        entries
    }
    /// Stable event content, independent of when the board was observed.
    pub fn event_lines(&self) -> Vec<String> {
        self.event_lines_since(&BTreeSet::new())
    }

    /// Pending-post lines are emitted once per post, including when another category changes.
    pub fn event_lines_since(&self, seen: &BTreeSet<(String, String)>) -> Vec<String> {
        let mut lines = self.lines();
        lines[0] = format!("PULL {}", lines[0].splitn(3, ' ').nth(2).unwrap_or(""));
        lines.retain(|line| !line.starts_with("  unanswered "));
        if self
            .volume
            .keys()
            .any(|handle| seen.contains(&("low_volume".into(), handle.clone())))
        {
            lines.retain(|line| !line.starts_with("  low_volume "));
        }
        lines.extend(self.unanswered_lines(seen));
        lines
    }

    /// What this digest says, for the next one to be measured against ([`Digest::changes_since`]).
    pub fn said(&self) -> Said {
        Said {
            counts: (self.counts().into_iter())
                .map(|(name, value, _)| (name.to_string(), value))
                .collect(),
            entries: self.entries(),
            liveness_unknown: self.liveness_unknown.clone(),
        }
    }

    /// What its recipient knows once it acknowledges this digest's PULL after the one that said
    /// `before`: what it says, but for unanswered posts when its scan failed, since it says nothing
    /// of them. Their count is then [`UNKNOWN`], so the next look whose scan answers says it, and
    /// the posts `before` named stay named, so that look resolves those gone.
    pub fn said_after(&self, before: Option<&Said>) -> Said {
        let mut said = self.said();
        if !self.unanswered_scanned {
            said.counts.insert("unanswered".into(), UNKNOWN.into());
            let named = before.into_iter().flat_map(|before| &before.entries);
            said.entries
                .extend(named.filter(|(name, _)| name == "unanswered").cloned());
        }
        said
    }

    /// The PULL of what changed since the one that said `said`, to a recipient told `told` of the
    /// entries the digest names now: its header names each count that moved, `name=now (was
    /// then)`, or says `counts unchanged`; the liveness line follows when the sessions it names
    /// changed; then each entry it was
    /// not told, as the whole digest lists it (the first [`SHOWN`] of a list), and one `resolved`
    /// line a list for the entries `said` named that are gone. Nothing else is said again. A look
    /// whose unanswered-post scan failed says nothing of those posts: neither their count nor any
    /// of them resolved.
    pub fn changes_since(&self, said: &Said, told: &BTreeSet<(String, String)>) -> Vec<String> {
        let known = |name: &str| self.unanswered_scanned || name != "unanswered";
        let counts: Vec<_> = (self.counts().into_iter())
            .filter(|(name, _, _)| known(name))
            .collect();
        let mut moved: Vec<String> = (counts.iter())
            .filter(|(name, value, _)| said.counts.get(*name) != Some(value))
            .map(|(name, value, _)| {
                let was = said.counts.get(*name).map_or("none", String::as_str);
                format!("{name}={value} (was {was})")
            })
            .collect();
        moved.extend(
            (said.counts.iter())
                .filter(|(name, _)| known(name) && !counts.iter().any(|(now, _, _)| now == name))
                .map(|(name, was)| format!("{name}=none (was {was})")),
        );
        if moved.is_empty() {
            moved.push("counts unchanged".into());
        }
        let mut out = vec![format!("PULL {}", moved.join(" "))];
        if self.liveness_unknown != said.liveness_unknown {
            out.push(match self.liveness_line() {
                Some(line) => line,
                None => "  liveness unknown: resolved".into(),
            });
        }
        let untold = |entry: (&str, &str)| !told.contains(&(entry.0.into(), entry.1.into()));
        if untold((ALARM, "review")) {
            out.extend(self.alarm_lines());
        }
        out.extend(
            self.low_volume_lines()
                .into_iter()
                .filter(|(handle, _)| untold(("low_volume", handle)))
                .map(|(_, line)| line),
        );
        let now = self.entries();
        for name in CATEGORIES {
            let added: Vec<String> = match name {
                "idle" => (self.idle.iter())
                    .filter(|handle| untold((name, handle)))
                    .map(|handle| format!("  idle {handle}"))
                    .collect(),
                "unanswered" => self.unanswered_lines(told),
                _ => (self.items(name).iter())
                    .filter(|task| untold((name, &task.slug)))
                    .map(|task| self.task_line(name, task))
                    .collect(),
            };
            out.extend(shown(name, added));
        }
        for name in CATEGORIES
            .iter()
            .chain(&["low_volume", ALARM])
            .filter(|name| known(name))
        {
            let resolved: Vec<&str> = (said.entries.iter())
                .filter(|entry| entry.0 == *name && !now.contains(*entry))
                .map(|(_, named)| named.as_str())
                .collect();
            if !resolved.is_empty() {
                out.push(format!("  resolved {name} {}", resolved.join(", ")));
            }
        }
        out
    }

    /// The header's counts in order, each with whether the whole digest's header shows it: it keeps
    /// the usual digest compact, and cancellation blockers, unanswered posts and unstaffed reds get
    /// a count only when present.
    fn counts(&self) -> Vec<(&'static str, String, bool)> {
        let mut counts: Vec<(&'static str, String, bool)> = ["executing"]
            .iter()
            .chain(CATEGORIES.iter())
            .map(|name| {
                let compact =
                    matches!(*name, "dependency_blocked" | "unanswered" | "red_unstaffed");
                let count = self.count(name);
                (*name, count.to_string(), !compact || count > 0)
            })
            .collect();
        if let (Some(held), Some(cap)) = (self.slots, self.cap) {
            counts.push(("slots", format!("{held}/{cap}"), true));
        }
        if let Some(raised) = &self.alarm {
            let now = raised.alarm.counts;
            let value = format!("{}/{}", now.review, now.in_progress);
            counts.push((ALARM, value, true));
        }
        counts
    }

    /// The standing pipeline alarm: since when, then the tasks longest in review.
    fn alarm_lines(&self) -> Vec<String> {
        let Some(raised) = &self.alarm else {
            return Vec::new();
        };
        let mut out = vec![raised.line(self.now)];
        for InReview { task, since, note } in &raised.longest {
            let note = note.as_ref().map(|n| format!("; {n}")).unwrap_or_default();
            out.push(format!(
                "  {ALARM} p{} {} — {}   (in review since {}{note})",
                task.priority,
                task.slug,
                task.title,
                when(*since, self.now)
            ));
        }
        out
    }

    fn liveness_line(&self) -> Option<String> {
        (!self.liveness_unknown.is_empty()).then(|| {
            format!(
                "  liveness unknown: native presence sweep overdue or failed; work remains held: {}",
                self.liveness_unknown.join(", ")
            )
        })
    }

    /// The scoped digest's low-volume condition, a line for each handle it names.
    fn low_volume_lines(&self) -> Vec<(String, String)> {
        let cap = (self.cap).map_or_else(|| "none".into(), |cap| cap.to_string());
        (self.volume.iter())
            .filter(|(_, count)| self.scoped && **count < self.low_volume)
            .map(|(handle, count)| {
                (handle.clone(), format!("  low_volume {handle} open={count} threshold={} cap={cap} — pick disjoint ready work or resolve owned blockers using actual resources and ownership", self.low_volume))
            })
            .collect()
    }

    fn task_line(&self, name: &str, task: &Task) -> String {
        let note = (self.notes.get(&task.slug))
            .map(|n| format!("   ({n})"))
            .unwrap_or_default();
        format!(
            "  {name} p{} {} — {}{note}",
            task.priority, task.slug, task.title
        )
    }

    fn unanswered_lines(&self, seen: &BTreeSet<(String, String)>) -> Vec<String> {
        let mut by_post: BTreeMap<(String, i64), Vec<String>> = BTreeMap::new();
        for pending in &self.unanswered {
            if !seen.contains(&("unanswered".into(), pending_key(pending))) {
                by_post
                    .entry((pending.slug.clone(), pending.post.id))
                    .or_default()
                    .push(format!("{}: {}", pending.handle, pending.command()));
            }
        }
        by_post
            .into_iter()
            .map(|((slug, id), commands)| {
                format!("  unanswered {slug} post #{id} — {}", commands.join("; "))
            })
            .collect()
    }
    /// How many there are in `name`, `executing` or a category.
    pub fn count(&self, name: &str) -> usize {
        match name {
            "executing" => self.executing,
            "idle" => self.idle.len(),
            _ => self.items(name).len(),
        }
    }

    /// The tasks in category `name` (none for `idle`, which holds handles).
    pub fn items(&self, name: &str) -> &[Task] {
        self.tasks.get(name).map_or(&[], Vec::as_slice)
    }

    /// The digest as orchestrator `handle` hears it: the rows of its own tasks and of unclaimed
    /// ones ([`Owners::reaches`]), and the idle sessions but the children of another orchestrator
    /// still here. Staffing was judged on the whole board, so work on a subtask still staffs its
    /// parent.
    pub fn only_for(mut self, board: &Board, handle: &str) -> Digest {
        let owners = Owners::new(&board.tasks);
        for listed in self.tasks.values_mut() {
            listed.retain(|task| owners.reaches(&task.slug, handle));
        }
        self.tasks.retain(|_, listed| !listed.is_empty());
        self.unanswered.retain(|p| owners.reaches(&p.slug, handle));
        self.running.retain(|slug| owners.reaches(slug, handle));
        self.executing = self.running.len();
        self.notes.retain(|slug, _| owners.reaches(slug, handle));
        let sessions: HashMap<&str, &Session> = (board.sessions.iter())
            .map(|s| (s.row.handle.as_str(), s))
            .collect();
        let elsewhere = |parent: &str| {
            let orchestrator = |p: &&Session| p.row.kind == "orchestrator" && p.health.holds();
            parent != handle && sessions.get(parent).is_some_and(orchestrator)
        };
        self.idle.retain(|idle| {
            let parent = sessions
                .get(idle.as_str())
                .and_then(|s| s.row.parent.as_deref());
            !parent.is_some_and(elsewhere)
        });
        let work = claimable_work(board);
        let heard = |held: &Vec<&Task>| held.iter().any(|t| owners.reaches(&t.slug, handle));
        self.liveness_unknown
            .retain(|quiet| work.get(quiet.as_str()).is_some_and(heard));
        // The condition and global entries were captured together. The caller's ownership
        // snapshot may be older; it scopes ordinary rows but must not replace this observation.
        if self.cap.is_some() {
            self.slots = self.volume.get(handle).copied();
        }
        self.volume.retain(|recipient, _| recipient == handle);
        self.scoped = true;
        self
    }

    pub fn lines(&self) -> Vec<String> {
        let counts: Vec<String> = (self.counts().into_iter())
            .filter(|(_, _, shown)| *shown)
            .map(|(name, value, _)| format!("{name}={value}"))
            .collect();
        let mut out = vec![format!(
            "PULL {} {}",
            &local_minute(self.now)[11..],
            counts.join(" ")
        )];
        out.extend(self.alarm_lines());
        out.extend(self.liveness_line());
        out.extend(self.low_volume_lines().into_iter().map(|(_, line)| line));
        for name in CATEGORIES {
            let listed: Vec<String> = if name == "idle" {
                self.idle.iter().map(|h| format!("  idle {h}")).collect()
            } else if name == "unanswered" {
                self.unanswered_lines(&BTreeSet::new())
            } else {
                (self.items(name).iter())
                    .map(|t| self.task_line(name, t))
                    .collect()
            };
            out.extend(shown(name, listed));
        }
        out
    }
}

/// The first [`SHOWN`] lines of list `name`, and how many more it holds; every unanswered post, each
/// with the command that answers it, since a post a PULL tells is never told again.
fn shown(name: &str, mut listed: Vec<String>) -> Vec<String> {
    if listed.len() > SHOWN && name != "unanswered" {
        let more = listed.len() - SHOWN;
        listed.truncate(SHOWN);
        listed.push(format!("  {name} … and {more} more"));
    }
    listed
}

/// Look at the board. `idle`: handles the agents API reports idle; `landed(task)`: whether the task's
/// branch is already on main (a git question the caller answers; None skips it, and a `landed <slug>
/// on main` line on the task's own thread says it too); `tips(task, record)`: its branch's tip now,
/// or the first commit it moved to from the commit a record judged after the record was written
/// ([`Tips`], another; None takes what a task's landing waits on as judged on its tip,
/// [`crate::alarm::Waits`]); `verifying`: handles the agents API reports
/// running a test, who are present however quiet the board says they are.
///
/// A task in review whose tip stands judged to need changes ([`crate::alarm::CHANGES`]) that its
/// branch has not answered since the record (moved past that tip, though it came back) is work in
/// progress: listed and counted as one, its executor at work, here and in the alarm alike.
pub fn digest_with_tips(
    store: &Store,
    idle: &[String],
    landed: Option<&dyn Fn(&Task) -> bool>,
    tips: Option<Tips<'_>>,
    now: f64,
    verifying: &[String],
) -> Result<Digest, Error> {
    let board = store.board()?;
    let holdings = store.slot_holdings()?;
    let observed = Observation {
        board: &board,
        holdings: &holdings,
    };
    digest_on(store, observed, idle, landed, tips, now, verifying)
}

/// The digest of `board` with the idle and verifying agents `view` reports (none without one).
pub(crate) fn viewed(
    store: &Store,
    observed: Observation<'_>,
    view: Option<&crate::cli::agents::View>,
    told: &BTreeMap<String, Health>,
    landed: Option<&dyn Fn(&Task) -> bool>,
    tips: Option<Tips<'_>>,
    now: f64,
) -> Result<Digest, Error> {
    let (idle, busy) = idle_and_busy(view, told);
    digest_on(store, observed, &idle, landed, tips, now, &busy)
}

/// The agents of `view` a PULL lists idle, and those running a test, which count as present
/// however quiet the board says they are. Orchestrators and the chat are never idle rows, nor is a
/// session the recipient is told went quiet (`told`, the sessions its look follows and their
/// health): the view calls one with no terminal idle once its board health is, and its STALE, LOST
/// or LIVENESS UNKNOWN line says it. The dispatch, the legacy monitor and `tasks pull` all read
/// idleness here, so they agree.
pub(crate) fn idle_and_busy(
    view: Option<&crate::cli::agents::View>,
    told: &BTreeMap<String, Health>,
) -> (Vec<String>, Vec<String>) {
    let (mut idle, mut busy) = (Vec::new(), Vec::new());
    for a in view.map_or(&[][..], |v| v.agents.as_slice()) {
        if a.state == "running_test" {
            busy.push(a.handle.clone());
        } else if a.state == "idle"
            && !matches!(a.kind.as_str(), "orchestrator" | "chat")
            && !told.get(&a.handle).is_some_and(|health| health.quiet())
        {
            idle.push(a.handle.clone());
        }
    }
    (idle, busy)
}

/// The whole digest as `handle` hears it now, read on demand (`tasks pull`): the agents view the
/// machine's probes make, and the branches on main in the checkout `env` runs in.
pub fn heard_now(store: &Store, env: &crate::cli::Env, handle: &str) -> Result<Digest, Error> {
    let mut board = store.board()?;
    // A PULL gives what no orchestrator holds first, when one that stays could take it.
    if store.give_if_due(&board) {
        board = store.board()?;
    }
    let holdings = store.slot_holdings()?;
    let probes = crate::cli::probes::machine(env);
    let now = store.now();
    let view = crate::cli::agents::describe_board(store, &probes, now, false, &board)?;
    let root = crate::cli::probes::repo_root(&env.cwd);
    let landed = |task: &Task| {
        (root.as_ref()).is_some_and(|root| crate::cli::probes::landed(root, &task.slug))
    };
    let kind = (board.sessions.iter())
        .find(|s| s.row.handle == handle)
        .map_or("", |s| s.row.kind.as_str());
    let told = crate::cli::attach::seen(&board, handle, kind).children;
    let digest = viewed(
        store,
        Observation {
            board: &board,
            holdings: &holdings,
        },
        Some(&view),
        &told,
        Some(&landed),
        Some(&crate::cli::probes::tips(root.as_deref())),
        now,
    )?;
    Ok(digest.only_for(&board, handle))
}

/// Build against the pass's retained board and single successful holdings observation.
pub(crate) fn digest_on(
    store: &Store,
    observed: Observation<'_>,
    idle: &[String],
    landed: Option<&dyn Fn(&Task) -> bool>,
    tips: Option<Tips<'_>>,
    now: f64,
    verifying: &[String],
) -> Result<Digest, Error> {
    let Observation { board, holdings } = observed;
    let busy: HashSet<&str> = verifying.iter().map(String::as_str).collect();
    let posted = store.landings_posted()?;
    let tasks = &board.tasks;
    // What ended each record of changes, read once for the work's classification now and the alarm.
    let fixes = Fixes::new(store, tips, now);
    let waits = waiting(tasks, &fixes, now);
    let reworking = |t: &Task| waits.get(t.slug.as_str()).is_some_and(Judged::reworking);
    let volume = board
        .sessions
        .iter()
        .filter(|session| session.row.kind == "orchestrator" && session.row.left_at.is_none())
        .map(|session| {
            (
                session.row.handle.clone(),
                holdings.get(&session.row.handle).map_or(0, BTreeSet::len),
            )
        })
        .collect();
    let liveness_unknown = tasks
        .iter()
        .flat_map(|t| {
            t.roles
                .iter()
                .filter(move |role| crate::health::claimable(&role.role, &t.status))
        })
        .filter(|r| r.health == crate::Health::Unknown)
        .map(|r| r.handle.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let by_slug: HashMap<&str, &Task> = tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
    let mut kids: HashMap<&str, Vec<&Task>> = HashMap::new();
    for t in tasks {
        if let Some(parent) = t.parent.as_deref().filter(|p| by_slug.contains_key(p)) {
            kids.entry(parent).or_default().push(t);
        }
    }
    let children = |slug: &str| kids.get(slug).cloned().unwrap_or_default();
    // The tasks under it still to be executed: a subtask in review, done or cancelled has an executor
    // that is waiting or finished, not on this work.
    let below = |task: &Task| -> Vec<&Task> {
        let (mut found, mut todo) = (Vec::new(), children(&task.slug));
        while let Some(child) = todo.pop() {
            if EXECUTING.contains(&child.status.as_str()) {
                found.push(child);
            }
            todo.extend(children(&child.slug));
        }
        found
    };
    let present = |task: &Task, role: &str| {
        task.roles
            .iter()
            .any(|r| r.role == role && (r.health.holds() || busy.contains(r.handle.as_str())))
    };
    let staffed = |task: &Task| {
        present(task, "executor") || below(task).iter().any(|c| present(c, "executor"))
    };
    let mut running = Vec::new();
    let mut found: BTreeMap<&'static str, Vec<Task>> = BTreeMap::new();
    let mut notes = BTreeMap::new();
    let mut executing = 0;
    // A scan the board does not answer leaves this look without unanswered posts and says why on
    // stderr: the other lists, and every other event of the pass, go out all the same. A post
    // already told keeps its identity (`pull_unanswered`), so its return is not told again.
    // A PULL diffed against an earlier one says nothing of them ([`Digest::said_after`]).
    let mut unanswered_scanned = true;
    let overdue = crate::store::unanswered::Asked::Overdue(tasks, now);
    let pending = store.pending_posts(board, overdue).unwrap_or_else(|error| {
        let reason = error.to_string().replace('\n', " ");
        eprintln!("PULL unanswered scan skipped: {reason}");
        unanswered_scanned = false;
        Vec::new()
    });
    let unanswered: Vec<_> = pending
        .into_iter()
        .filter(|p| !busy.contains(p.handle.as_str()))
        .collect();
    for t in tasks {
        if unanswered.iter().any(|p| p.slug == t.slug) {
            found.entry("unanswered").or_default().push(t.clone());
        }
    }
    for t in tasks {
        let cancelled: Vec<&str> = t
            .depends
            .iter()
            .filter(|slug| {
                by_slug
                    .get(slug.as_str())
                    .is_some_and(|d| d.status == "cancelled")
            })
            .map(String::as_str)
            .collect();
        let blocked = (!cancelled.is_empty())
            .then(|| format!("cancelled dependencies: {}", cancelled.join(", ")));
        let unstaffed = |name: &'static str,
                         found: &mut BTreeMap<&'static str, Vec<Task>>,
                         notes: &mut BTreeMap<String, String>| {
            found.entry(name).or_default().push(t.clone());
            let reserved = (t.roles.iter()).find(|r| r.role == "reserved" && r.health.holds());
            let note = (reserved
                .map(|r| format!("reserved {}", r.handle))
                .into_iter())
            .chain(blocked.clone())
            .collect::<Vec<_>>();
            if !note.is_empty() {
                notes.insert(t.slug.clone(), note.join("; "));
            }
        };
        // A red nobody executes once it has stood a while is named first, and only there, a
        // cancelled dependency of its said beside it: it is judged before anything else.
        let red = t.red()
            && !matches!(t.status.as_str(), "review" | "done" | "cancelled")
            && now - t.created >= UNSTAFFED_SECONDS
            && !staffed(t);
        if red {
            unstaffed("red_unstaffed", &mut found, &mut notes);
        } else if let Some(blocked) = &blocked
            && !matches!(t.status.as_str(), "done" | "cancelled")
        {
            found
                .entry("dependency_blocked")
                .or_default()
                .push(t.clone());
            notes.insert(t.slug.clone(), blocked.clone());
            continue;
        }
        let status = if reworking(t) {
            "in_progress"
        } else {
            t.status.as_str()
        };
        match status {
            "ready" if !red => {
                let free = t
                    .depends
                    .iter()
                    .filter_map(|d| by_slug.get(d.as_str()))
                    .all(|d| d.status == "done");
                if free && !staffed(t) {
                    unstaffed("ready_unstaffed", &mut found, &mut notes);
                }
            }
            "in_progress" => {
                if staffed(t) {
                    executing += 1;
                    running.push(t.slug.clone());
                } else if !red {
                    found
                        .entry("in_progress_orphaned")
                        .or_default()
                        .push(t.clone());
                    if reworking(t) {
                        notes.insert(t.slug.clone(), REWORK_NOTE.into());
                    }
                }
            }
            // A review whose landing's record says it waits on no review at the tip now waits on
            // no reviewer: the project's review gate replaces the Symphony's.
            "review"
                if !present(t, "reviewer")
                    && !below(t).iter().any(|c| present(c, "executor"))
                    && !waits.get(t.slug.as_str()).is_some_and(Judged::reviewed) =>
            {
                found
                    .entry("review_unreviewed")
                    .or_default()
                    .push(t.clone());
            }
            _ => {}
        }
        let open = t.status == "in_progress" || t.status == "review";
        if open && (posted.contains(&t.slug) || landed.is_some_and(|landed| landed(t))) {
            found.entry("landed_open").or_default().push(t.clone());
            notes
                .entry(t.slug.clone())
                .or_insert_with(|| format!("landed on main, task still {}", t.status));
        }
    }
    for listed in found.values_mut() {
        listed.sort_by(|a, b| (a.priority, &a.slug).cmp(&(b.priority, &b.slug)));
    }
    // The project's limits as the file is now, read once for the whole digest.
    let limits = store.limits();
    Ok(Digest {
        now,
        executing,
        running,
        tasks: found,
        idle: idle.to_vec(),
        notes,
        liveness_unknown,
        unanswered,
        unanswered_scanned,
        cap: limits.orchestrator_slots,
        slots: None,
        low_volume: limits.low_volume,
        alarm: review_alarm(store, tasks, &waits, &fixes, &limits.alarm, now)?,
        volume,
        scoped: false,
    })
}

fn pending_key(pending: &Pending) -> String {
    format!("{}#{}", pending.slug, pending.post.id)
}

#[cfg(test)]
mod tests;
