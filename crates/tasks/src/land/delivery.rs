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
//! - `merge`: one request to squash that pull request at that head, and no other. Answers
//!   `merged`, `refused` (with `kind`: `conflict`, `head`, `checks`, `permission`) or `unknown`.
//! - `result`: the bound pull request as the host holds it: `open`, `merged` (with the commit) or
//!   `closed`, its head, its base and its repository.
//! - `landed`: told once the board closed the task, for what the program does then.
//!
//! Any verb may answer `refused` or `unavailable` with `why`. The program decides nothing: what a
//! task needs, whether it stands and whether to ask for the merge are this land's, by
//! [`evidence::snapshot`], the one reading `tasks api`'s `evidence` answers too.
//!
//! The land keeps a [`Journal`] in the lands' folder, written before each step it names:
//!
//! 1. **Prepared**: the head, the snapshot's revision and the commit's title and body. The
//!    remote default branch is fetched and its project file must be the one committed here; a
//!    head that changes a generator's outputs, or merges a commit the remote default does not
//!    hold (an import of another history, which a squash would flatten), is refused.
//! 2. **Eligible**: published, and the snapshot read again: one of another revision is published
//!    again, never merged.
//! 3. **Merge requested**: written before the request. From here no run merges locally, whatever
//!    the settings say by then: a run reads the bound pull request first ([`Landing::settle`]).
//! 4. **Remote merged**: the commit **S** the host made is proved by git alone
//!    ([`Landing::prove`]): on the fetched default branch, of one parent, its tree the merge of
//!    that parent and the head. Anything else is held and closes nothing. A [`Receipt`] is kept
//!    for good, and the head stays reachable under [`RETAINED`], as a squash leaves it in no
//!    history.
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
}

impl Journal {
    pub fn path(folder: &Path, slug: &str) -> PathBuf {
        folder.join(format!("{}.delivery", file_stem(slug)))
    }

    pub fn read(folder: &Path, slug: &str) -> Option<Journal> {
        let text = fs::read_to_string(Journal::path(folder, slug)).ok()?;
        (serde_json::from_str::<Journal>(&text).ok()).filter(|journal| journal.slug == slug)
    }

    /// Kept in `folder`, whole or not at all, and on the disk before this returns.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        kept(&Journal::path(folder, &self.slug), self)
    }

    pub fn drop_in(folder: &Path, slug: &str) {
        let _ = fs::remove_file(Journal::path(folder, slug));
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
    /// The one parent of the squash: the base it was made on.
    pub parent: String,
    pub squash: String,
    pub tree: String,
    /// The snapshot's revision the merge was asked under.
    pub revision: String,
    pub by: String,
}

impl Receipt {
    pub fn path(folder: &Path, slug: &str) -> PathBuf {
        (folder.join(RECEIPTS)).join(format!("{}.json", file_stem(slug)))
    }

    pub fn read(folder: &Path, slug: &str) -> Option<Receipt> {
        let text = fs::read_to_string(Receipt::path(folder, slug)).ok()?;
        (serde_json::from_str::<Receipt>(&text).ok()).filter(|receipt| receipt.slug == slug)
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
         reads the pull request; with the host gone for good, see docs/TASKS.md, \"A delivery \
         whose host is gone\" (its journal is {})",
        journal.said(),
        Journal::path(folder, slug).display()
    ))
}

impl Journal {
    /// Where it stands, in a few words.
    fn said(&self) -> String {
        let pull = (self.binding.as_ref()).map_or_else(String::new, |b| {
            format!(", pull request #{} of {}", b.pull, b.name)
        });
        let held = (self.held.as_ref()).map_or_else(String::new, |why| format!(", held: {why}"));
        format!(
            "{} at {}{pull}{held}",
            match self.stage {
                Stage::Prepared => "prepared",
                Stage::Eligible => "eligible",
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

/// The `state` of an answer, and its `why`.
fn state(answer: &Value) -> (&str, &str) {
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

    fn ask(&self, verb: &str, request: &Value) -> Result<Value, String> {
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

    /// Deliver the task's `tip`, found ready as `judged`, by its pull request.
    pub(super) fn deliver(
        &mut self,
        task: &Task,
        tip: &str,
        judged: &Judged,
    ) -> Result<String, Stop> {
        let slug = self.slug;
        let default = self.settings.default_branch.clone();
        let base = (self.fetched()).map_err(|line| self.refuse(Refusal::here(line)))?;
        self.admit(&base, tip)
            .map_err(|refusal| self.refuse(refusal))?;
        self.owner_lines(&base, tip)
            .map_err(|refusal| self.refuse(refusal))?;
        let mut snapshot = self.eligible(task, tip)?;
        let (closes, stood): (&[&str], String) = match judged {
            Judged::Approved(_) => (&["done"], String::new()),
            Judged::Stamped(stood) => (&["review", "blocked", "done"], stood.clone()),
        };
        let earlier = Journal::read(&self.folder, slug).filter(|journal| journal.head == tip);
        let mut journal = Journal {
            slug: slug.into(),
            operation: TASK.into(),
            stage: Stage::Prepared,
            head: tip.into(),
            base_observed: base.clone(),
            revision: snapshot.revision.clone(),
            title: format!("{slug}: {}", task.title.lines().next().unwrap_or_default()),
            body: format!(
                "Squashed from {tip}, the head of {slug} its evidence stands on (revision {}).",
                snapshot.revision
            ),
            by: self.actor.into(),
            closes: closes.iter().map(|s| (*s).to_string()).collect(),
            stood,
            binding: earlier.and_then(|journal| journal.binding),
            squash: None,
            held: None,
        };
        let here = |line: String| Stop::Refused(line);
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
            let request = json!({
                "verb": "publish", "slug": slug, "by": self.actor, "head": tip,
                "branch": default, "title": journal.title, "body": journal.body,
                "snapshot": snapshot,
            });
            let answer = (self.ask("publish", &request))
                .map_err(|why| self.refuse(Refusal::here(format!("nothing merged: {why}"))))?;
            let binding = self
                .bound(&answer, &journal, tip)
                .map_err(|line| self.refuse(Refusal::here(line)))?;
            journal.binding = Some(binding);
            journal.revision = snapshot.revision.clone();
            journal.stage = Stage::Eligible;
            journal.write(&self.folder).map_err(here)?;
            let again = self.eligible(task, tip)?;
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
                 the land again",
                super::TRIES
            ))));
        }
        self.go_on(None).map_err(Stop::Refused)?;
        self.owner_lines(&base, tip)
            .map_err(|refusal| self.refuse(refusal))?;
        let binding = journal
            .binding
            .clone()
            .ok_or_else(|| here("unbound".into()))?;
        // Before the request: from here the pull request decides, never a merge made here.
        journal.stage = Stage::MergeRequested;
        journal.write(&self.folder).map_err(here)?;
        self.post(
            &format!(
                "land {slug}: asking {} to squash {tip} by pull request #{} ({}), its evidence at \
                 revision {}",
                binding.name,
                binding.pull,
                binding.url,
                short(&journal.revision)
            ),
            None,
        );
        let request = json!({
            "verb": "merge", "slug": slug, "by": self.actor, "head": tip,
            "repository": binding.repository, "pull": binding.pull, "method": "squash",
            "title": journal.title, "body": journal.body,
        });
        let asked = self.ask("merge", &request);
        self.settle(journal, Some(asked))
    }

