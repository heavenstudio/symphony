//! The board's rows: one struct per table of the SQLite board the board moved from, its fields the
//! table's columns, as `tasks-copy --board --out` writes them. The copy reads them ([`crate::Store::copy`]) and the tests seed
//! the board with them. A column added to a board after it was first written (`instruction`, the
//! liveness columns) may be missing from an old dump, and reads as its default; a table or a column
//! the copy does not know is refused by name ([`Dump::from_json`]), never dropped.
//!
//! A row of an open table ([`crate::schema::OPEN`]) is read past a field its struct does not know:
//! an additive step's, which a build without the step reads around and whose writes keep it. Each
//! row refuses one read exactly, as a dump is read ([`crate::objects::read_exact`]), since a copy,
//! an export or a move to another engine made of it would drop it; every other read hands the row
//! only the fields it names ([`open_row`], [`crate::objects::read`]), as a task's record does
//! ([`crate::store`]'s `Beside`).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// Every row of the board, table by table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct Dump {
    pub tasks: Vec<TaskRow>,
    pub task_links: Vec<LinkRow>,
    pub sessions: Vec<SessionRow>,
    pub roles: Vec<RoleRow>,
    pub messages: Vec<MessageRow>,
    pub deliveries: Vec<DeliveryRow>,
    pub barred_keys: Vec<BarredKeyRow>,
    pub task_sessions: Vec<TaskSessionRow>,
    pub delivery_keys: Vec<DeliveryKeyRow>,
    pub asks: Vec<AskRow>,
    pub usages: Vec<UsageRow>,
    /// Execution attempts (`crate::execution`), each row whole as the board keeps it: schemaless,
    /// so the copy carries what it holds without a column of its own. An attempt's row names its
    /// `attempt`, a fact its `attempt` and `seq`, a fence its `key`, an artifact its `attempt` and
    /// `name`: each its own record's id.
    pub execution_attempts: Vec<Value>,
    pub execution_facts: Vec<Value>,
    pub execution_fences: Vec<Value>,
    pub execution_artifacts: Vec<Value>,
    /// A custom delivery workflow's records, from schema 24; none on a board that never had one,
    /// whose export is then the text it was before, which a binary from before reads.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub workflow: Vec<WorkflowRow>,
}

impl Dump {
    /// A dump as `tasks-copy --board --out` writes it, refused whole if it has a table or a column the copy
    /// does not know, naming it: the copy would drop it.
    pub fn from_json(text: &str) -> Result<Dump, String> {
        let tables: Map<String, Value> = serde_json::from_str(text)
            .map_err(|e| format!("the dump is not a board's JSON: {e}"))?;
        Dump::from_tables(tables)
    }

    /// A dump from its tables, each a list of rows keyed by column, refused as [`Dump::from_json`]
    /// refuses one.
    pub fn from_tables(mut tables: Map<String, Value>) -> Result<Dump, String> {
        let dump = Dump {
            tasks: table(&mut tables, "tasks")?,
            task_links: table(&mut tables, "task_links")?,
            sessions: table(&mut tables, "sessions")?,
            roles: table(&mut tables, "roles")?,
            messages: table(&mut tables, "messages")?,
            deliveries: table(&mut tables, "deliveries")?,
            barred_keys: table(&mut tables, "barred_keys")?,
            task_sessions: table(&mut tables, "task_sessions")?,
            delivery_keys: table(&mut tables, "delivery_keys")?,
            asks: table(&mut tables, "asks")?,
            usages: table(&mut tables, "usages")?,
            execution_attempts: table(&mut tables, "execution_attempts")?,
            execution_facts: table(&mut tables, "execution_facts")?,
            execution_fences: table(&mut tables, "execution_fences")?,
            execution_artifacts: table(&mut tables, "execution_artifacts")?,
            workflow: table(&mut tables, "workflow")?,
        };
        match tables.keys().next() {
            Some(name) => Err(format!(
                "the dump has a table the copy does not know: {name}"
            )),
            None => Ok(dump),
        }
    }
}

/// `name`'s rows, taken out of `tables`: none when the dump has no such table (a board from before it).
/// Read exactly ([`crate::objects::read_exact`]): a row with a field the copy does not know is
/// refused, as the copy would drop it.
fn table<T: DeserializeOwned>(
    tables: &mut Map<String, Value>,
    name: &str,
) -> Result<Vec<T>, String> {
    tables.remove(name).map_or(Ok(Vec::new()), |rows| {
        crate::objects::read_exact(&rows)
            .map_err(|e| format!("the dump's {name} are not rows the copy knows: {e}"))
    })
}

