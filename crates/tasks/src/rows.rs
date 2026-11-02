//! The board's rows: one struct per table of the SQLite board the board moved from, its fields the
//! table's columns, as the move reads them ([`crate::cutover`]) and `tasks-copy --board --out` writes
//! them. The copy reads them ([`crate::Store::copy`]) and the tests seed
//! the board with them. A column added to a board after it was first written (`instruction`, the
//! liveness columns) may be missing from an old dump, and reads as its default; a table or a column
//! the copy does not know is refused by name ([`Dump::from_json`]), never dropped.

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
}

impl Dump {
    /// A dump as `tasks-copy --board --out` writes it, refused whole if it has a table or a column the copy
    /// does not know, naming it: the copy would drop it.
    pub fn from_json(text: &str) -> Result<Dump, String> {
        let tables: Map<String, Value> = serde_json::from_str(text)
            .map_err(|e| format!("the dump is not a board's JSON: {e}"))?;
        Dump::from_tables(tables)
    }

    /// A dump from its tables, each a list of rows keyed by column (the SQLite board's, read by the
    /// move), refused as [`Dump::from_json`] refuses one.
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
fn table<T: DeserializeOwned>(
    tables: &mut Map<String, Value>,
    name: &str,
) -> Result<Vec<T>, String> {
    tables.remove(name).map_or(Ok(Vec::new()), |rows| {
        serde_json::from_value(rows)
            .map_err(|e| format!("the dump's {name} are not rows the copy knows: {e}"))
    })
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRow {
    /// SQLite's row id: the order tasks were created in (`sid` in SurrealDB, whose `id` is the record's).
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

fn two() -> i64 {
    2
}

/// A boolean as SQLite writes it (0 or 1) or as SurrealDB does.
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
