//! The Symphony's wire types: the JSON the server (`apps/symphony-server`) sends and the client
//! (`apps/symphony-ui`) reads. Serde only, so one definition builds for the server and for wasm32.
//!
//! The board view, the front page `/`:
//!
//! - `GET` [`BOARD_PATH`] answers a [`Board`]: the next card and the four after it, the counts and
//!   the progress, at one [`Board::version`] of the feed.
//! - `GET` [`FEED_PATH`]`?since=VERSION` is a stream of Server-Sent Events: one [`FEED_EVENT`] event per
//!   change after `VERSION`, its `id` the new version and its `data` a [`Delta`] naming each region
//!   that moved with its new data. A client applies it in place and never fetches the page or the
//!   board again while the stream holds. Between changes the server sends a [`PING_EVENT`] event
//!   (its data empty: the line `data:`) at least every [`Board::ping_ms`], never more than
//!   [`PING_MAX_MS`] apart: an SSE comment never reaches a page's code, so a stream whose server
//!   went quiet with its socket open would otherwise look live for ever.
//!
//! On one stream, versions only go up. A server that restarts counts again from its own start,
//! so a version is never compared across streams, and nothing resumes by `Last-Event-ID`. A
//! stream is lost to the client when it fails (an error, a non-200 answer, a server gone), when
//! it says nothing for two and a half [`Board::ping_ms`], or when it sends an event the client
//! cannot read (that event's change would be lost while the page looked live). The client then
//! closes it, shows its board as stale, and after a wait (1 s, doubling to 30 s until a stream
//! holds 30 s) fetches [`BOARD_PATH`] again. That board replaces the one shown whatever its
//! version (the reset), and a new stream opens `?since=` it; the board is fresh again once that
//! stream is open. A board, or a `client` change, naming another [`Board::build`] than the
//! client's own loads the page again, once per build.
//!
//! A field added later is `#[serde(default)]`, and a region a client does not know yet reads as
//! [`Change::Other`], which it ignores: the server and an open client may be one build apart.
//!
//! The server's own route, [`FRONT_PATH`], answers a [`Front`]: the one server in front of the app,
//! as its starters and the agents see it.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

mod agent_probes;
mod agents;
mod blocks;
pub mod gates;
mod machine_work;
mod media;
mod project;
mod runs;
mod task_queries;
mod task_stage;
pub use gates::{Gate, GatePast, GateState, TASK_GATES_PATH, TaskGates};
pub use media::{GalleryItem, Media, View, Viewer};
pub use runs::{
    End, FailedTest, PIPELINES, RUN_SCHEMA, RunLine, Suite, TRIGGERS, fold, run_day, run_name,
};
pub use task_queries::{
    TaskKind, TaskStatus, subtasks, tasks, tasks_in_progress, tasks_in_review, tasks_in_status,
};
pub use task_stage::{TaskListing, TaskStage};
pub use {agent_probes::AgentProbes, agents::*, blocks::*, machine_work::*, project::*};

/// The route that answers a [`Front`].
pub const FRONT_PATH: &str = "/api/front";

/// `GET /api/front`: the server in front, as its starters and the agents see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Front {
    /// The server's own pid.
    pub pid: i64,
    /// The port it answers on.
    pub port: u16,
    /// The project it serves: the main checkout, for the one server.
    pub root: String,
    /// Whether the app behind it answers now.
    pub ready: bool,
    /// The app's pid, while one runs.
    pub app: Option<i64>,
    /// This machine's `MagicDNS` name, when Tailscale gives one: the address a phone reaches.
    pub tailnet: Option<String>,
    /// Symphony processes whose parent is init: leftovers nothing ends on its own (never killed).
    pub orphans: usize,
    /// Whether the server's first census of those leftovers has yet to end: until it does,
    /// `orphans` is 0 for want of a count. A server
    /// that does not send it reads as counted, so no start waits on it.
    #[serde(default)]
    pub uncounted: bool,
    /// A census has started and has no result yet. A starter returns the available status rather
    /// than waiting for the process-table command; a later status carries any completed count.
    #[serde(default)]
    pub census_pending: bool,
    /// The plugins the host binary compiles, by name: the one server serves a project only when
    /// it holds every plugin the project names.
    #[serde(default)]
    pub plugins: Vec<String>,
    /// The Symphony version the host was built at, its pin; 0 from a server before pins.
    #[serde(default)]
    pub pin: u32,
    /// The board schema the host reads and writes; 0 from a server before pins.
    #[serde(default)]
    pub schema: i64,
    /// The ids of the projects it serves under `/<id>/`, in the registry's order; none for a
    /// server of its own, which serves its one checkout at `/`.
    #[serde(default)]
    pub projects: Vec<String>,
}

