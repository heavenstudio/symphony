//! The Symphony's wire types: the JSON the server (`apps/symphony-server`) sends and the client
//! (`apps/symphony-web`) reads through its generated TypeScript. Serde only.
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
//! - The same `GET`, asking to upgrade (`Upgrade: websocket`), is the same stream on a WebSocket:
//!   each event one text message, its name, a line feed, then its data (a [`FEED_EVENT`]'s
//!   [`Delta`] or a [`SERVER_EVENT`]'s string, as JSON; nothing for a [`PING_EVENT`]), closed by
//!   the server where the stream would end.
//!   A browser keeps six HTTP/1.1 connections to a host and an event stream holds one while it is
//!   open, so six tabs following the board would leave a seventh none for its reads; it counts
//!   each WebSocket apart. The web client follows the feed so (`apps/symphony-web/src/feed.ts`). A
//!   WebSocket has no same-origin rule for what it reads, so an upgrade whose `Origin` is not
//!   `http://` and its own `Host` is refused (`403`), as such a write is.
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
//! The server's own routes answer before any project's: [`FRONT_PATH`] a [`Front`], the one server
//! in front of the app, as its starters and the agents see it, and [`PROJECTS_PATH`] a
//! [`ProjectList`], the projects it serves as the desktop app reads them ([`listing`]).

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub mod agent_activity;
mod agent_probes;
mod agents;
pub mod attempts;
pub mod gates;
pub mod health;
mod links;
pub mod listing;
mod machine_work;
mod people;
mod project;
pub mod report_page;
pub mod reports;
mod runs;
pub mod runs_rollup;
mod schema_updates;
pub mod settings;
mod task_diff;
mod task_queries;
mod task_stage;
pub mod timeline;
pub mod workflow;
pub use agent_activity::{AGENT_ACTIVITY_SCHEMA, AgentHour, AgentLine};
pub use attempts::{
    ATTEMPT_CONTROL_ROUTE, ATTEMPT_VIEW_ROUTE, AttemptControl, AttemptControls, AttemptLink,
    AttemptView, AttemptViewAsk, TASK_ATTEMPTS_PATH, TaskAttempts,
};
pub use gates::{Gate, GatePast, GateState, GatesSeen, TASK_GATES_PATH, TaskGates};
pub use health::{HEALTH_SCHEMA, HealthHour, HourPasses, HourRequests, HourSlowest, HourWrites};
pub use links::{app_path, linkable};
pub use listing::{ListedCard, ListedProject, PROJECTS_PATH, ProjectList, TaskAsks, TaskVerdict};
pub use people::{
    Addresses, GroupRow, MachineProfile, NoticeWay, PEOPLE_CHANGE_PATH, PEOPLE_PATH, PROFILE_PATH,
    PeopleAction, PeopleChange, PeopleLogLine, PeoplePage, PersonRow, ProfileWrite, WithheldPower,
};
pub use report_page::{Grain, REPORTS_LANDED_PATH, REPORTS_PATH, ReportBucket, ReportsPage};
pub use reports::{REPORT_SCHEMA, ReportDay, ReportHour, report_file};
pub use runs::{
    End, FailedTest, PIPELINES, RUN_LOG_MOST, RUN_LOG_PATH, RUN_LOG_TAIL, RUN_PATH, RUN_SCHEMA,
    RUNS_DAYS, RUNS_FILE_BYTES, RUNS_PAGE, RUNS_PATH, RUNS_READ_BYTES, RunLine, RunLog, RunPage,
    RunsPage, Suite, TRIGGERS, fold, run_day, run_name,
};
pub use runs_rollup::{RUNS_DAY_SCHEMA, RUNS_ROLLUP, RunsDay};
pub use schema_updates::{
    SCHEMA_UPDATES_PATH, SCHEMA_UPDATES_SHOWN, SchemaUpdate, SchemaUpdates, UpdateState, UpdateTime,
};
pub use settings::{
    EntryNeed, EntryState, SETTING_WRITE_PATH, SETTINGS_PATH, Setting, SettingAction, SettingClass,
    SettingKind, SettingScope, SettingSource, SettingValue, SettingWrite, SettingWritten,
    SettingsEntry, SettingsFile, SettingsPage, SettingsSection, StoredChange, StoredSetting,
};
pub use task_diff::{
    TASK_DIFF_FILE_PATH, TASK_DIFF_PATH, TaskDiff, TaskDiffFile, TaskDiffPatch, TaskDiffSource,
};
pub use task_queries::{
    TaskKind, TaskStatus, slug_order, subtasks, tasks, tasks_in_progress, tasks_in_review,
    tasks_in_status,
};
pub use task_stage::{TaskListing, TaskStage, WaitingBoard, WaitingColumn};
pub use timeline::{TASK_TIMELINE_PATH, TaskTimeline};
pub use {agent_probes::AgentProbes, agents::*, machine_work::*, project::*};

