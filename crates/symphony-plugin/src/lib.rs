//! What a Symphony plugin implements. A plugin is a value implementing [`Plugin`]; a host binary
//! compiles in the plugins it lists, and a project's `symphony.toml` names the ones it uses
//! (`[plugins] crates`). Each seam below takes and returns plain data (paths, text and the block
//! vocabulary of `symphony-blocks`, re-exported in [`vocabulary`]), never a Symphony library type,
//! so a plugin can answer from a process of its own as well as linked: each seam's call is the line
//! protocol's data in [`protocol`], and a plugin's binary whose `main` is [`serve`]`(&PLUGIN)`
//! answers it on its stdin and stdout (docs/engineering/PLUGIN_PROTOCOL.md).
//!
//! The client holds no plugin code: a component renders to [`vocabulary::Block`]s, which every
//! project's client draws with the same generic components.
//!
//! This crate is one of the plugin kit's three, which move together as their types cross: this one
//! (the seams, the data each call hands a plugin, a component's [`error`], the [`protocol`],
//! [`serve`] and the host a plugin's tests ask its seams through, [`testing`]), `symphony-blocks`
//! (the block vocabulary and the data the seams return, [`vocabulary`]) and `symphony-docs` (the doc
//! checks, [`DocCheck`], the docs' syntax they read, and the text and markup rules a plugin reads
//! and writes by). The kit's crates depend on no other workspace crate but each other, so a plugin
//! built on the kit links nothing of the Symphony's library, board or server
//! (docs/engineering/PLUGIN_PROTOCOL.md, "The kit").

#![forbid(unsafe_code)]

use std::any::Any;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// The block vocabulary a plugin renders to, and the data its seams return: `symphony-blocks`
/// re-exported whole, so every word a block carries is named from here. Everything public in that
/// crate is the kit's public vocabulary, and a type added there is a published addition, under the
/// kit's version rules (docs/engineering/PLUGIN_PROTOCOL.md, *Versions*).
pub mod vocabulary {
    pub use symphony_blocks::*;
}

mod renderer;
pub use renderer::{Drawing, File, Renderer};

pub mod protocol;

pub mod execution;

pub mod authority;
pub use authority::Authority;

pub mod actor;
pub use actor::{Actor, Sender};

mod serve;
pub use serve::{ENDING, WORKERS, serve, serve_on};

pub mod testing;

use vocabulary::{
    Block, FieldValue, Gate, InlineKind, NavItem, Picture, Presentation, Region, SettingsEntry,
    Span, TaskOffer,
};

/// What a host shows in a component's place for its error `problem`: one span of the class
/// `component-error`, `⚠ problem`. A plugin shows a problem of its own the same way with it, and its
/// tests compare a host's answer with it.
pub fn error(problem: &str) -> Block {
    Block::Paragraph {
        spans: vec![Span::Styled {
            tag: InlineKind::Span,
            presentation: Presentation {
                class: "component-error".into(),
                title: "component error".into(),
                ..Presentation::default()
            },
            spans: vec![Span::Text {
                text: format!("⚠ {problem}"),
            }],
        }],
    }
}

/// A plugin: its name (the one `[plugins] crates` lists) and what it adds at each seam. Every seam
/// but the name defaults to adding nothing.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;

    /// `[[name:argument|option]]` components, one per name.
    fn components(&self) -> Vec<&dyn Component> {
        Vec::new()
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        Vec::new()
    }
    /// The file types it draws, beside the Symphony's `markdown` and `media`.
    fn renderers(&self) -> Vec<&dyn Renderer> {
        Vec::new()
    }
    /// Answers under `/x/<path>/`.
    fn routes(&self) -> Vec<&dyn Route> {
        Vec::new()
    }
    /// Entries in the header and the home's links, after the Symphony's own.
    fn nav(&self) -> Vec<NavItem> {
        Vec::new()
    }
    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        Vec::new()
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        Vec::new()
    }
    fn board_fields(&self) -> Vec<&dyn BoardField> {
        Vec::new()
    }
    /// What answers the project's own gates of a task ([`Gates`]).
    fn gates(&self) -> Vec<&dyn Gates> {
        Vec::new()
    }
    /// The project's approval authorities it answers for, one per adapter ([`Authority`]).
    fn authorities(&self) -> Vec<&dyn Authority> {
        Vec::new()
    }
    /// The entries it adds to the project's Settings page, one per describer ([`Describer`]).
    fn describers(&self) -> Vec<&dyn Describer> {
        Vec::new()
    }
    fn watches(&self) -> Vec<&dyn Watch> {
        Vec::new()
    }
    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        Vec::new()
    }
    fn prose_rules(&self) -> Vec<&dyn ProseRule> {
        Vec::new()
    }
    fn generators(&self) -> Vec<&dyn Generator> {
        Vec::new()
    }
    /// Commands of the project's `symphony` binary, after the Symphony's own.
    fn commands(&self) -> Vec<&dyn Command> {
        Vec::new()
    }
    fn transports(&self) -> Vec<&dyn Transport> {
        Vec::new()
    }
    fn theme(&self) -> Option<Theme> {
        None
    }
}

