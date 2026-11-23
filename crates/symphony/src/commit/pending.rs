//! An owner's save in a project that delivers by pull request (`[land] delivery`,
//! [`super::delivers`]): kept, shown as pending, and delivered as a task's branch, never
//! committed to the default branch here. An agent's reply or tidy made in the checkout on that
//! branch (`symphony reply`, `symphony status --tidy`) is kept the same way ([`keep`]), its task
//! saying whose it is.
//!
//! Such a save writes nothing in the main checkout and moves no ref of the default branch: a
//! tracked edit left there would stop every delivered land's reconciliation, the save's own
//! included. Its immutable edit is the committer's journal entry, on the disk before the save is
//! answered, and stays there until the remote default branch and this checkout both hold its
//! text. Until then:
//!
//! - **The pages show it.** [`shown`] is the document as the pending saves leave it, each edit
//!   merged onto the checkout's text in the order saved, and [`note`] says above it which saves
//!   wait and for what. A later save is written against that text, so two answers on one card
//!   follow each other.
//! - **A branch holds it.** [`OutputJob::deliver`] commits the edit over a private index on
//!   `docs/owner-save-N`, N the entry's own number, on the default branch here. A later save of
//!   the same document waits for the one before it to be delivered, and is committed on the
//!   default branch that then holds it: no branch stands on another's undelivered commit, and
//!   each head is assessed as its own. It is shown meanwhile, as every kept save is. An agent's
//!   save takes with it the later agent's saves whose turn has come in the same pass
//!   ([`joining`]), one commit on its branch, so a sitting of replies and tidies is one task; an
//!   owner's save is always one of its own. The saves one tidy keeps go in one commit or not at
//!   all ([`keep_together`]), so no page goes without the ledger that holds its comments, and a
//!   tidy that cannot go is on no way as a whole ([`stopped`]), as a save alone is.
//! - **A task delivers it.** The branch is filed on the board as the owner's task of that slug,
//!   under a receipt of the branch and its commit, so a pass run again files nothing twice, and
//!   moved to review. What it needs is its assessor's to say from its diff, as for any task; the
//!   land delivers it by its pull request (`symphony_board::land::delivery`). Nothing here asks
//!   for a merge, and a conflict with the default branch is settled on the branch as any
//!   task's is, its new head assessed again.
//! - **A receipt proves it.** The save is delivered once the land's receipt of its own task
//!   names a head that holds the save's commit and a squash on the fetched remote default
//!   branch ([`own`]), and its entry is dropped once the default branch here holds that squash
//!   too. The proof is that identity, so a conflict settled on the branch, which leaves the
//!   owner's words beside lines they were not written against, is proved like any other. Text
//!   found on the remote proves nothing by itself: brought by another task's delivery, that
//!   task's receipt is named on the save's own task, whose close is its orchestrator's; with no
//!   receipt at all the save is held for a person ([`standing`]).
//!
//! Every pass of the output worker (a start, each save, each refresh of the documents, those
//! asked while a pass is pending or running asking for one more together) takes each entry up
//! again. A step that fails (a board that does not answer, an edit that no longer
//! applies, a task cancelled, or closed with no receipt that proves the save) is kept in the entry
//! and said by [`note`]; the entry and the owner's words stay, and a save on no way to the
//! default branch no longer holds back the next of its document. Words the page cannot draw,
//! as they no longer apply to the default branch here, are said so by name, never left out
//! without a word. Nothing ever falls back to a commit on the default branch here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use symphony_board::land::delivery::{Journal, RECEIPTS, Receipt, Stage};
use symphony_board::lock::Recovered as _;

use super::{
    Edit, JOURNAL, OutputJob, QueuedPublication, apply, checked, common_dir, git, made, main_ref,
    merge, on_main, record, report, run, saved_uploads, shared_lock,
};
use crate::project::Project;

/// What the branch of a pending save is named, its journal entry's number after it.
pub const BRANCHES: &str = "docs/owner-save-";

/// The key a pending save's note is drawn under on its page.
pub const NOTE: &str = "pending-delivery";

/// How far the delivery of a save went, kept in its journal entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct Delivery {
    /// The default branch's commit here as the save was kept: no delivery older than it is the
    /// save's ([`covering`]). None in an entry a build that did not know the key journaled.
    #[serde(default)]
    base: String,
    /// The branch that holds the edit, once it is committed.
    #[serde(default)]
    branch: String,
    /// That branch's commit.
    #[serde(default)]
    tip: String,
    /// Whether its task is on the board.
    #[serde(default)]
    filed: bool,
    /// Whether that task closed with no delivery: cancelled, or closed by a landing or by hand
    /// with no receipt that proves this save ([`closed`]). The save is kept, and is on no way
    /// to the default branch.
    #[serde(default)]
    closed: bool,
    /// Why the last pass went no further, when it did not.
    #[serde(default)]
    problem: String,
    /// Who saved it, when not the owner: the agent's handle, or `agent` for one that gave none
    /// ([`keep`]). Empty, and left out of the entry, for an owner's save.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    by: String,
    /// The later agent's saves committed on this save's branch with it, by their entries'
    /// numbers ([`joining`]): each takes that branch up as its own. Left out of the entry with
    /// none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    members: Vec<u64>,
    /// The saves one tidy kept with this one ([`keep_together`]), which go to the default
    /// branch in one commit or not at all. None for a save kept alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    together: Option<Together>,
}

/// The saves of one keep: the first's entry number, and how many it kept. A ledger and the pages
/// a tidy took its comments from are one, so no page reaches the default branch without the
/// ledger that holds what it gave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct Together {
    first: u64,
    count: u64,
}

impl Delivery {
    /// The branch and task that deliver the save `sequence`: the one it was committed on, else
    /// the one named after it, or after the first of the saves its tidy kept with it
    /// ([`Together`]), which it is committed on unless it goes with an earlier agent's save
    /// ([`joining`]).
    fn slug(&self, sequence: u64) -> String {
        if self.branch.is_empty() {
            let first = self.together.map_or(sequence, |together| together.first);
            format!("{BRANCHES}{first}")
        } else {
            self.branch.clone()
        }
    }

    /// Whether the save is still on its way to the default branch: not closed with no
    /// delivery, and committed on its branch or not stopped short of it. A later save of its
    /// document waits for it.
    fn on_its_way(&self) -> bool {
        !self.closed && (!self.tip.is_empty() || self.problem.is_empty())
    }

    /// A save kept now, in the checkout at `root`.
    pub(super) fn saved(root: &Path) -> Self {
        Self {
            base: git(root, &["rev-parse", "--verify", "-q", &main_ref(root)]).unwrap_or_default(),
            ..Self::default()
        }
    }
}