/// The plugin kit's block vocabulary (`symphony-blocks`), which these wire types carry: document
/// content as data, what a page draws that markup cannot, and the data a plugin's seams return
/// (a gate's own with [`gates`]). Re-exported under the names it has always had here.
pub use symphony_blocks::{
    Block, CalloutKind, CardGroup, CardState, Cell, ContainerKind, FieldValue, FigureSize,
    GalleryItem, Image, InlineKind, LinkTarget, Media, NavItem, Picture, Presentation, ProgressBar,
    Region, Span, TaskOffer, ThreadAuthor, ThreadEntry, VectorKind, VectorShape, Verdict, View,
    Viewer,
};

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
    /// Always 0: the server counts no other processes. It is sent because a starter built when
    /// the server counted its leftovers cannot read an answer without it.
    #[serde(default)]
    pub orphans: usize,
    /// The plugins the host binary compiles, by name: the one server serves a project only when
    /// it holds every plugin the project names linked, one with no `[plugins.process]` command
    /// (a plugin that answers from its process any host serves); a release's server compiles none.
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
    /// The release installed on the machine it runs, when it is one: no start replaces it, and its
    /// reload follows the machine's `current` link; none for a project's build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
}

/// The route that answers a [`Board`].
pub const BOARD_PATH: &str = "/api/board";
/// The route of the feed: Server-Sent Events, or the same events on a WebSocket, `?since=VERSION`.
pub const FEED_PATH: &str = "/api/feed";
/// The response header every JSON read names the feed's cursor at the read in, on its `200` and
/// its `304` alike: every event at or before it is in the answer. A page that draws the copy it
/// kept on a `304`, which has no body, opens the feed from it, and a page whose feed is open
/// drops what the feed says of a read at or before the read's cursor.
pub const VERSION_HEADER: &str = "symphony-version";
/// The guide a new project starts from, `docs/operations/GETTING_STARTED.md`, where a project that has no
/// Symphony checkout reads it: in the Symphony's repository. An empty board and an empty folder
/// of documents link it.
pub const GETTING_STARTED: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/operations/GETTING_STARTED.md"
);
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
/// The client cache's format (docs/engineering/analysis/client-cache.md, "Across versions"): what a page keeps
/// on the device is written under it, and a client built with another number clears what it
/// finds. It moves when a kept type's shape does, which the wire test ties it to; a server update
/// needs nothing, since a changed body has a new tag. 2: documents and an entrypoint's folder page
/// are kept too.
pub const CACHE_FORMAT: u32 = 2;
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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

/// The row of counts, each linking to where its detail lives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskCounts {
    pub in_progress: u32,
    pub in_review: u32,
    pub agents_live: u32,
}

/// The home's progress: the project's plugin regions and the document progress.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Progress {
    /// The regions the project's plugins add to the home, in the order they are registered.
    #[serde(default)]
    pub regions: Vec<Region>,
    /// The design documents (a folder's README is navigation, not one).
    pub docs: u32,
    /// The decisions waiting across them.
    pub decisions_waiting: u32,
}

/// One event of the feed: what changed since the version before.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Delta {
    /// The version after this change: the next `since`.
    pub version: u64,
    /// Each region that moved, with its new data.
    pub changes: Vec<Change>,
}

