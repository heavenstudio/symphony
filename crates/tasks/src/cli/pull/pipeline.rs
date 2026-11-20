//! The pipeline alarm as the PULL digest says it ([`crate::alarm`]): what the landing of each task in
//! review waits on, judged against its branch's tip now ([`waiting`]), and the alarm replayed over
//! the board's record from a window whose start decides it ([`review_alarm`]).

use super::{ALARM_SHOWN, InReview, ReviewAlarm};
use crate::alarm::{self, Alarm, Assessed, Line, Standing, Thresholds, Tip, Tips, Written};
use crate::land::needs::REVIEW;
use crate::{Error, Store, Task};
use std::cell::RefCell;
use std::collections::HashMap;

pub(crate) struct Observation<'a> {
    pub(crate) board: &'a crate::Board,
    pub(crate) holdings: &'a super::Holdings,
}

/// The existing digest entry without repository observations; tip-aware callers opt in explicitly.
pub fn digest(
    store: &Store,
    idle: &[String],
    landed: Option<&dyn Fn(&Task) -> bool>,
    now: f64,
    verifying: &[String],
) -> Result<super::Digest, Error> {
    super::digest_with_tips(store, idle, landed, None, now, verifying)
}

/// What a task in review's landing waits on ([`alarm::Waits`]), and its branch's tip now when the
/// repository tells, which the record judged or not.
#[derive(Debug, Clone)]
pub(super) struct Judged<'a> {
    waits: alarm::Waits<'a>,
    /// Made no later than the digest's now: a tip the board's clock has not reached exists.
    tip: Option<Tip>,
    /// Whether the branch moved past the tip a record of changes judged since it was written,
    /// though it came back to it: the request was answered, and asks nothing again until a fresh
    /// assessment records it again ([`Fixes`], as the alarm replays it).
    answered: bool,
}

impl Judged<'_> {
    /// Whether the record judged the tip now; with none known, it is taken to.
    fn current(&self) -> bool {
        self.waits
            .judged(self.tip.as_ref().map(|tip| tip.commit.as_str()))
    }

    /// Whether the task is being reworked: its tip now stands judged to need changes the branch has
    /// not answered since.
    pub(super) fn reworking(&self) -> bool {
        self.current() && self.waits.changes() && !self.answered
    }

    /// Whether its landing waits on no review at the tip now: the repository told that tip, the
    /// record judged it, and it names neither [`REVIEW`] nor [`alarm::CHANGES`]. The project's own
    /// review gate, as its landing keeps it, is then met or not needed there, so the task waits on
    /// no reviewer (`review_unreviewed`). A record of another tip, or of a tip nobody can tell (a
    /// branch not named for its slug, or gone), says nothing of the tip now, so the task is listed.
    pub(super) fn reviewed(&self) -> bool {
        self.tip.is_some()
            && self.current()
            && !self.waits.changes()
            && !self.waits.on.contains(&REVIEW)
    }

    /// What its line in the alarm says of it, after when it went into review.
    fn note(&self) -> String {
        if !self.current() {
            "its tip is newer than its landing's last look".into()
        } else if self.waits.on.is_empty() {
            "missing nothing".into()
        } else {
            format!("missing {}", self.waits.on.join(", "))
        }
    }
}

/// The record of what its landing waits on of each task of `tasks` in review that has one, with its
/// branch's tip at `now` (asked of those alone, through `fixes`' reader); a task whose tip is
/// unknown is taken to be at the tip its record judged. Whether the branch answered a record of
/// changes on the tip now is `fixes`' (by its occurrence, [`Assessed`]), as the alarm
/// replays it, so the two count alike; how long a task's past records stood is [`Fixes`]'s too,
/// whatever its status now.
pub(super) fn waiting<'a>(
    tasks: &'a [Task],
    fixes: &Fixes<'_>,
    now: f64,
) -> HashMap<&'a str, Judged<'a>> {
    (tasks.iter())
        .filter(|t| t.status == "review")
        .filter_map(|t| {
            let waits = alarm::Waits::of(t)?;
            let tip = (fixes.tips.and_then(|tips| tips(t, None))).map(|tip| Tip {
                at: tip.at.min(now),
                ..tip
            });
            let mut judged = Judged {
                waits,
                tip,
                answered: false,
            };
            judged.answered = judged.reworking()
                && fixes
                    .standing(t)
                    .is_some_and(|record| fixes.made(t, record).is_some());
            Some((t.slug.as_str(), judged))
        })
        .collect()
}

