//! The one server's list of its projects as data, [`PROJECTS_PATH`]: what the desktop app reads of
//! each server it follows (docs/product/analysis/desktop-client.md), and the task board's asks a project's
//! feed carries as they change ([`TaskAsks`], the feed's [`crate::Change::TaskAsks`]).
//!
//! The app, and a release's refresh, read only the fields the fixture test pins
//! (`tests/wire/listing.rs`): they are added to and never renamed. Everything else may change with
//! any release.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The route that answers a [`ProjectList`]: the one server's own, before any project's, answering
/// loopback and this machine's tailnet names as a project's routes do.
pub const PROJECTS_PATH: &str = "/api/projects";

/// `GET /api/projects`: the server and each project it serves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectList {
    /// The server's installation id (its machine folder's, `symphony::machine::installation_id`):
    /// one server reached by loopback and by a tailnet name answers the same id. None when the
    /// machine folder's identity cannot be read.
    pub installation: Option<String>,
    /// The Symphony version the server was built at, its pin ([`crate::Front::pin`]).
    pub pin: u32,
    /// The projects it serves, in the registry's order.
    pub projects: Vec<ListedProject>,
}

/// One project as [`ProjectList`] lists it, read from what the server holds in memory: no board or
/// document is read for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListedProject {
    /// Its id: its pages are under `/<id>/`, its feed at `/<id>/api/feed`.
    pub id: String,
    /// Its name, as its pages and the server's project list name it.
    pub name: String,
    /// Why this server does not serve it, when it does not; its counts are none then.
    pub refused: Option<String>,
    /// The line the server's project list draws under it while its board is not well: the warning
    /// its pages show while the board is unwell, else that it is slow or that no live orchestrator
    /// staffs the work waiting to start (docs/workflow/TASKS.md, "Board health"); none while it is well.
    pub unwell: Option<String>,
    /// The version of its feed the counts below were read at: its feed follows from here
    /// (`/<id>/api/feed?since=`). None until its board view is read, and while it is refused.
    pub version: Option<u64>,
    /// Cards waiting on the owner, the whole queue ([`crate::Counts::waiting`]), at `version`.
    pub waiting: Option<u32>,
    /// The first card waiting on the owner, none while none waits.
    pub next: Option<ListedCard>,
    /// What its task board asks of each person, at `version`; none when it has no task board or
    /// the board is not read yet.
    pub asks: Option<TaskAsks>,
    /// Whether this server writes its board, as its last read of the board found it: true while
    /// that read found the board at a schema this build writes (its own, or one ahead whose step is
    /// declared to keep it writing); false while this build only reads it (the board is a schema
    /// ahead whose step is not) or refuses it (behind this build, or more than one schema ahead). None while it keeps no board, its board is not read yet or its last read failed for
    /// another reason, while it is refused, and from a server built before it said
    /// (docs/workflow/RELEASES.md, "An update": a release's refresh reads it).
    #[serde(default)]
    pub writes: Option<bool>,
}

/// A card as the project list names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedCard {
    /// Its title ([`crate::Card::title`]).
    pub title: String,
    /// Its page on the server, under the project: `/<id>/docs/…/X.md#slug`.
    pub href: String,
}

/// What a task board asks of each person beyond the cards, by person: the board knows one today,
/// `owner`. People added later are names in these maps, never new fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskAsks {
    /// The questions agents asked each person on tasks (`just tasks say SLUG "…" --to owner`) that
    /// still wait on them: one waits until its recipient posts on that task's thread after it, or
    /// the task is done or cancelled. Every person the board knows is named, with 0 for none.
    pub questions: BTreeMap<String, u32>,
    /// The newest reviewer's verdict on the board, by the person it is news for: it asks nothing of
    /// them, so it counts toward no badge. A person is left out while the board holds none.
    pub verdicts: BTreeMap<String, TaskVerdict>,
}

/// A reviewer's verdict on a task (`just tasks verdict SLUG approve|changes`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskVerdict {
    /// Its line's id on the board: a newer verdict has a greater one.
    pub id: i64,
    /// The task's slug.
    pub task: String,
    /// `approve` or `changes`.
    pub verdict: String,
    /// The handle of the reviewer who gave it.
    pub reviewer: String,
    /// When it was given, in seconds since the epoch.
    pub at: f64,
}