/// The route that answers a [`Board`].
pub const BOARD_PATH: &str = "/api/board";
/// The route of the feed: Server-Sent Events, `?since=VERSION`.
pub const FEED_PATH: &str = "/api/feed";
/// A server-update failure, a JSON string (empty clears it). It does not advance the document
/// cursor. A retiring server finishes its feed normally; reconnects fetch a fresh snapshot.
pub const SERVER_EVENT: &str = "server";
/// The name of the feed's events; each one's data is a [`Delta`].
pub const FEED_EVENT: &str = "delta";
/// The name of the feed's keep-alive event, whose data is empty.
pub const PING_EVENT: &str = "ping";
/// The longest a server lets its feed go without a ping, in ms. The owner reads the board from a
/// phone over the tailnet, where a stream goes silent with its socket open on every network
/// change: at 5 s a client gives such a stream up within 12.5 s, and on localhost the pings cost
/// nothing.
pub const PING_MAX_MS: u32 = 5_000;
/// How many cards the board shows: the next one and the four after it.
pub const BOARD_CARDS: usize = 5;
/// The quick-task form and save endpoint accept at most this many Unicode characters in a title.
pub const QUICK_TASK_TITLE_MAX: usize = 200;

/// `POST` here with one file as the raw body and its name in `?name=`: the server stores it once
/// under its content's SHA-256 and answers an [`Upload`]. The owner's next post carries
/// [`Upload::markup`] as a line of its text, which is how the posted task, message, card line,
/// comment or ask keeps showing the file. Same Host and Origin checks as every save.
pub const UPLOAD_PATH: &str = "/api/upload";
/// The largest file [`UPLOAD_PATH`] takes, in bytes (10 MiB): a phone screenshot is 1-5 MB.
pub const UPLOAD_MAX_BYTES: usize = 10 * 1024 * 1024;
/// The file picker's `accept`: the pictures and documents the server stores. The server decides by
/// the file's content (and, for text and Office files, its extension), never by this list.
pub const UPLOAD_ACCEPT: &str = "image/png,image/jpeg,image/gif,image/webp,application/pdf,\
text/plain,text/markdown,text/csv,application/json,.png,.jpg,.jpeg,.gif,.webp,.pdf,.txt,.md,.csv,\
.json,.log,.docx,.xlsx,.pptx";

/// A stored upload, as [`UPLOAD_PATH`] answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    /// The file's address, `/artifacts/uploads/<sha256>.<ext>`, the same on every branch's pages.
    pub url: String,
    /// The owner's file name, made safe for one Markdown line.
    pub name: String,
    /// A picture the page draws inline; any other file is a link.
    pub picture: bool,
    pub bytes: u64,
    /// The one line a post carries to keep the file: `![name](url)` or `[name](url)`.
    pub markup: String,
}

/// `GET /api/board`: what the front page shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    /// The feed's version this snapshot is at: the client follows the feed from here.
    pub version: u64,
    /// The client build the server serves: the path of the shell's module script as the shell
    /// names it (`/assets/symphony-shell-….js`), whose hashed name changes with the client's
    /// code. A client whose own differs loads the page again.
    pub build: String,
    /// How often the server pings the feed, in ms: at most [`PING_MAX_MS`].
    pub ping_ms: u32,
    /// The first [`BOARD_CARDS`] owner decisions in mixed document-category order.
    pub next: Vec<Card>,
    pub counts: Counts,
    pub progress: Progress,
}

/// A card waiting on the owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    /// The owner's view order only; no verdict or document marker changes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub postponed: bool,
    /// The card's anchor in its document.
    pub slug: String,
    /// The document's app path: `docs/…/X.md`, or `@scope/docs/…/X.md` for a proposal.
    pub doc: String,
    /// The document's title.
    pub doc_title: String,
    /// A picture card by the slot it fills ("North · cover picture"); any other card by its
    /// first sentence, cut short.
    pub title: String,
    /// The words the title did not already say, cut short; empty when there are none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// The triage's reason, without its zero counts: "design · gates 11 items".
    pub why: String,
    /// When it was posted, as the register writes it: `YYYY-MM-DD HH:MM` or `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted: Option<String>,
    /// Its pictures in the card's order: the candidates (up to four) or the items it names (up
    /// to six). The first one is its thumbnail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<Picture>,
    /// The task proposing this version of the card, when it comes from another checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