/// One render of one document: the project's root, the page, the project's artifacts folder, and
/// what the page's components have loaded so far, so data several components read is read once per
/// render.
pub struct Page<'a> {
    pub root: &'a Path,
    /// The document being rendered, project-relative (empty outside a document).
    pub doc_path: &'a str,
    /// The project's artifacts folder, relative to its root: [`ARTIFACTS`] unless its
    /// `symphony.toml` names another (`[project] artifacts`). What its tools leave there a component
    /// finds by [`Page::artifact`].
    pub artifacts: &'a str,
    loaded: Loaded,
    /// The render this page is, unique in the process: what a request names it by
    /// ([`protocol::PageData::render`]).
    render: u64,
}

/// A project's artifacts folder when its `symphony.toml` names none, and on a page a host older than
/// protocol 0.2 asked for ([`protocol::PageData::artifacts`]).
pub const ARTIFACTS: &str = "artifacts";

/// What a page's components have loaded, by key: shared by the pages a plugin's process makes for
/// one render's requests ([`serve`]).
type Loaded = Arc<Mutex<BTreeMap<&'static str, Arc<dyn Any + Send + Sync>>>>;

impl<'a> Page<'a> {
    /// A page of the document `doc_path` in the project at `root`, its artifacts folder
    /// [`ARTIFACTS`] ([`Page::in_artifacts`] names another).
    pub fn new(root: &'a Path, doc_path: &'a str) -> Self {
        Self::sharing(root, doc_path, Loaded::default())
    }

    /// This page in a project whose artifacts folder is `artifacts`, relative to its root.
    #[must_use]
    pub fn in_artifacts(self, artifacts: &'a str) -> Self {
        Self { artifacts, ..self }
    }

    /// `rel` under the project's artifacts folder, relative to its root (`artifacts/screens`).
    pub fn artifact(&self, rel: &str) -> String {
        format!("{}/{rel}", self.artifacts)
    }

    /// A page whose components share what `loaded` holds with every other page made with it.
    fn sharing(root: &'a Path, doc_path: &'a str, loaded: Loaded) -> Self {
        static RENDERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            root,
            doc_path,
            artifacts: ARTIFACTS,
            loaded,
            render: RENDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// The value loaded under `key` on this page, `load`ing it the first time it is asked for.
    /// A key holds one type: asking for another under the same key loads that one in its place.
    pub fn once<T: Any + Send + Sync>(
        &self,
        key: &'static str,
        load: impl FnOnce() -> T,
    ) -> Arc<T> {
        if let Some(found) = self.cached(key) {
            return found;
        }
        // Loaded outside the lock, so a loader may ask for other keys of the same page.
        let value = Arc::new(load());
        let erased: Arc<dyn Any + Send + Sync> = value.clone();
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, erased);
        value
    }

    fn cached<T: Any + Send + Sync>(&self, key: &'static str) -> Option<Arc<T>> {
        let loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        loaded.get(key).cloned()?.downcast::<T>().ok()
    }
}

/// A `[[name:argument|option]]` component.
pub trait Component: Send + Sync {
    fn name(&self) -> &str;

    /// What the component shows for `argument` and `option` on `page`, or why it cannot.
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String>;

    /// Whether `argument` and `option` name something that exists, as the docs build checks a
    /// document. Rendering and dropping the result is the default.
    fn resolve(&self, page: &Page<'_>, argument: &str, option: Option<&str>) -> Result<(), String> {
        self.render(page, argument, option).map(drop)
    }

    /// A version covering every input `render` reads under the page's root. Without one, a
    /// document holding the component is rendered afresh on every read: a component with live
    /// data must never leave a cached picture or number.
    fn cache_version(&self, _page: &Page<'_>) -> Option<String> {
        None
    }
}

