//! The pane watch ([`pane_watch`]): BLOCKED once for a session it oversees ([`overseen`]) whose hc
//! terminal waits on an approval or a question, and WATCH once hc has failed at every sample for
//! [`OUTAGE_SECONDS`].

use crate::cli::agents::{Answer, Pane, find_pane};
use crate::orchestration::{Owners, claimable_work, hears, told_of};
use crate::rows::TaskSessionRow;
use crate::{Board, Task};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const WAITED: &str =
    "observed waiting on an approval or a question; inspect its terminal before acting";
const LEGACY_WAIT: &str = "its terminal waits on an approval or a question";
const OUTAGE: &str = " — blocked terminals go unreported until it answers";
const OBSERVED_OUTAGE: &str =
    " — blocked terminals may have gone unreported; inspect hc before acting";

/// How long hc fails at every sample before the watch says WATCH, in seconds; and the longest gap
/// between two samples that still continues an outage. One failed sample is a slow answer under
/// load, which the next answers. A longer gap is time the clock did not sample (the machine
/// asleep, or no server), which is nobody's outage: the stretch starts again at the sample after
/// it. The figure is the slack a sweep round may start late before the server takes the gap for
/// a sleep (`apps/symphony-server/src/live/presence.rs`).
pub const OUTAGE_SECONDS: f64 = 30.0;

fn generated_key(key: &str) -> bool {
    key.strip_prefix("pane-")
        .and_then(|tail| tail.split_once('-'))
        .is_some_and(|(revision, index)| {
            [revision, index]
                .iter()
                .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        })
}

struct BlockedLine<'text> {
    who: &'text str,
    task: &'text str,
    pane: &'text str,
}

fn blocked_line(text: &str) -> Option<BlockedLine<'_>> {
    let (identity, words) = text.split_once(" — ")?;
    if ![LEGACY_WAIT, WAITED].contains(&words) {
        return None;
    }
    let mut fields = identity.split(' ');
    match (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) {
        (Some("BLOCKED"), Some(who), Some(task), Some(pane), None)
            if !who.is_empty() && !task.is_empty() && !pane.is_empty() =>
        {
            Some(BlockedLine { who, task, pane })
        }
        _ => None,
    }
}

fn outage_line(text: &str) -> Option<&str> {
    text.strip_prefix("WATCH hc unanswered: ")
        .and_then(|why| why.strip_suffix(OUTAGE))
        .or_else(|| {
            text.strip_prefix("WATCH hc unanswered at observation: ")?
                .strip_suffix(OBSERVED_OUTAGE)
        })
}

/// The task/terminal identity and optional supported counter of an HC observation. In `blocked`
/// it records the last observed wait; in `known` it is only a monotone watermark.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Blocked {
    pub task: String,
    pub pane: String,
    #[serde(default)]
    pub turn: Option<u64>,
}

impl Blocked {
    fn same_place(&self, other: &Self) -> bool {
        self.task == other.task && self.pane == other.pane
    }

    fn stronger_than(&self, sample: &Self) -> bool {
        self.same_place(sample)
            && self
                .turn
                .is_some_and(|known| sample.turn.is_none_or(|turn| turn < known))
    }
}

/// What the pane watch last told a recipient: the sessions it said were blocked, and whether it
/// said hc could not be asked. Neither is said again until it ends.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PaneWatch {
    pub blocked: BTreeMap<String, Blocked>,
    /// Whether WATCH was said of the outage under way: not again until hc answers.
    pub unanswered: bool,
    /// While no WATCH is said, when hc began failing at every sample: the first failed sample
    /// since it last answered, or since a gap longer than [`OUTAGE_SECONDS`]. None while it answers.
    pub unanswered_since: Option<f64>,
    /// The last sample hc failed, beside `unanswered_since`: how far apart the samples are.
    pub unanswered_at: Option<f64>,
    /// Per-current-launch observed identities and counter watermarks, retained across resolved
    /// samples. A watermark is neither current blocked status, prompt identity nor native liveness.
    pub known: BTreeMap<String, Blocked>,
}

