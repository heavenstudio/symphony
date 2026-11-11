//! A task's gates (`symphony_api::gates`): what it needs to land, at its branch's tip. The Symphony
//! answers three of its own: [`review`], the board's reviewer verdicts ([`review_rounds`]);
//! [`check`], the project's `[verdicts] check` at the tip, from the run records the board's
//! recorder keeps (docs/MACHINE_WORK.md); and [`owner`], a card the task posted that waits on the
//! owner, by the sign-off lane's rule ([`posted_cards`]). A project's plugins answer its own
//! through the gates seam (`symphony_plugin::Gates`), each answer [`Kept`] by the branch's tip, and
//! a plugin's gate replaces the Symphony's of its name ([`merged`]).
//!
//! [`answer`] puts them together for the task page's `/api/task-gates`; the board's other readers
//! of a gate ask it the same way, through the same [`Kept`], so a plugin is asked once a tip.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use symphony_api::gates::{CHECK, OWNER, REVIEW, ranked};
use symphony_api::{Gate, GatePast, GateState, Pending, PendingItem, TaskGates};
use symphony_board::cli::machine_work::recorded::{self, Record, Step};
use symphony_plugin::{Gates, TaskRef};

use crate::plugins::Plugins;
use crate::project::Project;

/// How long a page waits for a plugin's answer before it says the gate is unknown. The answer
/// still arrives, and is kept, for the next read.
pub const WAIT: Duration = Duration::from_secs(10);
/// How long a plugin's error is shown before it is asked again for the same tip.
pub const RETRY: Duration = Duration::from_secs(30);

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

/// The review rounds on `thread`, oldest first. Every reader of a task's review rounds reads them
/// here, so where rounds are recorded can change in this one place.
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

/// The review gate of the task `slug`, from its `rounds`, its `reviewers` now and its `status`:
/// met by the last round's approval, missing until one, the earlier rounds its past.
pub fn review(slug: &str, status: &str, reviewers: &[&str], rounds: &[Round]) -> Gate {
    let total = rounds.len();
    let state = |round: &Round| {
        if round.approved {
            GateState::Met
        } else {
            GateState::Missing
        }
    };
    let detail = |n: usize, round: &Round| {
        if round.approved {
            format!("Approved in round {n} of {total}")
        } else if round.said.is_empty() {
            format!("Changes asked in round {n}")
        } else {
            format!("Changes asked in round {n}: {}", first_line(&round.said))
        }
    };
    let Some(last) = rounds.last() else {
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
        state: state(last),
        by: Some(last.by.clone()),
        at: Some(last.at),
        commit: last.tip.clone(),
        detail: detail(total, last),
        link: Some(format!("/task/{slug}#message-{}", last.id)),
        past: rounds[..total - 1]
            .iter()
            .enumerate()
            .map(|(index, round)| GatePast {
                state: state(round),
                at: round.at,
                by: Some(round.by.clone()),
                commit: round.tip.clone(),
                detail: detail(index + 1, round),
            })
            .collect(),
        ..Gate::default()
    }
}

/// Every run the work folder `folder` holds, each once: the history's, and the operations still in
/// the folder, which say more of one the history keeps too. A folder not there holds none.
pub fn runs(folder: &Path) -> Vec<Record> {
    let mut found: Vec<Record> = recorded::history(folder).unwrap_or_default();
    for (live, _) in recorded::read(folder).records {
        match found
            .iter_mut()
            .find(|run| run.operation_id == live.operation_id)
        {
            Some(kept) => *kept = live,
            None => found.push(live),
        }
    }
    found
}

