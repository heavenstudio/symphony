//! The plugin protocol's data (docs/engineering/PLUGIN_PROTOCOL.md): each seam's call as a request and the type
//! of its answer, both serde types, what a plugin declares once ([`Declaration`]), and the line
//! each travels in ([`Envelope`]). A request names the seam's object (a component, a card kind, a
//! route…) by its name, a watch by its place among the plugin's watches, and carries owned forms of
//! what the call borrows ([`PageData`], [`CardData`], [`TaskData`], [`FileData`]). A request is a
//! JSON object of one member, its call (its seam and call, `card_kinds.slot`, [`Ask::TAG`]) holding
//! its fields: `{"card_kinds.slot":{"kind":"widget","card":{…}}}`. Its answer is [`Answered`]:
//! `{"answer": …}` holding the value the seam's method returns as serde writes it (a `Result` as
//! `{"Ok": …}` or `{"Err": …}`, `()` as `null`), or `{"refused": "…"}` when the request could not
//! be read.
//!
//! **The line.** Between a host and a plugin's process each line is one [`Envelope`]: the call's
//! `id` and exactly one known member, from the host `request` (a [`Request`]) or `cancel` (the call
//! given up), from the plugin `answered` (an [`Answered`]), beside which a plugin may say its own
//! time for the call (`us`). `{"id":7,"request":{"card_kinds.slot":{…}}}` is answered
//! `{"id":7,"answered":{"answer":"North"},"us":0.4}`. Both members are read as raw JSON first, so
//! a request the plugin cannot read is still answered by its id, and an answer is read as its call's
//! type once its id says which call it answers. A member the reader does not know is ignored, at
//! the envelope's level as everywhere: the protocol's room for what a later minor adds around a
//! call (a deadline, a trace).
//!
//! **Each call's minor.** A call names the minor it came in ([`Ask::SINCE`], [`MINORS`]). A host
//! never sends a call newer than the minor it agreed with the plugin and answers it itself, with the
//! seam's own default, so a plugin built before a method was added shows the default (none, false,
//! nothing) where it would have answered, not an error. A method a seam requires has no default and
//! is in every minor of its major: a new one is a new major.
//!
//! **Loopback only.** `commands.run` is a call of the in-process loopback alone ([`Ask::PIPED`],
//! [`LOOPBACK_ONLY`]): a host never sends it on a pipe, as a plugin's command runs as the plugin's
//! binary, `PROGRAM command NAME ARGS`, with its own streams ([`crate::serve`]).
//!
//! **Batches.** `card_kinds.read` is no seam's method ([`BATCHES`]): a host asks a plugin's process
//! every card of a register read in one request ([`ReadCards`]), each card's answer every kind's
//! calls ([`KindAnswers`]), so a read costs a round trip and not eight for each card.
//!
//! **Any order.** Every object on the wire reads back whatever order its members come in, as JSON
//! objects are unordered and a writer that sorts keys is free to: the request, its fields, the
//! answer and every structure in it. The server's `serde_json` keeps numbers exact
//! (`arbitrary_precision`), and so cannot read a fractional number back through serde's buffered
//! content, which holds such a number as a map: a member read before the one that tells its type is
//! buffered so. So every enum the protocol defines is tagged outside its content (one member, as
//! the request and its answer are), and none uses `#[serde(flatten)]`, which buffers too. A board
//! field's value is the board's own [`FieldValue`], a bare number or text, whose reader takes a
//! number however serde hands it, the buffer's map of its text included. Three types of the block
//! vocabulary are internally tagged and so are read through that buffer: [`Block`] and [`Span`]
//! (tagged by `type`) and [`Media`] (by `kind`). They read back in any order because they hold no
//! fractional number, only integers, which the buffer keeps: each derives `Eq`, which no field of a
//! float can, and the protocol's tests hold both.
//!
//! **Versions and unknowns.** [`PROTOCOL`] is this build's version. The host names its own in
//! [`Declare`], the first request, and the plugin its own in its [`Declaration`]; they speak the
//! lower minor of one major ([`Version::agreed`]), and a host refuses a plugin of another major by
//! name, saying which side to update. Within a major a change is additive:
//! - a field the reader does not know is ignored, wherever it stands (a request's, a card's, an
//!   answer's), so a newer peer's additions reach an older one unread;
//! - a field missing is refused, unless the type gives it a default (a gate's `by`, `at`, a viewer's
//!   `views`): a new field comes with a default, or a new major;
//! - a call the plugin does not know, or a request it cannot read (a field missing, a second member
//!   beside the call), is answered `{"refused": …}`, naming what it could not read, and the host
//!   shows that in the call's place;
//! - an answer the host cannot read (a block type it does not know, a variant newer than the minor
//!   agreed) is refused whole, and the host shows the error in the call's place: a component's in
//!   its place in the page.
//!
//! So every struct here is `#[non_exhaustive]`: a field a minor adds is no new major of this crate,
//! as nothing outside it builds one by its fields. A request is made by its `new` (which takes every
//! field its call has: a field a later minor adds stays out of it, as `calls!` says), a page, a
//! card, a task, a file and a problem by their `of` from what the seam borrows, a kind's answers
//! of a card ([`KindAnswers`]) by its `of` from the kind, a [`Declaration`]
//! and what it lists by its `of` from a plugin (or from `Default`, its fields set after), a
//! [`Version`] by its `new`, and a line by [`Envelope::asking`], [`Envelope::cancelling`] or
//! [`Envelope::answering`]. [`Request`] is `#[non_exhaustive]` as well, as a minor adds a call. The
//! other enums here stay exhaustive, so their readers match on every variant: a variant of
//! [`Answered`] or [`Member`] is a line an older peer cannot read, a new major of the protocol, and
//! [`Lined`] is how a line reads, never on the wire.
//!
//! ```compile_fail,E0639
//! // Outside this crate a request is made by its `new`, never by its fields.
//! let _ = symphony_plugin::protocol::Changed { watch: 0, root: "/r".into() };
//! ```
//!
//! A request names no plugin: one process speaks for one plugin, and a project whose `[plugins]
//! crates` lists several has a process for each.
//!
//! What a linked call shares with the server and no request carries: the values a page's components
//! keep for one render ([`Page::once`]), and the reading of `docs/` a doc check is handed
//! ([`Docs`]). A request names them by what identifies them: a page by its root, its document and
//! its render ([`PageData::render`]), the requests of one render answered on one page of the
//! plugin's, so its components share their `once` values there too; a check by the project's root,
//! whose `docs/` the plugin reads itself. Under the loopback a command's streams pass through as
//! they are: its request is its arguments, its answer its exit code.
//!
//! [`Page::once`]: crate::Page::once
//! [`Docs`]: crate::Docs
//! [`Span`]: crate::vocabulary::Span
//! [`Media`]: crate::vocabulary::Media