/// Where a pending save stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Kept, its branch not made yet: just saved, or behind an earlier save of its document.
    Queued,
    /// On its branch, waiting for its pull request.
    Waiting,
    /// On its branch, and the land of its task has asked the host to merge its pull request
    /// (the land's [`Journal`]).
    Merging,
    /// On its branch, and its words no longer apply to the default branch here: the branch
    /// needs that conflict settled before its pull request can deliver it. The page cannot
    /// draw them meanwhile.
    Conflicted,
    /// A receipt proves it on the remote default branch; the one here has not followed yet.
    Following,
    /// Kept, and a step of its delivery failed: why.
    Held(String),
}

impl State {
    /// Where a save stands, in a sentence's words: `waiting for its pull request`. `default` is
    /// the default branch.
    pub fn said(&self, default: &str) -> String {
        match self {
            State::Queued => "kept, its branch not made yet".to_string(),
            State::Waiting => "waiting for its pull request".to_string(),
            State::Merging => "its land has asked for the merge of its pull request".into(),
            State::Conflicted => format!(
                "in conflict: its words no longer apply to `{default}` as it is here, so this \
                 page cannot show them; its branch holds them, and needs the conflict settled \
                 before its pull request delivers it"
            ),
            State::Following => {
                format!("delivered on the remote; `{default}` here has not followed yet")
            }
            State::Held(why) => format!("kept, not delivered: {}", flat(why)),
        }
    }
}

/// A save kept for delivery by pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Save {
    /// Its journal entry's number, the order saved.
    pub sequence: u64,
    /// The project-relative file it edits.
    pub file: String,
    /// The branch and task that deliver it.
    pub branch: String,
    /// Whether the page shows its words ([`shown`]): not once they no longer apply to the
    /// default branch here.
    pub shown: bool,
    /// Whether it is inert ([`inert`]): one of a tidy whose first save is not in the journal,
    /// which nothing shows or delivers, and whose state says why.
    pub inert: bool,
    pub state: State,
    /// Who saved it when not the owner, as its entry names them ([`keep`]); empty for the owner.
    pub by: String,
    /// For a settings edit of the project file ([`super::settings`]), the store keys it makes the
    /// project's; none for a document's save.
    pub keys: Vec<String>,
}

/// The git common folder of the checkout at `root`, found once for each: every page read asks.
fn common(root: &Path) -> Option<PathBuf> {
    static FOUND: Mutex<BTreeMap<PathBuf, PathBuf>> = Mutex::new(BTreeMap::new());
    let mut found = FOUND.lock().recover();
    if let Some(common) = found.get(root) {
        return Some(common.clone());
    }
    let common = common_dir(root).ok()?;
    found.insert(root.to_path_buf(), common.clone());
    Some(common)
}

/// The journal's folder below the git common folder of a checkout: what a server watches, so a
/// save another process kept there ([`keep`]) is shown and taken up at once, though no document
/// changed.
pub fn journal_below() -> String {
    format!("{}/{JOURNAL}", symphony_board::folders::SERVER)
}

/// The journal's folder for the checkout at `root`.
fn folder(root: &Path) -> Option<PathBuf> {
    Some(
        common(root)?
            .join(symphony_board::folders::SERVER)
            .join(JOURNAL),
    )
}

/// The number of the journal entry at `path`.
fn sequence_of(path: &Path) -> Option<u64> {
    path.file_stem()?.to_str()?.parse().ok()
}

/// The journal's pending saves, in the order saved. Read from the disk each time, as another
/// serving generation may have written or dropped one.
fn entries(root: &Path) -> Vec<(u64, QueuedPublication)> {
    let Some(read) = folder(root).and_then(|folder| std::fs::read_dir(folder).ok()) else {
        return Vec::new();
    };
    let mut found: Vec<(u64, QueuedPublication)> = read
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "json" {
                return None;
            }
            let sequence = sequence_of(&path)?;
            let saved: QueuedPublication =
                serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            saved.delivery.is_some().then_some((sequence, saved))
        })
        .collect();
    found.sort_by_key(|(sequence, _)| *sequence);
    found
}

/// The remote default branch as last fetched.
fn tracking(root: &Path) -> String {
    let remote = symphony_board::land::REMOTE;
    format!("refs/remotes/{remote}/{}", Project::of(root).default_branch)
}

/// Whether the commit `at` holds `edit`: merging it onto the page there changes nothing.
fn holds(root: &Path, at: &str, edit: &Edit) -> bool {
    let Ok(Ok(text)) = run(root, &["show", &format!("{at}:{}", edit.rel)], None, None).map(checked)
    else {
        return false;
    };
    merge(root, &text, &edit.base, &edit.new).is_ok_and(|merged| merged == text)
}

/// Whether the commit `within` holds the commit `commit`.
fn within(root: &Path, commit: &str, within: &str) -> bool {
    run(
        root,
        &["merge-base", "--is-ancestor", commit, within],
        None,
        None,
    )
    .is_ok_and(|done| done.status.success())
}

/// The lands' folder of the checkout at `root`, where a delivery's receipt is kept.
fn lands(root: &Path) -> Option<PathBuf> {
    Some(symphony_board::sweep::trigger::folder(&common(root)?).join(symphony_board::land::FOLDER))
}

/// Whether `receipt`, another task's, proves a delivery of `edit`: the commit it names is on
/// the remote default branch as fetched, and holds the edit.
fn proves(root: &Path, receipt: &Receipt, edit: &Edit) -> bool {
    within(root, &receipt.squash, &tracking(root)) && holds(root, &receipt.squash, edit)
}

/// The receipt of the save's own delivery: the one the land kept under its task, of a head that
/// holds the save's own commit `tip`, whose squash is on the remote default branch as fetched.
/// The binding is the receipt's identity, never the text: a conflict settled on the branch
/// leaves the owner's words beside lines the edit was not written against, and the land proved
/// the squash against that assessed head before it kept the receipt.
fn own(root: &Path, slug: &str, tip: &str) -> Option<Receipt> {
    Receipt::read(&lands(root)?, slug).filter(|receipt| {
        receipt.slug == slug
            && !tip.is_empty()
            && within(root, tip, &receipt.head)
            && within(root, &receipt.squash, &tracking(root))
    })
}

