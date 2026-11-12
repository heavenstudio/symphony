//! The plugin protocol's data: each seam's call as a request and the type of its answer, both serde
//! types, and what a plugin declares once ([`Declaration`]). A request names the seam's object (a
//! component, a card kind, a route…) by its name, a watch by its place among the plugin's watches,
//! and carries owned forms of what the call borrows ([`PageData`], [`CardData`], [`TaskData`],
//! [`FileData`], [`FieldData`]). On the wire a request is a JSON object whose `call` names its seam
//! and call (`card_kinds.slot`, [`Ask::TAG`]) and whose `args` holds its fields, the call first; its
//! answer is the value the seam's method returns, as serde writes it (a `Result` as `{"Ok": …}` or
//! `{"Err": …}`, `()` as `null`).
//!
//! Nothing on the wire is read through serde's buffered content (an untagged or internally tagged
//! enum), which the server's JSON cannot read a fractional number back from: the server's
//! `serde_json` keeps numbers exact (`arbitrary_precision`), and hands such a number to a buffer as a
//! map. So a request is tagged beside its fields, not among them, and a board field's value goes as
//! [`FieldData`], tagged, not as the board's untagged [`FieldValue`].
//!
//! What a linked call shares with the server and no request carries: the values a page's components
//! keep for one render ([`Page::once`]), and the reading of `docs/` a doc check is handed
//! ([`Docs`]). A request names them by what identifies them, the page's root and document and the
//! project's root, and the plugin keeps its own for the render or the check. A command's streams
//! pass through as they are: its request is its arguments, its answer its exit code.
//!
//! [`Page::once`]: crate::Page::once
//! [`Docs`]: crate::Docs

use std::fmt::Debug;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::vocabulary::{Block, FieldValue, Gate, NavItem, Picture, Region, TaskOffer};
use crate::{
    Card, Drawing, FieldKind, File, Page, Plugin, Problem, Reply, Submission, Submitted, TaskRef,
    Theme, ThreadLine,
};

/// A request, and what answers it.
pub trait Ask: Into<Request> + Sized {
    /// The request's `call` on the wire: its seam, as [`Plugin`]'s method names it, a dot, and the
    /// call, as the seam's method names it (`card_kinds.slot`).
    const TAG: &'static str;
    /// What the seam's method returns, owned.
    type Answer: Serialize + DeserializeOwned + PartialEq + Debug;

    /// The request this is, out of `request`; `None` when `request` is another call's.
    fn from_request(request: Request) -> Option<Self>;

    /// The seam and the call [`Ask::TAG`] names.
    fn names() -> (&'static str, &'static str) {
        Self::TAG.split_once('.').unwrap_or((Self::TAG, ""))
    }
}