/// A decision card as a card kind reads it.
#[derive(Clone, Copy, Debug)]
pub struct Card<'a> {
    pub slug: &'a str,
    /// The document holding it, project-relative.
    pub doc: &'a str,
    /// The heading it sits under.
    pub topic: &'a str,
    /// Its text after the marker, without its thread.
    pub text: &'a str,
    /// Its thread, oldest first.
    pub thread: &'a [ThreadLine<'a>],
}

/// One line of a card's thread.
#[derive(Clone, Copy, Debug)]
pub struct ThreadLine<'a> {
    /// Whether an agent wrote it (else the owner did).
    pub agent: bool,
    /// The owner's verdict word it carries (`approved`), if any.
    pub verdict: Option<&'a str>,
    pub text: &'a str,
}

/// A kind of decision card a plugin recognises by its text.
pub trait CardKind: Send + Sync {
    fn name(&self) -> &str;
    fn recognises(&self, card: &Card<'_>) -> bool;
    /// The pictures the board shows on the card, in its order, read in the checkout at `root`: the
    /// first kind to give some wins.
    fn pictures(&self, _root: &Path, _card: &Card<'_>) -> Vec<Picture> {
        Vec::new()
    }
    /// The slot the card fills, as a page names it.
    fn slot(&self, _card: &Card<'_>) -> Option<String> {
        None
    }
    /// The markers the kind reads in a card's text, by name (`Slot` for `*Slot:* …`). A card's
    /// title and summary leave each out, with the words after it.
    fn markers(&self) -> &[&str] {
        &[]
    }
    /// The triage's category for the card (`model · lantern`); the first kind to give one wins.
    fn triage_category(&self, _card: &Card<'_>) -> Option<String> {
        None
    }
    /// Whether an approval predicted from the owner's past verdicts may stand for the owner's. The
    /// triage lists a pending card some kind refuses as the owner's sign-off alone.
    fn auto_approvable(&self, _card: &Card<'_>) -> bool {
        true
    }
    /// What the card gates that this kind counts, as (how many, the word for one: `item`). A kind
    /// that counts something answers for every card, none included, so the triage's reason names
    /// it; the triage adds the cards waiting on it.
    fn gates(&self, _card: &Card<'_>) -> Option<(usize, String)> {
        None
    }
    /// Whether the owner's approval leaves agent work: an approved card that waits for an agent
    /// to apply it.
    fn approval_needs_agent(&self, _card: &Card<'_>) -> bool {
        false
    }
    /// The letter (`B`) of the candidate the card's approval chose, for a card offering two or
    /// more lettered candidates; the triage counts the letters the owner picks. An error when the
    /// card's thread names a choice it cannot hold.
    fn chosen(&self, _card: &Card<'_>) -> Result<Option<String>, String> {
        Ok(None)
    }
}

/// A command of the project's `symphony` binary: `symphony NAME ARGS…`.
pub trait Command: Send + Sync {
    /// The binary's first argument that runs it.
    fn name(&self) -> &str;
    /// What it does, in one line, lowercase and under 60 characters, which `symphony help` lists
    /// beside its name; none says it is the project's.
    fn summary(&self) -> &str {
        ""
    }
    /// Runs it with `args`, what follows its name, writing to `out` and `err`; its exit code.
    fn run(&self, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32;
}

/// What a route answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    /// A JSON document, already serialized.
    Json(String),
    /// A file to send as it is.
    File(PathBuf),
}

/// An answer under `/x/<path>/`, behind the same gate as every other request.
pub trait Route: Send + Sync {
    /// The segment after `/x/` (`shelf`).
    fn path(&self) -> &str;
    /// The answer for `rest`, what follows `/x/<path>/`, in the checkout at `root`; `None` is
    /// not found.
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply>;
}

/// A region of the home, recomputed when the board's inputs change.
pub trait HomeRegion: Send + Sync {
    fn id(&self) -> &str;
    /// The region for the checkout at `root`, or `None` when it has nothing to show.
    fn region(&self, root: &Path) -> Option<Region>;
}

/// A task as a board action reads it.
#[derive(Clone, Copy, Debug)]
pub struct TaskRef<'a> {
    pub slug: &'a str,
    pub status: &'a str,
    /// The branch its work is on.
    pub branch: &'a str,
}

/// What the owner sends back on an offer: its payload, as it came, and their note. Who sent it
/// travels beside it ([`Sender`], [`BoardAction::submit_from`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub payload: String,
    pub note: String,
}

/// What a submission did: the line the board says for the owner on the task, and the tasks it
/// releases, which the board moves back to review.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submitted {
    pub said: String,
    pub released: Vec<String>,
}