/// The receipt of another task's delivery that brought `edit` to the remote default branch: its
/// commit holds the edit, its parent does not, and it is no older than the save: not held by
/// `base`, the default branch as the save was kept. Equal text an earlier delivery brought is
/// some other owner action's, and proves nothing of this one; a save with no `base` is covered
/// by none. `own` is the save's own task, whose receipt is never another's.
fn covering(root: &Path, own: &str, edit: &Edit, base: &str) -> Option<Receipt> {
    if base.is_empty() {
        return None;
    }
    let read = std::fs::read_dir(lands(root)?.join(RECEIPTS)).ok()?;
    read.filter_map(|entry| {
        let text = std::fs::read(entry.ok()?.path()).ok()?;
        serde_json::from_slice::<Receipt>(&text).ok()
    })
    .find(|receipt| {
        receipt.slug != own
            && !within(root, &receipt.squash, base)
            && proves(root, receipt, edit)
            && !holds(root, &receipt.parent, edit)
    })
}

/// Where the delivery of a kept save stands, by git and the lands' receipts alone.
enum Standing {
    /// Not on the remote default branch.
    Open,
    /// Its own receipt proves it, or another task's does (`covered`); `here` once the default
    /// branch here holds that commit too.
    Proved {
        receipt: Box<Receipt>,
        covered: bool,
        here: bool,
    },
    /// The remote default branch holds its text and no receipt says which delivery brought it.
    Unproved,
}

fn standing(root: &Path, sequence: u64, edit: &Edit, delivery: &Delivery) -> Standing {
    let proved = |receipt: Receipt, covered| Standing::Proved {
        here: within(root, &receipt.squash, &main_ref(root)),
        receipt: Box::new(receipt),
        covered,
    };
    let slug = delivery.slug(sequence);
    if let Some(receipt) = own(root, &slug, &delivery.tip) {
        return proved(receipt, false);
    }
    if !holds(root, &tracking(root), edit) {
        return Standing::Open;
    }
    match covering(root, &slug, edit, &delivery.base) {
        Some(receipt) => proved(receipt, true),
        None => Standing::Unproved,
    }
}

/// Where the land of the task `slug` that delivers a save stands, by the delivery journal it
/// keeps while it runs: held for a person with why, or asking the host for the merge. None
/// before either, as with no land run yet: its task approved, the save still waits for its pull
/// request.
fn landing(root: &Path, slug: &str) -> Option<State> {
    let journal = Journal::read(&lands(root)?, slug)?;
    match journal.held {
        Some(why) => Some(State::Held(format!(
            "its delivery is held for reconciliation: {why}"
        ))),
        None => (journal.stage >= Stage::MergeRequested).then_some(State::Merging),
    }
}

/// What an unproved save is held with.
fn unproved(root: &Path) -> String {
    let default = &Project::of(root).default_branch;
    format!(
        "the remote {default} holds its text and no delivery receipt says which pull request \
         brought it: a person reconciles it, and removes its journal entry by name once settled"
    )
}

/// The pending saves of the checkout at `root`, in the order saved: none where it is not on its
/// default branch, and none of a save whose delivery a receipt proves and the default branch
/// here holds.
pub fn saves(root: &Path) -> Vec<Save> {
    listed(root, None)
}

/// [`saves`], of the document `only` alone when one is named: a page asks for its own.
fn listed(root: &Path, only: Option<&str>) -> Vec<Save> {
    let all = entries(root);
    let asked = |saved: &QueuedPublication| only.is_none_or(|file| saved.edit.rel == file);
    if !all.iter().any(|(_, saved)| asked(saved)) || !on_main(root) {
        return Vec::new();
    }
    // Each document as the saves before this one leave it, as `shown` folds them.
    let mut texts: BTreeMap<String, Option<String>> = BTreeMap::new();
    (all.iter())
        .filter(|(_, saved)| asked(saved))
        .filter_map(|(sequence, saved)| {
            let sequence = *sequence;
            let delivery = saved.delivery.clone()?;
            let edit = &saved.edit;
            let text = texts
                .entry(edit.rel.clone())
                .or_insert_with(|| std::fs::read_to_string(root.join(&edit.rel)).ok());
            let inert = inert(&all, &delivery);
            let merged = (text.as_deref())
                .filter(|_| !inert)
                .and_then(|text| merge(root, text, &edit.base, &edit.new).ok());
            let shown = merged.is_some();
            if let Some(merged) = merged {
                *text = Some(merged);
            }
            let branch = delivery.slug(sequence);
            let state = match standing(root, sequence, edit, &delivery) {
                Standing::Proved { here: true, .. } => return None,
                Standing::Proved { .. } => State::Following,
                Standing::Unproved => State::Held(unproved(root)),
                Standing::Open if !delivery.problem.is_empty() => State::Held(delivery.problem),
                Standing::Open if delivery.tip.is_empty() => {
                    match (delivery.together).and_then(|together| stopped(&all, together)) {
                        Some(why) => State::Held(why),
                        None => State::Queued,
                    }
                }
                Standing::Open => landing(root, &branch).unwrap_or(if shown {
                    State::Waiting
                } else {
                    State::Conflicted
                }),
            };
            let keys = match super::settings::changes(&saved.edit) {
                Some(Ok(changes)) => changes.into_iter().map(|change| change.key).collect(),
                _ => Vec::new(),
            };
            Some(Save {
                sequence,
                file: saved.edit.rel.clone(),
                branch,
                shown,
                inert,
                state,
                by: delivery.by,
                keys,
            })
        })
        .collect()
}

/// Whether the checkout at `root`, on its default branch, keeps a save for delivery: what a
/// server asks before it starts a committer only to take them up again
/// ([`crate::save::Saves::settle`]).
pub fn kept(root: &Path) -> bool {
    !entries(root).is_empty() && on_main(root)
}

/// Keep `edit`, an agent's, in the journal of the checkout at `root` as a save for delivery by
/// pull request: the entry an owner's save makes ([`OutputJob::new`]), naming `by` as who saved
/// it (an agent's handle; `agent` when blank). What `symphony reply` and `--tidy` do in a
/// checkout on the default branch of a project that delivers by pull request, where they write
/// no file: the pages show it at once ([`shown`]), and the next pass of a server's committer
/// commits it and files its task, behind any earlier save of the document, as for the owner's,
/// with the other agent's saves whose turn has come in that pass ([`joining`]). Its entry's
/// number, the order it was kept in.
///
/// `edit.base` is the document as [`shown`] leaves it, read under [`super::write_lock`], which
/// the caller holds until this returns: a save of the server's is written against that text
/// too, so neither is written against a page the other changed.
///
/// # Errors
///
/// The journal could not be written.
pub fn keep(root: &Path, edit: Edit, by: &str) -> Result<u64, String> {
    let backlog = super::Backlog::new(root)?;
    written(root, backlog.sequence, edit, by, None)?;
    Ok(backlog.sequence)
}

