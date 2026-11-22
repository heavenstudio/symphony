//! An owner's save in a project that delivers by pull request (`[land] delivery`,
//! [`super::delivers`]): kept, shown as pending, and delivered as a task's branch, never
//! committed to the default branch here.
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
//!   each head is assessed as its own. It is shown meanwhile, as every kept save is.
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
    Edit, JOURNAL, OutputJob, QueuedPublication, checked, common_dir, git, made, main_ref, merge,
    on_main, record, report, run, saved_uploads, shared_lock,
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
}

impl Delivery {
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
    pub state: State,
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
fn own(root: &Path, sequence: u64, tip: &str) -> Option<Receipt> {
    let slug = format!("{BRANCHES}{sequence}");
    Receipt::read(&lands(root)?, &slug).filter(|receipt| {
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
/// by none.
fn covering(root: &Path, sequence: u64, edit: &Edit, base: &str) -> Option<Receipt> {
    if base.is_empty() {
        return None;
    }
    let own = format!("{BRANCHES}{sequence}");
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
    if let Some(receipt) = own(root, sequence, &delivery.tip) {
        return proved(receipt, false);
    }
    if !holds(root, &tracking(root), edit) {
        return Standing::Open;
    }
    match covering(root, sequence, edit, &delivery.base) {
        Some(receipt) => proved(receipt, true),
        None => Standing::Unproved,
    }
}

/// Where the land of the save `sequence`'s task stands, by the delivery journal it keeps while
/// it runs: held for a person with why, or asking the host for the merge. None before either,
/// as with no land run yet: its task approved, the save still waits for its pull request.
fn landing(root: &Path, sequence: u64) -> Option<State> {
    let journal = Journal::read(&lands(root)?, &format!("{BRANCHES}{sequence}"))?;
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
    let mut entries = entries(root);
    entries.retain(|(_, saved)| only.is_none_or(|file| saved.edit.rel == file));
    if entries.is_empty() || !on_main(root) {
        return Vec::new();
    }
    // Each document as the saves before this one leave it, as `shown` folds them.
    let mut texts: BTreeMap<String, Option<String>> = BTreeMap::new();
    entries
        .into_iter()
        .filter_map(|(sequence, saved)| {
            let delivery = saved.delivery?;
            let edit = &saved.edit;
            let text = texts
                .entry(edit.rel.clone())
                .or_insert_with(|| std::fs::read_to_string(root.join(&edit.rel)).ok());
            let merged =
                (text.as_deref()).and_then(|text| merge(root, text, &edit.base, &edit.new).ok());
            let shown = merged.is_some();
            if let Some(merged) = merged {
                *text = Some(merged);
            }
            let state = match standing(root, sequence, edit, &delivery) {
                Standing::Proved { here: true, .. } => return None,
                Standing::Proved { .. } => State::Following,
                Standing::Unproved => State::Held(unproved(root)),
                Standing::Open if !delivery.problem.is_empty() => State::Held(delivery.problem),
                Standing::Open if delivery.tip.is_empty() => State::Queued,
                Standing::Open => landing(root, sequence).unwrap_or(if shown {
                    State::Waiting
                } else {
                    State::Conflicted
                }),
            };
            Some(Save {
                sequence,
                file: saved.edit.rel,
                branch: format!("{BRANCHES}{sequence}"),
                shown,
                state,
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
    for (_, saved) in entries {
        if saved.edit.rel == file
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
            let stands = match &save.state {
                State::Queued => "kept, its branch not made yet".to_string(),
                State::Waiting => "waiting for its pull request".to_string(),
                State::Merging => "its land has asked for the merge of its pull request".into(),
                State::Conflicted => format!(
                    "in conflict: its words no longer apply to `{default}` as it is here, so \
                     this page cannot show them; its branch holds them, and needs the conflict \
                     settled before its pull request delivers it"
                ),
                State::Following => {
                    format!("delivered on the remote; `{default}` here has not followed yet")
                }
                State::Held(why) => format!("kept, not delivered: {}", flat(why)),
            };
            // Never left out of the page without a word: the owner's answer is kept, and why
            // it is not drawn is said beside it.
            let unshown = match (&save.shown, &save.state) {
                (true, _) | (_, State::Conflicted) => "",
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
        if saves.len() == 1 {
            "an owner's save".to_string()
        } else {
            format!("{} owner's saves", saves.len())
        },
        each.join(", ")
    ))
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
/// own change alone.
fn behind(root: &Path, sequence: u64, file: &str) -> bool {
    entries(root).into_iter().any(|(earlier, saved)| {
        earlier < sequence
            && saved.edit.rel == file
            && saved.delivery.is_some_and(|delivery| {
                !delivery.closed && (!delivery.tip.is_empty() || delivery.problem.is_empty())
            })
    })
}

/// The edit committed on its branch, on the default branch here: the branch and its commit. A
/// branch a pass before made and never journaled is taken up, once it holds the edit.
fn committed(root: &Path, sequence: u64, edit: &Edit) -> Result<(String, String), String> {
    let branch = format!("{BRANCHES}{sequence}");
    let name = format!("refs/heads/{branch}");
    if let Ok(tip) = git(root, &["rev-parse", "--verify", "-q", &name]) {
        if holds(root, &name, edit) {
            return Ok((branch, tip));
        }
        return Err(format!(
            "the branch {branch} is there and does not hold this save"
        ));
    }
    let default = &Project::of(root).default_branch;
    let parent = git(root, &["rev-parse", "--verify", "-q", &main_ref(root)])?;
    let edits = std::slice::from_ref(edit);
    let (saved, refused) = saved_uploads(root, edits);
    let made = made(root, &parent, edits, &saved).map_err(String::from)?;
    if made.same {
        return Err(format!(
            "{default} here holds its text and the remote {default} does not: nothing here is \
             delivered until the two agree"
        ));
    }
    let message = format!("docs: owner verdicts\n\n{}", edit.rel);
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
    Ok((branch, commit))
}

/// The branch filed on the board as the owner's task, in review: once, by its receipt.
fn filed(root: &Path, delivery: &Delivery, edit: &Edit) -> Result<(), String> {
    use symphony_board::{BEFORE_WORK, NewTask};
    let owner = crate::postpone::OWNER;
    let store = crate::taskview::connect(root)?;
    let app = crate::verdicts::app_handle(&store).map_err(|e| e.to_string())?;
    let slug = &delivery.branch;
    let receipt = format!("document-save:{slug}:{}", delivery.tip);
    if (store.owner_saved(&receipt).map_err(|e| e.to_string())?).is_empty() {
        let project = Project::of(root);
        let page = project.route(&edit.rel).unwrap_or_else(|| edit.rel.clone());
        let default = &project.default_branch;
        let new = NewTask {
            slug: slug.clone(),
            title: format!("Deliver the owner's save of {page}"),
            by: owner.into(),
            body: format!(
                "The owner saved an answer on {page} in the Symphony, and this project delivers \
                 by pull request, so the save is not on {default}: the branch {slug} holds it, \
                 as the one commit {} of the owner's save of {}. Its work is done and its text is \
                 the owner's own: nothing on the branch is edited but to settle a conflict with \
                 {default}, by merging {default} into the branch and never by rebasing it: the \
                 delivery is proved by a head that holds that commit. Assess that diff for what \
                 it needs, then land it, which delivers it \
                 by its pull request. The Symphony keeps the save and shows it as pending until \
                 the delivery's receipt proves it on the remote {default}.",
                delivery.tip, edit.rel
            ),
            priority: 1,
            links: vec![page],
            ..NewTask::default()
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

/// Said once on the task of a save another task's delivery brought: nothing more is delivered
/// by it, and closing it is its orchestrator's.
fn covered(root: &Path, delivery: &Delivery, by: &Receipt) -> Result<(), String> {
    let store = crate::taskview::connect(root)?;
    let text = format!(
        "The owner's save this task delivers is on the remote default branch already: the \
         delivery of {} brought it as {} (pull request #{} of {}, from its head {}). Nothing \
         more is delivered by {}: close it as covered by that landing.",
        by.slug, by.squash, by.pull, by.name, by.head, delivery.branch
    );
    store
        .say_keyed(
            &delivery.branch,
            crate::postpone::OWNER,
            &text,
            Some("@orchestrator"),
            &format!("document-save-covered:{}", by.squash),
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
            Standing::Proved { covered: false, .. } => false,
            Standing::Proved { receipt, .. } => {
                // Its own task, if it has one, is told before the entry goes.
                match delivery.filed.then(|| covered(root, &delivery, &receipt)) {
                    Some(Err(why)) => {
                        delivery.problem = format!("its task was not told it is covered: {why}");
                        true
                    }
                    _ => false,
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
            if behind(root, sequence, &edit.rel) {
                delivery.problem.clear();
                return Ok(());
            }
            match committed(root, sequence, edit) {
                Ok((branch, tip)) => {
                    (delivery.branch, delivery.tip) = (branch, tip);
                    delivery.problem.clear();
                    // Journaled before its task is filed: the task's receipt names this commit.
                    self.saved.delivery = Some(delivery.clone());
                    self.save()?;
                }
                Err(why) => {
                    delivery.problem = why;
                    return Ok(());
                }
            }
        }
        if !delivery.filed {
            if let Err(why) = filed(root, delivery, edit) {
                delivery.problem = format!("its task is not on the board yet: {why}");
                return Ok(());
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