use std::fmt::Debug;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::authority::{Operation, Signed};
use crate::vocabulary::{Block, FieldValue, Gate, NavItem, Picture, Region, TaskOffer};
use crate::{
    ARTIFACTS, Card, CardKind, Drawing, FieldKind, File, Page, Plugin, Problem, Reply, Submission,
    Submitted, TaskRef, Theme, ThreadLine,
};

/// The protocol's version: a new major breaks what the one before wrote, a new minor only adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

/// The protocol this build speaks: major 0 while no process speaks it. Minor 2 hands a page its
/// project's artifacts folder ([`PageData::artifacts`]); minor 3 asks a register read's card kinds
/// in one request ([`ReadCards`]); minor 4 asks an approval authority for an attestation
/// ([`Attest`], [`Declaration::authorities`]); every other call is 1's.
pub const PROTOCOL: Version = Version { major: 0, minor: 4 };

/// This build's protocol.
impl Default for Version {
    fn default() -> Version {
        PROTOCOL
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl Version {
    /// The version `major.minor`.
    pub const fn new(major: u32, minor: u32) -> Version {
        Version { major, minor }
    }

    /// The version a host speaking `self` speaks with a plugin that declared `plugin`: the lower
    /// minor of their one major, so a newer plugin speaks down to the host's minor and an older one
    /// is answered at its own. Another major is refused, naming both and the side to update.
    pub fn agreed(self, plugin: Version) -> Result<Version, String> {
        if self.major == plugin.major {
            return Ok(Version {
                major: self.major,
                minor: self.minor.min(plugin.minor),
            });
        }
        let update = if plugin.major > self.major {
            format!("install a Symphony that speaks protocol {}", plugin.major)
        } else {
            format!(
                "rebuild the plugin with a symphony-plugin of protocol {}",
                self.major
            )
        };
        Err(format!(
            "the plugin speaks protocol {plugin} and this Symphony {self}: {update}"
        ))
    }
}

/// What answers a request on the wire: the seam's answer, or why the request was refused (a call the
/// plugin does not know, a field missing), for the host to show in the call's place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Answered<T> {
    #[serde(rename = "answer")]
    Answer(T),
    #[serde(rename = "refused")]
    Refused(String),
}

impl<T> Answered<T> {
    /// The answer, or the refusal as the error the host shows.
    pub fn shown(self) -> Result<T, String> {
        match self {
            Answered::Answer(answer) => Ok(answer),
            Answered::Refused(why) => Err(why),
        }
    }
}

/// One line between a host and a plugin's process: the call's `id` and exactly one known member,
/// from the host [`request`](Envelope::request) or [`cancel`](Envelope::cancel), from the plugin
/// [`answered`](Envelope::answered). Read, `request` and `answered` are raw JSON
/// ([`Raw`], the default types), so a request is answered by its id whether or not it reads, and an
/// answer is read as the type of the call its id names. A struct reads its members in any order
/// with nothing buffered, and a member it does not know is ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Envelope<Q = Box<RawValue>, A = Box<RawValue>> {
    /// The call's id, the host's to choose and unique among the calls it has not had answered: the
    /// answer and a cancel name the call by it.
    pub id: u64,
    /// A [`Request`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Q>,
    /// The host gave the call up: the plugin may stop it, and its answer, if one comes, is dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<Cancel>,
    /// An [`Answered`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<A>,
    /// Beside an answer, the plugin's own time for the call, in microseconds: what the host records
    /// as the call's own time under a process (docs/engineering/PLUGIN_CALLS.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub us: Option<f64>,
}