/// When a task's branch first moved past the tip a record of its judged, after the record was
/// written, which ends the changes that record asked for, whatever the task's status now: as the
/// board keeps it ([`alarm::kept_fix`]), else as the repository tells ([`Tips`]), which the board
/// then keeps for good, so a later commit, amend, rebase or return to that tip never moves it
/// ([`Store::record_fixed`]). Each record is its own: a new record of changes on a tip the branch
/// came back to waits on a move of its own. No later than the digest's now, and read once a record
/// however many windows are replayed, and by the digest's classification of the work now as by
/// the alarm ([`waiting`]).
pub(super) struct Fixes<'r> {
    store: &'r Store,
    tips: Option<Tips<'r>>,
    now: f64,
    asked: RefCell<HashMap<Asked, Option<f64>>>,
    /// The first write that failed, which fails the digest once the replay is done.
    failed: RefCell<Option<Error>>,
}

/// A record [`Fixes`] was asked of: its task, the tip it judged, and the unique sequence of its
/// thread line, which distinguishes independent assessments even when their timestamps tie.
#[derive(PartialEq, Eq, Hash)]
struct Asked {
    slug: String,
    tip: String,
    sid: i64,
}

impl<'r> Fixes<'r> {
    pub(super) fn new(store: &'r Store, tips: Option<Tips<'r>>, now: f64) -> Fixes<'r> {
        let (asked, failed) = (RefCell::default(), RefCell::default());
        Fixes {
            store,
            tips,
            now,
            asked,
            failed,
        }
    }

    /// When `task`'s branch first moved past the tip `record` judged after it was written; none
    /// while it has not, or with no repository to ask and nothing kept.
    fn made(&self, task: &Task, record: Written<'_>) -> Option<f64> {
        let key = Asked {
            slug: task.slug.clone(),
            tip: record.tip.to_string(),
            sid: record.sid,
        };
        if let Some(at) = self.asked.borrow().get(&key) {
            return *at;
        }
        let at = (self.learnt(task, record)).map(|tip| tip.at.min(self.now));
        self.asked.borrow_mut().insert(key, at);
        at
    }

    /// The commit `task`'s branch first moved to from the tip `record` judged after it was
    /// written, kept on the board the first time the repository tells it.
    fn learnt(&self, task: &Task, record: Written<'_>) -> Option<Tip> {
        if let Some(kept) = alarm::kept_fix(task, record) {
            return Some(kept);
        }
        let legacy = match self.store.legacy_fix(task, record) {
            Ok(legacy) => legacy,
            Err(error) => {
                self.failed.borrow_mut().get_or_insert(error);
                return None;
            }
        };
        let told = legacy.or_else(|| self.tips.and_then(|tips| tips(task, Some(record))))?;
        if !told.at.is_finite() || told.commit.is_empty() {
            return None;
        }
        let first = Tip {
            at: told.at.min(self.now),
            ..told
        };
        match (self.store).record_fixed(&task.slug, record, &first) {
            Ok(kept) => Some(kept.unwrap_or(first)),
            Err(error) => {
                self.failed.borrow_mut().get_or_insert(error);
                Some(first)
            }
        }
    }

    /// The standing occurrence uses the same thread identity as history replay, including boards
    /// written before assessment sequences were saved alongside the current field.
    fn standing<'t>(&self, task: &'t Task) -> Option<Written<'t>> {
        let waits = alarm::Waits::of(task)?;
        if let Some(assessed) = Assessed::of(task)
            && let Some(sid) = assessed.sid
        {
            return Some(Written {
                tip: waits.tip,
                at: assessed.at,
                sid,
            });
        }
        match self.store.waits_record(task) {
            Ok(Some(line)) => Some(Written {
                tip: waits.tip,
                at: line.ts,
                sid: line.sid,
            }),
            Ok(None) => None,
            Err(error) => {
                self.failed.borrow_mut().get_or_insert(error);
                None
            }
        }
    }

    /// The first write that failed, if one did.
    fn failure(&self) -> Result<(), Error> {
        self.failed.borrow_mut().take().map_or(Ok(()), Err)
    }
}