    /// Refuse a head a squash must not deliver: one whose project file is not the remote default
    /// branch's, one that changes a generator's outputs, which no hosted merge puts back, and one
    /// that merges a commit the remote default does not hold.
    fn admit(&self, base: &str, tip: &str) -> Result<(), Refusal> {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        let file = |at: &str| rev(&self.main, &format!("{at}:{}", super::MANIFEST));
        if file("HEAD") != file(base) {
            return Err(Refusal::here(format!(
                "{} here is not the one {REMOTE}/{default} commits at {}: the landing's settings \
                 are read from the remote default branch, so bring the two together first",
                super::MANIFEST,
                short(base)
            )));
        }
        let range = format!("{base}...{tip}");
        let changed = super::git_z(&self.main, &["diff", "--name-only", "-z", &range])?;
        let generated: Vec<&str> = (changed.iter())
            .filter(|file| (self.settings.generators.iter()).any(|g| owns(&g.outputs, file)))
            .map(String::as_str)
            .collect();
        if !generated.is_empty() {
            return Err(Refusal::to(
                format!(
                    "{slug} changes {} generated {} ({}), which a pull request would deliver as \
                     the branch built {}",
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
                ),
                "@executor",
                format!(
                    "Put them back as {default} has them on the branch (`git checkout \
                     {REMOTE}/{default} -- PATH`, committed), then have the new head assessed and \
                     stamped"
                ),
            ));
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
                    "An import of another history is not delivered as a task, and this build \
                     delivers no import by pull request: nothing landed"
                        .into(),
                ));
            }
        }
        Ok(())
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
    fn settle(
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
                            "{} refused to squash {slug} at {} by pull request #{}: {why}",
                            binding.name,
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
    fn hold(&self, mut journal: Journal, what: String) -> Stop {
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
    /// kept and the head retained once it is.
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
        let [parent] = parents.as_slice() else {
            return Err(Unproved::Held(format!(
                "{} has {} parents: the head was merged by a merge commit, not squashed, so its \
                 branch's commits are on {default}",
                short(&commit),
                parents.len()
            )));
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
            parent: (*parent).to_string(),
            squash: commit,
            tree,
            revision: journal.revision.clone(),
            by: journal.by.clone(),
        };
        // The head first: a squash leaves it in no history, and the receipt names it.
        let retained = format!("{RETAINED}{}", journal.slug);
        run_git(&self.main, &["update-ref", &retained, &journal.head]).map_err(Unproved::Later)?;
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
        let landed = format!(
            "landed {slug} on {default} at {}",
            &squash[..squash.len().min(12)]
        );
        let line = format!(
            "{landed}{}; squashed from {} by pull request #{} of {}",
            journal.stood, journal.head, binding.pull, binding.name
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
        if journal.stage == Stage::Closed {
            // Named before anything is started, so nothing is started twice.
            journal.stage = Stage::Dispatching;
            journal.write(&self.folder).map_err(here)?;
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
            if !self.settings.generators.is_empty() {
                self.say(&format!(
                    "land {slug}: the regenerator is not started: with [land] delivery it commits \
                     nothing to {default}, and its own pull request is not built"
                ));
            }
            self.after(&squash, false);
        } else {
            self.say(&format!(
                "land {slug}: what follows a land was started by a run before this one, and is \
                 not started twice"
            ));
        }
        Journal::drop_in(&self.folder, slug);
        Ok(line)
    }
}

/// Why a commit the host named is not proved.
enum Unproved {
    /// It is not what was read: held for a person.
    Held(String),
    /// It could not be read yet: a run again tries.
    Later(String),
}