/// The journal entry `sequence` of an agent's save of `edit` by `by` (`agent` when blank),
/// written whole and renamed into place, its lease let go at once: the server takes it up.
fn written(
    root: &Path,
    sequence: u64,
    edit: Edit,
    by: &str,
    together: Option<Together>,
) -> Result<(), String> {
    let by = by.trim();
    let by = if by.is_empty() { "agent" } else { by };
    let path = OutputJob::folder(root)?.join(format!("{sequence:020}.json"));
    let saved = QueuedPublication {
        edit,
        publication: None,
        dirty_retry: false,
        dirty_reported: false,
        delivery: Some(Delivery {
            by: by.into(),
            together,
            ..Delivery::saved(root)
        }),
    };
    drop(OutputJob::write_journal(&path, &saved)?);
    Ok(())
}

/// What a tidy could not keep ([`keep_together`]).
#[derive(Debug)]
pub struct Unkept {
    /// Which of its edits failed.
    pub at: usize,
    /// How many of its later edits were written before it: they stay in the journal until a
    /// person removes them, each listed as held on its page's note ([`note`]), and nothing shows
    /// or delivers them.
    pub left: usize,
    /// Why.
    pub problem: String,
}

/// The edits of one tidy kept as saves of `by` that go together ([`Together`]): delivered in one
/// commit, led by the first, or none of them. One edit is kept alone. Their entries' numbers, in
/// the order of the edits.
///
/// Their numbers are drawn first, and the first's entry is written last: a group whose first
/// entry is in the journal was kept whole, and one without it is no save at all. Until it is
/// written a pass finds the others inert ([`stopped`]): none shown, delivered or waited for.
///
/// # Errors
///
/// A number could not be drawn or an entry written: which edit, how many after it were written,
/// and why. Without the first's entry nothing of the tidy is shown or delivered, and nothing waits
/// for it.
pub fn keep_together(root: &Path, edits: Vec<Edit>, by: &str) -> Result<Vec<u64>, Unkept> {
    let mut backlogs = Vec::new();
    for at in 0..edits.len() {
        let backlog = super::Backlog::new(root).map_err(|problem| Unkept {
            at,
            left: 0,
            problem,
        })?;
        backlogs.push(backlog);
    }
    let numbers: Vec<u64> = backlogs.iter().map(|backlog| backlog.sequence).collect();
    let together = (numbers.len() > 1).then(|| Together {
        first: numbers[0],
        count: numbers.len() as u64,
    });
    let last = edits.len().saturating_sub(1);
    for (at, edit) in edits.into_iter().enumerate().rev() {
        written(root, numbers[at], edit, by, together).map_err(|problem| Unkept {
            at,
            left: last - at,
            problem,
        })?;
    }
    Ok(numbers)
}

/// Whether the default branch of the checkout at `root` holds the file `file`: a save of a file
/// it does not hold cannot be committed there, nor delivered.
pub fn holds_file(root: &Path, file: &str) -> bool {
    git(
        root,
        &["cat-file", "-e", &format!("{}:{file}", main_ref(root))],
    )
    .is_ok()
}

/// The document `file` of the checkout at `root` as its pending saves leave it: `text`, the
/// checkout's own, with each kept edit merged onto it in the order saved. An edit the text
/// already holds changes nothing, and one that no longer applies is left out, which [`note`]
/// says of it by name ([`Save::shown`]).
pub fn shown(root: &Path, file: &str, text: String) -> String {
    let entries = entries(root);
    if !entries.iter().any(|(_, saved)| saved.edit.rel == file) || !on_main(root) {
        return text;
    }
    let mut shown = text;
    for (_, saved) in &entries {
        if saved.edit.rel == file
            && !(saved.delivery.as_ref()).is_some_and(|delivery| inert(&entries, delivery))
            && let Ok(merged) = merge(root, &shown, &saved.edit.base, &saved.edit.new)
        {
            shown = merged;
        }
    }
    shown
}

/// What a page says above the document `file` while saves of it wait for delivery: a Markdown
/// note naming each and where it stands. None with none waiting.
pub fn note(root: &Path, file: &str) -> Option<String> {
    let saves = listed(root, Some(file));
    if saves.is_empty() {
        return None;
    }
    let default = Project::of(root).default_branch.clone();
    let each: Vec<String> = saves
        .iter()
        .map(|save| {
            let task = &save.branch;
            let stands = save.state.said(&default);
            // Never left out of the page without a word: the owner's answer is kept, and why
            // it is not drawn is said beside it, where its state does not say it already (an
            // inert save's says none of its tidy is shown).
            let unshown = match (&save.shown, &save.state) {
                (true, _) | (_, State::Conflicted) => "",
                (false, _) if save.inert => "",
                (false, _) => {
                    "; its words no longer apply to this page as it is here, so they are not \
                     shown: they are kept"
                }
            };
            format!("`{task}` ({stands}{unshown})")
        })
        .collect();
    // A page that cannot draw one of them keeps it rather than shows it.
    let keeps = if saves.iter().all(|save| save.shown) {
        "shows"
    } else {
        "keeps"
    };
    Some(format!(
        "> **Pending delivery.** This page {keeps} {} that `{default}` does not hold yet: this \
         project delivers by pull request, so each waits for the task that delivers it: {}. \
         Nothing is lost meanwhile, and nothing here is committed to `{default}`.",
        whose(&saves),
        each.join(", ")
    ))
}

/// The saves a page's note names, by whose they are: the owner's, an agent's ([`keep`]), or
/// both.
fn whose(saves: &[Save]) -> String {
    let owners = saves.iter().filter(|save| save.by.is_empty()).count();
    match (saves.len(), owners) {
        (1, 1) => "an owner's save".into(),
        (1, _) => "an agent's save".into(),
        (n, owners) if owners == n => format!("{n} owner's saves"),
        (n, _) => format!("{n} saves"),
    }
}

/// The most characters of a failure's words the note carries: a board that does not answer
/// says how to set it up, at a length no page's note should have. The entry keeps them whole.
const WHY_CHARS: usize = 200;

/// `text` on one line and cut to [`WHY_CHARS`], so no failure's words break out of the note's
/// paragraph or fill the page.
fn flat(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::dashboard::cut(&line, WHY_CHARS)
}