impl PaneWatch {
    /// A sample at `now` that hc failed with `why`: WATCH once it has failed at every sample for
    /// [`OUTAGE_SECONDS`], and nothing again while it goes on failing, so the record stays as it is.
    fn failed_sample(&mut self, now: f64, why: &str, emit: &mut dyn FnMut(&str)) {
        if self.unanswered {
            return;
        }
        let since = match (self.unanswered_since, self.unanswered_at) {
            (Some(since), Some(at)) if (0.0..=OUTAGE_SECONDS).contains(&(now - at)) => since,
            // The first failed sample, or the first after a gap: a sleep, or a clock set back.
            _ => now,
        };
        self.unanswered_since = Some(since);
        self.unanswered_at = Some(now);
        if now - since >= OUTAGE_SECONDS {
            emit(&format!(
                "WATCH hc unanswered at observation: {why}{OBSERVED_OUTAGE}"
            ));
            self.unanswered = true;
        }
    }

    fn observed_pane<'panes>(
        &self,
        board: &Board,
        panes: &'panes [Pane],
        record: &TaskSessionRow,
        who: &str,
    ) -> Option<&'panes Pane> {
        let pane = pane_of(board, panes, record, who)?;
        // A cached fallback can predate the current launch. Once that launch's recorded pane
        // was observed, weaker attribution cannot replace it or resolve its queued notice.
        let bound = [self.known.get(who), self.blocked.get(who)]
            .into_iter()
            .flatten()
            .any(|b| b.task == record.slug && b.pane == record.pane);
        (pane.id == record.pane || !bound).then_some(pane)
    }

    fn watermark(&self, who: &str, sample: &Blocked) -> Option<&Blocked> {
        [self.known.get(who), self.blocked.get(who)]
            .into_iter()
            .flatten()
            .filter(|b| b.same_place(sample))
            .max_by_key(|b| b.turn)
    }

    fn observe(&mut self, before: &Self, who: &str, sample: Blocked, status: Option<&str>) {
        if !matches!(
            status,
            Some("blocked" | "working" | "idle" | "done" | "error")
        ) {
            return;
        }
        let watermark = before.watermark(who, &sample);
        if watermark.is_some_and(|known| known.stronger_than(&sample)) {
            return;
        }
        let resolved = watermark
            .is_some_and(|known| known.turn.is_some() && known.turn == sample.turn)
            && !before
                .blocked
                .get(who)
                .is_some_and(|b| b.same_place(&sample));
        self.known.insert(who.into(), sample.clone());
        if status == Some("blocked") && !resolved {
            self.blocked.insert(who.into(), sample);
        } else {
            self.blocked.remove(who);
        }
    }

    /// Whether current watch evidence proves a generated, undelivered row obsolete. An absent
    /// or ambiguous HC identity and a missing/older counter leave its observation uncertain.
    pub fn retires(
        &self,
        board: &Board,
        overseen: &Overseen,
        open: &[TaskSessionRow],
        panes: Option<&Answer<Vec<Pane>>>,
        key: &str,
        text: &str,
    ) -> bool {
        if !generated_key(key) {
            return false;
        }
        if outage_line(text).is_some() {
            return matches!(panes, Some(Ok(_)));
        }
        let Some(BlockedLine {
            who,
            task,
            pane: id,
        }) = blocked_line(text)
        else {
            return false;
        };
        let launched = launches(board, overseen, open);
        let Some(record) = launched.get(who) else {
            return true;
        };
        if record.slug != task {
            return true;
        }
        let Some(Ok(panes)) = panes else {
            return false;
        };
        let Some(pane) = self.observed_pane(board, panes, record, who) else {
            return false;
        };
        let sample = Blocked {
            task: record.slug.clone(),
            pane: pane.id.clone(),
            turn: pane.turn,
        };
        if self
            .watermark(who, &sample)
            .is_some_and(|known| known.stronger_than(&sample))
        {
            return false;
        }
        if pane.id != id {
            return true;
        }
        match pane.status.as_deref() {
            Some("working" | "idle" | "done" | "error") => true,
            Some("blocked") => self.blocked.get(who).is_none_or(|old| {
                !old.same_place(&sample)
                    || old
                        .turn
                        .zip(sample.turn)
                        .is_some_and(|(old, new)| new > old)
            }),
            _ => false,
        }
    }

    /// Generated pane notices describe an observation, including legacy rows and retries sent
    /// outside a clock tick. A native recipient can read them after the terminal has moved on.
    /// Addressed messages and all other event kinds retain their exact text.
    pub fn delivery_text<'text>(key: &str, text: &'text str) -> Cow<'text, str> {
        if generated_key(key) {
            if let Some(BlockedLine { who, task, pane }) = blocked_line(text) {
                return Cow::Owned(format!("BLOCKED {who} {task} {pane} — {WAITED}"));
            }
            if let Some(why) = outage_line(text) {
                return Cow::Owned(format!(
                    "WATCH hc unanswered at observation: {why}{OBSERVED_OUTAGE}"
                ));
            }
        }
        Cow::Borrowed(text)
    }
}