/// A line as it is read: its members raw.
pub type Raw = Envelope<Box<RawValue>, Box<RawValue>>;

/// The `cancel` member: an empty object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Cancel {}

/// The one known member a line holds.
#[derive(Debug, PartialEq)]
pub enum Member<Q, A> {
    Request(Q),
    Cancel,
    Answered(A),
}

impl<Q, A> Envelope<Q, A> {
    /// The host's line asking `request` as the call `id`.
    pub fn asking(id: u64, request: Q) -> Envelope<Q, A> {
        Envelope {
            id,
            request: Some(request),
            cancel: None,
            answered: None,
            us: None,
        }
    }

    /// The host's line giving the call `id` up.
    pub fn cancelling(id: u64) -> Envelope<Q, A> {
        Envelope {
            id,
            request: None,
            cancel: Some(Cancel {}),
            answered: None,
            us: None,
        }
    }

    /// The plugin's line answering the call `id` with `answered`, its own time `us` beside it.
    pub fn answering(id: u64, answered: A, us: Option<f64>) -> Envelope<Q, A> {
        Envelope {
            id,
            request: None,
            cancel: None,
            answered: Some(answered),
            us,
        }
    }

    /// The one known member the line holds, or why it holds none or more than one.
    pub fn member(self) -> Result<Member<Q, A>, String> {
        match (self.request, self.cancel, self.answered) {
            (Some(request), None, None) => Ok(Member::Request(request)),
            (None, Some(_), None) => Ok(Member::Cancel),
            (None, None, Some(answered)) => Ok(Member::Answered(answered)),
            (None, None, None) => Err(format!(
                "the line of call {} holds none of request, cancel and answered",
                self.id
            )),
            (request, cancel, answered) => {
                let held: Vec<&str> = [
                    request.is_some().then_some("request"),
                    cancel.is_some().then_some("cancel"),
                    answered.is_some().then_some("answered"),
                ]
                .into_iter()
                .flatten()
                .collect();
                Err(format!(
                    "the line of call {} holds {}: a line holds one",
                    self.id,
                    held.join(" and ")
                ))
            }
        }
    }
}

/// The longest line either side writes or reads, its newline aside: 16 MiB. A reader that meets a
/// longer one ends the process it speaks with, naming the bound; a writer sends none, refusing the
/// call instead.
pub const LINE_MAX: usize = 16 << 20;