/// Whether a save of `file` kept before the entry `sequence` is still on its way: the next save
/// of that document waits for it, as it was written against it. One whose task closed with no
/// delivery (cancelled, or closed unproved), or that could not be committed (it conflicts with
/// the default branch), is on no way, and is waited for no longer: the later save carries its
/// own change alone. A save one tidy kept with others (`together`) waits while any of them
/// would: it goes only with the first of them, and only once each is kept. Those that go
/// nowhere ([`stopped`]) are on no way as a whole, and waited for no longer.
fn behind(root: &Path, sequence: u64, file: &str, together: Option<Together>) -> bool {
    let entries = entries(root);
    let Some(together) = together else {
        return before(&entries, sequence, file, &[]);
    };
    // A save kept with others goes only with the first of them, once each of them may go.
    if together.first != sequence {
        return true;
    }
    let mates = group(&entries, together);
    if mates.len() as u64 != together.count {
        return true;
    }
    let own: Vec<u64> = mates.iter().map(|(number, _)| *number).collect();
    (mates.iter()).any(|(number, saved)| before(&entries, *number, &saved.edit.rel, &own))
}

/// Whether a save of `file` kept before the save `sequence`, and none of `except`, is still on
/// its way to the default branch.
fn before(entries: &[(u64, QueuedPublication)], sequence: u64, file: &str, except: &[u64]) -> bool {
    entries.iter().any(|(earlier, saved)| {
        *earlier < sequence
            && !except.contains(earlier)
            && saved.edit.rel == file
            && saved
                .delivery
                .as_ref()
                .is_some_and(|delivery| going(entries, delivery))
    })
}

/// Why the saves one tidy kept as `together` go nowhere, when they do not: on no way to the
/// default branch as a whole, as a save alone that could not be committed is, so none of them is
/// waited for. Their first not in the journal (the tidy could not keep it, as it is written
/// last, or it was dropped before its commit as another delivery brought it: the journal keeps
/// nothing that tells the two apart), held short of its branch or closed, or one of them gone
/// once all were kept. None while they may go, and once their first is committed, where each
/// entry stands for itself.
fn stopped(entries: &[(u64, QueuedPublication)], together: Together) -> Option<String> {
    let Some((_, first)) = entries.iter().find(|(number, _)| *number == together.first) else {
        return Some(NO_FIRST.into());
    };
    let delivery = first.delivery.as_ref()?;
    if !delivery.tip.is_empty() {
        return None;
    }
    let file = &first.edit.rel;
    if delivery.closed {
        return Some(format!(
            "kept with the save of {file} its tidy kept first, whose task closed with no \
             delivery"
        ));
    }
    if !delivery.problem.is_empty() {
        return Some(format!(
            "kept with the save of {file} its tidy kept first, which is held: {}",
            flat(&delivery.problem)
        ));
    }
    let left = group(entries, together).len() as u64;
    (left != together.count).then(|| short(left, together.count))
}

/// Why the saves of a tidy whose first is not in the journal go nowhere ([`stopped`]): true of a
/// first the tidy could not keep and of one another delivery brought.
const NO_FIRST: &str = "the first of the saves its tidy kept is not in the journal: the tidy \
                        could not keep it, or another delivery brought it, so none of them is \
                        shown or delivered";

/// Why the saves of a tidy no longer whole go nowhere: `left` of its `count`.
fn short(left: u64, count: u64) -> String {
    format!("{left} of the {count} saves its tidy kept are left, and none goes without the others")
}

/// Whether the save `delivery` of `entries` is still on its way to the default branch
/// ([`Delivery::on_its_way`]), and the saves its tidy kept with it are too ([`stopped`]): what a
/// later save of its document waits for.
fn going(entries: &[(u64, QueuedPublication)], delivery: &Delivery) -> bool {
    delivery.on_its_way()
        && (!delivery.tip.is_empty()
            || delivery
                .together
                .is_none_or(|together| stopped(entries, together).is_none()))
}

/// Whether the agent's save `delivery` is inert: one of a tidy whose first save is not in the
/// journal, never kept or dropped as another delivery brought it, which nothing shows or
/// delivers ([`keep_together`], [`stopped`]).
fn inert(entries: &[(u64, QueuedPublication)], delivery: &Delivery) -> bool {
    delivery.tip.is_empty()
        && delivery
            .together
            .is_some_and(|together| !entries.iter().any(|(number, _)| *number == together.first))
}

/// The saves of `entries` kept together as `together` ([`keep_together`]), in the order kept.
fn group(
    entries: &[(u64, QueuedPublication)],
    together: Together,
) -> Vec<&(u64, QueuedPublication)> {
    (entries.iter())
        .filter(|(_, saved)| {
            (saved.delivery.as_ref()).is_some_and(|delivery| delivery.together == Some(together))
        })
        .collect()
}

/// The agent's saves kept after the agent's save `leader` that go on its branch with it, in the
/// order kept: each not yet committed, stopped or taken by another, whose every earlier save of
/// its document is delivered, on no way to the default branch, or going with it. An owner's save
/// never goes with one, so it holds back every later save of its document, as any save still on
/// its way does: a reply waits behind the owner's save it answers. The saves one tidy kept
/// together ([`Together`]) go all of them or none: the leader's own always, as it leads only
/// once each of them may go ([`behind`]), and another's once each of them is kept and free.
fn joining(root: &Path, leader: u64) -> Vec<(u64, QueuedPublication)> {
    let entries = entries(root);
    let taken: std::collections::BTreeSet<u64> = (entries.iter())
        .filter_map(|(_, saved)| saved.delivery.as_ref())
        .flat_map(|delivery| delivery.members.iter().copied())
        .collect();
    let free = |sequence: u64, delivery: &Delivery| {
        sequence > leader
            && !delivery.by.is_empty()
            && delivery.tip.is_empty()
            && delivery.problem.is_empty()
            && !delivery.closed
            && !taken.contains(&sequence)
    };
    let own = (entries.iter())
        .find(|(sequence, _)| *sequence == leader)
        .and_then(|(_, saved)| saved.delivery.as_ref()?.together);
    let mut held_back = std::collections::BTreeSet::new();
    let mut decided = std::collections::BTreeMap::new();
    let mut joining = std::collections::BTreeSet::new();
    for (sequence, saved) in &entries {
        let Some(delivery) = &saved.delivery else {
            continue;
        };
        if *sequence == leader {
            continue;
        }
        let goes = match delivery.together {
            Some(together) if Some(together) == own => *sequence > leader,
            // Decided once for all of them, where the first stands: they are kept one after
            // another, under the lock every save takes.
            Some(together) => *decided.entry(together.first).or_insert_with(|| {
                let mates = group(&entries, together);
                mates.len() as u64 == together.count
                    && (mates.iter()).all(|(number, saved)| {
                        (saved.delivery.as_ref()).is_some_and(|mate| free(*number, mate))
                            && !held_back.contains(&saved.edit.rel)
                    })
            }),
            None => free(*sequence, delivery) && !held_back.contains(&saved.edit.rel),
        };
        if goes {
            joining.insert(*sequence);
        } else if going(&entries, delivery) {
            held_back.insert(saved.edit.rel.clone());
        }
    }
    (entries.into_iter())
        .filter(|(sequence, _)| joining.contains(sequence))
        .collect()
}