/// A region of the board that moved, with its new data: `{"region": "...", "data": ...}`, the
/// region first (as the server writes it).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    /// What the task board asks of each person, as it is from now on: sent each time it changes,
    /// for the desktop app ([`listing`]). The web client reads none, so its wire types leave it out.
    #[cfg_attr(feature = "typescript", ts(skip))]
    TaskAsks(TaskAsks),
    /// A region this client does not know yet; it is ignored. Never sent, so the web client's wire
    /// types leave it out: it reads a region it does not know as any `{region, data}`.
    #[serde(skip_serializing)]
    #[cfg_attr(feature = "typescript", ts(skip))]
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
                        "task-asks" => Change::TaskAsks(map.next_value()?),
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

const KNOWN_REGIONS: [&str; 6] = ["card", "queue", "counts", "progress", "client", "task-asks"];

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn is_zero(value: &usize) -> bool {
    *value == 0
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Queue {
    pub waiting: Vec<Card>,
    pub spot_check: Vec<QueueItem>,
    pub agent: Vec<QueueItem>,
    /// Always empty; retained for clients from the preceding build that require this wire field.
    pub branches: Vec<QueueItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct QueueItem {
    pub href: String,
    pub path: String,
    pub slug: String,
    pub title: String,
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The owner's open ask a card waiting on an agent is on the board; none for a comment, a card
    /// to spot-check, a card no ask is open for, and until the board is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<QueueAsk>,
}

/// How long an open ask waits before it shows as late, in seconds: the board's own bound, which
/// `just tasks asks` says by.
pub const ASK_LATE_SECONDS: u32 = 600;

/// The owner's open ask on the board, as the row of the card it is about says it: how long it has
/// waited and who holds it, the board's own record and never a reading of a task's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct QueueAsk {
    /// Its number on the board: `ask N`.
    pub id: i64,
    /// When the owner said it, in whole seconds since the epoch: its age runs from here, and past
    /// [`ASK_LATE_SECONDS`] it is late.
    pub asked: i64,
    /// The session that holds it; none is nobody.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    /// Since when its holder holds it, in whole seconds since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Pending {
    pub items: Vec<PendingItem>,
    pub docs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct PendingItem {
    pub path: String,
    pub slug: String,
    pub href: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Activity {
    pub items: Vec<ActivityItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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

/// The task board's read records. These stay serde-only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    /// Replayed stretches in review, including an active round, for open and finished tasks.
    /// None means the summary is unavailable (including older servers), not zero reviews.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_rounds: Option<u32>,
    /// What an open task waits on; none for a closed one, or from a server before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waits_on: Option<WaitsOn>,
    /// The status a task the board holds blocked was blocked from, where it is known. A block that
    /// names nothing is not blocked on the wire: the task is sent in that status, `from` beside it,
    /// and its [`WaitsOn`] says no reason is recorded (docs/workflow/TASKS.md, what a task waits on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Where a custom workflow holds it ([`workflow::TaskPlace`]): none for a task the board's own
    /// writers keep, every task of a board with no custom workflow among them, and from a server
    /// before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<workflow::TaskPlace>,
}

/// What an open task waits on, why, and since when: the one line the board's card ends in, read by
/// the board's one rule (`symphony_board::waits_on`), the one the PULL digest applies.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WaitsOn {
    /// One of [`WAITS_KINDS`]. A client that does not know a kind draws it as `nobody`.
    pub kind: String,
    /// Who or what: the holder at work (`amber-heron`, `amber-heron (stale)`), the decision's
    /// name, the checks' names, the task's slug; empty where the kind names nothing.
    pub who: String,
    /// Why, in a few words: the role at work, a block's own words, what a landing lacks.
    pub why: String,
    /// When the task took the status it holds, in seconds since the epoch.
    pub since: f64,
}

/// The kinds of [`WaitsOn`], in the order every list and count of them keeps: what needs someone
/// first (the owner's decision, nobody, a check, other tasks), then work going on, then work that
/// waits by design.
pub const WAITS_KINDS: [&str; 7] = [
    "decision", "nobody", "check", "task", "working", "queued", "parked",
];

/// A session holding the orchestrator role on an open task, and its health (`live`, `stale`,
/// `unknown`, `lost` or `left`): the board says once when none is live.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Orchestrator {
    pub handle: String,
    pub health: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    /// Each session holding the orchestrator role on an open task, by handle.
    #[serde(default)]
    pub orchestrators: Vec<Orchestrator>,
    /// The ready tasks no live executor holds with nothing open before them: what waits to start,
    /// by the PULL digest's rule.
    #[serde(default)]
    pub unstarted: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Message {
    pub id: i64,
    pub slug: String,
    pub sender: String,
    pub recipient: Option<String>,
    pub kind: String,
    pub text: String,
    pub ts: f64,
    /// A person a board named wrote it, `sender` their short name: drawn as a person's line, as
    /// the owner's is. False for the owner's, whose sender says so, and for a session's; an older
    /// server sends none.
    #[serde(default)]
    pub person: bool,
}

/// Task metadata and its brief. Thread pages and the landing offer are fetched separately
/// when the owner reaches their controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    /// Where each of the task's role holders' Go to session controls go, by handle: a holder with
    /// none known is left out ([`SessionGo`]).
    #[serde(default)]
    pub sessions: std::collections::BTreeMap<String, SessionGo>,
    /// The page was asked from the server's own machine, where a pane is focused rather than its
    /// screen shown ([`SessionGo::control`]).
    #[serde(default)]
    pub local: bool,
    /// How many gates the last answer the server gave for the task had, and how many it waited on:
    /// the room the page holds for its gates while it reads them. `None` while the server has given
    /// none since it started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<GatesSeen>,
    /// How many execution attempts the task has had ([`TASK_ATTEMPTS_PATH`] reads them), left out
    /// at none: a page draws their history only for a task that has some.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: usize,
}

/// A chunk of a long brief: its blocks, the byte it ends at, the bytes still left and the
/// revision of the brief it was cut from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskBrief {
    pub blocks: Vec<Block>,
    pub next: usize,
    pub omitted: usize,
    #[serde(default)]
    pub revision: String,
}