/// Each call: its request's fields and what answers it, then the one [`Request`] they all go on
/// the wire as.
macro_rules! calls {
    ($(
        $(#[$doc:meta])*
        $name:ident = $tag:literal => $answer:ty {
            $( $(#[$field_doc:meta])* $field:ident : $type:ty ),* $(,)?
        }
    )*) => {
        /// Every call a plugin answers, as it goes on the wire: `call` names its seam and call,
        /// `args` holds its fields.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "call", content = "args")]
        pub enum Request {
            $(
                #[serde(rename = $tag)]
                $name($name),
            )*
        }

        $(
            $(#[$doc])*
            #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
            pub struct $name {
                $( $(#[$field_doc])* pub $field: $type, )*
            }

            impl From<$name> for Request {
                fn from(request: $name) -> Request {
                    Request::$name(request)
                }
            }

            impl Ask for $name {
                const TAG: &'static str = $tag;
                type Answer = $answer;

                fn from_request(request: Request) -> Option<Self> {
                    if let Request::$name(request) = request {
                        Some(request)
                    } else {
                        None
                    }
                }
            }
        )*
    };
}

calls! {
    /// What a plugin declares: asked once, before any other call.
    Declare = "plugin.declare" => Declaration {}

    /// [`Component::render`](crate::Component::render).
    Render = "components.render" => Result<Vec<Block>, String> {
        component: String,
        page: PageData,
        argument: String,
        option: Option<String>,
    }
    /// [`Component::resolve`](crate::Component::resolve).
    Resolve = "components.resolve" => Result<(), String> {
        component: String,
        page: PageData,
        argument: String,
        option: Option<String>,
    }
    /// [`Component::cache_version`](crate::Component::cache_version).
    CacheVersion = "components.cache_version" => Option<String> {
        component: String,
        page: PageData,
    }

    /// [`CardKind::recognises`](crate::CardKind::recognises).
    Recognises = "card_kinds.recognises" => bool { kind: String, card: CardData }
    /// [`CardKind::pictures`](crate::CardKind::pictures), the card read in the checkout at `root`.
    Pictures = "card_kinds.pictures" => Vec<Picture> {
        kind: String,
        root: PathBuf,
        card: CardData,
    }
    /// [`CardKind::slot`](crate::CardKind::slot).
    Slot = "card_kinds.slot" => Option<String> { kind: String, card: CardData }
    /// [`CardKind::triage_category`](crate::CardKind::triage_category).
    TriageCategory = "card_kinds.triage_category" => Option<String> {
        kind: String,
        card: CardData,
    }
    /// [`CardKind::auto_approvable`](crate::CardKind::auto_approvable).
    AutoApprovable = "card_kinds.auto_approvable" => bool { kind: String, card: CardData }
    /// [`CardKind::gates`](crate::CardKind::gates).
    CardGates = "card_kinds.gates" => Option<(usize, String)> { kind: String, card: CardData }
    /// [`CardKind::approval_needs_agent`](crate::CardKind::approval_needs_agent).
    ApprovalNeedsAgent = "card_kinds.approval_needs_agent" => bool {
        kind: String,
        card: CardData,
    }
    /// [`CardKind::chosen`](crate::CardKind::chosen).
    Chosen = "card_kinds.chosen" => Result<Option<String>, String> {
        kind: String,
        card: CardData,
    }

    /// [`Renderer::draw`](crate::Renderer::draw).
    Draw = "renderers.draw" => Result<Drawing, String> {
        renderer: String,
        page: PageData,
        file: FileData,
    }

    /// [`Route::serve`](crate::Route::serve).
    Serve = "routes.serve" => Option<Reply> { route: String, root: PathBuf, rest: String }

    /// [`HomeRegion::region`](crate::HomeRegion::region).
    ShowRegion = "home_regions.region" => Option<Region> { region: String, root: PathBuf }

    /// [`BoardAction::offer`](crate::BoardAction::offer).
    Offer = "board_actions.offer" => Result<TaskOffer, String> {
        action: String,
        root: PathBuf,
        task: TaskData,
    }
    /// [`BoardAction::submit`](crate::BoardAction::submit).
    Submit = "board_actions.submit" => Result<Submitted, String> {
        action: String,
        root: PathBuf,
        task: TaskData,
        submission: Submission,
    }

    /// [`BoardField::validate`](crate::BoardField::validate).
    Validate = "board_fields.validate" => Result<FieldData, String> {
        field: String,
        value: FieldData,
    }

    /// [`Gates::answer`](crate::Gates::answer).
    AnswerGates = "gates.answer" => Result<Vec<Gate>, String> {
        answerer: String,
        root: PathBuf,
        task: TaskData,
        tip: Option<String>,
    }
    /// [`Gates::kept_while`](crate::Gates::kept_while).
    KeptWhile = "gates.kept_while" => String { answerer: String, root: PathBuf, task: TaskData }

    /// [`Watch::changed`](crate::Watch::changed).
    Changed = "watches.changed" => Option<String> {
        /// The watch's place among the plugin's watches.
        watch: usize,
        root: PathBuf,
    }
    /// [`Watch::forget`](crate::Watch::forget).
    Forget = "watches.forget" => () {
        /// The watch's place among the plugin's watches.
        watch: usize,
        root: PathBuf,
    }

    /// [`DocCheck::check`](crate::DocCheck::check) over the `docs/` of the project at `root`.
    CheckDocs = "doc_checks.check" => Vec<ProblemData> { check: String, root: PathBuf }
    /// [`DocCheck::components`](crate::DocCheck::components) over the `docs/` of the project at
    /// `root`.
    DocComponents = "doc_checks.components" => Vec<String> { check: String, root: PathBuf }

    /// [`ProseRule::check`](crate::ProseRule::check).
    CheckProse = "prose_rules.check" => Vec<(usize, String)> {
        rule: String,
        rel: String,
        text: String,
    }

    /// [`Command::run`](crate::Command::run), its streams passed through as they are.
    RunCommand = "commands.run" => i32 { command: String, args: Vec<String> }

    /// [`Transport::register`](crate::Transport::register).
    RegisterAgent = "transports.register" => Result<String, String> {
        transport: String,
        handle: String,
        reported: String,
    }
    /// [`Transport::deliver`](crate::Transport::deliver).
    Deliver = "transports.deliver" => Result<(), String> {
        transport: String,
        address: String,
        message: String,
    }
    /// [`Transport::alive`](crate::Transport::alive).
    Alive = "transports.alive" => bool { transport: String, address: String }
}

/// A page as a request names it: the project's root and the document rendered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageData {
    pub root: PathBuf,
    pub doc_path: String,
}

impl PageData {
    pub fn of(page: &Page<'_>) -> PageData {
        PageData {
            root: page.root.to_path_buf(),
            doc_path: page.doc_path.to_string(),
        }
    }

    /// Whether this names `page`: its root and its document.
    pub fn names(&self, page: &Page<'_>) -> bool {
        self.root == page.root && self.doc_path == page.doc_path
    }
}

/// A [`Card`], owned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardData {
    pub slug: String,
    pub doc: String,
    pub topic: String,
    pub text: String,
    pub thread: Vec<ThreadLineData>,
}

/// A [`ThreadLine`], owned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadLineData {
    pub agent: bool,
    pub verdict: Option<String>,
    pub text: String,
}

impl CardData {
    pub fn of(card: &Card<'_>) -> CardData {
        CardData {
            slug: card.slug.into(),
            doc: card.doc.into(),
            topic: card.topic.into(),
            text: card.text.into(),
            thread: card
                .thread
                .iter()
                .map(|line| ThreadLineData {
                    agent: line.agent,
                    verdict: line.verdict.map(str::to_string),
                    text: line.text.into(),
                })
                .collect(),
        }
    }

    /// What `read` makes of the card this holds, borrowed as a card kind reads it.
    pub fn read<R>(&self, read: impl FnOnce(&Card<'_>) -> R) -> R {
        let thread: Vec<ThreadLine<'_>> = self
            .thread
            .iter()
            .map(|line| ThreadLine {
                agent: line.agent,
                verdict: line.verdict.as_deref(),
                text: &line.text,
            })
            .collect();
        read(&Card {
            slug: &self.slug,
            doc: &self.doc,
            topic: &self.topic,
            text: &self.text,
            thread: &thread,
        })
    }
}

/// A [`TaskRef`], owned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskData {
    pub slug: String,
    pub status: String,
    pub branch: String,
}

impl TaskData {
    pub fn of(task: &TaskRef<'_>) -> TaskData {
        TaskData {
            slug: task.slug.into(),
            status: task.status.into(),
            branch: task.branch.into(),
        }
    }

    pub fn task(&self) -> TaskRef<'_> {
        TaskRef {
            slug: &self.slug,
            status: &self.status,
            branch: &self.branch,
        }
    }
}

/// A renderer's [`File`], owned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileData {
    pub src: String,
    pub label: String,
}

impl FileData {
    pub fn of(file: &File<'_>) -> FileData {
        FileData {
            src: file.src.into(),
            label: file.label.into(),
        }
    }

    pub fn file(&self) -> File<'_> {
        File {
            src: &self.src,
            label: &self.label,
        }
    }
}

