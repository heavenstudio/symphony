//! A task's gates (`symphony_api::gates`): what it needs to land, at its branch's tip. The Symphony
//! answers three of its own: [`review`], the board's reviewer verdicts ([`review_rounds`]), or the
//! covered close a task is done by instead ([`review_of`]);
//! [`check`], the project's `[verdicts] check` at the tip, from the run records the board's
//! recorder keeps (docs/MACHINE_WORK.md), for a branch with work of its own ([`own_commits`]) and
//! never an instruction subtask's; and [`owner`], a card the task posted that waits on the
//! owner, by the sign-off lane's rule ([`posted_cards`]). A project's plugins answer its own
//! through the gates seam (`symphony_plugin::Gates`), all asked at once ([`theirs`]), each answer
//! [`Kept`] by the task and its branch's tip, and a plugin's gate replaces the Symphony's of its
//! name ([`merged`]), also while the plugin cannot answer ([`failed`]).
//!
//! An open or assessed top-level task's own gates are its stamps ([`stamps`]): each stamp of the
//! project's catalogue needed at the tip and read there, or waived by its assessment. A task done
//! or cancelled with no assessment, and a subtask, keep the reading above.
//!
//! [`answer`] puts them together for the task page's `/api/task-gates`; the server's other readers
//! of a gate ask it the same way, through the same [`Kept`], so a plugin is asked once a tip. A
//! reader that must ask nothing (the sign-off lane, `taskview::signoff`) reads only what is kept
//! and known ([`kept_theirs`]) and merges it the same way.
//!
//! What the Symphony's own gates are read from is kept too ([`inputs`]): the task and its thread
//! by the task's stamp on the board, the recorder's runs by their files, the stamps catalogue by
//! the main checkout's head. A read with nothing new asks the board for that stamp, resolves the
//! refs, and reads nothing else again ([`Kept::reads`] counts what is).

use std::any::Any;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use symphony_api::gates::{CHECK, OWNER, REVIEW, ranked};
use symphony_api::{Gate, GatePast, GateState, GatesSeen, Pending, PendingItem, TaskGates};
use symphony_board::cli::machine_work::recorded::{self, Record, Step};
use symphony_board::lock::Recovered as _;
use symphony_board::stamps::read;
use symphony_plugin::{Gates, TaskRef};

use crate::plugins::Plugins;
use crate::project::Project;

pub mod inputs;
pub mod stamps;

pub use inputs::Reads;

/// How long a page waits for a plugin's answer before it says the gate is unknown. The answer
/// still arrives, and is kept, for the next read.
pub const WAIT: Duration = Duration::from_secs(10);
/// How long a plugin's error is shown before it is asked again for the same tip.
pub const RETRY: Duration = Duration::from_secs(30);
/// How long a plugin's call may run before it is abandoned and asked again.
pub const CEILING: Duration = Duration::from_secs(60);
/// How many of one answerer's calls may run at once, abandoned ones among them.
pub const CALLS: usize = 4;

/// One review round: an `approve` or `changes` verdict a reviewer gave the task.
#[derive(Clone, Debug, PartialEq)]
pub struct Round {
    /// The thread's line that holds it.
    pub id: i64,
    pub by: String,
    pub at: f64,
    pub approved: bool,
    /// What the reviewer said after the verdict's word.
    pub said: String,
    /// The commit the round judged, when its record names one.
    pub tip: Option<String>,
}

/// The review rounds on `thread`, oldest first: the Symphony's one reader of a task's review rounds,
/// so where rounds are recorded changes here. The board's own commands (`crates/tasks`: its stages,
/// alarms and reaper) read the verdict words apart.
pub fn review_rounds(thread: &[symphony_board::Message]) -> Vec<Round> {
    thread
        .iter()
        .filter(|line| line.kind == "verdict")
        .filter_map(|line| {
            let (approved, rest) = ["approve", "changes"].iter().find_map(|word| {
                let rest = line.text.strip_prefix(word)?;
                (rest.is_empty() || rest.starts_with(':')).then_some((*word == "approve", rest))
            })?;
            Some(Round {
                id: line.id,
                by: line.sender.clone(),
                at: line.ts,
                approved,
                said: rest.trim_start_matches(':').trim().to_string(),
                tip: None,
            })
        })
        .collect()
}

/// A covered close on a task's thread: its last verdict, closing it as covered by another task's
/// landing (`symphony_board::covered_by`), with no review of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Covered {
    /// The thread's line that holds it.
    pub id: i64,
    /// Who closed it.
    pub by: String,
    pub at: f64,
    /// The task whose landing holds its work.
    pub other: String,
    /// The commit that landing names.
    pub commit: String,
    pub note: String,
}

/// The covered close on `thread`, when its last verdict is one.
pub fn covered_close(thread: &[symphony_board::Message]) -> Option<Covered> {
    let last = thread.iter().rev().find(|line| line.kind == "verdict")?;
    let said = symphony_board::covered_by(&last.text)?;
    Some(Covered {
        id: last.id,
        by: last.sender.clone(),
        at: last.ts,
        other: said.other.to_string(),
        commit: said.commit.to_string(),
        note: said.note.to_string(),
    })
}