impl Card {
    /// Where the card is: its document, at its anchor.
    pub fn href(&self) -> String {
        format!("/{}#{}", self.doc, self.slug)
    }
}

/// One picture a card shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picture {
    /// Where the picture is served; `None` for an item not drawn yet ("picture coming").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// The candidate's letter ("A"), or the item's name.
    pub caption: String,
    /// The picture still waits on the owner's review.
    #[serde(default, skip_serializing_if = "is_false")]
    pub waiting: bool,
}

/// The row of counts, each linking to where its detail lives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Cards waiting on the owner: the whole queue, not only the ones the board shows.
    pub waiting: u32,
    /// The documents those cards are in.
    pub docs_waiting: u32,
    /// Auto-approved cards to spot-check.
    pub spot_check: u32,
    /// Cards an agent answered the owner on today.
    pub answered_today: u32,
    /// The task board's counts; `None` when the server has no task board.
    #[serde(default)]
    pub tasks: Option<TaskCounts>,
}

/// The task board's counts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCounts {
    pub in_progress: u32,
    pub in_review: u32,
    pub agents_live: u32,
}

/// The home's progress: the project's plugin regions and the document progress.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// The regions the project's plugins add to the home, in the order they are registered.
    #[serde(default)]
    pub regions: Vec<Region>,
    /// The design documents (a folder's README is navigation, not one).
    pub docs: u32,
    /// The decisions waiting across them.
    pub decisions_waiting: u32,
}

/// A region of the home a plugin adds: a headline over bars of done, waiting and remaining work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// The region's id among the project's regions, stable across snapshots.
    pub id: String,
    pub title: String,
    /// The line under the headline's percentage (`21 of 31 items approved`).
    pub note: String,
    pub bars: Vec<ProgressBar>,
    /// The legend's words for done, waiting and remaining, in that order.
    pub legend: [String; 3],
}

/// One bar of a home region: how much of `total` is done, waiting, and (the rest) remaining.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressBar {
    pub key: String,
    pub name: String,
    /// Where the bar links: a project-relative path.
    pub page: String,
    /// What the bar says on hover.
    pub title: String,
    pub done: u32,
    pub waiting: u32,
    pub total: u32,
}

/// One event of the feed: what changed since the version before.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    /// The version after this change: the next `since`.
    pub version: u64,
    /// Each region that moved, with its new data.
    pub changes: Vec<Change>,
}

/// A region of the board that moved, with its new data: `{"region": "...", "data": ...}`, the
/// region first (as the server writes it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "region", content = "data", rename_all = "kebab-case")]
pub enum Change {
    /// One card's words or pictures changed; it keeps its place in the queue.
    Card(Card),
    /// The queue's order or members changed: its new first [`BOARD_CARDS`].
    Queue(Vec<Card>),
    Counts(Counts),
    Progress(Progress),
    /// The client build the server serves from now on, as [`Board::build`] names it.
    Client {
        build: String,
    },
    /// A region this client does not know yet; it is ignored. Never sent.
    #[serde(skip_serializing)]
    Other,
}

/// `Client`'s data.
#[derive(Deserialize)]
struct Build {
    build: String,
}

/// Read by hand rather than derived: serde's `other` cannot skip an unknown region's data, so a
/// derived reader would refuse a whole event for one region it does not know.
impl<'de> Deserialize<'de> for Change {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ChangeVisitor)
    }
}

struct ChangeVisitor;

impl<'de> serde::de::Visitor<'de> for ChangeVisitor {
    type Value = Change;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(r#"a region of the board: {"region": "...", "data": ...}"#)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Change, A::Error> {
        use serde::de::{Error, IgnoredAny};
        let mut region: Option<String> = None;
        let mut change = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "region" => region = Some(map.next_value()?),
                "data" => {
                    let Some(name) = region.as_deref() else {
                        return Err(A::Error::custom(
                            "a change names its region before its data",
                        ));
                    };
                    change = Some(match name {
                        "card" => Change::Card(map.next_value()?),
                        "queue" => Change::Queue(map.next_value()?),
                        "counts" => Change::Counts(map.next_value()?),
                        "progress" => Change::Progress(map.next_value()?),
                        "client" => Change::Client {
                            build: map.next_value::<Build>()?.build,
                        },
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                            Change::Other
                        }
                    });
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        match (region, change) {
            (_, Some(change)) => Ok(change),
            (Some(name), None) if !KNOWN_REGIONS.contains(&name.as_str()) => Ok(Change::Other),
            (Some(name), None) => Err(A::Error::custom(format!("the {name} region has no data"))),
            (None, None) => Err(A::Error::missing_field("region")),
        }
    }
}

