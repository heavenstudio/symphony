//! The machine's schema updates as a page reads them, [`SCHEMA_UPDATES_PATH`]: each registered
//! project's board reaching a schema the machine's server must come to write, and how long until it
//! did (docs/workflow/RELEASES.md, "An update"). The server reads them from the machine folder's ledger
//! (`symphony::machine::updates`), which a release's refresh writes, and says each in words a page
//! draws as they come.

use serde::{Deserialize, Serialize};

/// `GET`: the machine's schema updates ([`SchemaUpdates`]), read from its ledger and never from a
/// board, answered by every server for any project it serves.
pub const SCHEMA_UPDATES_PATH: &str = "/api/schema-updates";

/// How many of the newest updates the answer holds beside every open one.
pub const SCHEMA_UPDATES_SHOWN: usize = 20;

/// What [`SCHEMA_UPDATES_PATH`] answers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SchemaUpdates {
    /// Every open update and the [`SCHEMA_UPDATES_SHOWN`] newest, newest first: the order the
    /// ledger was written in, turned round.
    pub updates: Vec<SchemaUpdate>,
    /// Why the ledger could not be read, in a sentence naming it; none while it was read, or there
    /// is none yet.
    pub unread: Option<String>,
}

/// One board's update to one schema, in one generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SchemaUpdate {
    /// `ID@N`, `ID@N.G` from its second generation.
    pub id: String,
    /// The project whose board it is: its id on the machine's server.
    pub project: String,
    /// The schema its board reached.
    pub schema: i64,
    /// Its generation: 1, then 2 for the board's next arrival at the same schema after it went back.
    pub generation: u32,
    pub state: UpdateState,
    /// When its board reached the schema, as far as a record says.
    pub reached: Option<UpdateTime>,
    /// When the first green release serving the schema was installed on the machine.
    pub available: Option<UpdateTime>,
    /// When the machine's server first wrote the board at the schema.
    pub served: Option<UpdateTime>,
    /// From reached to served, in seconds, once both are known.
    pub total: Option<f64>,
    /// [`SchemaUpdate::total`] as the owner reads a wait: `4 h 46 min`, `3 min 10 s`, `40 s`.
    pub total_said: Option<String>,
    /// How much of the wait came after the release was installed, in seconds, once both are known.
    pub after_available: Option<f64>,
    /// [`SchemaUpdate::after_available`] as the owner reads a wait.
    pub after_available_said: Option<String>,
    /// How many refreshes ended with it incomplete though a green release serving it was here.
    pub failures: u32,
    /// What the last of them found; none while none failed.
    pub failure: Option<String>,
    /// The page of its task, filed at its first failure, on the server that answers: an app path
    /// (`/<project>/task/SLUG` under the machine's one server, while the project is registered);
    /// none while it has none, its board is an unregistered one's, or its task is another
    /// project's board's and the server serves its one project alone.
    pub task: Option<String>,
}

/// Where an update stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    /// Still to be served: the machine's server does not write its board at the schema yet.
    Open,
    /// The machine's server wrote its board at the schema.
    Served,
    /// Ended unserved: its board went back below the schema or moved past it, a later schema took
    /// its place, or its project left the machine's registry. One served before its board went back
    /// stays [`UpdateState::Served`].
    Left,
}

/// A time an update's record gives, and which record gives it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct UpdateTime {
    /// Seconds since the epoch.
    pub at: f64,
    /// Where it comes from, named as `release --status` names it, the record's words before their
    /// first colon: `schema-seen.log`, `release-2026-11-19's manifest, written by its install`,
    /// `the server's hand-off`, `this refresh's move`, or a refresh's own look.
    pub source: String,
    /// The record's words whole, its name first and then what it says of the time (`schema-seen.log:
    /// the first store that saw the board at this schema; …`), as the ledger holds them.
    pub words: String,
}