/// The review gate of the task `slug` from its `thread`, its `reviewers` now and its `status`:
/// [`review`] of its rounds, or, for a task done by a covered close ([`covered_close`]), the gate
/// saying so: waived, as its work landed with another task's, never approved by it nor landed
/// without review.
pub fn review_of(
    slug: &str,
    status: &str,
    reviewers: &[&str],
    thread: &[symphony_board::Message],
) -> Gate {
    let rounds = review_rounds(thread);
    let Some(covered) = covered_close(thread).filter(|_| status == "done") else {
        return review(slug, status, reviewers, &rounds);
    };
    Gate {
        name: REVIEW.into(),
        state: GateState::Waived,
        by: Some(covered.by.clone()),
        at: Some(covered.at),
        commit: Some(covered.commit.clone()),
        why: Some(format!(
            "its work landed with {}'s branch, so none of its own is left to review; closed by \
             {}: {}",
            covered.other,
            covered.by,
            first_line(&covered.note)
        )),
        detail: format!("Covered by {}, landed at {}", covered.other, covered.commit),
        link: Some(format!("/task/{slug}#message-{}", covered.id)),
        past: past(&rounds, rounds.len()),
    }
}

/// The review gate of the task `slug`, from its `rounds`, its `reviewers` now and its `status`:
/// met by the last round's approval, missing until one, the earlier rounds its past. A task done
/// with no round landed without review, as a bug fix may (`workflow/fast-bugfixes`): waived.
pub fn review(slug: &str, status: &str, reviewers: &[&str], rounds: &[Round]) -> Gate {
    let total = rounds.len();
    let Some(last) = rounds.last() else {
        if status == "done" {
            return Gate {
                name: REVIEW.into(),
                state: GateState::Waived,
                why: Some(
                    "a bug fix lands on its evidence, with a reviewer only when its risk calls \
                     for one (workflow/fast-bugfixes)"
                        .into(),
                ),
                detail: "Landed with no review verdict".into(),
                ..Gate::default()
            };
        }
        let (by, detail) = match reviewers.first() {
            Some(reviewer) if status == "review" => {
                (Some((*reviewer).to_string()), "In review, no verdict yet")
            }
            _ if status == "review" => (None, "In review, waiting on a reviewer"),
            _ => (None, "Not reviewed yet"),
        };
        return Gate {
            name: REVIEW.into(),
            state: GateState::Missing,
            by,
            detail: detail.into(),
            ..Gate::default()
        };
    };
    Gate {
        name: REVIEW.into(),
        state: round_state(last),
        by: Some(last.by.clone()),
        at: Some(last.at),
        commit: last.tip.clone(),
        detail: round_detail(total, total, last),
        link: Some(format!("/task/{slug}#message-{}", last.id)),
        past: past(&rounds[..total - 1], total),
        ..Gate::default()
    }
}

/// A round's state: met by its approval, missing on changes.
fn round_state(round: &Round) -> GateState {
    if round.approved {
        GateState::Met
    } else {
        GateState::Missing
    }
}

/// What round `n` of `total` says.
fn round_detail(n: usize, total: usize, round: &Round) -> String {
    if round.approved {
        format!("Approved in round {n} of {total}")
    } else if round.said.is_empty() {
        format!("Changes asked in round {n}")
    } else {
        format!("Changes asked in round {n}: {}", first_line(&round.said))
    }
}

/// `rounds`, the first of `total`, as a review gate's past.
fn past(rounds: &[Round], total: usize) -> Vec<GatePast> {
    rounds
        .iter()
        .enumerate()
        .map(|(index, round)| GatePast {
            state: round_state(round),
            at: round.at,
            by: Some(round.by.clone()),
            commit: round.tip.clone(),
            detail: round_detail(index + 1, total, round),
        })
        .collect()
}

/// Every run the work folder holds, each once: the board's reader, which the land reads the `tests`
/// stamp by too.
pub use recorded::runs;