/// The pipeline alarm over `tasks`, the board's now, at `now`, while it stands ([`crate::alarm`]):
/// replayed over the window of the thresholds' lookback, or one twice as long while the replays
/// from raised and from lowered disagree, until they agree or it opens before the board's first
/// task, when the alarm was lowered ([`replayed`], its branches read through `fixes`); then the
/// work in review, longest first, each with when it went in (from its own thread for one in review
/// all through the window) and what its landing waits on.
pub(super) fn review_alarm(
    store: &Store,
    tasks: &[Task],
    waits: &HashMap<&str, Judged<'_>>,
    fixes: &Fixes<'_>,
    thresholds: &Thresholds,
    now: f64,
) -> Result<Option<ReviewAlarm>, Error> {
    let first = (tasks.iter()).map(|t| t.created).fold(now, f64::min);
    let mut lookback = thresholds.lookback();
    let (since, window) = loop {
        let since = now - lookback;
        let window = replayed(store, tasks, fixes, thresholds, since, now)?;
        fixes.failure()?;
        if window.from_lowered.raised == window.from_raised.raised || since <= first {
            break (since, window);
        }
        lookback *= 2.0;
    };
    let state = window.from_lowered;
    if !state.raised {
        return Ok(None);
    }
    let mut entered = window.in_review;
    let by_slug: HashMap<&str, &Task> = tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
    let throughout: Vec<&str> = (entered.iter())
        .filter(|(slug, at)| {
            **at <= since && by_slug.get(*slug).is_some_and(|t| t.created <= since)
        })
        .map(|(slug, _)| *slug)
        .collect();
    let entered_at = store.entered_review(&throughout)?;
    for (slug, at) in entered_at {
        if let Some(held) = entered.get_mut(slug.as_str()) {
            *held = at;
        }
    }
    // The digest owns the tasks it names, as its categories do: it outlives this read of the board.
    let mut longest: Vec<InReview> = (entered.into_iter())
        .filter_map(|(slug, since)| {
            Some(InReview {
                task: (*by_slug.get(slug)?).clone(),
                since,
                note: waits.get(slug).map(Judged::note),
            })
        })
        .collect();
    crate::order::sort_by(&mut longest, &|a, b| {
        let (a_task, b_task) = (&a.task, &b.task);
        (a.since.total_cmp(&b.since))
            .then_with(|| (a_task.priority, &a_task.slug).cmp(&(b_task.priority, &b_task.slug)))
    });
    longest.truncate(ALARM_SHOWN);
    Ok(Some(ReviewAlarm {
        alarm: state,
        thresholds: *thresholds,
        longest,
    }))
}

/// The alarm replayed over one window, from `since` to `now`.
struct Window<'t> {
    /// From lowered at `since`.
    from_lowered: Alarm,
    /// From raised at `since`.
    from_raised: Alarm,
    /// Since when each task in review at `now` has been, the window's start at the earliest
    /// ([`alarm::held_since`]).
    in_review: HashMap<&'t str, f64>,
}