/// The branch an earlier agent's save took the save `sequence` onto ([`joining`]), with its
/// commit, once that one is committed: the branch that delivers it too.
fn led(root: &Path, sequence: u64) -> Option<(String, String)> {
    entries(root).into_iter().find_map(|(_, saved)| {
        let delivery = saved.delivery?;
        (delivery.members.contains(&sequence) && !delivery.tip.is_empty())
            .then_some((delivery.branch, delivery.tip))
    })
}

/// A save going on a branch: its entry's number, its edit, and who saved it, empty for the owner
/// ([`Delivery::by`]).
#[derive(Clone, Copy)]
struct Batched<'a> {
    sequence: u64,
    edit: &'a Edit,
    by: &'a str,
    together: Option<Together>,
}

/// `batch` cut into what goes or stays as one, in the order of each one's first save: the saves
/// one tidy kept together, wherever they stand, else each save alone.
fn units<'b, 'a>(batch: &'b [Batched<'a>]) -> Vec<Vec<&'b Batched<'a>>> {
    let mut units: Vec<Vec<&Batched>> = Vec::new();
    for save in batch {
        let unit = save.together.and_then(|together| {
            (units.iter()).position(|unit| unit[0].together == Some(together))
        });
        match unit {
            Some(at) => units[at].push(save),
            None => units.push(vec![save]),
        }
    }
    units
}

/// Of `batch`, the leader's save first, the saves that apply on the default branch's commit
/// `parent` each after those before it, in the order kept: the leader's alone fails, with those
/// it was kept together with, and a later one that no longer applies is left out with every
/// later save of its document, which was written against it. The saves one tidy kept together
/// apply all of them or are all left out ([`units`]). Their edits, and the numbers of those after
/// the leader.
fn applying(root: &Path, parent: &str, batch: &[Batched]) -> Result<(Vec<Edit>, Vec<u64>), String> {
    let units = units(batch);
    let mut edits: Vec<Edit> = units[0].iter().map(|save| save.edit.clone()).collect();
    apply(root, parent, &edits).map_err(String::from)?;
    let mut members: Vec<u64> = units[0][1..].iter().map(|save| save.sequence).collect();
    let mut left: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for unit in &units[1..] {
        let files = unit.iter().map(|save| save.edit.rel.as_str());
        if unit
            .iter()
            .any(|save| left.contains(save.edit.rel.as_str()))
        {
            left.extend(files);
            continue;
        }
        let before = edits.len();
        edits.extend(unit.iter().map(|save| save.edit.clone()));
        if apply(root, parent, &edits).is_ok() {
            members.extend(unit.iter().map(|save| save.sequence));
        } else {
            edits.truncate(before);
            left.extend(files);
        }
    }
    Ok((edits, members))
}

/// The saves of `batch` committed as one commit on the branch named after the first, on the
/// default branch here: the branch, its commit, and the numbers of the later saves it holds
/// ([`applying`]). A save alone, the owner's always, is one edit as it always was. A branch a
/// pass before made and never journaled is taken up, once it holds the first save, and holds
/// the later ones it was made of: made again from its parent, it is the same tree.
fn committed(root: &Path, batch: &[Batched]) -> Result<(String, String, Vec<u64>), String> {
    let first = &batch[0];
    let branch = format!("{BRANCHES}{}", first.sequence);
    let name = format!("refs/heads/{branch}");
    if let Ok(tip) = git(root, &["rev-parse", "--verify", "-q", &name]) {
        if batch.len() == 1 {
            if !holds(root, &name, first.edit) {
                return Err(format!(
                    "the branch {branch} is there and does not hold this save"
                ));
            }
            return Ok((branch, tip, Vec::new()));
        }
        // The saves it was made of were those kept before it: of the ones that go with it now,
        // the longest run from the first that makes its tree, those kept since left to wait.
        // Its tree, never the first save's text alone, which a later one may have written
        // beside.
        let parent = git(root, &["rev-parse", &format!("{name}^")])?;
        let tree = git(root, &["rev-parse", &format!("{name}^{{tree}}")])?;
        let units = units(batch);
        for count in (1..=units.len()).rev() {
            let prefix: Vec<Batched> = units[..count].iter().flatten().map(|save| **save).collect();
            let (edits, members) = applying(root, &parent, &prefix)?;
            let (saved, _) = saved_uploads(root, &edits);
            let made = made(root, &parent, &edits, &saved).map_err(String::from)?;
            if made.tree == tree {
                return Ok((branch, tip, members));
            }
        }
        return Err(format!(
            "the branch {branch} is there and does not hold this save and those that went with it"
        ));
    }
    let default = &Project::of(root).default_branch;
    let parent = git(root, &["rev-parse", "--verify", "-q", &main_ref(root)])?;
    let (edits, members) = if batch.len() == 1 {
        (vec![first.edit.clone()], Vec::new())
    } else {
        applying(root, &parent, batch)?
    };
    let (saved, refused) = saved_uploads(root, &edits);
    let made = made(root, &parent, &edits, &saved).map_err(String::from)?;
    if made.same {
        return Err(format!(
            "{default} here holds its text and the remote {default} does not: nothing here is \
             delivered until the two agree"
        ));
    }
    let message = if first.by.is_empty() {
        match super::settings::changes(first.edit) {
            Some(changes) => format!(
                "{}\n\nMade the project's on the Settings page.",
                super::settings::message(&changes?)
            ),
            None => format!("docs: owner verdicts\n\n{}", first.edit.rel),
        }
    } else {
        let held = batch
            .iter()
            .filter(|save| save.sequence == first.sequence || members.contains(&save.sequence));
        let (mut by, mut files): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
        for save in held {
            if !by.contains(&save.by) {
                by.push(save.by);
            }
            if !files.contains(&save.edit.rel.as_str()) {
                files.push(&save.edit.rel);
            }
        }
        let saves = if members.is_empty() { "save" } else { "saves" };
        format!(
            "docs: an agent's {saves} ({})\n\n{}",
            by.join(", "),
            files.join("\n")
        )
    };
    let commit = git(
        root,
        &["commit-tree", &made.tree, "-p", &parent, "-m", &message],
    )?;
    record(root, &commit)?;
    // Made only: a branch of that name is never moved.
    git(root, &["update-ref", &name, &commit, ""])?;
    for note in &refused {
        report(root, note);
    }
    Ok((branch, commit, members))
}