/// Whether `name` is a struct of an open table's rows ([`crate::schema::OPEN`]), by the name its
/// `Deserialize` gives: what the store's reader hands only the fields the struct names, but where
/// it reads exactly ([`crate::objects`]). A match, so each struct's reader, whose name is its own
/// constant, decides it as it is compiled, and only an open table's row names its fields.
#[inline(always)]
pub(crate) fn open_row(name: &str) -> bool {
    matches!(
        name,
        "TaskRow"
            | "LinkRow"
            | "SessionRow"
            | "RoleRow"
            | "MessageRow"
            | "DeliveryRow"
            | "DeliveryKeyRow"
            | "BarredKeyRow"
            | "TaskSessionRow"
            | "AskRow"
            | "UsageRow"
    )
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRow {
    /// SQLite's row id: the order tasks were created in (`sid` on the board, whose `id` is the record's).
    #[serde(alias = "sid")]
    pub id: i64,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub status: String,
    #[serde(default = "two")]
    pub priority: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub created_by: String,
    pub created: f64,
    pub updated: f64,
    /// An instruction subtask: SQLite keeps it as 0 or 1.
    #[serde(default, deserialize_with = "flag")]
    pub instruction: bool,
    /// The plugins' board fields: plugin, then field, then its value; absent until one is set.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fields:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkRow {
    pub slug: String,
    /// `doc` or `dep`.
    pub kind: String,
    pub target: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRow {
    pub handle: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub worktree: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub affinity: String,
    pub connected: f64,
    pub last_seen: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_until: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_call: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tool: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRow {
    pub slug: String,
    pub handle: String,
    /// `orchestrator`, `executor`, `reviewer` or `reserved`.
    pub role: String,
    pub since: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRow {
    #[serde(alias = "sid")]
    pub id: i64,
    pub slug: String,
    pub sender: String,
    /// A handle, `@role`, `owner`, or none for everyone on the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    pub kind: String,
    pub text: String,
    pub ts: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryRow {
    pub message: i64,
    pub handle: String,
    pub acked: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarredKeyRow {
    pub key: String,
}

/// A post made once per key on a thread (`Store::say_keyed`): the message it made, and when. Never a
/// SQLite table; a dump from before it has none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryKeyRow {
    pub slug: String,
    pub key: String,
    pub message: i64,
    pub ts: f64,
}

/// What the owner said that waits on an agent's answer (`Store::open_ask`): one for each thing the
/// owner waits on, open until it is answered, never a task. Never a SQLite table; a dump from before
/// it has none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskRow {
    /// The order asks were opened in: `ask N` (`sid` on the board, whose `id` is the record's).
    #[serde(alias = "sid")]
    pub id: i64,
    /// `verdict`, `message` (a card's), `comment`, `chat` or `task` (a message on a task's thread).
    pub kind: String,
    /// What it is about, as one key: a second line the owner says about it while it is open joins it.
    pub about: String,
    /// The document, for an ask on a card, a comment or a chat about a document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The card's slug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    /// The commented block, as the Symphony names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    /// The worktree's branch the owner said it on (`/@TREE/…`), none on the main checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
    /// The task it is about: a task message's thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The ask store's thread a chat ask is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The owner's first unanswered line, quoted short.
    pub words: String,
    /// When the owner said it: the ask's age runs from here.
    pub asked: f64,
    /// How many of the owner's lines it holds, and when the last one came.
    pub lines: i64,
    pub last: f64,
    /// The session that holds it now, and since when; none is nobody.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<f64>,
    /// Each earlier holder, the first first.
    #[serde(default)]
    pub before: Vec<HeldRow>,
    /// Open until an answer, or a close by hand, ends it.
    pub open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<f64>,
    /// Who answered, when the answer names its session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    /// Where the answer is, or why it was closed by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Closed by hand (`just tasks asks close`), as needing no answer.
    #[serde(default)]
    pub closed: bool,
    /// While open: what it is about is gone, as the Symphony last read it (the card or comment no
    /// longer in its document, the worktree removed, the chat's thread pruned), in these words, and
    /// since when. Gone is no answer: the ask stays open until one comes or a close by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gone_since: Option<f64>,
}

/// One earlier holder of an ask: from when, until it moved on, and why.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldRow {
    pub handle: String,
    pub since: f64,
    pub until: f64,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSessionRow {
    pub slug: String,
    pub handle: String,
    pub role: String,
    pub claude_session: String,
    pub worktree: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub pane: String,
    #[serde(default)]
    pub model: String,
    pub started: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<f64>,
    #[serde(default)]
    pub closed_why: String,
    #[serde(default)]
    pub restore: String,
}

/// The tokens one session spent on one task, in one role, on one model at one speed, in one round
/// (`crate::usage`): what its beats added up. Never a SQLite table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageRow {
    pub handle: String,
    /// The task, "" for calls made while the session worked none.
    pub slug: String,
    /// Its role on the task, else the session's kind.
    pub role: String,
    pub model: String,
    pub speed: String,
    /// 1 and the moves into review the task's thread held then; 0 with no task.
    pub round: i64,
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
    pub calls: u64,
    /// When the first and the last beat added to it.
    pub first: f64,
    pub last: f64,
}

/// A custom delivery workflow's record (`crate::schema`'s `workflow`, from schema 24): the parts
/// of its id, its kind, what it records and when it was written. Its key is its id because a
/// record names what it is of (`['event', slug, revision]`), and the task fence finds a task's
/// transition by it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRow {
    pub key: Value,
    pub kind: String,
    pub body: Map<String, Value>,
    pub at: f64,
}

fn two() -> i64 {
    2
}

/// A boolean as SQLite writes it (0 or 1) or as the board does.
fn flag<'de, D: Deserializer<'de>>(from: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Flag {
        Bool(bool),
        Int(i64),
    }
    Ok(match Flag::deserialize(from)? {
        Flag::Bool(b) => b,
        Flag::Int(i) => i != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dump_from_sqlite_reads_with_its_column_names_and_flags() {
        let dump: Dump = serde_json::from_value(serde_json::json!({
            "tasks": [{"id": 7, "slug": "sim/jump", "title": "Jump", "body": "", "status": "ready", "priority": 1,
                       "parent": null, "created_by": "owner", "created": 1.0, "updated": 2.0, "instruction": 1}],
            "sessions": [{"handle": "amber-heron", "kind": "worker", "parent": null, "worktree": "", "branch": "",
                          "model": "", "pid": null, "host": "", "affinity": "", "connected": 1.0, "last_seen": 2.0,
                          "left_at": null, "agent": null, "busy_until": null, "busy_call": null, "last_tool": null}],
        }))
        .unwrap();
        assert_eq!(dump.tasks[0].id, 7);
        assert!(dump.tasks[0].instruction);
        assert_eq!(dump.tasks[0].parent, None);
        assert_eq!(dump.sessions[0].pid, None);
        assert!(dump.roles.is_empty());
    }

    #[test]
    fn a_board_from_before_the_added_columns_reads_with_their_defaults() {
        let task: TaskRow = serde_json::from_value(serde_json::json!({
            "id": 1, "slug": "a", "title": "A", "body": "", "status": "ready", "priority": 2, "parent": null,
            "created_by": "owner", "created": 1.0, "updated": 1.0}))
        .unwrap();
        assert!(!task.instruction);
        let session: SessionRow = serde_json::from_value(serde_json::json!({
            "handle": "a", "kind": "worker", "parent": null, "worktree": "", "branch": "", "model": "", "pid": 3,
            "host": "h", "affinity": "", "connected": 1.0, "last_seen": 1.0, "left_at": null}))
        .unwrap();
        assert_eq!(
            (session.agent, session.busy_until, session.last_tool),
            (None, None, None)
        );
    }

    #[test]
    fn a_row_written_back_leaves_out_what_it_does_not_have() {
        let role = RoleRow {
            slug: "a".into(),
            handle: "h".into(),
            role: "executor".into(),
            since: 1.0,
            expires: None,
        };
        assert_eq!(
            serde_json::to_value(&role).unwrap(),
            serde_json::json!({"slug": "a", "handle": "h", "role": "executor", "since": 1.0})
        );
    }

    /// Each open table with the struct its rows are read as, in `schema::OPEN`'s order: a table
    /// added there, or a struct renamed, without its line here fails.
    #[test]
    fn the_open_rows_are_the_structs_of_the_open_tables() {
        use std::any::type_name;
        let read_as = [
            ("task", type_name::<TaskRow>()),
            ("link", type_name::<LinkRow>()),
            ("session", type_name::<SessionRow>()),
            ("role", type_name::<RoleRow>()),
            ("message", type_name::<MessageRow>()),
            ("delivery", type_name::<DeliveryRow>()),
            ("delivery_key", type_name::<DeliveryKeyRow>()),
            ("barred_key", type_name::<BarredKeyRow>()),
            ("task_session", type_name::<TaskSessionRow>()),
            ("ask", type_name::<AskRow>()),
            ("usage", type_name::<UsageRow>()),
        ];
        assert_eq!(read_as.map(|(table, _)| table), crate::schema::OPEN);
        for (table, path) in read_as {
            assert!(
                open_row(path.rsplit("::").next().unwrap()),
                "{table}: {path}"
            );
        }
        for closed in [
            "HeldRow",
            "WorkflowRow",
            "Dump",
            "Snapshot",
            "taskrow",
            "TaskRow ",
        ] {
            assert!(!open_row(closed), "{closed}");
        }
    }

    #[test]
    fn a_task_without_a_priority_has_the_boards_default() {
        let task: TaskRow = serde_json::from_value(serde_json::json!({
            "id": 1, "slug": "a", "title": "A", "status": "ready", "created_by": "owner", "created": 1.0, "updated": 1.0}))
        .unwrap();
        assert_eq!((task.priority, task.body.as_str()), (2, ""));
    }

    #[test]
    fn a_time_reads_as_exactly_the_number_python_wrote() {
        let task: TaskRow = serde_json::from_str(
            r#"{"id": 1, "slug": "a", "title": "A", "status": "done", "created_by": "o", "created": 1791264679.9922419,
                "updated": 1791338010.7950459}"#,
        )
        .unwrap();
        assert_eq!(
            (task.created, task.updated),
            (1791264679.9922419, 1791338010.7950459)
        );
    }
}