/// The route the owner's answer to a [`TaskOffer`] is posted to: `slug`, `action`, `payload` and
/// `note`.
pub const TASK_ACTION_ROUTE: &str = "/api/task-action";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskEntry {
    pub message: Message,
    pub stamp: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskHistory {
    pub entries: Vec<TaskEntry>,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AgentPage {
    pub view: Agents,
    pub tasks: Vec<Task>,
    pub total: usize,
    pub next: Option<String>,
    /// The page was asked from the server's own machine ([`SessionGo::control`]).
    #[serde(default)]
    pub local: bool,
}

/// A document snapshot and the feed cursor taken atomically with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DocumentComparison {
    pub task: String,
}

/// Metadata for a document whose bodies and threads are fetched near the viewport. Anchor ids
/// reserve their destinations before their section's markup is mounted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DocumentOutline {
    pub document: Document,
    pub anchors: std::collections::BTreeMap<String, Vec<String>>,
    /// The hash of each card's thread, by the card's slug, as a section's `hash` is taken. A thread
    /// is no part of its card section's `hash`, so a client that keeps a card's body and thread on
    /// the device tells by this whether the thread changed since (docs/engineering/analysis/client-cache.md,
    /// "What is kept").
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub thread_hashes: std::collections::BTreeMap<String, String>,
}

/// The most a page reads ahead of a document it links to, in bytes of the JSON the server answers
/// (`/api/document?view=first`, [`DocumentOutline::first`]): a screen of the document or a bit more.
/// Measured on this repository's documents, the leading sections within it, beside the headings,
/// hold 990 to 7,300 characters of text (`docs/DECISIONS.md` 990 in its first 6 sections, its next
/// a 12 KB card, `docs/workflow/TASKS.md` 6,627 in 10), where a phone's screen shows about 1,200 and a
/// desktop's about 2,700; their whole
/// outlines are 9 to 27 KB, and the consuming project's register's 56 KB.
pub const AHEAD_BYTES: usize = 16_000;