/// The branch filed on the board as the owner's task, in review: once, by its receipt. `saves`
/// are what the branch holds, the first save's and then those that went with it ([`joining`]),
/// each an edit and who saved it.
fn filed(root: &Path, delivery: &Delivery, saves: &[(Edit, String)]) -> Result<(), String> {
    use symphony_board::{BEFORE_WORK, NewTask};
    let owner = crate::postpone::OWNER;
    let store = crate::taskview::connect(root)?;
    let app = crate::verdicts::app_handle(&store).map_err(|e| e.to_string())?;
    let slug = &delivery.branch;
    let receipt = format!("document-save:{slug}:{}", delivery.tip);
    if (store.owner_saved(&receipt).map_err(|e| e.to_string())?).is_empty() {
        let project = Project::of(root);
        let default = &project.default_branch;
        // A settings edit is the owner's alone, made on the Settings page.
        let settings = (delivery.by.is_empty())
            .then(|| super::settings::changes(&saves[0].0))
            .flatten();
        let new = if let Some(changes) = settings {
            let (title, body) = super::settings::task(&changes?, slug, &delivery.tip, default);
            NewTask {
                slug: slug.clone(),
                title,
                by: owner.into(),
                body,
                priority: 1,
                ..NewTask::default()
            }
        } else {
            let (mut pages, mut files, mut by) = (Vec::new(), Vec::new(), Vec::new());
            for (edit, who) in saves {
                let page = project.route(&edit.rel).unwrap_or_else(|| edit.rel.clone());
                if !pages.contains(&page) {
                    pages.push(page);
                    files.push(edit.rel.as_str());
                }
                if !by.contains(&who.as_str()) {
                    by.push(who.as_str());
                }
            }
            let page = pages[0].clone();
            // An agent's saves, kept from the checkout on the default branch ([`keep`]), are filed
            // the same way, in words that say whose they are.
            let (title, saved, whose, own) = if delivery.by.is_empty() {
                (
                    format!("Deliver the owner's save of {page}"),
                    format!("The owner saved an answer on {page} in the Symphony"),
                    "the owner's save".to_string(),
                    " and its text is the owner's own",
                )
            } else if saves.len() == 1 {
                (
                    format!("Deliver an agent's save of {page}"),
                    format!(
                        "An agent ({}) saved a reply or a tidy of {page} in the checkout on {default}",
                        by.join(", ")
                    ),
                    "the agent's save".to_string(),
                    "",
                )
            } else {
                let more = match pages.len() {
                    1 => String::new(),
                    n => format!(" and {} more", n - 1),
                };
                (
                    format!("Deliver an agent's saves of {page}{more}"),
                    format!(
                        "An agent ({}) saved {} replies or tidied files of {} in the checkout on \
                         {default}",
                        by.join(", "),
                        saves.len(),
                        pages.join(", ")
                    ),
                    "the agent's saves".to_string(),
                    "",
                )
            };
            let files = files.join(", ");
            NewTask {
                slug: slug.clone(),
                title,
                by: owner.into(),
                body: format!(
                    "{saved}, and this project delivers by pull request, so the save is not on \
                     {default}: the branch {slug} holds it, as the one commit {} of {whose} of {}. \
                     Its work is done{own}: nothing on the branch is edited but to settle a conflict \
                     with {default}, by merging {default} into the branch and never by rebasing it: \
                     the delivery is proved by a head that holds that commit. Assess that diff for \
                     what it needs, then land it, which delivers it by its pull request. The \
                     Symphony keeps the save and shows it as pending until the delivery's receipt \
                     proves it on the remote {default}.",
                    delivery.tip, files
                ),
                priority: 1,
                links: pages,
                ..NewTask::default()
            }
        };
        store
            .create_owner_task(&new, &receipt)
            .map_err(|e| e.to_string())?;
    }
    let mut task = store.get(slug).map_err(|e| e.to_string())?;
    // Its work is done, and nobody holds a role on it: held blocked by the Symphony's own
    // session, then sent to review by the owner whose save it is, the one move to review the
    // board gives the owner. There it is assessed.
    if BEFORE_WORK.contains(&task.status.as_str()) {
        task = (store.move_to(slug, "blocked", &app)).map_err(|e| e.to_string())?;
    }
    if task.status == "blocked" {
        (store.move_to(slug, "review", owner)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Why the task of a filed save delivers nothing more, when it closed with no delivery: it was
/// cancelled, or a landing closed it (or a hand did) and no receipt proves this save ([`own`],
/// read before this). A task its reviewer approved is done on the board before its land, and
/// through the whole wait for the host's merge: while that approve stands with no landing after
/// it ([`symphony_board::land::approval`]) the save is on its way, and still waited for.
fn closed(root: &Path, delivery: &Delivery) -> Result<Option<String>, String> {
    let store = crate::taskview::connect(root)?;
    let slug = &delivery.branch;
    let task = store.get(slug).map_err(|e| e.to_string())?;
    Ok(match task.status.as_str() {
        "cancelled" => Some(format!(
            "its task {slug} was cancelled: the owner's words are kept here and are not delivered"
        )),
        "done" => {
            let thread = store.thread(slug).map_err(|e| e.to_string())?;
            symphony_board::land::approval(&task, &thread)
                .is_err()
                .then(|| {
                    format!(
                        "its task {slug} was closed and no delivery receipt proves this save on \
                         the remote: the owner's words are kept here, and a person reconciles \
                         them and removes its journal entry by name once settled"
                    )
                })
        }
        _ => None,
    })
}

/// Whether a delivered save's entry is still kept: for a settings edit ([`super::settings`]), until
/// each store entry it made the project's is removed ([`super::settings::cleared`]), which a
/// later pass tries again where the board did not answer, saying why meanwhile; none kept for a
/// document's save.
fn cleared(root: &Path, edit: &Edit, delivery: &mut Delivery) -> bool {
    let Some(Ok(changes)) = super::settings::changes(edit) else {
        return false;
    };
    let removed =
        crate::taskview::connect(root).and_then(|store| super::settings::cleared(&store, &changes));
    match removed {
        Ok(()) => false,
        Err(why) => {
            delivery.problem = format!(
                "delivered, and the board's store still holds its entry, which was not removed: \
                 {why}"
            );
            true
        }
    }
}

/// Said once on the task of a save another task's delivery brought: nothing more is delivered
/// by it, and closing it is its orchestrator's.
fn covered(
    root: &Path,
    sequence: u64,
    edit: &Edit,
    delivery: &Delivery,
    by: &Receipt,
) -> Result<(), String> {
    let store = crate::taskview::connect(root)?;
    let brought = format!(
        "the delivery of {} brought it as {} (pull request #{} of {}, from its head {})",
        by.slug, by.squash, by.pull, by.name, by.head
    );
    let alone = delivery.branch == format!("{BRANCHES}{sequence}") && delivery.members.is_empty();
    // A save that went with others ([`joining`]) is one of the task's: the task is covered only
    // once each of them is said to be.
    let (text, key) = if alone {
        (
            format!(
                "The {} save this task delivers is on the remote default branch already: \
                 {brought}. Nothing more is delivered by {}: close it as covered by that landing.",
                if delivery.by.is_empty() {
                    "owner's"
                } else {
                    "agent's"
                },
                delivery.branch
            ),
            format!("document-save-covered:{}", by.squash),
        )
    } else {
        (
            format!(
                "The save of {} kept as {sequence}, one of those {} delivers, is on the remote \
                 default branch already: {brought}. Close it as covered by that landing only once \
                 each save it delivers is said so here.",
                edit.rel, delivery.branch
            ),
            format!("document-save-covered:{}:{sequence}", by.squash),
        )
    };
    store
        .say_keyed(
            &delivery.branch,
            crate::postpone::OWNER,
            &text,
            Some("@orchestrator"),
            &key,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

impl OutputJob {
    /// One pass over a save the host delivers. Whether its journal entry is kept: a save that
    /// still waits.
    ///
    /// - A receipt proves its delivery ([`standing`]): dropped once the default branch here
    ///   holds that commit, after saying so on its own task where another task's delivery
    ///   brought it. Kept while the checkout has not followed.
    /// - The remote holds its text and no receipt proves how: kept, held for a person.
    /// - Else committed on its branch, once no earlier save of the document waits, and filed as
    ///   its task, as far as each step goes; what stopped a step is kept in the entry.
    ///
    /// # Errors
    ///
    /// The journal could not be written.
    pub(super) fn deliver(&mut self, root: &Path) -> Result<bool, String> {
        let edit = self.job.edit.clone();
        let sequence = sequence_of(&self.path)
            .ok_or_else(|| format!("{}: not a journal entry's name", self.path.display()))?;
        let _commits = shared_lock(root, "verdict-commits.lock")?;
        let mut delivery = self.saved.delivery.clone().unwrap_or_default();
        let kept = match standing(root, sequence, &edit, &delivery) {
            Standing::Proved { here: false, .. } | Standing::Unproved => true,
            Standing::Proved { covered: false, .. } => cleared(root, &edit, &mut delivery),
            Standing::Proved { receipt, .. } => {
                // Its own task, if it has one, is told before the entry goes.
                match (delivery.filed).then(|| covered(root, sequence, &edit, &delivery, &receipt))
                {
                    Some(Err(why)) => {
                        delivery.problem = format!("its task was not told it is covered: {why}");
                        true
                    }
                    _ => cleared(root, &edit, &mut delivery),
                }
            }
            Standing::Open => {
                self.open(root, sequence, &edit, &mut delivery)?;
                true
            }
        };
        if kept && self.saved.delivery.as_ref() != Some(&delivery) {
            self.saved.delivery = Some(delivery);
            self.save()?;
        }
        Ok(kept)
    }

    /// The steps of a save not yet on the remote: its branch, then its task.
    fn open(
        &mut self,
        root: &Path,
        sequence: u64,
        edit: &Edit,
        delivery: &mut Delivery,
    ) -> Result<(), String> {
        if delivery.tip.is_empty() {
            if let Some((branch, tip)) = led(root, sequence) {
                // Gone with an earlier agent's save onto its branch: that branch delivers it.
                (delivery.branch, delivery.tip) = (branch, tip);
                delivery.problem.clear();
                self.saved.delivery = Some(delivery.clone());
                self.save()?;
            } else {
                // A tidy's first save whose group lost one of them once kept goes nowhere, and
                // says so: none of them is waited for ([`stopped`]).
                if let Some(together) = delivery.together.filter(|t| t.first == sequence) {
                    let left = group(&entries(root), together).len() as u64;
                    if left != together.count {
                        delivery.problem = short(left, together.count);
                        return Ok(());
                    }
                }
                if behind(root, sequence, &edit.rel, delivery.together) {
                    delivery.problem.clear();
                    return Ok(());
                }
                // An agent's save takes the agent's saves whose turn has come with it.
                let by = delivery.by.clone();
                let joining = if by.is_empty() {
                    Vec::new()
                } else {
                    joining(root, sequence)
                };
                let mut batch = vec![Batched {
                    sequence,
                    edit,
                    by: &by,
                    together: delivery.together,
                }];
                batch.extend(joining.iter().filter_map(|(number, saved)| {
                    let delivery = saved.delivery.as_ref()?;
                    Some(Batched {
                        sequence: *number,
                        edit: &saved.edit,
                        by: &delivery.by,
                        together: delivery.together,
                    })
                }));
                match committed(root, &batch) {
                    Ok((branch, tip, members)) => {
                        (delivery.branch, delivery.tip) = (branch, tip);
                        delivery.members = members;
                        delivery.problem.clear();
                        // Journaled before its task is filed: the task's receipt names this
                        // commit, and the saves that went with it take the branch up from here.
                        self.saved.delivery = Some(delivery.clone());
                        self.save()?;
                    }
                    Err(why) => {
                        delivery.problem = why;
                        return Ok(());
                    }
                }
            }
        }
        if !delivery.filed {
            if delivery.branch == format!("{BRANCHES}{sequence}") {
                let mut saves = vec![(edit.clone(), delivery.by.clone())];
                let entries = entries(root);
                for member in &delivery.members {
                    if let Some((_, saved)) = entries.iter().find(|(number, _)| number == member) {
                        let by = saved.delivery.as_ref().map(|d| d.by.clone());
                        saves.push((saved.edit.clone(), by.unwrap_or_default()));
                    }
                }
                if let Err(why) = filed(root, delivery, &saves) {
                    delivery.problem = format!("its task is not on the board yet: {why}");
                    return Ok(());
                }
            } else {
                // Its task is the one of the save it went with, which files it.
                let found = crate::taskview::connect(root)
                    .and_then(|store| store.get(&delivery.branch).map_err(|e| e.to_string()));
                if let Err(why) = found {
                    delivery.problem = format!(
                        "its task {} is not on the board yet, filed by the save it went with: \
                         {why}",
                        delivery.branch
                    );
                    return Ok(());
                }
            }
            delivery.filed = true;
        }
        match closed(root, delivery) {
            Ok(why) => {
                delivery.closed = why.is_some();
                delivery.problem = why.unwrap_or_default();
            }
            Err(why) => delivery.problem = format!("its task could not be read: {why}"),
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "pending_tests.rs"]
mod tests;