/// A board field's [`FieldValue`], as the wire carries it: tagged by its kind
/// (`{"Number": 3.5}`), since the server's JSON reads no fractional number back into the board's
/// untagged value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FieldData {
    Number(f64),
    Text(String),
}

impl From<FieldValue> for FieldData {
    fn from(value: FieldValue) -> FieldData {
        match value {
            FieldValue::Number(number) => FieldData::Number(number),
            FieldValue::Text(text) => FieldData::Text(text),
        }
    }
}

impl From<FieldData> for FieldValue {
    fn from(value: FieldData) -> FieldValue {
        match value {
            FieldData::Number(number) => FieldValue::Number(number),
            FieldData::Text(text) => FieldValue::Text(text),
        }
    }
}

/// A doc check's [`Problem`], as the wire carries it: `symphony-docs` is std only, so its type
/// takes no serde derive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemData {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl ProblemData {
    pub fn of(problem: &Problem) -> ProblemData {
        ProblemData {
            file: problem.file.clone(),
            line: problem.line,
            message: problem.message.clone(),
        }
    }

    pub fn problem(self) -> Problem {
        Problem {
            file: self.file,
            line: self.line,
            message: self.message,
        }
    }
}

/// What a plugin declares once, before any call: its name, each seam object's name and what it
/// says of itself, its navigation and its theme, each list in the plugin's order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    pub name: String,
    pub components: Vec<String>,
    pub card_kinds: Vec<DeclaredKind>,
    pub renderers: Vec<DeclaredRenderer>,
    /// Each route's path.
    pub routes: Vec<String>,
    pub nav: Vec<NavItem>,
    /// Each home region's id.
    pub home_regions: Vec<String>,
    pub board_actions: Vec<String>,
    pub board_fields: Vec<DeclaredField>,
    /// Each gates answerer's name.
    pub gates: Vec<String>,
    /// Each watch's paths.
    pub watches: Vec<Vec<String>>,
    pub doc_checks: Vec<DeclaredCheck>,
    pub prose_rules: Vec<String>,
    pub generators: Vec<DeclaredGenerator>,
    pub commands: Vec<DeclaredCommand>,
    pub transports: Vec<String>,
    pub theme: Option<Theme>,
}