/// A button on a task, with the server's handler behind it.
pub trait BoardAction: Send + Sync {
    fn name(&self) -> &str;
    /// What the action offers on `task` in the project at `root`, whatever the task's status: the
    /// action decides which work it offers on. An empty offer shows nothing.
    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String>;
    /// The owner's answer to the offer on `task`, or why it is refused (the board then says
    /// nothing and moves nothing).
    fn submit(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String>;
    /// The answer to the offer on `task`, sent by `sender`: what the host calls, so that what the
    /// action writes for them (a commit, a file, a record of its own) can name them and be
    /// authored as them. An action that does not read who sent it implements [`Self::submit`]
    /// alone, which this answers by.
    fn submit_from(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
        sender: &Sender,
    ) -> Result<Submitted, String> {
        let _ = sender;
        self.submit(root, task, submission)
    }
}

/// What answers a project's own gates of a task: the stamps or checks its landing counts, as
/// typed data the Symphony draws the same way for every project and can count
/// (`symphony_api::gates`). A gate it names as one of the Symphony's own (`review`, `check`,
/// `owner`) takes that one's place.
///
/// The Symphony keeps an answer by the task's status, its branch's tip and [`Gates::kept_while`], so
/// `answer`, which may start a landing binary, runs once for each; an error is asked again after a
/// while. All of a project's answerers are asked at once. An answer that errs, panics or takes
/// longer than the server waits (10 s) shows each gate the answerer answered before as unknown,
/// its error as the detail, in place of the Symphony's of that name (one gate named
/// [`Gates::name`] before it ever answered), and the rest of the page still arrives.
///
/// An answerer bounds its own work: a call still running after a minute is abandoned and asked
/// again on the next read, and while four of one answerer's calls run, abandoned ones among them,
/// none starts and its gates read unknown. A call that never returns holds its thread, and any
/// child it started, for the server's life.
pub trait Gates: Send + Sync {
    /// The answerer's name, unique among the project's: the gate shown in its place when it fails
    /// before it ever answered.
    fn name(&self) -> &str;
    /// The gates of `task` in the project at `root`, its branch at `tip` (`None` when the project's
    /// repository has no such branch), in any order; or why it cannot say.
    fn answer(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        tip: Option<&str>,
    ) -> Result<Vec<Gate>, String>;
    /// What the answer reads besides the task's status and its branch's tip, cheap to tell and
    /// never blocking: the default branch's tip, a stamp file's change time. A kept answer is used
    /// while those and this hold. Empty, the default, keeps it for the status and tip alone. A
    /// panic here reads as the answerer's error, like one in `answer`.
    fn kept_while(&self, _root: &Path, _task: &TaskRef<'_>) -> String {
        String::new()
    }
}

/// An entry a plugin adds to its project's Settings page, in the one shape the Symphony's own
/// entries take ([`SettingsEntry`]): its name, what it is for, its state and why, what it needs
/// and whether this machine has it, and its settings, each a preference or reviewed. The page
/// draws it with the component that draws the Symphony's own, after them, and knows it by no name;
/// only the plugins a project selects (`[plugins] crates`) describe for it.
///
/// A description declares: it runs nothing, writes nothing and reaches no outside service. It
/// cannot lower a setting's class: a setting the Symphony would run or that decides what lands
/// (a key the local settings file refuses) is shown reviewed whatever the describer says, and a
/// describer may mark a setting of its own reviewed. Nor can it take the board's scope
/// ([`SettingScope::Board`](vocabulary::SettingScope::Board)): the board's store keeps the
/// Symphony's own preferences alone, so the page shows a description's setting without it. A
/// linked plugin and one answering from its process describe alike (`describers.describe`,
/// protocol 0.5).
pub trait Describer: Send + Sync {
    /// Its id, unique among the plugin's describers: its entry's anchor on the page is the
    /// plugin's name and this, whatever id the entry it answers gives, counted on where an entry
    /// before took it.
    fn id(&self) -> &str;
    /// The entry for the project at `root`, or why it cannot say, which the page shows in the
    /// entry's place.
    fn describe(&self, root: &Path) -> Result<SettingsEntry, String>;
}

#[cfg(test)]
#[path = "describer_tests.rs"]
mod describer_tests;

/// What a board field holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FieldKind {
    Text,
    /// One of these words.
    Choice(Vec<String>),
    Number,
    /// A URL or a project-relative path.
    Link,
}

