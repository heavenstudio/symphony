//! A land delivered by a hosted pull request: what `tasks land` does at its merge when the
//! project's committed `[land] delivery` names a command, in place of [`Landing::integrate`] and
//! [`Landing::push`], which it then never runs. The host merges, and the checkout follows.
//!
//! The board names no host. `delivery` is a command the project's own program answers, a
//! [`Transport`] of four verbs, each `COMMAND VERB --request FILE` run in the main checkout, FILE
//! one JSON object and its standard output another whose `state` says what it came to. Each is safe
//! to run twice and to interrupt, which is the program's to hold:
//!
//! - `publish`: the head made public without a force, the pull request bound to this task found
//!   or made, and the checks of the [`Snapshot`] it is handed written and read back. Answers
//!   `published` with the repository's number and name, the pull request's number and address, and
//!   the head the host holds.
//! - `merge`: one request to merge that pull request at that head, and no other, by the method
//!   the request names: `squash`, or `merge` for an import. Answers `merged`, `refused` (with
//!   `kind`: `conflict`, `head`, `checks`, `permission`) or `unknown`.
//! - `result`: the bound pull request as the host holds it: `open`, `merged` (with the commit) or
//!   `closed`, its head, its base and its repository.
//! - `landed`: told once the board closed the task, for what the program does then.
//!
//! Any verb may answer `refused` or `unavailable` with `why`. The program decides nothing: what a
//! task needs, whether it stands and whether to ask for the merge are this land's, by
//! [`evidence::snapshot`], the one reading `tasks api`'s `evidence` answers too.
//!
//! **Two operations.** A task's head is squashed into one commit ([`TASK`], and [`GENERATED`] for
//! what the regenerator made). A head that imports the project's upstream is merged by a merge
//! commit, which keeps the upstream's commit in the default branch's history ([`IMPORT`]), and
//! only where three things agree ([`Landing::import`]): the project file names the upstream
//! (`[land.upstream]`); the head's assessment, at that head, names the upstream commit **U** it
//! imports ([`crate::stamps::Needs::import`]); and the history fetched now from the upstream says
//! so: U on the upstream's branch and newer than the last import, merged on the head's own line,
//! and every other commit the head adds its line's own or U's. Each publication and merge of an
//! import tells the program the operation, the upstream and U ([`delivering`]), which pins the
//! upstream at its host and answers its number ([`pinned`]). Where an upstream is named, a head
//! that holds any of its commits and is not assessed as an import is refused, never squashed,
//! and the upstream is fetched for every land to tell, so one that does not answer refuses a
//! task's land as well. Neither operation falls back to the other, nor to a merge here.
//!
//! Before it judges anything a land reads the pull request a run before bound the task to
//! ([`Landing::bound_before`]): one merged since is settled or held, never published to, and one
//! whose evidence no longer stands is told so by a snapshot that blocks. And it delivers nothing
//! past the merge check the project asks for, which no squash is run through
//! ([`Landing::unchecked`]).
//!
//! The land keeps a [`Journal`] in the lands' folder, written before each step it names:
//!
//! 1. **Prepared**: the head, the operation (and for an import, what it imports), the snapshot's
//!    revision and the commit's title and body. The remote default branch is fetched and its
//!    project file must be the one committed here; a head that changes a generator's outputs and
//!    is not what the regenerator made ([`Landing::generated`]), or merges a commit the remote
//!    default does not hold and is not an import of the upstream (another history, which a
//!    squash would flatten), is refused. A release candidate's fix ([`CANDIDATE`]) is told to
//!    keep its outputs and deliver a merge of the default branch that keeps the default branch's
//!    ([`Landing::outputs`]).
//! 2. **Eligible**: published, and the snapshot read again: one of another revision is published
//!    again, never merged.
//! 3. **Merge requested**: written before the request. From here no run merges locally, whatever
//!    the settings say by then: a run reads the bound pull request first ([`Landing::settle`]).
//! 4. **Remote merged**: the commit **S** the host made is proved by git alone
//!    ([`Landing::prove`]): on the fetched default branch, of one parent, its tree the merge of
//!    that parent and the head; for an import, of two parents, the base then the head, its tree
//!    the merge of the two, and U in its history. Anything else (an import squashed, a task
//!    merged by a merge commit) is held and closes nothing. A [`Receipt`] is kept for good, and
//!    the head stays reachable under [`RETAINED`], as a squash leaves it in no history.
//!
//!    A slug is delivered once. Its receipt and its retained head are what the release run
//!    counts, holds a release by and reads its notes from, so neither is ever replaced. A landed
//!    task moves no further on the board, so what reaches a second delivery is the slug filed
//!    again as another task: its land is refused by name before anything is published
//!    ([`Landing::repeated`]) where the slug's receipt is of another head, or of the head its
//!    branch is at with no word of this task's own that it landed it, and so is one whose
//!    receipt file does not read as its own. A merge a run before asked
//!    for, found beside a receipt of another delivery or a retained head that is another
//!    commit, is held for a person ([`Landing::prove`]); the same delivery proved again keeps
//!    the receipt it has.
//! 5. **Reconciled**: a clean main checkout whose default branch the remote's descends from is
//!    fast-forwarded. A dirty or diverged one is left as it is: landed remotely, reconciliation
//!    pending, and the next run tries again.
//! 6. **Closed**, then **Dispatching**: the task closed once, by its landed line, and what
//!    follows a land started once. A run that finds it dispatching starts nothing again.

use super::evidence::{self, Snapshot};
use super::moving::Move;
use super::{
    Ended, GIT_VARS, Judged, Landing, REMOTE, Refusal, Stop, at, file_stem, owns, run_git, short,
    signal_name, watched,
};
use crate::sweep::git::{ancestor, git, rev, succeeds};
use crate::sys::{self, Stdio};
use crate::{Message, Task};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// Under which a delivered head is kept reachable, the task's slug after it.
pub const RETAINED: &str = "refs/symphony/delivered/";
/// The folder of the receipts, in the lands' folder.
pub const RECEIPTS: &str = "receipts";
/// How a task's head is delivered: squashed into one commit on the default branch.
pub const TASK: &str = "task";
/// How what the regenerator made is delivered ([`super::regenerate::Pending`]): squashed as a
/// task's head is, once it is the generators' outputs alone, made of what the default branch holds.
pub const GENERATED: &str = "generated";
/// How a head that imports the project's upstream ([`super::Upstream`]) is delivered: merged by a
/// merge commit, which keeps the upstream's commit in the default branch's history ([`Import`]).
pub const IMPORT: &str = "import";
/// Where the upstream's default branch is fetched to in the main checkout, read for an import.
pub const UPSTREAM: &str = "refs/symphony/upstream";
/// The local branch that names the release candidate's tip while the release run holds one
/// (RELEASES, the release candidate), where a fix of one of its reds starts.
pub const CANDIDATE: &str = "release-candidate";

/// Where a head stands to the release candidate ([`Landing::candidate`]).
enum Candidate {
    /// A fix of it: the head holds its tip, at this commit, and beyond it only its own commits.
    Fix(String),
    /// One is held, at this commit, and the head does not hold it.
    Behind(String),
    /// None is held, or the head holds what the remote default branch holds beyond it.
    None,
}

/// `[land] delivery` as the main checkout `main` has it committed at its head: the command, or
/// nothing where the land merges here. Its shape is [`super::Settings::parse`]'s to refuse.
///
/// # Errors
///
/// The project file does not read.
pub fn command(main: &Path) -> Result<Vec<String>, String> {
    let at = format!("HEAD:{}", super::MANIFEST);
    if !succeeds(main, &["cat-file", "-e", &at]) {
        return Ok(Vec::new());
    }
    of(&run_git(main, &["show", &at])?)
}

/// `[land] delivery` in the project file `text`.
///
/// # Errors
///
/// The file does not read as the land reads it.
pub fn of(text: &str) -> Result<Vec<String>, String> {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Manifest {
        land: Land,
    }
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Land {
        delivery: Vec<String>,
    }
    super::Settings::parse(text)?;
    let file: Manifest = toml::from_str(text)
        .map_err(|error| format!("{}: {}", super::MANIFEST, error.to_string().trim_end()))?;
    Ok(file.land.delivery)
}

/// What answers the verbs.
pub trait Transport {
    /// The answer to `verb` asked with `request`.
    ///
    /// # Errors
    ///
    /// No answer was read: whether the verb was acted on is not known.
    fn ask(&self, verb: &str, request: &Value) -> Result<Value, String>;
}

/// How far a delivery went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Prepared,
    Eligible,
    /// Published, and the host's own merge of that head armed: it merges once every check it
    /// requires passes, and the watcher closes the task once it has ([`super::dispatch`]).
    Armed,
    MergeRequested,
    RemoteMerged,
    Reconciled,
    Closed,
    Dispatching,
}

/// The pull request a delivery is bound to, as the transport named it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// The repository's number at its host, which a rename keeps.
    pub repository: i64,
    /// Its name when bound: a display name.
    pub name: String,
    pub pull: i64,
    pub url: String,
}