/// The check gate: the newest run of the command `label` at `tip`, green (met), red or still going
/// (missing), or none there (unknown); the earlier finished runs of it on `branch` or at `tip`, its
/// past.
pub fn check(label: &str, branch: &str, tip: Option<&str>, runs: &[Record]) -> Gate {
    let short = |commit: &str| commit.chars().take(7).collect::<String>();
    let ours = |run: &&Record| {
        run.label == label
            && run
                .commit
                .as_deref()
                .is_some_and(|commit| Some(commit) == tip || run.branch.as_deref() == Some(branch))
    };
    let began = |run: &Record| run.started_at.unwrap_or(run.queued_at);
    let mut mine: Vec<&Record> = runs.iter().filter(ours).collect();
    mine.sort_by(|a, b| began(a).total_cmp(&began(b)));
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
    let current = tip.and_then(|tip| {
        mine.iter()
            .rposition(|run| run.commit.as_deref() == Some(tip))
    });
    let mut past = mine
        .iter()
        .enumerate()
        .filter(|(index, run)| Some(*index) != current && run.state == Step::Finished)
        .filter_map(|(_, run)| {
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
    past.sort_by(|a, b| a.at.total_cmp(&b.at));
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
    let Some(run) = current.map(|index| mine[index]) else {
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
    posted.sort_by_key(|card| cards.iter().position(|c| std::ptr::eq(*c, *card)));
    posted
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
/// ([`symphony_api::gates::ranked`]).
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
    ranked(gates)
}

/// The gate shown for an answerer called `name` that could not answer: unknown, `why` its detail.
pub fn failed(name: &str, why: &str) -> Gate {
    Gate {
        name: name.into(),
        state: GateState::Unknown,
        detail: first_line(why).to_string(),
        ..Gate::default()
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default().trim()
}

/// A task as a gates answerer is asked about it, owned, so the answer can run on a thread of its
/// own.
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

/// One answerer's answer for one task: asked at a branch, tip and [`Gates::kept_while`], and what
/// came back, once it did.
struct Slot {
    branch: String,
    tip: Option<String>,
    kept_while: String,
    asked: Instant,
    done: Mutex<Option<(Answer, Instant)>>,
    ready: Condvar,
}

impl Slot {
    fn holds(&self, asked: &Asked, kept_while: &str) -> bool {
        self.branch == asked.branch && self.tip == asked.tip && self.kept_while == kept_while
    }
}

/// Whose answer a slot holds: an answerer of a plugin, for a task of a project.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    root: PathBuf,
    plugin: String,
    answerer: String,
    slug: String,
}

impl Key {
    fn new(root: &Path, plugin: &str, answerer: &str, slug: &str) -> Key {
        Key {
            root: root.to_path_buf(),
            plugin: plugin.to_string(),
            answerer: answerer.to_string(),
            slug: slug.to_string(),
        }
    }
}

/// The plugins' answers, kept by the task's branch tip and what each answerer says it reads besides
/// ([`Gates::kept_while`]): one answer an answerer and task, the newest asked, so an answer that
/// starts a landing binary runs once a tip. A read waits for an answer at most `wait` from when
/// it was asked, then says it is unknown while the answer goes on, to be kept for the next read;
/// no second call starts while one runs. An error is kept for `retry`, then asked again.
pub struct Kept {
    slots: Mutex<HashMap<Key, Arc<Slot>>>,
    wait: Duration,
    retry: Duration,
}

impl Default for Kept {
    /// Waiting [`WAIT`], an error asked again after [`RETRY`].
    fn default() -> Self {
        Self::new(WAIT, RETRY)
    }
}

impl Kept {
    pub fn new(wait: Duration, retry: Duration) -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            wait,
            retry,
        }
    }

    /// The answer of `answerer`, of the plugin `plugin`, for `asked` in the project at `root`: kept,
    /// or asked now, on a thread of its own.
    pub fn ask(
        &self,
        root: &Path,
        plugin: &str,
        answerer: &'static dyn Gates,
        asked: &Asked,
    ) -> Answer {
        let kept_while = answerer.kept_while(root, &asked.task());
        let key = Key::new(root, plugin, answerer.name(), &asked.slug);
        let slot = {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            let current = slots
                .get(&key)
                .filter(|slot| slot.holds(asked, &kept_while))
                .filter(|slot| {
                    let done = slot.done.lock().unwrap_or_else(PoisonError::into_inner);
                    match &*done {
                        Some((Err(_), at)) => at.elapsed() < self.retry,
                        _ => true,
                    }
                })
                .cloned();
            match current {
                Some(slot) => slot,
                None => {
                    let slot = Arc::new(Slot {
                        branch: asked.branch.clone(),
                        tip: asked.tip.clone(),
                        kept_while,
                        asked: Instant::now(),
                        done: Mutex::new(None),
                        ready: Condvar::new(),
                    });
                    if let Err(why) = start(&slot, root, answerer, asked) {
                        *slot.done.lock().unwrap_or_else(PoisonError::into_inner) =
                            Some((Err(why), Instant::now()));
                    }
                    slots.insert(key, Arc::clone(&slot));
                    slot
                }
            }
        };
        let deadline = slot.asked + self.wait;
        let mut done = slot.done.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some((answer, _)) = &*done {
                return answer.clone();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!(
                    "{} gave no answer within {} s",
                    answerer.name(),
                    self.wait.as_secs_f64()
                ));
            }
            done = slot
                .ready
                .wait_timeout(done, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// What is kept of `answerer`'s answer, of the plugin `plugin`, for the task `slug` at `branch`
    /// and `tip` in the project at `root`, asking nothing: `None` when nothing came back for that
    /// tip yet.
    pub fn kept(
        &self,
        root: &Path,
        plugin: &str,
        answerer: &str,
        slug: &str,
        branch: &str,
        tip: Option<&str>,
    ) -> Option<Answer> {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = slots
            .get(&Key::new(root, plugin, answerer, slug))
            .filter(|slot| slot.branch == branch && slot.tip.as_deref() == tip)?;
        let done = slot.done.lock().unwrap_or_else(PoisonError::into_inner);
        done.as_ref().map(|(answer, _)| answer.clone())
    }
}

/// Run `answerer` for `asked` on a thread of its own, its answer (a panic an error) put in `slot`.
fn start(
    slot: &Arc<Slot>,
    root: &Path,
    answerer: &'static dyn Gates,
    asked: &Asked,
) -> Result<(), String> {
    let (slot, root, asked) = (Arc::clone(slot), root.to_path_buf(), asked.clone());
    std::thread::Builder::new()
        .name(format!("gates {}", answerer.name()))
        .spawn(move || {
            let answer = catch_unwind(AssertUnwindSafe(|| {
                answerer.answer(&root, &asked.task(), asked.tip.as_deref())
            }))
            .unwrap_or_else(|_| Err(format!("{} panicked", answerer.name())));
            *slot.done.lock().unwrap_or_else(PoisonError::into_inner) =
                Some((answer, Instant::now()));
            slot.ready.notify_all();
        })
        .map(drop)
        .map_err(|e| format!("{} could not start: {e}", answerer.name()))
}

/// The tip of `branch` in the repository at `root`, a full commit id; `None` without one.
pub fn tip(root: &Path, branch: &str) -> Option<String> {
    let repo = gix::discover(root).ok()?;
    let id = repo
        .rev_parse_single(format!("refs/heads/{branch}^{{commit}}").as_str())
        .ok()?;
    Some(id.to_string())
}

/// The work folder the board's recorder keeps runs in for the repository at `root`.
fn work_folder(root: &Path) -> Option<PathBuf> {
    let repo = gix::discover(root).ok()?;
    let common = repo.common_dir().canonicalize().ok()?;
    Some(recorded::in_common(&common))
}

/// The gates of `record` in the project at `root`: the Symphony's own, `pending` the cards waiting
/// on the owner, and its plugins' answers through `kept`.
pub fn answer(
    store: &symphony_board::Store,
    record: &symphony_board::Task,
    root: &Path,
    pending: &Pending,
    kept: &Kept,
) -> Result<TaskGates, String> {
    let branch =
        crate::taskview::approval_branch(store, record).map_err(|error| error.to_string())?;
    let tip = tip(root, &branch);
    let thread = store.thread(&record.slug).map_err(|e| e.to_string())?;
    let project = Project::of(root);
    let mut own = Vec::new();
    if record.parent.is_none() {
        own.push(review(
            &record.slug,
            &record.status,
            &record.holders("reviewer"),
            &review_rounds(&thread),
        ));
    }
    if !project.verdicts.check.is_empty() {
        let runs = work_folder(root)
            .map(|folder| runs(&folder))
            .unwrap_or_default();
        own.push(check(
            &project.verdicts.check.join(" "),
            &branch,
            tip.as_deref(),
            &runs,
        ));
    }
    let cards = linked_cards(&record.links, pending);
    let texts = std::iter::once(record.body.as_str()).chain(thread.iter().map(|m| m.text.as_str()));
    own.extend(owner(&posted_cards(&cards, texts)));
    let asked = Asked {
        slug: record.slug.clone(),
        status: record.status.clone(),
        branch: branch.clone(),
        tip: tip.clone(),
    };
    let mut theirs = Vec::new();
    for plugin in Plugins::for_project(&project).list() {
        let plugin: &'static dyn symphony_plugin::Plugin = *plugin;
        for answerer in plugin.gates() {
            match kept.ask(root, plugin.name(), answerer, &asked) {
                Ok(gates) => theirs.extend(gates),
                Err(why) => theirs.push(failed(answerer.name(), &why)),
            }
        }
    }
    Ok(TaskGates {
        slug: record.slug.clone(),
        branch,
        tip,
        gates: merged(own, theirs),
    })
}

#[cfg(test)]
#[path = "gates/tests.rs"]
mod tests;