impl DocumentOutline {
    /// What a page reads ahead of a document it links to (`/api/document?view=first`): this
    /// outline's head, its headings all among it, and `document`'s leading sections whole, as many
    /// as `fits` takes in turn, each card among them with its thread and each with its anchors; the
    /// cards, waiting slugs, anchors and threads of the sections left out are left out with them.
    /// Opened, the page draws its top and its whole contents from the device at once, so nothing
    /// moves when it reads the rest. `None` when `fits` takes not even the head with no section.
    pub fn first(
        &self,
        document: &Document,
        fits: impl Fn(&DocumentOutline) -> bool,
    ) -> Option<DocumentOutline> {
        let mut part = self.clone();
        part.document.sections.clear();
        part.document.cards.clear();
        part.document.waiting.clear();
        part.document.threads.clear();
        part.anchors.clear();
        part.thread_hashes.clear();
        if !fits(&part) {
            return None;
        }
        for section in &document.sections {
            let mut next = part.clone();
            let ids = self.anchors.get(&section.key).cloned().unwrap_or_default();
            next.anchors.insert(section.key.clone(), ids);
            if let Some(slug) = section.key.strip_prefix("card:") {
                next.document
                    .cards
                    .extend(document.cards.iter().filter(|c| c.slug == slug).cloned());
                if self.document.waiting.iter().any(|waiting| waiting == slug) {
                    next.document.waiting.push(slug.to_string());
                }
                if let Some(thread) = document.threads.get(slug) {
                    next.document
                        .threads
                        .insert(slug.to_string(), thread.clone());
                }
                if let Some(hash) = self.thread_hashes.get(slug) {
                    next.thread_hashes.insert(slug.to_string(), hash.clone());
                }
            }
            next.document.sections.push(section.clone());
            if !fits(&next) {
                break;
            }
            part = next;
        }
        Some(part)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DocumentSections {
    pub version: u64,
    pub change: DocumentChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Crumb {
    pub name: String,
    pub href: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Heading {
    pub level: u8,
    pub anchor: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Fragment {
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// Internal compatibility with the server's Markdown pipeline; never sent to the client.
    #[serde(default, skip_serializing)]
    #[cfg_attr(feature = "typescript", ts(skip))]
    pub html: String,
    #[serde(default, skip_serializing)]
    #[cfg_attr(feature = "typescript", ts(skip))]
    pub media: Vec<Media>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Section {
    pub start: usize,
    pub end: usize,
    pub key: String,
    pub hash: String,
    pub part: Option<usize>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default, skip_serializing)]
    #[cfg_attr(feature = "typescript", ts(skip))]
    pub html: String,
    #[serde(default, skip_serializing)]
    #[cfg_attr(feature = "typescript", ts(skip))]
    pub media: Vec<Media>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<SectionComparison>,
    /// The kinds of the first and last blocks the section draws ([`Section::ends_of`]), so the
    /// outline tells a page how a section it has not read yet will meet its neighbours. Left out
    /// for a section that draws nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends: Option<[String; 2]>,
}

impl Section {
    /// The kinds of the first and last blocks `blocks` draw: a block's `type`, a heading's with its
    /// level (`heading-3`), and a container drawn with the `callout` class as a `callout`. A group
    /// draws its blocks, so its edges are theirs; a comment draws nothing and is passed over.
    pub fn ends_of(blocks: &[Block]) -> Option<[String; 2]> {
        Some([edges(blocks, false)?, edges(blocks, true)?])
    }
}

/// The kind of the first block `blocks` draw, or of the last when `last`.
fn edges(blocks: &[Block], last: bool) -> Option<String> {
    if last {
        blocks.iter().rev().find_map(|block| edge(block, true))
    } else {
        blocks.iter().find_map(|block| edge(block, false))
    }
}

/// The kind `block` draws as at a section's first edge, or its last when `last`.
fn edge(block: &Block, last: bool) -> Option<String> {
    let kind = match block {
        Block::Group { blocks } => return edges(blocks, last),
        Block::Comment { .. } => return None,
        Block::Heading { level, .. } => return Some(format!("heading-{level}")),
        Block::Container { presentation, .. }
            if presentation
                .class
                .split_whitespace()
                .any(|word| word == "callout") =>
        {
            "callout"
        }
        Block::Paragraph { .. } => "paragraph",
        Block::List { .. } => "list",
        Block::Table { .. } => "table",
        Block::Code { .. } => "code",
        Block::Quote { .. } => "quote",
        Block::Rule => "rule",
        Block::Callout { .. } => "callout",
        Block::Card { .. } => "card",
        Block::Component { .. } => "component",
        Block::Media { .. } => "media",
        Block::Image { .. } => "image",
        Block::Figure { .. } => "figure",
        Block::Caption { .. } => "caption",
        Block::Comparison { .. } => "comparison",
        Block::Container { .. } => "container",
        Block::Details { .. } => "details",
        Block::SettledCard { .. } => "settled-card",
        Block::Thread { .. } => "thread",
        Block::Unknown { .. } => "unknown",
        // The kit's lists grow in a minor, so no match outside it covers them whole
        // (`tests/vocabulary_drawn.rs` holds every kind to an arm here).
        _ => "later",
    };
    Some(kind.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum Difference {
    Added,
    Changed,
    Removed,
}

/// Main's previous content is loaded with this section's body, never in its outline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SectionComparison {
    pub kind: Difference,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DocumentCard {
    pub slug: String,
    pub state: String,
    pub text: String,
    pub group: String,
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DocumentTask {
    pub slug: String,
    pub title: String,
    pub status: String,
}

/// The board area of the tasks the Symphony filed for the owner's verdicts waiting on an agent
/// before those were asks (`symphony verdict-asks` turns the open ones into asks):
/// `verdicts/2026-11-11-1244`. They answer the owner, and are no work on the documents they link.
pub const VERDICT_TASKS: &str = "verdicts/";

impl DocumentTask {
    /// Whether this is a task the Symphony filed for the owner's verdicts ([`VERDICT_TASKS`]).
    pub fn answers_verdicts(&self) -> bool {
        self.slug.starts_with(VERDICT_TASKS)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Folder {
    pub path: String,
    pub title: String,
    pub crumbs: Vec<Crumb>,
    pub docs: usize,
    pub intro: Option<Document>,
    pub entries: Vec<FolderEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SectionPosition {
    pub key: String,
    pub start: usize,
    pub end: usize,
    pub part: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Version {
    pub sha: String,
    pub date: String,
    pub subject: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DocumentDiff {
    pub sha: String,
    pub subject: String,
    pub diff: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AskAnswer {
    pub text: String,
    pub by: String,
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AskMessage {
    pub seq: i64,
    pub kind: String,
    pub by: String,
    pub text: String,
    pub at: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Legacy reads only; `format=blocks` leaves these fields empty, so the web client, which reads
    /// blocks alone, is never sent them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[cfg_attr(feature = "typescript", ts(skip))]
    pub html: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "typescript", ts(skip))]
    pub media: Vec<Media>,
}

/// A list region's change. `key` is stable (task slug, session handle, activity id, or href).
/// Upserts replace that record, removals delete it; an order event moves one key after `after`,
/// or to the front when `after` is null. The server never sends the whole task board in a delta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ItemChange<T> {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub removed: bool,
}

/// A task's body is fetched in the initial snapshot. A task status/role update carries its changed
/// fields only; this type lists every mutable field and lets a client merge it into its record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    /// Present when the known review-round count changes. Losing the summary replaces the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_rounds: Option<u32>,
    /// Present only when what it waits on changes; one of no kind (empty) clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waits_on: Option<WaitsOn>,
    /// Present only when the status it was blocked from changes. Empty string clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Present only when its place in a custom workflow changes; a place is never taken away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<workflow::TaskPlace>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskChange {
    pub key: String,
    pub fields: TaskFields,
}