/// A delivery under way, kept in the lands' folder as `SLUG.delivery`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub slug: String,
    pub operation: String,
    pub stage: Stage,
    /// The whole head delivered.
    pub head: String,
    /// The remote default branch's tip seen before the request.
    pub base_observed: String,
    /// The revision of the snapshot published.
    pub revision: String,
    pub title: String,
    pub body: String,
    pub by: String,
    /// The statuses its landing closes the task from.
    pub closes: Vec<String>,
    /// What its landed line adds for a task that needed no review.
    pub stood: String,
    pub binding: Option<Binding>,
    /// The commit the host made, once proved.
    pub squash: Option<String>,
    /// Why it is held for a person, when it is.
    pub held: Option<String>,
    /// What an import imports; none for any other operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<Import>,
    /// When its evidence was first published eligible at the head armed, in whole seconds since
    /// 1970 ([`super::dispatch::seconds`]): what `[land] merge_within_minutes` counts from. None
    /// while it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green: Option<i64>,
    /// Whether the host's own merge of its pull request is armed, as far as the board knows: set
    /// once the host answers an arming, cleared only once it answers a disarm. A publication
    /// between keeps it, as the host keeps the merge armed across a push: what the watcher reads
    /// ([`Journal::watched`]), so a publication that stops before the host answers an arming
    /// again leaves a merge the host may still make in sight.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub armed: bool,
}

/// What an import delivers, as the land read it before it published, and what it proved after:
/// the upstream it is of, the commit it imports and where the default branch stood against it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    /// `[land.upstream] url`, as the land fetched it.
    pub url: String,
    /// `[land.upstream] branch`.
    pub branch: String,
    /// The whole upstream commit imported: the one the head's assessment names.
    pub commit: String,
    /// The upstream's branch as the land fetched it.
    pub tip: String,
    /// The last import: the merge base of the default branch and the upstream's branch before.
    pub before: String,
    /// The upstream's number at its host, as the host pinned it when it published; 0 before.
    #[serde(default)]
    pub repository: i64,
    /// Every parent of the commit the host made, in order, once proved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
    /// The merge base of that commit and the upstream's branch, once proved: the commit imported,
    /// or a newer one of the upstream's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after: String,
}

impl Journal {
    pub fn path(folder: &Path, slug: &str) -> PathBuf {
        folder.join(format!("{}.delivery", file_stem(slug)))
    }

    pub fn read(folder: &Path, slug: &str) -> Option<Journal> {
        let text = fs::read_to_string(Journal::path(folder, slug)).ok()?;
        crate::objects::parse::<Journal>(&text).filter(|journal| journal.slug == slug)
    }

    /// Kept in `folder`, whole or not at all, and on the disk before this returns.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        kept(&Journal::path(folder, &self.slug), self)
    }

    pub fn drop_in(folder: &Path, slug: &str) {
        let _ = fs::remove_file(Journal::path(folder, slug));
    }

    /// Whether the watcher reads it: its merge armed at the host and nothing here settling it
    /// yet, the stage short of a merge asked and no person holding it; or its land past the merge
    /// asked and not ended ([`Journal::settling`]). One whose last publication stopped before the
    /// host answered its arming again ([`Stage::Prepared`] or [`Stage::Eligible`]) is among the
    /// armed: the host may merge it all the same.
    pub fn watched(&self) -> bool {
        self.settling()
            || ((self.armed || self.stage == Stage::Armed)
                && self.held.is_none()
                && self.stage < Stage::MergeRequested)
    }

    /// Whether its land went past the merge asked and did not end, no person holding it: a run
    /// cut short between the host's merge and the journal dropped (the board lost, a crash, a
    /// reboot), which only the land's tail gone on with finishes ([`Landing::resumed`]). The
    /// watcher runs it each round until it does ([`super::dispatch`]).
    pub fn settling(&self) -> bool {
        self.held.is_none() && self.stage >= Stage::MergeRequested
    }
}

/// What a delivery proved, kept for good as `receipts/SLUG.json` in the lands' folder: the head
/// that was read and stamped, and the commit the host made of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub slug: String,
    pub task: i64,
    pub operation: String,
    pub repository: i64,
    pub name: String,
    pub pull: i64,
    pub head: String,
    pub base_observed: String,
    /// The one parent of the squash, or an import's first: the base it was made on.
    pub parent: String,
    pub squash: String,
    pub tree: String,
    /// The snapshot's revision the merge was asked under.
    pub revision: String,
    pub by: String,
    /// For an import, what it imported and the commit's two parents, the base then the head:
    /// [`Receipt::parent`] is the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<Import>,
}

impl Receipt {
    pub fn path(folder: &Path, slug: &str) -> PathBuf {
        (folder.join(RECEIPTS)).join(format!("{}.json", file_stem(slug)))
    }

    pub fn read(folder: &Path, slug: &str) -> Option<Receipt> {
        let text = fs::read_to_string(Receipt::path(folder, slug)).ok()?;
        crate::objects::parse::<Receipt>(&text).filter(|receipt| receipt.slug == slug)
    }

    /// The receipt kept for `slug` in `folder`, none where no file is kept for it: what a land
    /// reads before it would write one, as [`Receipt::read`] says none for a file it cannot read.
    ///
    /// # Errors
    ///
    /// A file is kept there that does not read as the receipt of `slug`, named with why: what a
    /// delivery proved is never written over.
    pub fn held(folder: &Path, slug: &str) -> Result<Option<Receipt>, String> {
        let path = Receipt::path(folder, slug);
        let unread = |why: String| {
            format!(
                "the receipt kept for {slug}, {}, does not read as its own ({why}): what a \
                 delivery proved is never written over, and a person judges the file",
                path.display()
            )
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(unread(error.to_string())),
        };
        let receipt: Receipt =
            serde_json::from_str(&text).map_err(|error| unread(error.to_string()))?;
        if receipt.slug != slug {
            return Err(unread(format!("it names {}", receipt.slug)));
        }
        Ok(Some(receipt))
    }
}

/// `value` as JSON at `path`, staged beside it, synced and renamed over it.
fn kept<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    use std::io::Write as _;
    let folder = path.parent().ok_or("no folder")?;
    fs::create_dir_all(folder).map_err(|e| at(folder, &e))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let staged = folder.join(format!(".{name}.{}", std::process::id()));
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let mut file = fs::File::create(&staged).map_err(|e| at(&staged, &e))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| at(&staged, &e))?;
    fs::rename(&staged, path).map_err(|e| at(path, &e))
}

/// What a land with no `[land] delivery` does about a journal a delivery left: nothing with none
/// or one that asked for no merge yet, which it drops; a refusal once a merge was asked for, as
/// no run merges locally over a request whose answer may be a commit on the remote.
pub(super) fn left(folder: &Path, slug: &str, default: &str) -> Result<(), String> {
    let Some(journal) = Journal::read(folder, slug) else {
        return Ok(());
    };
    if journal.stage < Stage::MergeRequested {
        Journal::drop_in(folder, slug);
        return Ok(());
    }
    Err(format!(
        "{slug} has a delivery by pull request under way ({}), and [land] delivery is no longer \
         set on {default}: nothing is merged here over it. Set it again and run the land, which \
         reads the pull request; with the host gone for good, see docs/workflow/TASKS.md, \"A delivery \
         whose host is gone\" (its journal is {})",
        journal.said(),
        Journal::path(folder, slug).display()
    ))
}

impl Journal {
    /// How the host is asked to merge it: an import by a merge commit, anything else squashed.
    fn method(&self) -> &'static str {
        if self.operation == IMPORT {
            "merge"
        } else {
            "squash"
        }
    }

    /// Where it stands, in a few words.
    pub(super) fn said(&self) -> String {
        let pull = (self.binding.as_ref()).map_or_else(String::new, |b| {
            format!(", pull request #{} of {}", b.pull, b.name)
        });
        let held = (self.held.as_ref()).map_or_else(String::new, |why| format!(", held: {why}"));
        format!(
            "{} at {}{pull}{held}",
            match self.stage {
                Stage::Prepared => "prepared",
                Stage::Eligible => "eligible",
                Stage::Armed => "armed",
                Stage::MergeRequested => "merge requested",
                Stage::RemoteMerged => "merged remotely",
                Stage::Reconciled => "reconciled",
                Stage::Closed => "closed",
                Stage::Dispatching => "dispatching",
            },
            short(&self.head)
        )
    }
}

impl Landing<'_> {
    /// The answer of the command `[land] delivery` names to `verb`, run within the land's network
    /// timeout in a process group of its own.
    fn program(&self, verb: &str, request: &Value) -> Result<Value, String> {
        let landing = self;
        let command = &landing.delivery;
        let file = (landing.folder).join(format!("{}.request", file_stem(landing.slug)));
        kept(&file, request)?;
        let said = format!("{} {verb}", command.join(" "));
        if let Some(signal) = landing.interrupted() {
            return Err(format!(
                "{said}: not run, the land took {}",
                signal_name(signal)
            ));
        }
        let mut child = sys::command_with(&command[0], &landing.env.vars);
        for name in GIT_VARS {
            child.env_remove(name);
        }
        child
            .args(&command[1..])
            .arg(verb)
            .arg("--request")
            .arg(&file)
            .current_dir(&landing.main)
            .process_group(0)
            .stdin(Stdio::null());
        let child = sys::spawn_output(&mut child).map_err(|e| format!("{said}: {e}"))?;
        let group = child.id();
        landing.running(group, &said, false);
        let limit = landing.settings.network();
        let (done, ended) = watched(group, limit, landing.interrupts, landing.now, || {
            sys::wait_output(child)
        });
        landing.ran();
        let _ = fs::remove_file(&file);
        let done = match ended {
            Ended::Late => {
                return Err(format!("{said}: no answer within {} s", limit.as_secs()));
            }
            Ended::Interrupted(signal) => {
                return Err(format!(
                    "{said}: ended, the land took {}",
                    signal_name(signal)
                ));
            }
            Ended::No => done.map_err(|e| format!("{said}: {e}"))?,
        };
        let first = |text: &[u8]| {
            (String::from_utf8_lossy(text).lines().map(str::trim))
                .find(|line| !line.is_empty())
                .map(str::to_string)
        };
        if !done.status.success() {
            return Err(format!(
                "{said}: {}",
                first(&done.stderr).unwrap_or_else(|| done.status.to_string())
            ));
        }
        serde_json::from_slice::<Value>(&done.stdout)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| format!("{said}: its answer is not a JSON object"))
    }
}