/// A line as [`read_line`] reads it.
#[derive(Debug, PartialEq, Eq)]
pub enum Lined {
    /// A whole line, its newline left out: bytes, whatever they hold.
    Line(Vec<u8>),
    /// A line longer than the bound: its first bytes, as many as the bound, the rest left unread.
    TooLong(Vec<u8>),
    /// The input ended.
    End,
}

/// The next line of `input`, read as bytes and never more than `max` of them: a line that is not
/// UTF-8 is a line all the same, for its reader to skip, and the last line may lack its newline.
///
/// # Errors
///
/// When `input` fails, as [`BufRead::fill_buf`] does: an interrupted read is tried again.
pub fn read_line(input: &mut impl std::io::BufRead, max: usize) -> std::io::Result<Lined> {
    let mut line = Vec::new();
    loop {
        let available = match input.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok(if line.is_empty() {
                Lined::End
            } else {
                Lined::Line(line)
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let taken = newline.unwrap_or(available.len());
        if line.len() + taken > max {
            let room = max - line.len();
            line.extend_from_slice(&available[..room]);
            input.consume(room);
            return Ok(Lined::TooLong(line));
        }
        line.extend_from_slice(&available[..taken]);
        input.consume(taken + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(Lined::Line(line));
        }
    }
}

/// A request, and what answers it.
pub trait Ask: Into<Request> + Sized {
    /// The request's call on the wire: its seam, as [`Plugin`]'s method names it, a dot, and the
    /// call, as the seam's method names it (`card_kinds.slot`).
    const TAG: &'static str;
    /// The minor of the protocol the call came in: a host sends it only to a plugin it agreed that
    /// minor or a later one with, and answers it with the seam's default otherwise.
    const SINCE: u32;
    /// Whether the call goes on a pipe to a plugin's process; `commands.run` does not
    /// ([`LOOPBACK_ONLY`]).
    const PIPED: bool;
    /// What the seam's method returns, owned.
    type Answer: Serialize + DeserializeOwned + PartialEq + Debug;

    /// The request this is, out of `request`; `None` when `request` is another call's.
    fn from_request(request: Request) -> Option<Self>;

    /// The seam and the call [`Ask::TAG`] names.
    fn names() -> (&'static str, &'static str) {
        Self::TAG.split_once('.').unwrap_or((Self::TAG, ""))
    }
}

/// Whether a call marked as `calls!` marks it goes on a pipe: unmarked it does, `loopback` it does
/// not.
macro_rules! piped {
    () => {
        true
    };
    (loopback) => {
        false
    };
}

/// The tags of the calls `calls!` marks `loopback`, gathered one call at a time into the list.
macro_rules! loopback_only {
    ([$($found:literal),*]) => {
        &[$($found),*]
    };
    ([$($found:literal),*] ($tag:literal loopback) $($rest:tt)*) => {
        loopback_only!([$($found,)* $tag] $($rest)*)
    };
    ([$($found:literal),*] ($tag:literal) $($rest:tt)*) => {
        loopback_only!([$($found),*] $($rest)*)
    };
}

/// Each call: the minor it came in, its request's fields and what answers it, then the one
/// [`Request`] they all go on the wire as. [`Request`] is `#[non_exhaustive]`, so a call a later
/// minor adds is a new variant and no new major of this crate. A request is `#[non_exhaustive]` too,
/// made outside this crate by its `new`, which takes every field in the call's braces: a field
/// added there changes `new`'s arguments, a new major of this crate. No call has a field a later
/// minor added yet. The first such field gives this macro a group of its own after the call's
/// fields, each `#[serde(default)]` and left out of `new`, which fills it with its default, so the
/// field is a minor of this crate as of the protocol, set on the request after `new`.
macro_rules! calls {
    ($(
        $(#[$doc:meta])*
        $name:ident = $tag:literal since $since:literal $(, $only:ident only)? => $answer:ty {
            $( $(#[$field_doc:meta])* $field:ident : $type:ty ),* $(,)?
        }
    )*) => {
        /// Every call a plugin answers, as it goes on the wire: one member, named by its seam and
        /// call, holding its fields. A later minor adds a call, so a match on it outside this crate
        /// ends with `_`.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[non_exhaustive]
        pub enum Request {
            $(
                #[serde(rename = $tag)]
                $name($name),
            )*
        }

        /// Every call's [`Ask::TAG`], in the protocol's order.
        pub const CALLS: &[&str] = &[$($tag),*];

        /// Every call's [`Ask::TAG`] and the minor it came in ([`Ask::SINCE`]), in the protocol's
        /// order.
        pub const MINORS: &[(&str, u32)] = &[$(($tag, $since)),*];

        /// The calls of the in-process loopback alone, never sent on a pipe ([`Ask::PIPED`]).
        pub const LOOPBACK_ONLY: &[&str] = loopback_only!([] $(($tag $($only)?))*);

        $(
            $(#[$doc])*
            #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
            #[non_exhaustive]
            pub struct $name {
                $( $(#[$field_doc])* pub $field: $type, )*
            }

            impl $name {
                /// The request of these fields, every field its call has.
                pub fn new($($field: $type),*) -> $name {
                    $name { $($field),* }
                }
            }

            impl From<$name> for Request {
                fn from(request: $name) -> Request {
                    Request::$name(request)
                }
            }

            impl Ask for $name {
                const TAG: &'static str = $tag;
                const SINCE: u32 = $since;
                const PIPED: bool = piped!($($only)?);
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

// A field added to a call's braces below changes its `new`, a new major of this crate: a field a
// later minor adds goes in a group `new` leaves out, which `calls!`'s doc describes.
calls! {
    /// What a plugin declares: asked once, before any other call, naming the host's protocol.
    Declare = "plugin.declare" since 1 => Declaration { protocol: Version }

    /// [`Component::render`](crate::Component::render).
    Render = "components.render" since 1 => Result<Vec<Block>, String> {
        component: String,
        page: PageData,
        argument: String,
        option: Option<String>,
    }
    /// [`Component::resolve`](crate::Component::resolve).
    Resolve = "components.resolve" since 1 => Result<(), String> {
        component: String,
        page: PageData,
        argument: String,
        option: Option<String>,
    }
    /// [`Component::cache_version`](crate::Component::cache_version).
    CacheVersion = "components.cache_version" since 1 => Option<String> {
        component: String,
        page: PageData,
    }

    /// [`CardKind::recognises`](crate::CardKind::recognises).
    Recognises = "card_kinds.recognises" since 1 => bool { kind: String, card: CardData }
    /// [`CardKind::pictures`](crate::CardKind::pictures), the card read in the checkout at `root`.
    Pictures = "card_kinds.pictures" since 1 => Vec<Picture> {
        kind: String,
        root: PathBuf,
        card: CardData,
    }
    /// [`CardKind::slot`](crate::CardKind::slot).
    Slot = "card_kinds.slot" since 1 => Option<String> { kind: String, card: CardData }
    /// [`CardKind::triage_category`](crate::CardKind::triage_category).
    TriageCategory = "card_kinds.triage_category" since 1 => Option<String> {
        kind: String,
        card: CardData,
    }
    /// [`CardKind::auto_approvable`](crate::CardKind::auto_approvable).
    AutoApprovable = "card_kinds.auto_approvable" since 1 => bool { kind: String, card: CardData }
    /// [`CardKind::gates`](crate::CardKind::gates).
    CardGates = "card_kinds.gates" since 1 => Option<(usize, String)> { kind: String, card: CardData }
    /// [`CardKind::approval_needs_agent`](crate::CardKind::approval_needs_agent).
    ApprovalNeedsAgent = "card_kinds.approval_needs_agent" since 1 => bool {
        kind: String,
        card: CardData,
    }
    /// [`CardKind::chosen`](crate::CardKind::chosen).
    Chosen = "card_kinds.chosen" since 1 => Result<Option<String>, String> {
        kind: String,
        card: CardData,
    }
    /// Every card kind's calls of each of `cards`, one register read's, in one request: for each
    /// card in the order sent, what each kind of the plugin says of it ([`KindAnswers`]), in the
    /// plugin's order, its pictures read in the checkout at `root`. A host sends a read's cards in
    /// as many of these as keep each line within [`LINE_MAX`], and to a plugin of 0.2 or older
    /// each card kind's calls of each card instead.
    ReadCards = "card_kinds.read" since 3 => Vec<Vec<KindAnswers>> {
        root: PathBuf,
        cards: Vec<CardData>,
    }

    /// [`Renderer::draw`](crate::Renderer::draw).
    Draw = "renderers.draw" since 1 => Result<Drawing, String> {
        renderer: String,
        page: PageData,
        file: FileData,
    }

    /// [`Route::serve`](crate::Route::serve).
    Serve = "routes.serve" since 1 => Option<Reply> { route: String, root: PathBuf, rest: String }

    /// [`HomeRegion::region`](crate::HomeRegion::region).
    ShowRegion = "home_regions.region" since 1 => Option<Region> { region: String, root: PathBuf }

    /// [`BoardAction::offer`](crate::BoardAction::offer).
    Offer = "board_actions.offer" since 1 => Result<TaskOffer, String> {
        action: String,
        root: PathBuf,
        task: TaskData,
    }
    /// [`BoardAction::submit`](crate::BoardAction::submit).
    Submit = "board_actions.submit" since 1 => Result<Submitted, String> {
        action: String,
        root: PathBuf,
        task: TaskData,
        submission: Submission,
    }

    /// [`BoardField::validate`](crate::BoardField::validate).
    Validate = "board_fields.validate" since 1 => Result<FieldValue, String> {
        field: String,
        value: FieldValue,
    }

    /// [`Gates::answer`](crate::Gates::answer).
    AnswerGates = "gates.answer" since 1 => Result<Vec<Gate>, String> {
        answerer: String,
        root: PathBuf,
        task: TaskData,
        tip: Option<String>,
    }
    /// [`Gates::kept_while`](crate::Gates::kept_while).
    KeptWhile = "gates.kept_while" since 1 => String { answerer: String, root: PathBuf, task: TaskData }

    /// [`Authority::attest`](crate::Authority::attest) of the authority answering for the adapter
    /// `authority`, within `within_ms` milliseconds.
    Attest = "authorities.attest" since 4 => Result<Signed, String> {
        authority: String,
        root: PathBuf,
        operation: Operation,
        within_ms: u64,
    }

    /// [`Watch::changed`](crate::Watch::changed).
    Changed = "watches.changed" since 1 => Option<String> {
        /// The watch's place among the plugin's watches.
        watch: usize,
        root: PathBuf,
    }
    /// [`Watch::forget`](crate::Watch::forget).
    Forget = "watches.forget" since 1 => () {
        /// The watch's place among the plugin's watches.
        watch: usize,
        root: PathBuf,
    }

    /// [`DocCheck::check`](crate::DocCheck::check) over the `docs/` of the project at `root`.
    CheckDocs = "doc_checks.check" since 1 => Vec<ProblemData> { check: String, root: PathBuf }
    /// [`DocCheck::components`](crate::DocCheck::components) over the `docs/` of the project at
    /// `root`.
    DocComponents = "doc_checks.components" since 1 => Vec<String> { check: String, root: PathBuf }

    /// [`ProseRule::check`](crate::ProseRule::check).
    CheckProse = "prose_rules.check" since 1 => Vec<(usize, String)> {
        rule: String,
        rel: String,
        text: String,
    }

    /// [`Command::run`](crate::Command::run), its streams passed through as they are: the loopback's
    /// alone, as a plugin's process runs its command as its binary, `PROGRAM command NAME ARGS`.
    RunCommand = "commands.run" since 1, loopback only => i32 { command: String, args: Vec<String> }

    /// [`Transport::register`](crate::Transport::register).
    RegisterAgent = "transports.register" since 1 => Result<String, String> {
        transport: String,
        handle: String,
        reported: String,
    }
    /// [`Transport::deliver`](crate::Transport::deliver).
    Deliver = "transports.deliver" since 1 => Result<(), String> {
        transport: String,
        address: String,
        message: String,
    }
    /// [`Transport::alive`](crate::Transport::alive).
    Alive = "transports.alive" since 1 => bool { transport: String, address: String }
}

/// The calls that carry several of their seam's calls at once, a method of none: a register read's
/// card kinds ([`ReadCards`]). Only a host speaking to a plugin's process sends one.
pub const BATCHES: &[&str] = &[ReadCards::TAG];

/// A page as a request names it: the project's root, the document rendered, the render it is part
/// of, and the project's artifacts folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PageData {
    pub root: PathBuf,
    pub doc_path: String,
    /// The render: the same for every request one [`Page`] makes, so a plugin's process answers
    /// them on one page of its own and its components share their `once` values as they do
    /// linked. 0, or none written, is a page of its own.
    #[serde(default)]
    pub render: u64,
    /// The project's artifacts folder, relative to its root ([`Page::artifacts`]): since 0.2, and
    /// [`ARTIFACTS`] when none is written, as a host of 0.1 writes none.
    #[serde(default = "artifacts")]
    pub artifacts: String,
}

/// [`PageData::artifacts`] when none is written.
fn artifacts() -> String {
    ARTIFACTS.into()
}

impl PageData {
    pub fn of(page: &Page<'_>) -> PageData {
        PageData {
            root: page.root.to_path_buf(),
            doc_path: page.doc_path.to_string(),
            render: page.render,
            artifacts: page.artifacts.to_string(),
        }
    }

    /// Whether this names `page`: its root, its document and its project's artifacts folder.
    pub fn names(&self, page: &Page<'_>) -> bool {
        self.root == page.root && self.doc_path == page.doc_path && self.artifacts == page.artifacts
    }
}

/// A [`Card`], owned.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CardData {
    pub slug: String,
    pub doc: String,
    pub topic: String,
    pub text: String,
    pub thread: Vec<ThreadLineData>,
}

/// A [`ThreadLine`], owned.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
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

/// What one card kind says of one card, each of its calls' answer: a register read's answer for
/// that card and kind ([`ReadCards`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KindAnswers {
    /// The kind's name.
    pub kind: String,
    pub recognises: bool,
    pub pictures: Vec<Picture>,
    pub slot: Option<String>,
    pub triage_category: Option<String>,
    pub auto_approvable: bool,
    pub gates: Option<(usize, String)>,
    pub approval_needs_agent: bool,
    pub chosen: Result<Option<String>, String>,
}

impl KindAnswers {
    /// What `kind` says of `card`, its pictures read in the checkout at `root`.
    pub fn of(kind: &dyn CardKind, root: &Path, card: &Card<'_>) -> KindAnswers {
        KindAnswers {
            kind: kind.name().into(),
            recognises: kind.recognises(card),
            pictures: kind.pictures(root, card),
            slot: kind.slot(card),
            triage_category: kind.triage_category(card),
            auto_approvable: kind.auto_approvable(card),
            gates: kind.gates(card),
            approval_needs_agent: kind.approval_needs_agent(card),
            chosen: kind.chosen(card),
        }
    }
}

/// A [`TaskRef`], owned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
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

/// A doc check's [`Problem`], as the wire carries it: `symphony-docs` is std only, so its type
/// takes no serde derive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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

/// What a plugin declares once, before any call: the protocol it speaks, its name, each seam
/// object's name and what it says of itself, its navigation and its theme, each list in the
/// plugin's order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Declaration {
    pub protocol: Version,
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
    /// Each approval authority's adapter: since 0.4, and none when none is written, as a plugin
    /// of 0.3 writes none.
    #[serde(default)]
    pub authorities: Vec<String>,
}

/// A card kind: its name and the markers it reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeclaredKind {
    pub name: String,
    pub markers: Vec<String>,
}

/// A renderer: its name, its files' extensions and the content type they are served with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeclaredRenderer {
    pub name: String,
    pub extensions: Vec<String>,
    pub content_type: String,
}

/// A board field: its name, its kind and whether a task must hold it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeclaredField {
    pub name: String,
    pub kind: FieldKind,
    pub required: bool,
}

/// A doc check: its name and the files outside `docs/` it reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeclaredCheck {
    pub name: String,
    pub inputs: Vec<String>,
}

/// A generator, as [`Generator`](crate::Generator) says it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeclaredGenerator {
    pub name: String,
    pub outputs: String,
    pub command: Vec<String>,
    pub when_changed: bool,
}

/// A command: its name and its line in `symphony help`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeclaredCommand {
    pub name: String,
    pub summary: String,
}

impl Declaration {
    /// What `plugin` declares.
    pub fn of(plugin: &dyn Plugin) -> Declaration {
        let names = |list: Vec<&str>| list.into_iter().map(str::to_string).collect::<Vec<_>>();
        Declaration {
            protocol: PROTOCOL,
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
            authorities: names(plugin.authorities().iter().map(|a| a.adapter()).collect()),
        }
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
pub(crate) mod tests;