const KNOWN_REGIONS: [&str; 5] = ["card", "queue", "counts", "progress", "client"];

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// The document's first folder under docs, shared by main and scoped branch proposals.
/// Root-level documents belong to docs; categories are path names, not inferred card kinds.
pub fn queue_category(path: &str) -> &str {
    let path = if path.starts_with('@') {
        path.split_once('/').map_or(path, |(_, path)| path)
    } else {
        path
    };
    path.strip_prefix("docs/")
        .and_then(|path| path.split_once('/'))
        .map_or("docs", |(folder, _)| folder)
}

/// The complete review queue, with mixed owner decisions, and the two queues below it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    pub waiting: Vec<Card>,
    pub spot_check: Vec<QueueItem>,
    pub agent: Vec<QueueItem>,
    /// Always empty; retained for clients from the preceding build that require this wire field.
    pub branches: Vec<QueueItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueItem {
    pub href: String,
    pub path: String,
    pub slug: String,
    pub title: String,
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub items: Vec<PendingItem>,
    pub docs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingItem {
    pub path: String,
    pub slug: String,
    pub href: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub items: Vec<ActivityItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityItem {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub slug: String,
    pub ts: String,
    pub text: String,
    pub href: String,
    pub answered: bool,
}

/// The task board's read records. These stay serde-only so native and wasm clients share them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub priority: i64,
    pub parent: Option<String>,
    pub created_by: String,
    pub created: f64,
    pub updated: f64,
    pub links: Vec<String>,
    pub depends: Vec<String>,
    pub roles: Vec<Role>,
    pub instruction: bool,
    /// The plugins' board fields: plugin name, then field name, then its value.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fields: std::collections::BTreeMap<String, std::collections::BTreeMap<String, FieldValue>>,
}

/// A board field's value: a number, or text (a text, a choice or a link).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Number(f64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[expect(clippy::struct_field_names, reason = "the field is the wire's name")]
pub struct Role {
    pub handle: String,
    pub role: String,
    pub since: f64,
    pub expires: Option<f64>,
    pub health: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub handle: String,
    pub kind: String,
    pub parent: Option<String>,
    pub worktree: String,
    pub branch: String,
    pub model: String,
    pub host: String,
    pub affinity: Vec<String>,
    pub connected: f64,
    pub last_seen: f64,
    pub last_tool: Option<f64>,
    pub left_at: Option<f64>,
    pub health: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[expect(clippy::struct_field_names, reason = "the field is the wire's name")]
pub struct Tasks {
    pub version: String,
    pub tasks: Vec<Task>,
    pub sessions: Vec<Session>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub slug: String,
    pub sender: String,
    pub recipient: Option<String>,
    pub kind: String,
    pub text: String,
    pub ts: f64,
}

/// Task metadata and its brief. Thread pages and the landing offer are fetched separately
/// when the owner reaches their controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskPage {
    #[serde(default)]
    pub machine_work: MachineWork,
    pub task: Task,
    pub blocks: Vec<Block>,
    pub children: Vec<Task>,
    pub children_count: usize,
    /// Bytes of the brief the page left out; `/api/task-brief` has all of it.
    #[serde(default)]
    pub brief_omitted: usize,
    /// Where the shown part of the brief ends; ask `/api/task-brief` from there.
    #[serde(default)]
    pub brief_next: usize,
    /// The revision of the whole brief, which moves whenever its text does: what was read of one
    /// revision is never shown after the part another revision's page sent.
    #[serde(default)]
    pub brief_revision: String,
}

/// A chunk of a long brief: its blocks, the byte it ends at, the bytes still left and the
/// revision of the brief it was cut from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskBrief {
    pub blocks: Vec<Block>,
    pub next: usize,
    pub omitted: usize,
    #[serde(default)]
    pub revision: String,
}

/// What a project's board action offers the owner on a task: a button named `label` beside the
/// task's `branch`, shown while there is a `payload`, which the button sends back to the action
/// `action` as it came (approve to land: the exact patch to land).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOffer {
    /// The board action's name.
    pub action: String,
    pub label: String,
    pub branch: String,
    pub payload: Option<String>,
}