/// The check gate: the newest run of the command `label` at `tip`, whoever ran it, as
/// [`read::newest_of`] reads it, green (met), red or still going (missing), or none there
/// (unknown); the earlier finished runs of it on `branch` or at `tip`, its past.
pub fn check(label: &str, branch: &str, tip: Option<&str>, runs: &[Record]) -> Gate {
    let short = |commit: &str| commit.chars().take(7).collect::<String>();
    let ours = |run: &&Record| {
        run.label == label
            && run
                .commit
                .as_deref()
                .is_some_and(|commit| Some(commit) == tip || run.branch.as_deref() == Some(branch))
    };
    let current = tip.and_then(|tip| read::newest_of(runs, tip, label));
    let finished = |run: &Record| -> (GateState, String) {
        let at = short(run.commit.as_deref().unwrap_or_default());
        match (run.exit_code, run.signal) {
            (Some(0), _) => (GateState::Met, format!("`{label}` green at {at}")),
            (Some(code), _) => (
                GateState::Missing,
                format!("`{label}` red at {at}: exit {code}"),
            ),
            (None, Some(signal)) => (
                GateState::Missing,
                format!("`{label}` red at {at}: ended by signal {signal}"),
            ),
            (None, None) if run.reason.is_empty() => (
                GateState::Missing,
                format!("`{label}` at {at} ended without a status"),
            ),
            (None, None) => (
                GateState::Missing,
                format!("`{label}` at {at} ended without a status: {}", run.reason),
            ),
        }
    };
    let mut past = (runs.iter())
        .filter(ours)
        .filter(|run| !current.is_some_and(|current| std::ptr::eq(*run, current)))
        .filter(|run| run.state == Step::Finished)
        .filter_map(|run| {
            let (state, detail) = finished(run);
            Some(GatePast {
                state,
                at: run.ended_at?,
                by: run.handle.clone(),
                commit: run.commit.clone(),
                detail,
            })
        })
        .collect::<Vec<_>>();
    symphony_board::order::sort_by(&mut past, &|a, b| a.at.total_cmp(&b.at));
    let gate = |state, detail: String| Gate {
        name: CHECK.into(),
        state,
        detail,
        past: past.clone(),
        ..Gate::default()
    };
    let Some(tip) = tip else {
        return gate(
            GateState::Unknown,
            format!("No branch {branch} in this repository"),
        );
    };
    let Some(run) = current else {
        return Gate {
            commit: Some(tip.to_string()),
            ..gate(
                GateState::Unknown,
                format!("`{label}` has not run at {}", short(tip)),
            )
        };
    };
    let (state, detail, at) = match run.state {
        Step::Finished => {
            let (state, detail) = finished(run);
            (state, detail, run.ended_at)
        }
        Step::Running => (
            GateState::Missing,
            format!("`{label}` running at {}", short(tip)),
            run.started_at,
        ),
        Step::Queued => (
            GateState::Missing,
            format!("`{label}` queued at {}", short(tip)),
            Some(run.queued_at),
        ),
    };
    Gate {
        by: run.handle.clone(),
        at,
        commit: Some(tip.to_string()),
        ..gate(state, detail)
    }
}