/// `request` saying what `journal` delivers: its operation, and an import's upstream and commit,
/// which the program pins at its host before it pushes or merges anything.
fn delivering(mut request: Value, journal: &Journal) -> Value {
    request["operation"] = json!(journal.operation);
    if let Some(import) = &journal.import {
        request["upstream"] = json!({
            "url": import.url, "branch": import.branch, "commit": import.commit,
        });
    }
    request
}

/// The upstream commit `task`'s assessment names its head `tip` an import of: none where it names
/// none, or where it read another commit than `tip`, which binds nothing.
fn imported(task: &Task, tip: &str) -> Option<String> {
    let needs = task.needs()?;
    needs.import.filter(|_| needs.tip.as_deref() == Some(tip))
}

/// Whether `snapshot`'s assessment names the import `journal` delivers at its head, and nothing
/// where it delivers none: what is published and merged is what was read. An import named at
/// another head binds nothing at this one.
fn agrees(snapshot: &Snapshot, journal: &Journal) -> bool {
    let named = (snapshot.assessment.as_ref())
        .filter(|read| read.tip.as_deref() == Some(journal.head.as_str()))
        .and_then(|read| read.import.as_deref());
    named == journal.import.as_ref().map(|import| import.commit.as_str())
}

/// The upstream's number the program pinned `import` to, from its `published` answer: an import
/// is published only by a program that held the upstream it was told to its number at the host.
fn pinned(answer: &Value, import: &Import) -> Result<i64, String> {
    (answer["upstream"]["id"].as_i64().filter(|id| *id > 0)).ok_or_else(|| {
        format!(
            "the publication of the import of {} pins no upstream: the delivery command does not \
             deliver imports; nothing merged",
            short(&import.commit)
        )
    })
}

/// The `state` of an answer, and its `why`.
pub(super) fn state(answer: &Value) -> (&str, &str) {
    (
        answer["state"].as_str().unwrap_or_default(),
        answer["why"].as_str().unwrap_or("it gave no reason"),
    )
}