fn launches<'launches>(
    board: &Board,
    overseen: &Overseen,
    open: &'launches [TaskSessionRow],
) -> BTreeMap<&'launches str, &'launches TaskSessionRow> {
    let owners = Owners::new(&board.tasks);
    let mut launches = BTreeMap::new();
    launches.extend(
        (open.iter())
            .filter(|record| overseen.hears(board, &owners, record))
            .map(|record| (record.handle.as_str(), record)),
    );
    launches
}

fn pane_of<'panes>(
    board: &Board,
    panes: &'panes [Pane],
    record: &TaskSessionRow,
    who: &str,
) -> Option<&'panes Pane> {
    let bound = board
        .sessions
        .iter()
        .find(|s| s.row.handle == who)
        .and_then(|s| s.row.agent.as_deref());
    let pane = find_pane(panes, Some(record), bound, who)?;
    let conversation = record
        .claude_session
        .strip_prefix(crate::launch::CODEX)
        .unwrap_or(&record.claude_session);
    let matches_conversation = |pane: &&Pane| {
        pane.session
            .as_deref()
            .is_some_and(|s| (!conversation.is_empty() && s == conversation) || bound == Some(s))
    };
    // Keep the existing attribution preference, but do not turn multiple possible identities
    // into evidence that a previously observed prompt was resolved.
    let candidates = if pane.id == record.pane {
        panes.iter().filter(|p| p.id == pane.id).count()
    } else if matches_conversation(&pane) {
        panes.iter().filter(matches_conversation).count()
    } else {
        panes
            .iter()
            .filter(|p| p.name.as_deref() == Some(who) || p.title.as_deref() == Some(who))
            .count()
    };
    (candidates == 1).then_some(pane)
}

/// The sessions a recipient oversees ([`overseen`]), and whether it is an orchestrator, which hears
/// of a launch only for a task it is told of, as LOST says that session's work, or one it
/// orchestrates (`orchestrating`, the tasks it holds the role on and those below, whatever its own
/// liveness reads).
#[derive(Debug, Clone, PartialEq)]
pub struct Overseen<'b> {
    pub recipient: &'b str,
    pub orchestrator: bool,
    pub orchestrating: HashSet<&'b str>,
    pub sessions: BTreeSet<&'b str>,
    delegated: BTreeSet<(&'b str, &'b str)>,
}

impl Overseen<'_> {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Whether the recipient hears of `launch`: a session it oversees, launched for a task it is
    /// told of (an orchestrator's own, or nobody's when it is the session's parent).
    fn hears(&self, board: &Board, owners: &Owners, launch: &TaskSessionRow) -> bool {
        self.hears_of(board, owners, &launch.handle, &launch.slug)
    }

    /// Whether the recipient hears of session `who`, launched for task `slug`: a session it
    /// oversees, on a task it is told of.
    pub fn hears_of(&self, board: &Board, owners: &Owners, who: &str, slug: &str) -> bool {
        if !self.sessions.contains(who) {
            return false;
        }
        let row = (board.sessions.iter()).find(|s| s.row.handle == who);
        let parent = row.and_then(|s| s.row.parent.as_deref());
        !self.orchestrator
            || self.orchestrating.contains(slug)
            || hears(owners, slug, parent, self.recipient)
            || self.delegated.contains(&(who, slug))
    }
}

fn delegated_coordinators<'board>(
    board: &'board Board,
    task: &'board Task,
    parent: &'board str,
) -> impl Iterator<Item = &'board str> {
    task.roles
        .iter()
        .filter(|role| role.role == "orchestrator" && role.health.holds())
        .filter(|role| {
            board.sessions.iter().any(|session| {
                session.row.handle == role.handle
                    && session.row.kind == "orchestrator"
                    && session.row.parent.as_deref() == Some(parent)
                    && session.row.left_at.is_none()
            })
        })
        .map(|role| role.handle.as_str())
}