/// Whether `text` names the card `slug` whole: a card named `x` is not named by `x-2`.
fn names(text: &str, slug: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    text.match_indices(slug).any(|(at, _)| {
        let before = text.get(..at).and_then(|text| text.chars().next_back());
        let after = text
            .get(at + slug.len()..)
            .and_then(|text| text.chars().next());
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// The cards of `pending` in a document one of `links` names: the cards the task could have posted.
pub fn linked_cards<'a>(links: &[String], pending: &'a Pending) -> Vec<&'a PendingItem> {
    pending
        .items
        .iter()
        .filter(|card| {
            links
                .iter()
                .any(|link| link.split('#').next() == Some(card.path.as_str()))
        })
        .collect()
}

/// The cards of `cards` the task posted: named whole by one of `texts`, its body and its thread's
/// lines (the sign-off lane's rule, docs/TASKS.md).
pub fn posted_cards<'a, 't>(
    cards: &[&'a PendingItem],
    mut texts: impl Iterator<Item = &'t str>,
) -> Vec<&'a PendingItem> {
    let mut left: Vec<&PendingItem> = cards.to_vec();
    let mut posted = Vec::new();
    while !left.is_empty() {
        let Some(text) = texts.next() else { break };
        left.retain(|card| {
            let named = names(text, &card.slug);
            if named {
                posted.push(*card);
            }
            !named
        });
    }
    if posted.len() < 2 {
        return posted;
    }
    drop(left);
    let mut ordered = Vec::with_capacity(posted.len());
    for &card in cards {
        // The old stable sort used the first pointer-equal input position: repeated references
        // stay together there, while equal-valued but distinct cards stay separate.
        let before = posted.len();
        posted.retain(|selected| !std::ptr::eq(*selected, card));
        ordered.extend(std::iter::repeat_n(card, before - posted.len()));
        if posted.is_empty() {
            break;
        }
    }
    ordered
}

/// The owner gate: held while a card the task posted waits on the owner; none while none does.
pub fn owner(cards: &[&PendingItem]) -> Option<Gate> {
    let first = cards.first()?;
    let detail = match cards {
        [card] => format!("Card {} in {} waits on the owner", card.slug, card.path),
        _ => format!(
            "Cards {} wait on the owner",
            cards
                .iter()
                .map(|card| card.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    Some(Gate {
        name: OWNER.into(),
        state: GateState::Held,
        by: Some("owner".into()),
        why: Some("the task asked the owner and waits on the answer".into()),
        detail,
        link: Some(first.href.clone()),
        ..Gate::default()
    })
}

/// The Symphony's gates `own`, each replaced in place by the project's gate of its name, then the
/// project's others in their order; a name given twice keeps its first. Ranked
/// ([`symphony_api::gates::ranked`]). A link that is neither a path of the app nor
/// a web address ([`symphony_api::linkable`]) is dropped, so no page is handed a `javascript:` or
/// other link to run: the gate is drawn without one.
pub fn merged(own: Vec<Gate>, project: Vec<Gate>) -> Vec<Gate> {
    let mut project: Vec<Option<Gate>> = project.into_iter().map(Some).collect();
    let mut take = |name: &str| {
        project
            .iter_mut()
            .find(|gate| gate.as_ref().is_some_and(|gate| gate.name == name))
            .and_then(Option::take)
    };
    let mut gates: Vec<Gate> = own
        .into_iter()
        .map(|gate| take(&gate.name).unwrap_or(gate))
        .collect();
    for gate in project.into_iter().flatten() {
        if !gates.iter().any(|kept| kept.name == gate.name) {
            gates.push(gate);
        }
    }
    for gate in &mut gates {
        gate.link = gate.link.take().filter(|link| symphony_api::linkable(link));
    }
    ranked(gates)
}

/// The gates shown for the answerer `answerer` while it cannot answer, `why` their detail: one
/// unknown gate for each of `names`, the gates it answered before, so the Symphony's own of those
/// names never shows in their place; before it ever answered, one gate named after it.
pub fn failed(answerer: &str, names: &[String], why: &str) -> Vec<Gate> {
    let gate = |name: &str| Gate {
        name: name.into(),
        state: GateState::Unknown,
        by: Some(answerer.into()),
        detail: first_line(why).to_string(),
        ..Gate::default()
    };
    if names.is_empty() {
        vec![gate(answerer)]
    } else {
        names.iter().map(|name| gate(name)).collect()
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default().trim()
}

/// A task as a gates answerer is asked about it, owned, so the answer can run on a thread of its
/// own. An answer is kept for all of it: a new status, branch or tip asks again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    pub slug: String,
    pub status: String,
    pub branch: String,
    pub tip: Option<String>,
}

impl Asked {
    fn task(&self) -> TaskRef<'_> {
        TaskRef {
            slug: &self.slug,
            status: &self.status,
            branch: &self.branch,
        }
    }
}

/// What an answerer gave: its gates, or why it could not say.
pub type Answer = Result<Vec<Gate>, String>;

/// How long a read waits, an error is kept and a call may run, and how many calls of one answerer
/// run at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How long a read waits for an answer, from when it was asked, before it says none came; the
    /// call goes on, its answer kept for the next read.
    pub wait: Duration,
    /// How long an error is kept before the answerer is asked again.
    pub retry: Duration,
    /// How long a call may run before a read abandons it and asks again.
    pub ceiling: Duration,
    /// How many calls of one answerer may run at once, abandoned ones among them; past it none
    /// starts, so calls that never return cannot pile up.
    pub calls: usize,
}

impl Default for Limits {
    /// [`WAIT`], [`RETRY`], [`CEILING`] and [`CALLS`].
    fn default() -> Self {
        Self {
            wait: WAIT,
            retry: RETRY,
            ceiling: CEILING,
            calls: CALLS,
        }
    }
}

/// One answerer's answer for one task: asked as `asked` at [`Gates::kept_while`], and what came
/// back, once it did.
struct Slot {
    asked: Asked,
    kept_while: String,
    at: Instant,
    done: Mutex<Option<(Answer, Instant)>>,
    ready: Condvar,
}

impl Slot {
    fn holds(&self, asked: &Asked, kept_while: &str) -> bool {
        self.asked == *asked && self.kept_while == kept_while
    }

    /// What came back and when, once it has.
    fn answer(&self) -> Option<(Answer, Instant)> {
        self.done.lock().recover().clone()
    }

    /// What came back, waited for until `deadline`; `None` if nothing did by then.
    fn wait_until(&self, deadline: Instant) -> Option<Answer> {
        let mut done = self.done.lock().recover();
        loop {
            if let Some((answer, _)) = &*done {
                return Some(answer.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            done = self.ready.wait_timeout(done, left).recover().0;
        }
    }
}

/// An answerer of a plugin in a project.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Who {
    root: PathBuf,
    plugin: String,
    answerer: String,
}

impl Who {
    fn new(root: &Path, plugin: &str, answerer: &str) -> Who {
        Who {
            root: root.to_path_buf(),
            plugin: plugin.to_string(),
            answerer: answerer.to_string(),
        }
    }
}

/// What is kept of one answerer: its newest answer for each task, by slug; how many of its calls
/// run; and the names of the gates it has answered, in the order first answered.
#[derive(Default)]
struct Ledger {
    slots: HashMap<String, Arc<Slot>>,
    running: Arc<AtomicUsize>,
    names: Vec<String>,
}

/// One call counted among its answerer's running ones until it ends, however it ends.
struct Running(Arc<AtomicUsize>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A read's hold on an answer: there now, or a call to wait for.
enum Waiting {
    Now(Answer),
    For(Arc<Slot>),
}

/// The plugins' answers, kept by the task (its status, branch and tip, [`Asked`]) and what each
/// answerer says it reads besides ([`Gates::kept_while`]): one answer an answerer and task, the
/// newest asked, so an answer that starts a landing binary runs once a tip. A read waits for an
/// answer at most [`Limits::wait`] from when it was asked, then says none came while the call goes
/// on, its answer kept for the next read; no second call starts while one runs, until it has run
/// [`Limits::ceiling`]. An error is kept for [`Limits::retry`], then asked again. At most
/// [`Limits::calls`] of one answerer's calls run at once.
pub struct Kept {
    ledgers: Mutex<HashMap<Who, Ledger>>,
    /// The shape of the last whole answer given for each task, by its checkout and slug.
    seen: Mutex<HashMap<(PathBuf, String), GatesSeen>>,
    limits: Limits,
    /// How many answers have come back, whichever call made them ([`Kept::answered`]).
    answered: Arc<AtomicUsize>,
    /// What the Symphony's own gates are read from.
    inputs: inputs::Inputs,
}

impl Default for Kept {
    /// [`Limits::default`].
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

impl Kept {
    pub fn new(limits: Limits) -> Self {
        Self {
            ledgers: Mutex::new(HashMap::new()),
            seen: Mutex::new(HashMap::new()),
            limits,
            answered: Arc::default(),
            inputs: inputs::Inputs::default(),
        }
    }

    /// How many times each part the Symphony's own gates are read from was read anew, since this
    /// was made ([`inputs`]): a read of a task's gates that found nothing new adds to none.
    pub fn reads(&self) -> Reads {
        self.inputs.reads()
    }

    /// Every run the recorder holds for the repository at `root` ([`runs`]), each of its files
    /// read again only once it is another: what the gates and the timeline read.
    pub fn runs(&self, root: &Path) -> Arc<Vec<Record>> {
        self.inputs.runs(root)
    }

    /// How many answers have come back so far, by any call, an error among them: a reader of what
    /// is kept ([`Kept::kept`], [`kept_theirs`]) that counted before it read reads again once the
    /// count moved, as an answer it found missing may be there now. An answer is counted before any
    /// read can find it.
    pub fn answered(&self) -> usize {
        self.answered.load(Ordering::SeqCst)
    }

    /// The answer of `answerer`, of the plugin `plugin`, for `asked` in the project at `root`:
    /// kept, or asked now, on a thread of its own.
    pub fn ask(
        &self,
        root: &Path,
        plugin: &str,
        answerer: &'static dyn Gates,
        asked: &Asked,
    ) -> Answer {
        let waiting = self.begin(root, plugin, answerer, asked);
        self.finish(root, plugin, answerer, waiting)
    }

    /// The answers of `answerers`, each with its plugin's name, for `asked` in the project at
    /// `root`, in their order: all asked before any is waited for, so a read waits for the slowest
    /// alone.
    pub fn ask_all(
        &self,
        root: &Path,
        answerers: &[(&str, &'static dyn Gates)],
        asked: &Asked,
    ) -> Vec<Answer> {
        let waiting: Vec<Waiting> = answerers
            .iter()
            .map(|(plugin, answerer)| self.begin(root, plugin, *answerer, asked))
            .collect();
        waiting
            .into_iter()
            .zip(answerers)
            .map(|(waiting, (plugin, answerer))| self.finish(root, plugin, *answerer, waiting))
            .collect()
    }

    /// What is kept of `answerer`'s answer, of the plugin `plugin`, for `asked` in the project at
    /// `root`, asking nothing: the answer a read would use now, `None` while there is none.
    pub fn kept(
        &self,
        root: &Path,
        plugin: &str,
        answerer: &dyn Gates,
        asked: &Asked,
    ) -> Option<Answer> {
        let kept_while = match told(answerer, root, asked) {
            Ok(kept_while) => kept_while,
            Err(why) => return Some(Err(why)),
        };
        let ledgers = self.ledgers.lock().recover();
        let slot = ledgers
            .get(&Who::new(root, plugin, answerer.name()))?
            .slots
            .get(&asked.slug)
            .filter(|slot| slot.holds(asked, &kept_while))?;
        match slot.answer()? {
            (Err(_), at) if at.elapsed() >= self.limits.retry => None,
            (answer, _) => Some(answer),
        }
    }

    /// How many gates the last answer given for the task `slug` in the project at `root` had, how
    /// many it waited on and how many said no one and no time ([`answer`] keeps it): the room its
    /// page holds before it reads them.
    pub fn seen(&self, root: &Path, slug: &str) -> Option<GatesSeen> {
        self.seen
            .lock()
            .recover()
            .get(&(root.to_path_buf(), slug.to_string()))
            .copied()
    }

    /// Keep the shape of `answer`, given for its task in the project at `root`.
    pub fn saw(&self, root: &Path, answer: &TaskGates) {
        self.seen
            .lock()
            .recover()
            .insert((root.to_path_buf(), answer.slug.clone()), answer.seen());
    }

    /// Forget the shapes kept for every task `open` does not name, in every checkout, and what
    /// its gates were read from: one that closed or left the board has no page to hold room in,
    /// so what is kept stays as many as the open tasks read since the server started. The hub
    /// calls it when the board's tasks change.
    pub fn keep_open(&self, open: impl Fn(&str) -> bool) {
        self.seen.lock().recover().retain(|(_, slug), _| open(slug));
        self.inputs.keep_open(open);
    }

    /// The names of the gates `answerer`, of the plugin `plugin`, has answered in the project at
    /// `root`, in the order first answered.
    pub fn names(&self, root: &Path, plugin: &str, answerer: &str) -> Vec<String> {
        self.ledgers
            .lock()
            .recover()
            .get(&Who::new(root, plugin, answerer))
            .map(|ledger| ledger.names.clone())
            .unwrap_or_default()
    }

    /// The answer a read of `answerer` for `asked` uses: kept, or a call started now.
    fn begin(
        &self,
        root: &Path,
        plugin: &str,
        answerer: &'static dyn Gates,
        asked: &Asked,
    ) -> Waiting {
        let name = answerer.name();
        let kept_while = match told(answerer, root, asked) {
            Ok(kept_while) => kept_while,
            Err(why) => return Waiting::Now(Err(why)),
        };
        let mut ledgers = self.ledgers.lock().recover();
        let ledger = ledgers.entry(Who::new(root, plugin, name)).or_default();
        if let Some(slot) = ledger
            .slots
            .get(&asked.slug)
            .filter(|slot| slot.holds(asked, &kept_while))
        {
            let used = match slot.answer() {
                Some((Ok(_), _)) => true,
                Some((Err(_), at)) => at.elapsed() < self.limits.retry,
                None => slot.at.elapsed() < self.limits.ceiling,
            };
            if used {
                return Waiting::For(Arc::clone(slot));
            }
        }
        let running = ledger.running.load(Ordering::SeqCst);
        if running >= self.limits.calls {
            return Waiting::Now(Err(format!("{name} has {running} calls still running")));
        }
        let slot = Arc::new(Slot {
            asked: asked.clone(),
            kept_while,
            at: Instant::now(),
            done: Mutex::new(None),
            ready: Condvar::new(),
        });
        if let Err(why) = start(&slot, root, answerer, &ledger.running, &self.answered) {
            *slot.done.lock().recover() = Some((Err(why), Instant::now()));
            self.answered.fetch_add(1, Ordering::SeqCst);
        }
        ledger.slots.insert(asked.slug.clone(), Arc::clone(&slot));
        Waiting::For(slot)
    }

    /// What `waiting` holds once it came or the wait ran out; the names of a set of gates learnt.
    fn finish(&self, root: &Path, plugin: &str, answerer: &dyn Gates, waiting: Waiting) -> Answer {
        let answer = match waiting {
            Waiting::Now(answer) => answer,
            Waiting::For(slot) => {
                slot.wait_until(slot.at + self.limits.wait)
                    .unwrap_or_else(|| {
                        Err(format!(
                            "{} gave no answer within {} s",
                            answerer.name(),
                            self.limits.wait.as_secs_f64()
                        ))
                    })
            }
        };
        if let Ok(gates) = &answer {
            let mut ledgers = self.ledgers.lock().recover();
            let ledger = ledgers
                .entry(Who::new(root, plugin, answerer.name()))
                .or_default();
            for gate in gates {
                if !ledger.names.contains(&gate.name) {
                    ledger.names.push(gate.name.clone());
                }
            }
        }
        answer
    }
}

/// The gates `answerers`, each with its plugin's name, give for `asked` in the project at `root`,
/// all asked at once through `kept`; one that cannot answer stands in as [`failed`].
pub fn theirs(
    kept: &Kept,
    root: &Path,
    answerers: &[(&str, &'static dyn Gates)],
    asked: &Asked,
) -> Vec<Gate> {
    kept.ask_all(root, answerers, asked)
        .into_iter()
        .zip(answerers)
        .flat_map(|(answer, (plugin, answerer))| match answer {
            Ok(gates) => gates,
            Err(why) => failed(
                answerer.name(),
                &kept.names(root, plugin, answerer.name()),
                &why,
            ),
        })
        .collect()
}

/// The known gates `answerers` have answered for `asked`, as `kept` holds them now, asking nothing,
/// for a reader that must start no answerer's work (the sign-off lane). An answerer with nothing
/// kept for the task as it is now, or an error kept, gives none, and neither does a gate answered
/// `unknown`: what a reader that never asks cannot know leaves the Symphony's gate of its name to
/// decide, never a stand-in made of the names the answerer gave other tasks.
pub fn kept_theirs(
    kept: &Kept,
    root: &Path,
    answerers: &[(&str, &'static dyn Gates)],
    asked: &Asked,
) -> Vec<Gate> {
    answerers
        .iter()
        .filter_map(
            |(plugin, answerer)| match kept.kept(root, plugin, *answerer, asked) {
                Some(Ok(gates)) => Some(gates),
                Some(Err(_)) | None => None,
            },
        )
        .flatten()
        .filter(|gate| gate.state != GateState::Unknown)
        .collect()
}

/// The gates answerers of the project at `root`, `project` its file, each with its plugin's name,
/// in the plugins' order.
pub fn answerers(project: &Project, root: &Path) -> Vec<(&'static str, &'static dyn Gates)> {
    Plugins::at(project, root)
        .list()
        .iter()
        .flat_map(|plugin| {
            let plugin: &'static dyn symphony_plugin::Plugin = *plugin;
            plugin
                .gates()
                .into_iter()
                .map(move |answerer| (plugin.name(), answerer))
        })
        .collect()
}

/// What `answerer` says its answer for `asked` reads besides the tip; a panic is why it cannot.
fn told(answerer: &dyn Gates, root: &Path, asked: &Asked) -> Result<String, String> {
    catch_unwind(AssertUnwindSafe(|| {
        answerer.kept_while(root, &asked.task())
    }))
    .map_err(|panic| panicked(answerer.name(), "telling what its answer reads", &*panic))
}

/// Why `name` could not answer: it panicked while `doing` it, saying what `payload` says.
pub(crate) fn panicked(name: &str, doing: &str, payload: &(dyn Any + Send)) -> String {
    let said = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message");
    format!("{name} panicked {doing}: {said}")
}

/// Run `answerer` for `slot`'s task on a thread of its own, counted in `running` until it ends,
/// its answer (a panic an error) put in `slot` and counted in `answered`.
fn start(
    slot: &Arc<Slot>,
    root: &Path,
    answerer: &'static dyn Gates,
    running: &Arc<AtomicUsize>,
    answered: &Arc<AtomicUsize>,
) -> Result<(), String> {
    let (slot, root, answered) = (Arc::clone(slot), root.to_path_buf(), Arc::clone(answered));
    running.fetch_add(1, Ordering::SeqCst);
    let counted = Running(Arc::clone(running));
    std::thread::Builder::new()
        .name(format!("gates {}", answerer.name()))
        .spawn(move || {
            let _counted = counted;
            let answer = catch_unwind(AssertUnwindSafe(|| {
                answerer.answer(&root, &slot.asked.task(), slot.asked.tip.as_deref())
            }))
            .unwrap_or_else(|panic| Err(panicked(answerer.name(), "answering", &*panic)));
            // Counted under the answer's lock, so a read that finds the answer finds it counted.
            let mut done = slot.done.lock().recover();
            *done = Some((answer, Instant::now()));
            answered.fetch_add(1, Ordering::SeqCst);
            drop(done);
            slot.ready.notify_all();
        })
        .map(drop)
        .map_err(|e| format!("{} could not start: {e}", answerer.name()))
}

/// The tip of `branch` in the repository at `root`, a full commit id; `None` without one.
pub fn tip(root: &Path, branch: &str) -> Option<String> {
    commit(root, &format!("refs/heads/{branch}"))
}

/// The commit each of the revisions `specs` names in the repository at `root`, a full id, `None`
/// where it names none: the repository is opened once for them all.
fn commits<const N: usize>(root: &Path, specs: [&str; N]) -> [Option<String>; N] {
    let repo = gix::discover(root).ok();
    specs.map(|spec| {
        let id = (repo.as_ref()?)
            .rev_parse_single(format!("{spec}^{{commit}}").as_str())
            .ok()?;
        Some(id.to_string())
    })
}

/// Whether the commit `tip` in the repository at `root` holds work of its own: it is not on the
/// branch `default` (the project's default branch), as a branch made there and not yet committed
/// to is, or one already landed. A tip with no default branch to compare, or no merge base with
/// it, holds its own.
pub fn own_commits(root: &Path, tip: &str, default: &str) -> bool {
    let on_default = || -> Option<bool> {
        let repo = gix::discover(root).ok()?;
        let tip = repo.rev_parse_single(tip).ok()?.detach();
        let default = repo
            .rev_parse_single(format!("refs/heads/{default}^{{commit}}").as_str())
            .ok()?
            .detach();
        Some(repo.merge_base(tip, default).ok()?.detach() == tip)
    };
    !on_default().unwrap_or(false)
}

/// The commit the revision `spec` names in the repository at `root`, a full id; `None` when it
/// names none there.
fn commit(root: &Path, spec: &str) -> Option<String> {
    let repo = gix::discover(root).ok()?;
    let id = repo
        .rev_parse_single(format!("{spec}^{{commit}}").as_str())
        .ok()?;
    Some(id.to_string())
}

/// The work folder the board's recorder keeps runs in for the repository at `root`.
pub fn work_folder(root: &Path) -> Option<PathBuf> {
    let repo = gix::discover(root).ok()?;
    let common = repo.common_dir().canonicalize().ok()?;
    Some(recorded::in_common(&common))
}

/// The gates of the task `slug` in the project at `root`: the Symphony's own, `pending` the cards
/// waiting on the owner, and its plugins' answers through `kept`. The task, its thread and the
/// recorder's runs are as `kept` holds them while nothing moved ([`inputs`]), so a read with
/// nothing new asks the board once, for the task's stamp. The board's refusal or failure is the
/// answer's.
pub fn answer(
    store: &symphony_board::Store,
    slug: &str,
    root: &Path,
    pending: &Pending,
    kept: &Kept,
) -> Result<TaskGates, symphony_board::Error> {
    let task = kept.inputs.task(store, root, slug)?;
    // The recorder's history is read only for a task whose gates read its runs: the project's
    // check, or the stamps' tests.
    let runs = if Project::of(root).verdicts.check.is_empty() && !stamps::stamped(&task.record) {
        Arc::default()
    } else {
        kept.runs(root)
    };
    let read = Read {
        record: &task.record,
        thread: &task.thread,
        runs: &runs,
        branch: task.branch.clone(),
        ours: &|| task.ours(store).to_vec(),
    };
    Ok(put_together(&read, root, pending, kept))
}

/// [`answer`] of `record` from its `thread` and the recorder's `runs`, read already: the timeline
/// reads each once, for its own lanes and for the gates beside them.
pub fn answer_from(
    store: &symphony_board::Store,
    record: &symphony_board::Task,
    root: &Path,
    pending: &Pending,
    kept: &Kept,
    thread: &[symphony_board::Message],
    runs: &[Record],
) -> Result<TaskGates, String> {
    let branch =
        crate::taskview::approval_branch(store, record).map_err(|error| error.to_string())?;
    let read = Read {
        record,
        thread,
        runs,
        branch,
        ours: &|| stamps::family(store, &record.slug),
    };
    Ok(put_together(&read, root, pending, kept))
}

/// What a task's gates are put together from, read already.
struct Read<'a> {
    record: &'a symphony_board::Task,
    thread: &'a [symphony_board::Message],
    /// The recorder's runs.
    runs: &'a [Record],
    /// The branch the gates are answered for (`taskview::approval_branch`).
    branch: String,
    /// The task and the work under it ([`stamps::family`]), asked only when a gate reads their
    /// runs.
    ours: &'a dyn Fn() -> Vec<String>,
}

/// The gates of the task `read` holds, in the project at `root`: the Symphony's own from what was
/// read, the refs as they are now, the catalogue and whether the tip holds work of its own as
/// `kept` holds them ([`inputs`]), and the plugins' answers through `kept`.
fn put_together(read: &Read<'_>, root: &Path, pending: &Pending, kept: &Kept) -> TaskGates {
    let Read {
        record,
        thread,
        runs,
        ..
    } = *read;
    let branch = read.branch.clone();
    let project = Project::of(root);
    let [tip, default_tip] = commits(
        root,
        [
            format!("refs/heads/{branch}").as_str(),
            format!("refs/heads/{}", project.default_branch).as_str(),
        ],
    );
    let cards = linked_cards(&record.links, pending);
    let texts = std::iter::once(record.body.as_str()).chain(thread.iter().map(|m| m.text.as_str()));
    let posted = posted_cards(&cards, texts);
    // As before stamps: the review rounds of a top-level task, and the cards it posted.
    let before = || {
        let mut own = Vec::new();
        if record.parent.is_none() {
            own.push(review_of(
                &record.slug,
                &record.status,
                &record.holders("reviewer"),
                thread,
            ));
        }
        own.extend(owner(&posted));
        own
    };
    let mut own = match stamps::stamped(record).then(|| kept.inputs.catalogue(root)) {
        Some(Ok(catalogue)) => {
            // A landed task's branch goes with the sweep: its stamps stand on what its land merged.
            let at = tip.clone().or_else(|| {
                (record.status == "done")
                    .then(|| stamps::landed_tip(root, &record.slug, thread))
                    .flatten()
            });
            let needed = read::needed(&catalogue, record.needs().as_ref(), at.as_deref());
            // The work under the task is read only when its runs are.
            let ours = if needed.iter().any(|need| need == stamps::TESTS) {
                (read.ours)()
            } else {
                vec![record.slug.clone()]
            };
            let folder = read::folder(&project.artifacts, &record.slug);
            let by_cards =
                |tip: &str| (kept.inputs).posted(root, &record.slug, tip, default_tip.as_deref());
            let evidence = stamps::Evidence {
                thread,
                runs,
                ours: &ours,
                folder: &folder,
                branch: &branch,
                tip: at.as_deref(),
                cards: &posted,
                posted: &by_cards,
            };
            stamps::stamps(record, &catalogue, &evidence)
        }
        Some(Err(why)) => {
            let mut own = before();
            own.push(stamps::unreadable(&why));
            own
        }
        None => before(),
    };
    // The check reads the task's own work: none on an instruction, whose work lands with its
    // parent, nor on a branch with no commit of its own (none yet, or landed).
    let checked = tip.as_deref().filter(|tip| {
        !project.verdicts.check.is_empty()
            && !record.instruction
            && kept.inputs.own_commits(
                root,
                &record.slug,
                tip,
                (&project.default_branch, default_tip.as_deref()),
            )
    });
    if let Some(checked) = checked {
        own.push(check(
            &project.verdicts.check.join(" "),
            &branch,
            Some(checked),
            runs,
        ));
    }
    let asked = Asked {
        slug: record.slug.clone(),
        status: record.status.clone(),
        branch: branch.clone(),
        tip: tip.clone(),
    };
    let answerers = answerers(&project, root);
    let answer = TaskGates {
        slug: record.slug.clone(),
        branch,
        tip,
        gates: merged(own, theirs(kept, root, &answerers, &asked)),
    };
    kept.saw(root, &answer);
    answer
}

#[cfg(test)]
#[path = "gates/tests.rs"]
mod tests;