impl Landing<'_> {
    /// Whether this land is delivered by a pull request.
    pub(super) fn delivers(&self) -> bool {
        self.task && !self.delivery.is_empty()
    }

    pub(super) fn ask(&self, verb: &str, request: &Value) -> Result<Value, String> {
        match self.transport {
            Some(transport) => transport.ask(verb, request),
            None => self.program(verb, request),
        }
    }

    fn tracking(&self) -> String {
        format!("refs/remotes/{REMOTE}/{}", self.settings.default_branch)
    }

    /// The remote default branch's tip, fetched now.
    fn fetched(&self) -> Result<String, String> {
        let default = &self.settings.default_branch;
        if !succeeds(&self.main, &["remote", "get-url", REMOTE]) {
            return Err(format!(
                "[land] delivery is set and there is no {REMOTE} remote: a delivery by pull \
                 request has a remote to deliver to"
            ));
        }
        let tracking = self.tracking();
        let fetch = format!("+{}:{tracking}", self.branch());
        self.remote(&["fetch", "--quiet", "--no-tags", REMOTE, &fetch])?;
        rev(&self.main, &tracking).ok_or_else(|| format!("{REMOTE} has no branch {default}"))
    }

    /// A delivery of this task a run before left with its merge asked for: gone on with from the
    /// pull request, never from the stamps, which a request already made no longer waits on.
    /// None with no such journal.
    pub(super) fn resumed(&mut self) -> Option<Result<String, Stop>> {
        let journal = Journal::read(&self.folder, self.slug)
            .filter(|journal| journal.stage >= Stage::MergeRequested)?;
        self.say(&format!(
            "land {}: a delivery is under way ({}): going on from it",
            self.slug,
            journal.said()
        ));
        Some(self.settle(journal, None))
    }

    /// Refuse a delivery where the land's merge check is set (`[land] check`, on unless the
    /// project turns it off, with a `[verdicts] check`): the default land runs it on its merge
    /// before main moves, and the commit a host squashes is checked by nothing here, so the
    /// check would be lost without a word.
    pub(super) fn unchecked(&self) -> Result<(), Refusal> {
        if self.settings.check.is_empty() {
            return Ok(());
        }
        Err(Refusal::to(
            format!(
                "[land] delivery is set and so is the land's merge check (`{}`), which a squash \
                 made by the host is never run on: nothing is delivered past a check the project \
                 asks for",
                self.settings.check.join(" ")
            ),
            "@orchestrator",
            "The project's owner decides: the pull request's required checks take the check's \
             place and the project file says `[land] check = false`, or the key is taken out"
                .into(),
        ))
    }

    /// Refuse a second delivery of this slug, before anything is asked of the host. A landed
    /// task moves no further on the board, so a second delivery is the slug filed again as
    /// another task (a board made again over a repository that keeps its receipts), whose branch
    /// may be at another head or at the very head delivered before. Refused where the slug's
    /// kept receipt is of another head than its branch is at; where it is of that head and is
    /// not this task's own delivery ([`Landing::own`]); and where the file kept for it does not
    /// read as its receipt. Nothing where it has none, or where it is this task's own, which a
    /// land run again answers as it did.
    pub(super) fn repeated(&self) -> Result<(), Stop> {
        let slug = self.slug;
        let then = "What it still needs goes in a new task, whose slug is its own: nothing was \
                    published or merged, and the receipt is as it was";
        let refused = |line: String| self.refuse(Refusal::to(line, "@orchestrator", then.into()));
        let earlier = Receipt::held(&self.folder, slug).map_err(refused)?;
        let (Some(earlier), Some(tip)) = (earlier, rev(&self.main, &format!("refs/heads/{slug}")))
        else {
            return Ok(());
        };
        let now = if earlier.head != tip {
            format!("its branch is now at {}", short(&tip))
        } else if self.own(&earlier)? {
            return Ok(());
        } else {
            "its branch is at that head again, as a task that never landed it".to_string()
        };
        Err(refused(format!(
            "{slug} was delivered already as {} from {} by pull request #{} of {}, and {now}: a \
             slug is delivered once, its receipt and the head kept under {RETAINED}{slug} never \
             replaced",
            short(&earlier.squash),
            short(&earlier.head),
            earlier.pull,
            earlier.name,
        )))
    }

    /// Whether `receipt`, kept for this slug of the head its branch is at, is this task's own
    /// delivery: its thread has the landed line naming the receipt's squash (a land run again
    /// after it closed), or the journal kept for the slug is of that head, bound to the receipt's
    /// pull request (a run cut between the receipt and the journal's next stage). A task id is no
    /// such word: a board made again numbers its tasks again.
    fn own(&self, receipt: &Receipt) -> Result<bool, Stop> {
        let slug = self.slug;
        let landed = landed_line(slug, &self.settings.default_branch, &receipt.squash);
        if (self.store.thread(slug)?.iter()).any(|post| post.text.starts_with(&landed)) {
            return Ok(true);
        }
        let journal = Journal::read(&self.folder, slug);
        Ok(journal.is_some_and(|journal| {
            journal.head == receipt.head
                && (journal.binding.as_ref()).is_some_and(|binding| {
                    binding.repository == receipt.repository && binding.pull == receipt.pull
                })
        }))
    }

    /// The pull request a run before bound this task to, read before the land judges anything:
    /// none with no such binding, or with one still open whose evidence stands.
    ///
    /// - Merged at the branch's head while its evidence stands (a merge whose lost answer was
    ///   read as open and that the host finished after, or one made by hand): settled as any
    ///   merge is, proved or held.
    /// - Merged or closed otherwise: held, by name, and nothing closed.
    /// - Open, the evidence no longer eligible (a changes verdict, a red run, a new head): the
    ///   snapshot that blocks is published, so the check the host requires says so, and the
    ///   land goes on to its own refusal.
    pub(super) fn bound_before(&mut self) -> Option<Result<String, Stop>> {
        let slug = self.slug;
        let journal = Journal::read(&self.folder, slug)?;
        let binding = journal.binding.clone()?;
        let tip = rev(&self.main, &format!("refs/heads/{slug}"))?;
        let request = json!({
            "verb": "result", "slug": slug,
            "repository": binding.repository, "pull": binding.pull,
        });
        let result = match self.ask("result", &request) {
            Ok(result) => result,
            Err(why) => {
                self.say(&format!(
                    "land {slug}: pull request #{} could not be read ({why})",
                    binding.pull
                ));
                return None;
            }
        };
        let (said, _) = state(&result);
        let snapshot = match (self.store.get(slug), self.store.thread(slug)) {
            (Ok(task), Ok(thread)) => {
                evidence::snapshot(self.store, self.env, &self.main, &task, &thread, &tip).ok()
            }
            _ => None,
        };
        let stands = snapshot.as_ref().is_some_and(|snapshot| snapshot.eligible);
        match said {
            "open" => {
                if let Some(snapshot) = snapshot.filter(|snapshot| !snapshot.eligible) {
                    let _ = self.withdraw(slug, &journal, &binding, &tip, &snapshot);
                }
                None
            }
            "merged" if journal.head == tip && stands => {
                self.say(&format!(
                    "land {slug}: pull request #{} of {} was merged since it was read as open: \
                     going on from it",
                    binding.pull, binding.name
                ));
                Some(self.settle(journal, None))
            }
            "merged" | "closed" => {
                let what = if said == "closed" {
                    format!("pull request #{} was closed without a merge", binding.pull)
                } else if journal.head != tip {
                    format!(
                        "pull request #{} was merged at {} and {slug}'s branch is at {}",
                        binding.pull,
                        short(result["head"].as_str().unwrap_or(&journal.head)),
                        short(&tip)
                    )
                } else {
                    format!(
                        "pull request #{} was merged at {} and the evidence of {slug} does not \
                         stand there",
                        binding.pull,
                        short(&tip)
                    )
                };
                Some(Err(self.hold(journal, what)))
            }
            _ => None,
        }
    }

    /// Publish `snapshot`, which is not eligible, to the pull request `binding`: the head the
    /// branch is at now and checks that fail, the one the host requires first. What the land
    /// refuses with follows; a publication that fails is said and changes none of it, and its
    /// reason is the error.
    fn withdraw(
        &mut self,
        slug: &str,
        journal: &Journal,
        binding: &Binding,
        tip: &str,
        snapshot: &Snapshot,
    ) -> Result<(), String> {
        let request = json!({
            "verb": "publish", "slug": slug, "by": self.actor, "head": tip,
            "branch": self.settings.default_branch, "title": journal.title,
            "body": journal.body, "snapshot": snapshot, "pull": binding.pull,
        });
        let told = match self.ask("publish", &delivering(request, journal)) {
            Ok(answer) if state(&answer).0 == "published" => Ok(()),
            Ok(answer) => Err(state(&answer).1.to_string()),
            Err(why) => Err(why),
        };
        let stands = format!("land {slug}: its evidence does not stand at {}", short(tip));
        self.say(&match &told {
            Ok(()) => format!(
                "{stands}, and pull request #{} of {} says so now",
                binding.pull, binding.name
            ),
            Err(why) => format!(
                "{stands}, and pull request #{} was not told ({why}): its checks may still say it \
                 does",
                binding.pull
            ),
        });
        told
    }

    /// For the task `slug`, which will not be delivered (cancelled as stale): the pull request a land bound
    /// it to is told its evidence no longer stands, by the snapshot that blocks. Done where it is
    /// bound to none, and where the pull request is no longer open.
    ///
    /// # Errors
    ///
    /// The pull request could not be read or told, or the task's evidence still reads as
    /// standing, its status said: its checks may still pass.
    pub(super) fn retract(&mut self, slug: &str) -> Result<(), String> {
        let Some(journal) = Journal::read(&self.folder, slug) else {
            return Ok(());
        };
        let Some(binding) = journal.binding.clone() else {
            return Ok(());
        };
        let request = json!({
            "verb": "result", "slug": slug,
            "repository": binding.repository, "pull": binding.pull,
        });
        let result = self.ask("result", &request)?;
        match state(&result) {
            ("closed" | "merged", _) => return Ok(()),
            ("open", _) => {}
            (_, why) => return Err(why.to_string()),
        }
        let task = self.store.get(slug).map_err(|error| error.to_string())?;
        let thread = self.store.thread(slug).map_err(|error| error.to_string())?;
        let tip = &journal.head;
        let snapshot = evidence::snapshot(self.store, self.env, &self.main, &task, &thread, tip)?;
        // A task named before it was cancelled, and not cancelled yet, may still read as standing.
        if snapshot.eligible {
            return Err(format!(
                "{slug} is {} and its evidence stands at {}",
                task.status,
                short(tip)
            ));
        }
        self.withdraw(slug, &journal, &binding, tip, &snapshot)
    }

    /// Deliver the task's `tip`, found ready as `judged`, by its pull request: published, then the
    /// host's own merge of it armed ([`Landing::arm`]).
    pub(super) fn deliver(
        &mut self,
        task: &Task,
        tip: &str,
        judged: &Judged,
    ) -> Result<String, Stop> {
        let (closes, stood): (&[&str], String) = match judged {
            Judged::Approved(_) => (&["done"], String::new()),
            Judged::Stamped(stood) => (&["review", "blocked", "done"], stood.clone()),
        };
        let (journal, eligible) = self.publish_head(task, tip, closes, stood, true)?;
        self.arm(journal, eligible)
    }

    /// Publish the task's `tip` and its evidence by its pull request, refused first where the
    /// head is not one a squash delivers: the journal it leaves at [`Stage::Eligible`], bound to
    /// that pull request, and whether its evidence stood there. A land publishes only evidence
    /// that is `eligible_only`; a dispatch publishes it whatever it says, so the check the host
    /// requires holds the merge until it stands ([`super::dispatch`]). The task, once landed,
    /// closes from one of `closes`, its landed line adding `stood`.
    pub(super) fn publish_head(
        &mut self,
        task: &Task,
        tip: &str,
        closes: &[&str],
        stood: String,
        eligible_only: bool,
    ) -> Result<(Journal, bool), Stop> {
        let slug = self.slug;
        let default = self.settings.default_branch.clone();
        let base = (self.fetched()).map_err(|line| self.refuse(Refusal::here(line)))?;
        // The settings a head is judged by are the remote default branch's, whatever is delivered.
        // An import is one only where the policy, the head's assessment and its history agree.
        let (operation, import) = (self.committed(&base))
            .and_then(|()| match self.generated(&base, tip) {
                Some(generated) => generated.map(|()| (GENERATED, None)),
                None => match imported(task, tip) {
                    Some(commit) => (self.outputs(&base, tip))
                        .and_then(|()| self.import(&base, tip, &commit))
                        .map(|import| (IMPORT, Some(import))),
                    None => self.admit(&base, tip).map(|()| (TASK, None)),
                },
            })
            .map_err(|refusal| self.refuse(refusal))?;
        self.owner_lines(&base, tip)
            .map_err(|refusal| self.refuse(refusal))?;
        let mut snapshot = self.read_evidence(task, tip, eligible_only)?;
        let changed = || {
            Stop::Refused(format!(
                "the assessment of {slug} changed what it says {} imports while it was \
                 delivered: nothing merged; run the land again",
                short(tip)
            ))
        };
        // One task is delivered by one pull request, whatever head it was published at before;
        // the time its evidence first stood is the head's.
        let earlier = Journal::read(&self.folder, slug);
        let green = (earlier.as_ref())
            .filter(|journal| journal.head == tip)
            .and_then(|journal| journal.green);
        // A merge armed before stays armed at the host through this publication.
        let armed = (earlier.as_ref()).is_some_and(|journal| journal.armed);
        let (title, body) = match &import {
            Some(import) => (
                format!("Merge branch '{slug}'"),
                format!(
                    "Merged from {tip}, the head of {slug} its evidence stands on (revision {}): \
                     an import of {} from {}.",
                    snapshot.revision, import.commit, import.url
                ),
            ),
            None => (
                format!("{slug}: {}", task.title.lines().next().unwrap_or_default()),
                format!(
                    "Squashed from {tip}, the head of {slug} its evidence stands on (revision {}).",
                    snapshot.revision
                ),
            ),
        };
        let mut journal = Journal {
            slug: slug.into(),
            operation: operation.into(),
            stage: Stage::Prepared,
            head: tip.into(),
            base_observed: base.clone(),
            revision: snapshot.revision.clone(),
            title,
            body,
            by: self.actor.into(),
            closes: closes.iter().map(|s| (*s).to_string()).collect(),
            stood,
            binding: earlier.and_then(|journal| journal.binding),
            squash: None,
            held: None,
            import,
            green,
            armed,
        };
        let here = |line: String| Stop::Refused(line);
        if !agrees(&snapshot, &journal) {
            return Err(changed());
        }
        journal.write(&self.folder).map_err(here)?;
        // Published under the snapshot read; one that changed meanwhile is published again.
        let mut published = false;
        for _ in 0..super::TRIES {
            self.go_on(None).map_err(Stop::Refused)?;
            self.say(&format!(
                "land {slug}: publishing {} and its evidence ({})",
                short(tip),
                short(&snapshot.revision)
            ));
            // The pull request the journal binds, or none: the program forgets one closed
            // unmerged only once no land names it, as one names it until it reads it closed.
            let request = json!({
                "verb": "publish", "slug": slug, "by": self.actor, "head": tip,
                "branch": default, "title": journal.title, "body": journal.body,
                "snapshot": snapshot,
                "pull": journal.binding.as_ref().map(|binding| binding.pull),
            });
            let answer = (self.ask("publish", &delivering(request, &journal)))
                .map_err(|why| self.refuse(Refusal::here(format!("nothing merged: {why}"))))?;
            let binding = self.bound(&answer, &journal, tip).map_err(|line| {
                // Refused, as the host merged this head since the dispatch read it: no stop to
                // say. A host away, or one that did not answer, is said as it is.
                let merged = (state(&answer).0 == "refused")
                    .then(|| self.merged_meanwhile(&journal, tip))
                    .flatten();
                match merged {
                    Some(landed) => Stop::Refused(landed),
                    None => self.refuse(Refusal::here(line)),
                }
            })?;
            if let Some(import) = &mut journal.import {
                import.repository =
                    pinned(&answer, import).map_err(|line| self.refuse(Refusal::here(line)))?;
            }
            journal.binding = Some(binding);
            journal.revision = snapshot.revision.clone();
            journal.stage = Stage::Eligible;
            journal.write(&self.folder).map_err(here)?;
            // Bound: a later write dispatches it by its binding, no longer by its mark.
            super::dispatch::unmark(&self.folder, slug);
            let again = self.read_evidence(task, tip, eligible_only)?;
            if !agrees(&again, &journal) {
                return Err(changed());
            }
            if again.revision == snapshot.revision {
                published = true;
                break;
            }
            self.say(&format!(
                "land {slug}: its evidence changed while it was published; publishing it again"
            ));
            snapshot = again;
        }
        if !published {
            return Err(self.refuse(Refusal::here(format!(
                "{slug}'s evidence changed {} times while it was published; nothing merged: run \
                 the land again, or a dispatch publishes it once it settles",
                super::TRIES
            ))));
        }
        self.go_on(None).map_err(Stop::Refused)?;
        self.owner_lines(&base, tip)
            .map_err(|refusal| self.refuse(refusal))?;
        // What the regenerator made is judged again on the remote default branch as it is now:
        // the last reading before the request, which nothing here recalls. A record that no
        // longer names the task (a round cancelled it as stale meanwhile) asks for no merge.
        if operation == GENERATED {
            let now = (self.fetched()).map_err(|line| self.refuse(Refusal::here(line)))?;
            let again = (self.committed(&now)).and_then(|()| {
                self.generated(&now, tip).unwrap_or_else(|| {
                    Err(Refusal::here(format!(
                        "{slug} is no longer what the regenerator delivers: no merge asked"
                    )))
                })
            });
            again.map_err(|refusal| self.refuse(refusal))?;
        }
        Ok((journal, snapshot.eligible))
    }

    /// Arm the host's own merge of the head `journal` published, whose evidence was `eligible`
    /// there or not: the host squashes it once every check it requires passes, a land here
    /// waiting for none of it, and the watcher closes the task once it has
    /// ([`super::dispatch`]). A pull request the host would merge now (its evidence standing, no
    /// other check to wait on) is asked to merge by one request instead, as the host arms nothing
    /// that waits on nothing; one merged or closed already is settled.
    pub(super) fn arm(&mut self, mut journal: Journal, eligible: bool) -> Result<String, Stop> {
        let slug = self.slug;
        let here = |line: String| Stop::Refused(line);
        let binding = journal
            .binding
            .clone()
            .ok_or_else(|| here("unbound".into()))?;
        let request = json!({
            "verb": "arm", "slug": slug, "by": self.actor, "head": journal.head,
            "repository": binding.repository, "pull": binding.pull, "method": journal.method(),
            "title": journal.title, "body": journal.body,
        });
        // Armed before, the host may merge the head published all the same: the watcher reads
        // it every round and arms it again ([`Journal::watched`]).
        let (before, at) = (journal.armed, short(&journal.head).to_string());
        let not_armed = |why: &str| {
            if before {
                format!(
                    "pull request #{} of {slug} was not armed again at {at} ({why}): its merge \
                     armed before stands, which the watcher reads every round and arms again",
                    binding.pull
                )
            } else {
                format!(
                    "pull request #{} of {slug} was not armed ({why}): nothing merged, and the \
                     next dispatch asks again",
                    binding.pull
                )
            }
        };
        let answer = match self.ask("arm", &delivering(request, &journal)) {
            Ok(answer) => answer,
            Err(why) => {
                if before && let Some(said) = super::dispatch::watch_start(self.env, &self.main) {
                    self.say(&format!("land {slug}: {said}"));
                }
                return Err(self.refuse(Refusal::here(not_armed(&why))));
            }
        };
        let (said, why) = state(&answer);
        match said {
            "armed" => {
                // Said once an arming: not again at each publication its merge stays armed through.
                let first = !journal.armed;
                journal.stage = Stage::Armed;
                journal.armed = true;
                journal.green = if eligible {
                    journal.green.or_else(|| Some(super::dispatch::seconds()))
                } else {
                    None
                };
                journal.write(&self.folder).map_err(here)?;
                let line = format!(
                    "land {slug}: dispatched {} as pull request #{} of {} ({}), which merges it \
                     itself once symphony/eligible is green{}",
                    short(&journal.head),
                    binding.pull,
                    binding.name,
                    binding.url,
                    if eligible {
                        ", as it is now"
                    } else {
                        ": its evidence says so once it stands"
                    }
                );
                if first {
                    self.post(&line, None);
                }
                if let Some(said) = super::dispatch::watch_start(self.env, &self.main) {
                    self.say(&format!("land {slug}: {said}"));
                }
                Ok(line)
            }
            "mergeable" if eligible => {
                // Before the request: from here the pull request decides, never a merge here.
                journal.stage = Stage::MergeRequested;
                journal.write(&self.folder).map_err(here)?;
                self.post(
                    &format!(
                        "land {slug}: asking {} to {} {} by pull request #{} ({}), its evidence at \
                         revision {}",
                        binding.name,
                        journal.method(),
                        journal.head,
                        binding.pull,
                        binding.url,
                        short(&journal.revision)
                    ),
                    None,
                );
                let request = json!({
                    "verb": "merge", "slug": slug, "by": self.actor, "head": journal.head,
                    "repository": binding.repository, "pull": binding.pull,
                    "method": journal.method(), "title": journal.title, "body": journal.body,
                });
                let asked = self.ask("merge", &delivering(request, &journal));
                self.settle(journal, Some(asked))
            }
            "mergeable" => Err(self.refuse(Refusal::to(
                format!(
                    "{} would merge pull request #{} of {slug} now, while its evidence does not \
                     stand at {}: nothing was armed or merged, as the default branch's rules hold \
                     nothing to its evidence",
                    binding.name,
                    binding.pull,
                    short(&journal.head)
                ),
                "@orchestrator",
                "`symphony-github preflight` says what the rules lack: put it right, then \
                 dispatch again"
                    .into(),
            ))),
            "merged" | "closed" => {
                journal.stage = Stage::MergeRequested;
                journal.write(&self.folder).map_err(here)?;
                self.settle(journal, None)
            }
            "refused" => {
                let line = format!(
                    "{} refused to arm pull request #{} of {slug} at {}: {why}",
                    binding.name,
                    binding.pull,
                    short(&journal.head)
                );
                Err(self.refuse(match answer["kind"].as_str().unwrap_or_default() {
                    "conflict" | "head" => Refusal::to(
                        line,
                        "@executor",
                        "Settle it on the branch; a new head is assessed and stamped again"
                            .into(),
                    ),
                    _ => Refusal::to(
                        line,
                        "@orchestrator",
                        "Nothing merged, here or there: put right what it names, then dispatch \
                         again"
                            .into(),
                    ),
                }))
            }
            other => {
                let line = not_armed(if why.is_empty() { other } else { why });
                if before && let Some(said) = super::dispatch::watch_start(self.env, &self.main) {
                    self.say(&format!("land {slug}: {said}"));
                }
                Err(self.refuse(Refusal::here(line)))
            }
        }
    }

    /// Refuse while the project file here is not the one the remote default branch commits at
    /// `base`: every delivery, a task's or the regenerator's, is judged by the settings there.
    fn committed(&self, base: &str) -> Result<(), Refusal> {
        let default = &self.settings.default_branch;
        let file = |at: &str| rev(&self.main, &format!("{at}:{}", super::MANIFEST));
        if file("HEAD") != file(base) {
            return Err(Refusal::here(format!(
                "{} here is not the one {REMOTE}/{default} commits at {}: the landing's settings \
                 are read from the remote default branch, so bring the two together first",
                super::MANIFEST,
                short(base)
            )));
        }
        Ok(())
    }

    /// Refuse a head a squash must not deliver: one that changes a generator's outputs
    /// ([`Landing::outputs`]); where the project names an upstream, one that holds any of its
    /// commits as fetched now ([`Landing::upstreamed`]), which only an import assessed as one
    /// delivers ([`Landing::import`]); and one that merges a commit the remote default does not
    /// hold.
    fn admit(&self, base: &str, tip: &str) -> Result<(), Refusal> {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        self.outputs(base, tip)?;
        if let Some(upstream) = &self.settings.upstream {
            let fetched = self.upstreamed(upstream)?;
            let theirs = |not: &[&str]| -> Result<Vec<String>, Refusal> {
                let args = [&["rev-list", tip, "--not", base][..], not].concat();
                Ok((run_git(&self.main, &args)?.lines().map(str::to_string)).collect())
            };
            let ours = theirs(&[&fetched])?;
            if let Some(held) = (theirs(&[])?.into_iter()).find(|commit| !ours.contains(commit)) {
                let newest = run_git(&self.main, &["merge-base", tip, &fetched])?;
                return Err(Refusal::to(
                    format!(
                        "{slug}'s head {} holds {}, a commit of the upstream {} that \
                         {REMOTE}/{default} does not hold: a task is squashed, which would deliver \
                         its files and drop it from the history, and its assessment names no \
                         import at this head",
                        short(tip),
                        short(&held),
                        upstream.url
                    ),
                    "@orchestrator",
                    format!(
                        "If it is an import, its assessor names it at this head: `just tasks \
                         needs {slug} STAMP… --tip {tip} --import {newest}`; if not, the branch \
                         drops those commits. Nothing landed"
                    ),
                ));
            }
        }
        let range = format!("{base}..{tip}");
        let merges = run_git(&self.main, &["rev-list", "--merges", "--parents", &range])?;
        for line in merges.lines() {
            let commits: Vec<&str> = line.split(' ').collect();
            let foreign =
                (commits.iter().skip(2)).find(|parent| !ancestor(&self.main, parent, base));
            if let Some(parent) = foreign {
                return Err(Refusal::to(
                    format!(
                        "{slug}'s head {} merges {}, a commit {REMOTE}/{default} does not hold \
                         ({}): a squash would deliver its files and drop it from the history",
                        short(tip),
                        short(parent),
                        short(commits[0])
                    ),
                    "@orchestrator",
                    format!(
                        "An import of another history is delivered only from the upstream \
                         {} names ([land.upstream]), assessed as one: nothing landed",
                        super::MANIFEST
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Refuse a head that changes a generator's outputs, which no hosted merge puts back. What the
    /// refusal asks for is told by where the head stands to the release candidate
    /// ([`Landing::candidate`]): a fix of it keeps the outputs its own sources make, which the
    /// candidate takes, and delivers a merge of the default branch that keeps the default
    /// branch's instead (RELEASES, a red's fix); any other head puts them back.
    fn outputs(&self, base: &str, tip: &str) -> Result<(), Refusal> {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        let range = format!("{base}...{tip}");
        let changed = super::git_z(&self.main, &["diff", "--name-only", "-z", &range])?;
        let generated: Vec<&str> = (changed.iter())
            .filter(|file| (self.settings.generators.iter()).any(|g| owns(&g.outputs, file)))
            .map(String::as_str)
            .collect();
        if generated.is_empty() {
            return Ok(());
        }
        let line = format!(
            "{slug} changes {} generated {} ({}), which a pull request would deliver as the \
             branch built {}",
            generated.len(),
            if generated.len() == 1 {
                "path"
            } else {
                "paths"
            },
            generated
                .iter()
                .take(super::NAMED_PATHS)
                .copied()
                .collect::<Vec<_>>()
                .join(", "),
            if generated.len() == 1 { "it" } else { "them" },
        );
        let back = format!(
            "Put them back as {default} has them on the branch (`git checkout \
             {REMOTE}/{default} -- PATH`, committed), then have the new head assessed and stamped"
        );
        let next = match self.candidate(base, tip)? {
            Candidate::Fix(at) => {
                let outputs: Vec<&str> = (self.settings.generators.iter())
                    .map(|g| g.outputs.as_str())
                    .filter(|output| generated.iter().any(|file| owns(output, file)))
                    .collect();
                format!(
                    "{slug} starts at {CANDIDATE} ({}), a fix of the release candidate, which \
                     takes the outputs its own sources make: keep {} as it is, approved by a \
                     fresh reviewer naming it whatever its assessment needs, as the candidate \
                     takes only a commit an approve names and a land with none drops it. Then \
                     merge {REMOTE}/{default} into the branch once, keeping {default}'s side of \
                     every generated path (`git restore --source={REMOTE}/{default} --staged \
                     --worktree -- {}` before the merge's commit), and have that head assessed, \
                     tested and approved: it is delivered, and the candidate advances to the \
                     commit approved before the merge",
                    short(&at),
                    short(tip),
                    outputs.join(" ")
                )
            }
            Candidate::Behind(at) => format!(
                "{back}. A fix of the release candidate starts at {CANDIDATE}, now at {}, which \
                 this head does not hold: rebased there while nothing approved it, its land says \
                 how its outputs are delivered; approved already, it lands as any task and its \
                 land drops the candidate",
                short(&at)
            ),
            Candidate::None => back,
        };
        Err(Refusal::to(line, "@executor", next))
    }

    /// Where `tip` stands to the release candidate, as the branch [`CANDIDATE`] names its tip in
    /// the main checkout while the release run holds one: a fix of it holds that tip and, beyond
    /// it, only commits of its own, none the remote default branch holds at `base`.
    fn candidate(&self, base: &str, tip: &str) -> Result<Candidate, Refusal> {
        let Some(at) = rev(&self.main, &format!("refs/heads/{CANDIDATE}")) else {
            return Ok(Candidate::None);
        };
        if !ancestor(&self.main, &at, tip) {
            return Ok(Candidate::Behind(at));
        }
        let beyond = |not: &[&str]| -> Result<String, Refusal> {
            let not_at = format!("^{at}");
            let args = [&["rev-list", "--count", tip, &not_at][..], not].concat();
            Ok(run_git(&self.main, &args)?)
        };
        let own = beyond(&[])?;
        let fix = own != "0" && own == beyond(&[&format!("^{base}")])?;
        Ok(if fix {
            Candidate::Fix(at)
        } else {
            Candidate::None
        })
    }

    /// The upstream's branch fetched now from `upstream`'s address to [`UPSTREAM`]: its tip. A head
    /// is judged by the upstream as it is now, a task's as well as an import's, never as it was
    /// last fetched, so one that does not answer (the network, its host, credentials, an address
    /// that moved) refuses every land here before anything is published, the line naming it.
    fn upstreamed(&self, upstream: &super::Upstream) -> Result<String, Refusal> {
        let default = &self.settings.default_branch;
        let unfetched = |why: &str| {
            Refusal::here(format!(
                "the upstream {} was not fetched: {why}: every land reads it while {} on \
                 {default} names it ([land.upstream]), a task's as well as an import's, so \
                 nothing landed. Land again once it answers; an address that no longer answers is \
                 corrected on {default} by the project's owner, as no land delivers it",
                upstream.url,
                super::MANIFEST
            ))
        };
        let fetch = format!("+refs/heads/{}:{UPSTREAM}", upstream.branch);
        let args = ["fetch", "--quiet", "--no-tags", &upstream.url, &fetch];
        let done = self.remote_output(&args).map_err(|why| unfetched(&why))?;
        if !done.status.success() {
            let said = String::from_utf8_lossy(&done.stderr);
            let said = (said.lines().map(str::trim)).find(|line| !line.is_empty());
            return Err(unfetched(said.unwrap_or("git fetch failed")));
        }
        rev(&self.main, UPSTREAM)
            .ok_or_else(|| Refusal::here(format!("{} has no {}", upstream.url, upstream.branch)))
    }

    /// The import of `commit` that `tip` is, held to the project's upstream and to the history it
    /// fetched from there now: `commit` on the upstream's branch and newer than the last import
    /// (the merge base of the default branch `base` and that branch), `tip` holding it and `base`
    /// not, merged by a commit of `tip`'s own first-parent line, and every other commit `tip` adds
    /// its own line's or the upstream's. A commit neither the branch's own nor the upstream's
    /// refuses.
    fn import(&self, base: &str, tip: &str, commit: &str) -> Result<Import, Refusal> {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        let to = |line: String, then: &str| Refusal::to(line, "@orchestrator", then.into());
        let Some(upstream) = &self.settings.upstream else {
            return Err(to(
                format!(
                    "{slug}'s assessment names an import of {}, and {} on {default} names no \
                     upstream ([land.upstream]): no import is delivered here",
                    short(commit),
                    super::MANIFEST
                ),
                "The project names its upstream in a landing of its own, or the assessment names \
                 no import: nothing landed",
            ));
        };
        let url = &upstream.url;
        let fetched = self.upstreamed(upstream)?;
        let holds = |commit: &str, tip: &str| ancestor(&self.main, commit, tip);
        let then = "Its assessor names the upstream commit the head imports, or the branch is put \
                    right: nothing landed";
        if rev(&self.main, &format!("{commit}^{{commit}}")).as_deref() != Some(commit)
            || !holds(commit, &fetched)
        {
            return Err(to(
                format!(
                    "{} is not on {url}'s {} as fetched now ({}): an import is of a commit the \
                     upstream's default branch holds",
                    short(commit),
                    upstream.branch,
                    short(&fetched)
                ),
                then,
            ));
        }
        let Ok(before) = run_git(&self.main, &["merge-base", base, &fetched]) else {
            return Err(to(
                format!(
                    "{default} and {url}'s {} share no commit: the default branch is seeded \
                     again from the upstream's history by its owner before any import",
                    upstream.branch
                ),
                "Nothing landed, and no history is merged without a common ancestor",
            ));
        };
        if !holds(&before, commit) {
            return Err(to(
                format!(
                    "{}, where {default} and {url}'s {} last met, is not an ancestor of {}: an \
                     import is of an upstream commit newer than all {default} holds of it",
                    short(&before),
                    upstream.branch,
                    short(commit)
                ),
                then,
            ));
        }
        if holds(commit, base) {
            return Err(to(
                format!(
                    "{REMOTE}/{default} holds {} already: nothing to import",
                    short(commit)
                ),
                "The task is cancelled, or its assessor names a newer upstream commit",
            ));
        }
        let line = run_git(
            &self.main,
            &[
                "rev-list",
                "--first-parent",
                "--parents",
                tip,
                "--not",
                base,
            ],
        )?;
        let own: Vec<&str> = (line.lines()).filter_map(|l| l.split(' ').next()).collect();
        let merges = (line.lines()).any(|l| l.split(' ').skip(2).any(|parent| parent == commit));
        if !holds(commit, tip) || !merges {
            return Err(to(
                format!(
                    "{slug}'s head {} merges {} on no commit of its own first-parent line: an \
                     import is a merge of the upstream commit (`git merge --no-ff {}`) on its \
                     branch, then any fixes",
                    short(tip),
                    short(commit),
                    commit
                ),
                then,
            ));
        }
        let others = run_git(&self.main, &["rev-list", tip, "--not", base, commit])?;
        if let Some(foreign) = (others.lines()).find(|other| !own.contains(other)) {
            return Err(to(
                format!(
                    "{slug}'s head {} holds {}, a commit neither of its own first-parent line \
                     nor of {}'s history: an import delivers its branch's own commits and the \
                     upstream's alone",
                    short(tip),
                    short(foreign),
                    short(commit)
                ),
                then,
            ));
        }
        Ok(Import {
            url: url.clone(),
            branch: upstream.branch.clone(),
            commit: commit.into(),
            tip: fetched,
            before,
            repository: 0,
            parents: Vec::new(),
            after: String::new(),
        })
    }

    /// The task's evidence at `tip`, read now: refused unless it is eligible where `eligible_only`.
    pub(super) fn read_evidence(
        &self,
        task: &Task,
        tip: &str,
        eligible_only: bool,
    ) -> Result<Snapshot, Stop> {
        if eligible_only {
            return self.eligible(task, tip);
        }
        let slug = self.slug;
        let (task, thread): (Task, Vec<Message>) =
            match (self.store.get(slug), self.store.thread(slug)) {
                (Ok(task), Ok(thread)) => (task, thread),
                _ => (task.clone(), Vec::new()),
            };
        evidence::snapshot(self.store, self.env, &self.main, &task, &thread, tip)
            .map_err(|line| self.refuse(Refusal::here(line)))
    }

    /// The task's evidence at `tip`, read now, refused unless it is eligible.
    fn eligible(&self, task: &Task, tip: &str) -> Result<Snapshot, Stop> {
        let slug = self.slug;
        // Read again: the task and its thread may have changed since the land began.
        let (task, thread): (Task, Vec<Message>) =
            match (self.store.get(slug), self.store.thread(slug)) {
                (Ok(task), Ok(thread)) => (task, thread),
                _ => (task.clone(), Vec::new()),
            };
        let snapshot = evidence::snapshot(self.store, self.env, &self.main, &task, &thread, tip)
            .map_err(|line| self.refuse(Refusal::here(line)))?;
        if snapshot.eligible {
            return Ok(snapshot);
        }
        Err(self.refuse(Refusal::to(
            format!(
                "{slug} is not eligible for delivery by pull request at {}: {}",
                short(tip),
                snapshot.blocking.join("; ")
            ),
            "@orchestrator",
            "A delivery by pull request takes an assessment of the head and each stamp it needs \
             at that head, an approve naming it: nothing was merged"
                .into(),
        )))
    }

    /// The pull request a `publish` answer binds the delivery to: the same one a run before bound.
    fn bound(&self, answer: &Value, journal: &Journal, tip: &str) -> Result<Binding, String> {
        let (said, why) = state(answer);
        if said != "published" {
            return Err(format!(
                "{} was not published ({}): {why}; nothing merged",
                self.slug,
                if said.is_empty() { "no state" } else { said }
            ));
        }
        let binding = Binding {
            repository: answer["repository"]["id"].as_i64().unwrap_or_default(),
            name: answer["repository"]["name"]
                .as_str()
                .unwrap_or_default()
                .into(),
            pull: answer["pull"]["number"].as_i64().unwrap_or_default(),
            url: answer["pull"]["url"].as_str().unwrap_or_default().into(),
        };
        if binding.repository <= 0 || binding.pull <= 0 {
            return Err(
                "the publication names no repository or pull request; nothing merged".into(),
            );
        }
        if answer["head"].as_str() != Some(tip) {
            return Err(format!(
                "pull request #{} is at {}, not the head {}: nothing merged",
                binding.pull,
                answer["head"].as_str().map_or("no commit", short),
                short(tip)
            ));
        }
        match &journal.binding {
            Some(earlier)
                if earlier.repository != binding.repository || earlier.pull != binding.pull =>
            {
                Err(format!(
                    "{} was bound to pull request #{} of repository {} and is now answered with \
                     #{} of {}: one task is delivered by one pull request; nothing merged",
                    self.slug, earlier.pull, earlier.repository, binding.pull, binding.repository
                ))
            }
            _ => Ok(binding),
        }
    }

    /// Go on from a delivery whose merge was asked for: read the bound pull request, whatever
    /// `asked` (the request's own answer, when this run made it) said, and act on what the host
    /// holds. A command's word alone proves nothing.
    pub(super) fn settle(
        &mut self,
        mut journal: Journal,
        asked: Option<Result<Value, String>>,
    ) -> Result<String, Stop> {
        let slug = self.slug;
        let here = |line: String| Stop::Refused(line);
        if journal.stage >= Stage::RemoteMerged {
            return self.reconcile(journal);
        }
        let binding = journal
            .binding
            .clone()
            .ok_or_else(|| here("unbound".into()))?;
        let refused = match &asked {
            Some(Ok(answer)) if state(answer).0 == "refused" => Some((
                answer["kind"].as_str().unwrap_or_default().to_string(),
                state(answer).1.to_string(),
            )),
            _ => None,
        };
        let request = json!({
            "verb": "result", "slug": slug,
            "repository": binding.repository, "pull": binding.pull,
        });
        let unknown = |why: &str| {
            Stop::Refused(format!(
                "the merge of {slug} by pull request #{} was asked for and its result is not \
                 known ({why}): nothing is merged here. Run the land again, which reads the pull \
                 request first",
                binding.pull
            ))
        };
        let result = self.ask("result", &request).map_err(|why| unknown(&why))?;
        let (said, why) = state(&result);
        let head = result["head"].as_str().unwrap_or_default();
        match said {
            "merged" if head == journal.head => {}
            "open" => {
                // Nothing merged: asked again only by a run that validates again.
                journal.stage = Stage::Eligible;
                journal.held = None;
                journal.write(&self.folder).map_err(here)?;
                let (line, to, then) = match refused {
                    Some((kind, why)) => {
                        let line = format!(
                            "{} refused to {} {slug} at {} by pull request #{}: {why}",
                            binding.name,
                            journal.method(),
                            short(&journal.head),
                            binding.pull
                        );
                        match kind.as_str() {
                            "conflict" | "head" => (
                                line,
                                "@executor",
                                "Settle it on the branch; a new head is assessed and stamped \
                                 again"
                                    .to_string(),
                            ),
                            _ => (
                                line,
                                "@orchestrator",
                                "Nothing merged, here or there: put right what it names, then \
                                 land again"
                                    .to_string(),
                            ),
                        }
                    }
                    None => (
                        format!(
                            "pull request #{} of {slug} is still open: nothing merged",
                            binding.pull
                        ),
                        "@orchestrator",
                        "Run the land again, which validates its evidence again before it asks \
                         again"
                            .to_string(),
                    ),
                };
                return Err(self.refuse(Refusal::to(line, to, then)));
            }
            "merged" | "closed" => {
                let what = if said == "closed" {
                    format!("pull request #{} was closed without a merge", binding.pull)
                } else {
                    format!(
                        "pull request #{} was merged at {}, not the head {} that was read",
                        binding.pull,
                        if head.is_empty() {
                            "no commit"
                        } else {
                            short(head)
                        },
                        short(&journal.head)
                    )
                };
                return Err(self.hold(journal, what));
            }
            _ => return Err(unknown(why)),
        }
        if result["repository"].as_i64() != Some(binding.repository)
            || result["base"].as_str() != Some(self.settings.default_branch.as_str())
        {
            let what = format!(
                "pull request #{} answers for repository {} and base {}, not the ones bound",
                binding.pull, result["repository"], result["base"]
            );
            return Err(self.hold(journal, what));
        }
        let Some(squash) = result["sha"].as_str().map(str::to_string) else {
            return Err(unknown("merged, with no commit named"));
        };
        match self.prove(&journal, &binding, &squash) {
            Ok(receipt) => {
                journal.stage = Stage::RemoteMerged;
                journal.squash = Some(receipt.squash);
                journal.held = None;
                journal.write(&self.folder).map_err(here)?;
                self.reconcile(journal)
            }
            Err(Unproved::Held(what)) => Err(self.hold(journal, what)),
            Err(Unproved::Later(why)) => Err(unknown(&why)),
        }
    }

    /// Hold the delivery for a person: said on the thread to the orchestrator, nothing closed.
    pub(super) fn hold(&self, mut journal: Journal, what: String) -> Stop {
        let known = journal.held.as_deref() == Some(what.as_str());
        journal.held = Some(what.clone());
        let _ = journal.write(&self.folder);
        let line = format!(
            "the delivery of {} is held for reconciliation: {what}. Nothing is closed and nothing \
             is merged here",
            self.slug
        );
        // Said once: a run again finds it held as it was.
        if known {
            return Stop::Refused(line);
        }
        self.refuse(Refusal::to(
            line,
            "@orchestrator",
            format!(
                "A person judges what the remote holds; the delivery's journal is {}",
                Journal::path(&self.folder, self.slug).display()
            ),
        ))
    }

    /// Prove the commit `squash` the host made, by git alone: fetched, on the remote default
    /// branch, of one parent, and its tree the merge of that parent and the head. The receipt is
    /// kept and the head retained once it is, and neither over another: a receipt of another
    /// delivery of the slug, one that does not read, or a retained head that is another commit
    /// holds this one for a person, and the receipt of this same delivery is kept as it is.
    fn prove(
        &self,
        journal: &Journal,
        binding: &Binding,
        squash: &str,
    ) -> Result<Receipt, Unproved> {
        let default = &self.settings.default_branch;
        let remote = self.fetched().map_err(Unproved::Later)?;
        let commit = rev(&self.main, &format!("{squash}^{{commit}}")).ok_or_else(|| {
            Unproved::Later(format!("{REMOTE} does not give the commit {squash} yet"))
        })?;
        if !ancestor(&self.main, &commit, &remote) {
            return Err(Unproved::Held(format!(
                "{} is not on {REMOTE}/{default}",
                short(&commit)
            )));
        }
        let listed = run_git(&self.main, &["rev-list", "--parents", "-n", "1", &commit])
            .map_err(Unproved::Later)?;
        let parents: Vec<&str> = listed.split(' ').skip(1).collect();
        let import = journal
            .import
            .as_ref()
            .filter(|_| journal.operation == IMPORT);
        let parent = match (import, parents.as_slice()) {
            (None, [parent]) => *parent,
            (None, _) => {
                return Err(Unproved::Held(format!(
                    "{} has {} parents: the head was merged by a merge commit, not squashed, so \
                     its branch's commits are on {default}",
                    short(&commit),
                    parents.len()
                )));
            }
            (Some(_), [first, second]) if *second == journal.head => *first,
            (Some(import), [_]) => {
                return Err(Unproved::Held(format!(
                    "{} has one parent: the import of {} was squashed, its files on {default} \
                     without the upstream's commit in its history. A new import task of the same \
                     commit, whose merge changes no file, restores it; this one does not land",
                    short(&commit),
                    import.commit
                )));
            }
            (Some(_), _) => {
                return Err(Unproved::Held(format!(
                    "{} has the parents {}, not the base then the head {}: what landed is not the \
                     import that was read",
                    short(&commit),
                    parents
                        .iter()
                        .map(|parent| short(parent))
                        .collect::<Vec<_>>()
                        .join(", "),
                    short(&journal.head)
                )));
            }
        };
        let merged = git(
            &self.main,
            &["merge-tree", "--write-tree", parent, &journal.head],
        )
        .ok_or_else(|| Unproved::Later("git merge-tree did not run".into()))?;
        let expected = match merged.status.code() {
            Some(0) => (String::from_utf8_lossy(&merged.stdout).lines().next())
                .map(str::to_string)
                .unwrap_or_default(),
            Some(1) => String::new(),
            _ => {
                return Err(Unproved::Later(format!(
                    "git merge-tree could not merge {} and {}",
                    short(parent),
                    short(&journal.head)
                )));
            }
        };
        let tree = rev(&self.main, &format!("{commit}^{{tree}}")).unwrap_or_default();
        if tree.is_empty() || tree != expected {
            return Err(Unproved::Held(format!(
                "the tree of {} is not what {} merged into its parent {} makes: what landed is \
                 not what was read",
                short(&commit),
                short(&journal.head),
                short(parent)
            )));
        }
        let import = match import {
            Some(import) if !ancestor(&self.main, &import.commit, &commit) => {
                return Err(Unproved::Held(format!(
                    "{} does not hold {}, the upstream commit the import was read as",
                    short(&commit),
                    short(&import.commit)
                )));
            }
            Some(import) => Some(Import {
                parents: parents.iter().map(|parent| (*parent).to_string()).collect(),
                after: run_git(&self.main, &["merge-base", &commit, &import.tip])
                    .unwrap_or_default(),
                ..import.clone()
            }),
            None => None,
        };
        let retained = format!("{RETAINED}{}", journal.slug);
        let earlier = Receipt::held(&self.folder, &journal.slug).map_err(Unproved::Held)?;
        if let Some(earlier) = &earlier
            && (earlier.head != journal.head || earlier.squash != commit)
        {
            return Err(Unproved::Held(format!(
                "{} was delivered already as {} from {}, and {REMOTE}/{default} holds {} of {} \
                 as well: its receipt and the head kept under {retained} are not replaced",
                journal.slug,
                short(&earlier.squash),
                short(&earlier.head),
                short(&commit),
                short(&journal.head)
            )));
        }
        if let Some(other) = rev(&self.main, &retained).filter(|kept| *kept != journal.head) {
            return Err(Unproved::Held(format!(
                "{retained} keeps {}, not the head {} that {} is of: it is not moved",
                short(&other),
                short(&journal.head),
                short(&commit)
            )));
        }
        let receipt = Receipt {
            slug: journal.slug.clone(),
            task: self
                .store
                .get(&journal.slug)
                .map(|task| task.id)
                .unwrap_or_default(),
            operation: journal.operation.clone(),
            repository: binding.repository,
            name: binding.name.clone(),
            pull: binding.pull,
            head: journal.head.clone(),
            base_observed: journal.base_observed.clone(),
            parent: parent.to_string(),
            squash: commit,
            tree,
            revision: journal.revision.clone(),
            by: journal.by.clone(),
            import,
        };
        // The head first: a squash leaves it in no history, and the receipt names it.
        run_git(&self.main, &["update-ref", &retained, &journal.head]).map_err(Unproved::Later)?;
        // This delivery's receipt, kept by a run cut short after it: as it was written.
        if let Some(earlier) = earlier {
            return Ok(earlier);
        }
        kept(&Receipt::path(&self.folder, &journal.slug), &receipt).map_err(Unproved::Later)?;
        Ok(receipt)
    }

    /// From a merge proved: follow the remote in the main checkout, close the task once, and
    /// start once what follows a land.
    fn reconcile(&mut self, mut journal: Journal) -> Result<String, Stop> {
        let slug = self.slug;
        let default = self.settings.default_branch.clone();
        let here = |line: String| Stop::Refused(line);
        let squash = journal
            .squash
            .clone()
            .ok_or_else(|| here("unproved".into()))?;
        let binding = journal
            .binding
            .clone()
            .ok_or_else(|| here("unbound".into()))?;
        if journal.stage == Stage::RemoteMerged {
            let pending = |why: String| {
                format!(
                    "{slug} landed remotely as {} by pull request #{} of {}; reconciliation \
                     pending: {why}. Nothing here was reset, merged or pushed: put the main \
                     checkout right, then run the land again, which fast-forwards it and closes \
                     the task",
                    short(&squash),
                    binding.pull,
                    binding.name
                )
            };
            let said = |landing: &Self, why: String| {
                let line = pending(why);
                // Said once on the thread, however often it is tried.
                let told = (landing.store.thread(slug).unwrap_or_default().iter()).any(|post| {
                    post.text.contains("reconciliation pending")
                        && post.text.contains(short(&squash))
                });
                if told {
                    Stop::Refused(line)
                } else {
                    landing.refuse(Refusal::to(
                        line,
                        "@orchestrator",
                        "The task stays open until its checkout follows the remote".into(),
                    ))
                }
            };
            let remote = self.fetched().map_err(|why| said(self, why))?;
            self.clean().map_err(|why| said(self, why))?;
            let local = self.base().map_err(|why| said(self, why))?;
            if !ancestor(&self.main, &local, &remote) {
                return Err(said(
                    self,
                    format!(
                        "{default} here, at {}, holds commits {REMOTE}/{default} does not",
                        short(&local)
                    ),
                ));
            }
            if local != remote {
                match self.fast_forward(&local, &remote, false).map_err(here)? {
                    Move::Done => {}
                    Move::Raced(now) => {
                        return Err(said(
                            self,
                            format!("{default} moved here to {}", short(&now)),
                        ));
                    }
                    Move::Not { why, .. } => return Err(said(self, why)),
                }
            }
            journal.stage = Stage::Reconciled;
            journal.write(&self.folder).map_err(here)?;
        }
        let landed = landed_line(slug, &default, &squash);
        let how = match &journal.import {
            Some(import) => format!(
                "merged from {}, an import of {},",
                journal.head, import.commit
            ),
            None => format!("squashed from {}", journal.head),
        };
        let line = format!(
            "{landed}{}; {how} by pull request #{} of {}",
            journal.stood, binding.pull, binding.name
        );
        if journal.stage == Stage::Reconciled {
            // Closed once: a run cut short after the board's write finds its line there.
            let thread = self.store.thread(slug)?;
            if !thread.iter().any(|post| post.text.starts_with(&landed)) {
                let said = format!("{line}{}", self.tokens_said());
                let from: Vec<&str> = journal.closes.iter().map(String::as_str).collect();
                if let Err(error) = self.store.landed(slug, self.actor, &said, true, &from) {
                    return Err(Stop::Refused(format!(
                        "{line}, but the board did not close {slug}: {error}. Run the land again, \
                         which closes it"
                    )));
                }
            }
            journal.stage = Stage::Closed;
            journal.write(&self.folder).map_err(here)?;
        }
        // Told on every run that reaches here: the verb is safe to ask twice, and a run cut short
        // after the stage was written and before it was asked would otherwise never tell it.
        let dispatched = journal.stage == Stage::Dispatching;
        {
            let request = json!({
                "verb": "landed", "slug": slug, "by": self.actor, "head": journal.head,
                "repository": binding.repository, "pull": binding.pull, "squash": squash,
            });
            match self.ask("landed", &request) {
                Ok(answer) if state(&answer).0 == "noted" => {
                    if let Some(said) = answer["said"].as_str().filter(|said| !said.is_empty()) {
                        self.say(&format!("land {slug}: {said}"));
                    }
                }
                Ok(answer) => self.say(&format!(
                    "land {slug}: the delivery command did not note the landing: {}",
                    state(&answer).1
                )),
                Err(why) => self.say(&format!(
                    "land {slug}: the delivery command did not note the landing: {why}"
                )),
            }
        }
        if !dispatched {
            // Named before anything is started, so nothing is started twice.
            journal.stage = Stage::Dispatching;
            journal.write(&self.folder).map_err(here)?;
            self.after(&squash, true);
        } else {
            self.say(&format!(
                "land {slug}: what follows a land was started by a run before this one, and is \
                 not started twice"
            ));
            // But the regenerator, which a run cut short before it started it never did: a start
            // makes nothing twice, by its lock, the tip it finished with and what it has pending.
            let started = super::regenerate::start(self.env, &self.main, &self.settings);
            if let Some(said) = started {
                self.say(&format!("land {slug}: {said}"));
            }
        }
        Journal::drop_in(&self.folder, slug);
        super::dispatch::unmark(&self.folder, slug);
        super::dispatch::unsaid(&self.folder, slug);
        super::dispatch::unstopped(&self.folder, slug);
        Ok(line)
    }
}

/// How the landed line of `slug`, delivered to `default` as `squash`, starts: what a land that
/// closed its task wrote, and what a land run again reads.
fn landed_line(slug: &str, default: &str, squash: &str) -> String {
    format!(
        "landed {slug} on {default} at {}",
        &squash[..squash.len().min(12)]
    )
}

/// Why a commit the host named is not proved.
enum Unproved {
    /// It is not what was read: held for a person.
    Held(String),
    /// It could not be read yet: a run again tries.
    Later(String),
}