/// The route the owner's answer to a [`TaskOffer`] is posted to: `slug`, `action`, `payload` and
/// `note`.
pub const TASK_ACTION_ROUTE: &str = "/api/task-action";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskEntry {
    pub message: Message,
    pub stamp: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskHistory {
    pub entries: Vec<TaskEntry>,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentPage {
    pub view: Agents,
    pub tasks: Vec<Task>,
    pub total: usize,
    pub next: Option<String>,
}

/// A document snapshot and the feed cursor taken atomically with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub version: u64,
    pub path: String,
    pub title: String,
    pub waiting: Vec<String>,
    pub crumbs: Vec<Crumb>,
    pub headings: Vec<Heading>,
    pub sections: Vec<Section>,
    pub cards: Vec<DocumentCard>,
    pub threads: std::collections::BTreeMap<String, Fragment>,
    pub tasks: Vec<DocumentTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<DocumentComparison>,
}

/// Why this document shows differences from main. The task link is global, while document links
/// and all writes retain the selected checkout's scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentComparison {
    pub task: String,
}

/// Metadata for a document whose bodies and threads are fetched near the viewport. Anchor ids
/// reserve their destinations before their section's markup is mounted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentOutline {
    pub document: Document,
    pub anchors: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSections {
    pub version: u64,
    pub change: DocumentChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crumb {
    pub name: String,
    pub href: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub level: u8,
    pub anchor: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fragment {
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// Internal compatibility with the server's Markdown pipeline; never sent to the client.
    #[serde(default, skip_serializing)]
    pub html: String,
    #[serde(default, skip_serializing)]
    pub media: Vec<Media>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub start: usize,
    pub end: usize,
    pub key: String,
    pub hash: String,
    pub part: Option<usize>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default, skip_serializing)]
    pub html: String,
    #[serde(default, skip_serializing)]
    pub media: Vec<Media>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<SectionComparison>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difference {
    Added,
    Changed,
    Removed,
}

/// Main's previous content is loaded with this section's body, never in its outline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionComparison {
    pub kind: Difference,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentCard {
    pub slug: String,
    pub state: String,
    pub text: String,
    pub group: String,
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentTask {
    pub slug: String,
    pub title: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub path: String,
    pub title: String,
    pub crumbs: Vec<Crumb>,
    pub docs: usize,
    pub intro: Option<Document>,
    pub entries: Vec<FolderEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderEntry {
    pub kind: String,
    pub path: String,
    pub title: String,
    pub docs: usize,
    pub waiting: usize,
    pub summary: String,
}

/// Document deltas never carry an unchanged fragment. Positions are separate from content, so an
/// inserted line does not resend every card below it. `order` is present only when keys move.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentChange {
    pub path: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub removed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<SectionPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub threads: std::collections::BTreeMap<String, Option<Fragment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crumbs: Option<Vec<Crumb>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headings: Option<Vec<Heading>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<ItemChange<DocumentCard>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<DocumentTask>>,
    /// An empty vector clears the comparison; otherwise contains its single context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<Vec<DocumentComparison>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionPosition {
    pub key: String,
    pub start: usize,
    pub end: usize,
    pub part: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub sha: String,
    pub date: String,
    pub subject: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentDiff {
    pub sha: String,
    pub subject: String,
    pub diff: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ask {
    pub id: i64,
    pub path: String,
    pub anchor: String,
    pub quote: String,
    pub question: String,
    pub asked: f64,
    pub updated: f64,
    pub answers: Vec<AskAnswer>,
    pub messages: Vec<AskMessage>,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskAnswer {
    pub text: String,
    pub by: String,
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskMessage {
    pub seq: i64,
    pub kind: String,
    pub by: String,
    pub text: String,
    pub at: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Legacy reads only; `format=blocks` leaves these fields empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<Media>,
}

/// A list region's change. `key` is stable (task slug, session handle, activity id, or href).
/// Upserts replace that record, removals delete it; an order event moves one key after `after`,
/// or to the front when `after` is null. The server never sends the whole task board in a delta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct ItemChange<T> {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub removed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderChange {
    pub key: String,
    pub after: Option<String>,
}

/// A task's body is fetched in the initial snapshot. A task status/role update carries its changed
/// fields only; this type lists every mutable field and lets a client merge it into its record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
    /// Present only when parent changes. Empty string clears the parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<Role>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskChange {
    pub key: String,
    pub fields: TaskFields,
}