/// A typed value a plugin declares on every task of its project. Its value is stored in the task
/// record's `fields` object under the plugin's name; the server asks the field to validate a
/// value before the write's transaction.
pub trait BoardField: Send + Sync {
    fn name(&self) -> &str;
    fn kind(&self) -> FieldKind;
    fn required(&self) -> bool {
        false
    }
    /// The value to store for `value`, or why it is refused. The default checks it against the
    /// field's kind.
    fn validate(&self, value: &FieldValue) -> Result<FieldValue, String> {
        check_kind(&self.kind(), value)
    }
}

/// `value` as a field of `kind` stores it: a number field takes a number or text that reads as
/// one, the other kinds take text, a choice one of its words, a link a URL or a path.
pub fn check_kind(kind: &FieldKind, value: &FieldValue) -> Result<FieldValue, String> {
    let text = match value {
        FieldValue::Text(text) => text.trim(),
        FieldValue::Number(number) if *kind == FieldKind::Number => {
            return finite(*number);
        }
        FieldValue::Number(number) => return Err(format!("{number} is a number, not text")),
    };
    match kind {
        FieldKind::Number => text
            .parse::<f64>()
            .map_err(|_| format!("{text:?} is not a number"))
            .and_then(finite),
        _ if text.is_empty() => Err("the value is empty".into()),
        FieldKind::Text => Ok(FieldValue::Text(text.into())),
        FieldKind::Choice(words) => words
            .iter()
            .find(|word| word.as_str() == text)
            .map(|word| FieldValue::Text(word.clone()))
            .ok_or_else(|| format!("{text:?} is not one of {}", words.join(", "))),
        FieldKind::Link => {
            if text.contains(char::is_whitespace) {
                Err(format!("{text:?} is not a link: it holds a space"))
            } else {
                Ok(FieldValue::Text(text.into()))
            }
        }
    }
}

fn finite(number: f64) -> Result<FieldValue, String> {
    if number.is_finite() {
        Ok(FieldValue::Number(number))
    } else {
        Err(format!("{number} is not a finite number"))
    }
}

/// Paths whose change invalidates a plugin's caches.
pub trait Watch: Send + Sync {
    /// Folders or files under a checkout, relative to it.
    fn paths(&self) -> Vec<String>;
    /// Called after a change below one of `paths` in the checkout at `root`, and when the server
    /// first serves it: drop what is stale and warm it again. Returns a line for the server's log.
    fn changed(&self, root: &Path) -> Option<String>;
    /// Called when the server stops serving the checkout at `root`: drop what is kept for it.
    fn forget(&self, _root: &Path) {}
}

/// A check over the project's documents and data, run by the project's docs build
/// (`symphony_docs::check`) and by `symphony-status --lint`, over one reading of its `docs/`.
pub use symphony_docs::{DocCheck, Docs, Problem};

/// A rule for one document's prose.
pub trait ProseRule: Send + Sync {
    fn name(&self) -> &str;
    /// Each problem in the document `rel` (project-relative) whose text is `text`: its 1-based
    /// line and what is wrong there.
    fn check(&self, rel: &str, text: &str) -> Vec<(usize, String)>;
}

/// A command that regenerates files after the owner's edits are committed.
pub trait Generator: Send + Sync {
    fn name(&self) -> &str;
    /// The path it writes, project-relative.
    fn outputs(&self) -> &str;
    /// The command, its program first, run at the checkout's root.
    fn command(&self) -> Vec<String>;
    /// Whether it runs only after an earlier generator changed something.
    fn when_changed(&self) -> bool {
        false
    }
}

/// A way to reach an agent: register its address, deliver a message to it, probe whether it lives.
pub trait Transport: Send + Sync {
    /// The transport's name (`claude-uds`).
    fn name(&self) -> &str;
    /// The address an agent of `handle` registers with, from what the agent's session reports.
    fn register(&self, handle: &str, reported: &str) -> Result<String, String>;
    fn deliver(&self, address: &str, message: &str) -> Result<(), String>;
    fn alive(&self, address: &str) -> bool;
}

/// A project's look: a package of tokens, stylesheets, an icon and layout variants.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// The package's name, as `[theme] package` names it.
    pub package: String,
    /// The name shown in the header and the tab.
    pub name: String,
    pub icon: Option<String>,
    /// Token names (`primary`, `background`, `foreground`, `font-sans`, …) and their CSS values
    /// (docs/engineering/PLUGIN_WRITING.md lists them).
    pub tokens: BTreeMap<String, String>,
    /// Stylesheets appended after the tokens, project-relative.
    pub stylesheets: Vec<String>,
    /// The named layout variant chosen for each region the client offers variants of.
    pub layouts: BTreeMap<String, String>,
}