/// The alarm over the window from `since` to `now`: the work's timelines, each walked back from its
/// status now through its lines since then to what its thread said before them where that matters
/// ([`alarm::rests_on_before`], [`Store::lines_before`]), a task in review counted in progress while
/// reworked ([`alarm::reworked`], each record until its branch moved past the tip it judged,
/// `fixes`), each counted while it was work of its own as the tree stood then
/// ([`alarm::own_over`]), added up and replayed from each state the window may open in.
fn replayed<'t>(
    store: &Store,
    tasks: &'t [Task],
    fixes: &Fixes<'_>,
    thresholds: &Thresholds,
    since: f64,
    now: f64,
) -> Result<Window<'t>, Error> {
    let lines = store.status_lines(since)?;
    // A task a custom workflow holds says its moves by its recorded transitions, read by id for
    // those touched since the window opened ([`alarm::transition_lines`]).
    let touched: Vec<(&str, u64)> = (tasks.iter())
        .filter(|t| t.updated >= since)
        .filter_map(|t| {
            let position = crate::store::workflow::task_position(&t.fields)?;
            Some((t.slug.as_str(), position.revision))
        })
        .collect();
    let moved_by_workflow = alarm::transition_lines(&store.transitions(&touched)?, since);
    let mut threads: HashMap<&str, Vec<&Line>> = HashMap::new();
    for line in lines.iter().filter(|line| line.ts <= now) {
        threads.entry(line.slug.as_str()).or_default().push(line);
    }
    // Each in its place on its thread, which the board's own lines keep in time order.
    for line in moved_by_workflow.iter().filter(|line| line.ts <= now) {
        let thread = threads.entry(line.slug.as_str()).or_default();
        let at = thread.partition_point(|said| said.ts <= line.ts);
        thread.insert(at, line);
    }
    let mut moves: HashMap<&str, Vec<(f64, alarm::Moved<'_>)>> = HashMap::new();
    for (slug, lines) in &threads {
        let moved: Vec<_> = (lines.iter())
            .filter_map(|l| Some((l.ts, alarm::moved(l)?)))
            .collect();
        if !moved.is_empty() {
            moves.insert(slug, moved);
        }
    }
    let counted = |t: &Task| matches!(t.status.as_str(), "review" | "in_progress");
    let seen: Vec<&Task> = (tasks.iter())
        .filter(|t| counted(t) || threads.contains_key(t.slug.as_str()))
        .collect();
    let own = alarm::own_over(tasks, &seen, &moves, since, now);
    let ever_own = |t: &Task| (own.get(t.slug.as_str())).is_none_or(|o| o.iter().any(|(_, f)| *f));
    let work: Vec<(&Task, &[&Line])> = (seen.into_iter())
        .filter(|t| ever_own(t))
        .map(|t| {
            let lines = threads.get(t.slug.as_str()).map_or(&[][..], Vec::as_slice);
            (t, lines)
        })
        .collect();
    let asked: Vec<&str> = (work.iter())
        .filter(|(t, lines)| t.created <= since && alarm::rests_on_before(&t.status, lines))
        .map(|(t, _)| t.slug.as_str())
        .collect();
    let before = store.lines_before(&asked, since)?;
    let timelines: HashMap<&str, Vec<(f64, Option<&str>)>> = (work.iter())
        .map(|(t, lines)| {
            let said: Vec<&Line> = (before.get(t.slug.as_str()).into_iter().flatten()).collect();
            let opening = alarm::opening(&said);
            let walked = alarm::timeline(&t.status, t.created, lines, since, opening.status);
            let fixed = |record: Written<'_>| fixes.made(t, record);
            let reworked = alarm::reworked(lines, opening.record.as_ref(), &fixed, since);
            let worked = alarm::as_worked(&walked, &reworked);
            let counts = match own.get(t.slug.as_str()) {
                Some(own) => alarm::own_only(&worked, own),
                None => worked,
            };
            (t.slug.as_str(), counts)
        })
        .collect();
    let all: Vec<&[(f64, Option<&str>)]> = timelines.values().map(Vec::as_slice).collect();
    let (start, changes) = alarm::counted(&all);
    let replay = |opened| alarm::replay(thresholds, opened, start, &changes, since, now);
    Ok(Window {
        from_lowered: replay(Standing::Lowered),
        from_raised: replay(Standing::Raised),
        in_review: alarm::held_since(&timelines, "review"),
    })
}