/// The sessions `handle` oversees: its children as LOST reaches them (an orchestrator's only when
/// their work is its own or nobody's), and whoever holds a role on a task it orchestrates or on one
/// below it that no other orchestrator holds. Never itself, and none that left.
pub fn overseen<'b>(board: &'b Board, handle: &'b str) -> Overseen<'b> {
    let mut kids: HashMap<&str, Vec<&Task>> = HashMap::new();
    for task in &board.tasks {
        if let Some(parent) = &task.parent {
            kids.entry(parent.as_str()).or_default().push(task);
        }
    }
    let orchestrated =
        |t: &&Task| (t.roles.iter()).any(|r| r.handle == handle && r.role == "orchestrator");
    let mut todo: Vec<&Task> = board.tasks.iter().filter(orchestrated).collect();
    let owners = Owners::new(&board.tasks);
    let mut orchestrating: HashSet<&str> = HashSet::new();
    let mut holders: HashSet<&str> = HashSet::new();
    let mut delegated = BTreeSet::new();
    while let Some(task) = todo.pop() {
        if orchestrating.insert(&task.slug) {
            holders.extend(task.roles.iter().map(|r| r.handle.as_str()));
            let below = kids.get(task.slug.as_str()).into_iter().flatten();
            for kid in below {
                if !owners.elsewhere(&kid.slug, handle) {
                    todo.push(kid);
                } else if owners.of(&task.slug).contains(&handle) {
                    // The task owner hears its directly parented coordinator itself, without
                    // taking over that coordinator's workers or any other task it holds.
                    let coordinators = delegated_coordinators(board, kid, handle);
                    delegated.extend(coordinators.map(|who| (who, kid.slug.as_str())));
                }
            }
        }
    }
    let orchestrator =
        (board.sessions.iter()).any(|s| s.row.handle == handle && s.row.kind == "orchestrator");
    let work = orchestrator.then(|| claimable_work(board));
    let mut sessions = BTreeSet::new();
    sessions.extend(
        (board.sessions.iter())
            .map(|s| &s.row)
            .filter(|s| s.left_at.is_none() && s.handle != handle)
            .filter(|s| {
                told_of(&owners, work.as_ref(), s, handle)
                    || holders.contains(s.handle.as_str())
                    || delegated.iter().any(|(who, _)| *who == s.handle)
            })
            .map(|s| s.handle.as_str()),
    );
    Overseen {
        recipient: handle,
        orchestrator,
        orchestrating,
        sessions,
        delegated,
    }
}

/// One look at `now` at the panes of the `overseen` sessions launched into one still open (`open`,
/// oldest first) for a task the recipient hears of: BLOCKED for each whose terminal hc reports
/// `blocked` and did not at the last look, and WATCH once hc has failed at every sample for
/// [`OUTAGE_SECONDS`], while no BLOCKED is looked for. `panes` is asked only when there is a
/// terminal to look at. Returns what the watch remembers for the next look.
pub fn pane_watch(
    board: &Board,
    overseen: &Overseen,
    open: &[TaskSessionRow],
    panes: &dyn Fn() -> Answer<Vec<Pane>>,
    now: f64,
    before: &PaneWatch,
    emit: &mut dyn FnMut(&str),
) -> PaneWatch {
    // Each session's latest open launch it hears of: the pane it was launched into.
    let launched = launches(board, overseen, open);
    if launched.is_empty() {
        return PaneWatch::default();
    }
    let mut after = before.clone();
    // A launch retirement or task change is board evidence, independent of HC availability.
    for remembered in [&mut after.blocked, &mut after.known] {
        remembered.retain(|who, b| launched.get(who.as_str()).is_some_and(|r| r.slug == b.task));
    }
    let panes = match panes() {
        Ok(panes) => panes,
        Err(why) => {
            after.failed_sample(now, &why, emit);
            return after;
        }
    };
    for (who, record) in launched {
        if let Some(pane) = before.observed_pane(board, &panes, record, who) {
            let sample = Blocked {
                task: record.slug.clone(),
                pane: pane.id.clone(),
                turn: pane.turn,
            };
            after.observe(before, who, sample, pane.status.as_deref());
        }
    }
    for (who, b) in &after.blocked {
        if before.blocked.get(who).is_none_or(|old| {
            !old.same_place(b) || old.turn.zip(b.turn).is_some_and(|(old, new)| new > old)
        }) {
            emit(&format!("BLOCKED {who} {} {} — {WAITED}", b.task, b.pane));
        }
    }
    // An answer ends the outage, said or not, and the next is judged afresh.
    after.unanswered = false;
    after.unanswered_since = None;
    after.unanswered_at = None;
    after
}