/// A card kind: its name and the markers it reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredKind {
    pub name: String,
    pub markers: Vec<String>,
}

/// A renderer: its name, its files' extensions and the content type they are served with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredRenderer {
    pub name: String,
    pub extensions: Vec<String>,
    pub content_type: String,
}

/// A board field: its name, its kind and whether a task must hold it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredField {
    pub name: String,
    pub kind: FieldKind,
    pub required: bool,
}

/// A doc check: its name and the files outside `docs/` it reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredCheck {
    pub name: String,
    pub inputs: Vec<String>,
}

/// A generator, as [`Generator`](crate::Generator) says it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredGenerator {
    pub name: String,
    pub outputs: String,
    pub command: Vec<String>,
    pub when_changed: bool,
}

/// A command: its name and its line in `symphony help`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredCommand {
    pub name: String,
    pub summary: String,
}

impl Declaration {
    /// What `plugin` declares.
    pub fn of(plugin: &dyn Plugin) -> Declaration {
        let names = |list: Vec<&str>| list.into_iter().map(str::to_string).collect::<Vec<_>>();
        Declaration {
            name: plugin.name().into(),
            components: names(plugin.components().iter().map(|c| c.name()).collect()),
            card_kinds: plugin
                .card_kinds()
                .iter()
                .map(|kind| DeclaredKind {
                    name: kind.name().into(),
                    markers: names(kind.markers().to_vec()),
                })
                .collect(),
            renderers: plugin
                .renderers()
                .iter()
                .map(|renderer| DeclaredRenderer {
                    name: renderer.name().into(),
                    extensions: names(renderer.extensions().to_vec()),
                    content_type: renderer.content_type().into(),
                })
                .collect(),
            routes: names(plugin.routes().iter().map(|r| r.path()).collect()),
            nav: plugin.nav(),
            home_regions: names(plugin.home_regions().iter().map(|r| r.id()).collect()),
            board_actions: names(plugin.board_actions().iter().map(|a| a.name()).collect()),
            board_fields: plugin
                .board_fields()
                .iter()
                .map(|field| DeclaredField {
                    name: field.name().into(),
                    kind: field.kind(),
                    required: field.required(),
                })
                .collect(),
            gates: names(plugin.gates().iter().map(|g| g.name()).collect()),
            watches: plugin.watches().iter().map(|w| w.paths()).collect(),
            doc_checks: plugin
                .doc_checks()
                .iter()
                .map(|check| DeclaredCheck {
                    name: check.name().into(),
                    inputs: check.inputs(),
                })
                .collect(),
            prose_rules: names(plugin.prose_rules().iter().map(|r| r.name()).collect()),
            generators: plugin
                .generators()
                .iter()
                .map(|generator| DeclaredGenerator {
                    name: generator.name().into(),
                    outputs: generator.outputs().into(),
                    command: generator.command(),
                    when_changed: generator.when_changed(),
                })
                .collect(),
            commands: plugin
                .commands()
                .iter()
                .map(|command| DeclaredCommand {
                    name: command.name().into(),
                    summary: command.summary().into(),
                })
                .collect(),
            transports: names(plugin.transports().iter().map(|t| t.name()).collect()),
            theme: plugin.theme(),
        }
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
