//! The board's tables on PostgreSQL, their indexes, and the triggers that are SurrealDB's events:
//! the board's marker and each sender's last word ([`DEFINE`]).
//!
//! Every table keys its rows by `id`, the SurrealDB record's id as JSON (a text, a number, or the
//! parts of a compound id), so a row is named alike on either database ([`super::super::raw`]).
//! A SCHEMAFULL table's fields are typed columns ([`TABLES`]): a required field `NOT NULL`, an
//! `option<…>` one nullable, an object or an array `jsonb`, a field held to a few values checked
//! against them. A SCHEMALESS table holds its fields whole in `doc`. A row is read as SurrealDB
//! gives it, its unset fields left out: `jsonb_strip_nulls(to_jsonb(row)) - 'id'`.
//!
//! The marker is the sum of revision rows that no two writers share, so it moves with every commit
//! a reader can see and is never where two writes meet: each connection's own row,
//! `counter:['board', <its backend's pid>]`, moved once in each of its transactions that changes a
//! board input, and each session's `session_revision`, moved with that session's row as on
//! SurrealDB. A connection runs one transaction at a time, so its row is never written by two at
//! once, and a pid is reused only after its process ended. A sequence, or the highest id of a log,
//! would be no marker: a commit that ends later can hold a lower number than one a reader saw.
//!
//! The deleted search by meaning's three tables and `moved`, the record of the one-time move from
//! SQLite, have no place here: no PostgreSQL board held them.

use std::fmt::Write as _;

/// What a column holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::store) enum Kind {
    Text,
    Int,
    Float,
    Bool,
    /// An object, as `jsonb`.
    Object,
    /// An array, as `jsonb`.
    Array,
}

impl Kind {
    pub(in crate::store) fn sql(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Int => "bigint",
            Kind::Float => "double precision",
            Kind::Bool => "boolean",
            Kind::Object | Kind::Array => "jsonb",
        }
    }

    /// What `jsonb_typeof` says of a value of this kind, for the kinds `jsonb` holds.
    fn json(self) -> Option<&'static str> {
        match self {
            Kind::Object => Some("object"),
            Kind::Array => Some("array"),
            _ => None,
        }
    }
}

/// One field of a SCHEMAFULL table.
#[derive(Clone, Copy, Debug)]
pub(in crate::store) struct Column {
    pub name: &'static str,
    pub kind: Kind,
    /// SurrealDB's `TYPE T` rather than `TYPE option<T>`.
    pub required: bool,
    /// What a row made without it holds.
    pub default: Option<&'static str>,
    /// The values it is held to (`ASSERT $value IN [...]`); none holds it to none.
    pub one_of: &'static [&'static str],
}

const fn req(name: &'static str, kind: Kind) -> Column {
    Column {
        name,
        kind,
        required: true,
        default: None,
        one_of: &[],
    }
}

const fn opt(name: &'static str, kind: Kind) -> Column {
    Column {
        name,
        kind,
        required: false,
        default: None,
        one_of: &[],
    }
}

const fn of(name: &'static str, one_of: &'static [&'static str]) -> Column {
    Column {
        name,
        kind: Kind::Text,
        required: true,
        default: None,
        one_of,
    }
}

const fn defaulted(name: &'static str, kind: Kind, default: &'static str) -> Column {
    Column {
        name,
        kind,
        required: true,
        default: Some(default),
        one_of: &[],
    }
}

/// A table: its fields, or none for a SCHEMALESS one, which holds them whole in `doc`.
#[derive(Clone, Copy, Debug)]
pub(in crate::store) struct Table {
    pub name: &'static str,
    pub columns: &'static [Column],
}

use Kind::{Array, Bool, Float, Int, Object, Text};

/// The board's tables, each in the order of its fields.
pub(in crate::store) const TABLES: [Table; 22] = [
    Table {
        name: "artifact_ledger",
        columns: &[req("revision", Int), req("body", Object)],
    },
    Table {
        name: "task",
        columns: &[
            req("sid", Int),
            req("slug", Text),
            req("title", Text),
            req("body", Text),
            of(
                "status",
                &[
                    "backlog",
                    "concept",
                    "design",
                    "ready",
                    "in_progress",
                    "review",
                    "done",
                    "blocked",
                    "cancelled",
                ],
            ),
            req("priority", Int),
            opt("parent", Text),
            req("created_by", Text),
            req("created", Float),
            req("updated", Float),
            defaulted("instruction", Bool, "false"),
            defaulted("version", Int, "0"),
            opt("fields", Object),
        ],
    },
    Table {
        name: "link",
        columns: &[
            req("slug", Text),
            of("kind", &["doc", "dep"]),
            req("target", Text),
        ],
    },
    Table {
        name: "session",
        columns: &[
            req("handle", Text),
            req("kind", Text),
            opt("parent", Text),
            req("worktree", Text),
            req("branch", Text),
            req("model", Text),
            opt("pid", Int),
            req("host", Text),
            req("affinity", Text),
            req("connected", Float),
            req("last_seen", Float),
            opt("left_at", Float),
            opt("agent", Text),
            opt("busy_until", Float),
            opt("busy_call", Text),
            opt("last_tool", Float),
        ],
    },
    Table {
        name: "session_revision",
        columns: &[req("last", Int), opt("writes", Int)],
    },
    Table {
        name: "role",
        columns: &[
            req("slug", Text),
            req("handle", Text),
            of(
                "role",
                &["orchestrator", "executor", "reviewer", "reserved"],
            ),
            req("since", Float),
            opt("expires", Float),
        ],
    },
    Table {
        name: "message",
        columns: &[
            req("sid", Int),
            req("slug", Text),
            req("sender", Text),
            opt("recipient", Text),
            of("kind", &["system", "msg", "verdict"]),
            req("text", Text),
            req("ts", Float),
        ],
    },
    Table {
        name: "said",
        columns: &[req("sid", Int), req("slug", Text)],
    },
    Table {
        name: "delivery",
        columns: &[
            req("message", Int),
            req("handle", Text),
            req("acked", Float),
        ],
    },
    Table {
        name: "delivery_key",
        columns: &[
            req("slug", Text),
            req("key", Text),
            req("message", Int),
            req("ts", Float),
        ],
    },
    Table {
        name: "barred_key",
        columns: &[req("key", Text)],
    },
    Table {
        name: "task_session",
        columns: &[
            req("slug", Text),
            req("handle", Text),
            req("role", Text),
            req("claude_session", Text),
            req("worktree", Text),
            req("branch", Text),
            req("pane", Text),
            req("model", Text),
            req("started", Float),
            opt("closed", Float),
            req("closed_why", Text),
            req("restore", Text),
        ],
    },
    Table {
        name: "counter",
        columns: &[req("last", Int), opt("ts", Float)],
    },
    Table {
        name: "meta",
        columns: &[req("version", Int)],
    },
    Table {
        name: "writer",
        columns: &[],
    },
    Table {
        name: "callback_state",
        columns: &[],
    },
    Table {
        name: "callback_lease",
        columns: &[],
    },
    Table {
        name: "callback_event",
        columns: &[],
    },
    Table {
        name: "chat_event",
        columns: &[],
    },
    Table {
        name: "unanswered_restore",
        columns: &[],
    },
    Table {
        name: "ask",
        columns: &[
            req("sid", Int),
            of("kind", &["verdict", "message", "comment", "chat", "task"]),
            req("about", Text),
            opt("path", Text),
            opt("card", Text),
            opt("block", Text),
            opt("tree", Text),
            opt("task", Text),
            opt("thread", Text),
            req("words", Text),
            req("asked", Float),
            req("lines", Int),
            req("last", Float),
            opt("holder", Text),
            opt("held", Float),
            req("before", Array),
            req("open", Bool),
            opt("answered", Float),
            opt("answered_by", Text),
            opt("answer", Text),
            req("closed", Bool),
            opt("gone", Text),
            opt("gone_since", Float),
        ],
    },
    Table {
        name: "usage",
        columns: &[
            req("handle", Text),
            req("slug", Text),
            req("role", Text),
            req("model", Text),
            req("speed", Text),
            req("round", Int),
            req("input", Int),
            req("cache_write_5m", Int),
            req("cache_write_1h", Int),
            req("cache_read", Int),
            req("output", Int),
            req("calls", Int),
            req("first", Float),
            req("last", Float),
        ],
    },
];

/// The board's table named `name`, when it is one: a table a newer schema adds is SCHEMALESS here.
pub(in crate::store) fn table(name: &str) -> Option<&'static Table> {
    TABLES.iter().find(|table| table.name == name)
}

/// The indexes SurrealDB's schema defines that a read here uses, the full-text ones left to the
/// search's own slice, and the marker's own.
const INDEXES: &str = "
CREATE INDEX IF NOT EXISTS task_status ON task (status);
CREATE INDEX IF NOT EXISTS task_parent ON task (parent);
CREATE INDEX IF NOT EXISTS link_slug ON link (slug);
CREATE INDEX IF NOT EXISTS link_target ON link (target);
CREATE INDEX IF NOT EXISTS session_agent ON session (agent);
CREATE INDEX IF NOT EXISTS role_slug ON role (slug);
CREATE INDEX IF NOT EXISTS role_handle ON role (handle);
CREATE INDEX IF NOT EXISTS role_role ON role (role);
CREATE INDEX IF NOT EXISTS message_thread ON message (slug, sid);
CREATE INDEX IF NOT EXISTS message_sid ON message (sid);
CREATE INDEX IF NOT EXISTS message_ts ON message (ts);
CREATE INDEX IF NOT EXISTS message_recipient ON message (recipient, sid);
CREATE INDEX IF NOT EXISTS message_sender ON message (sender, kind, sid, slug);
CREATE INDEX IF NOT EXISTS delivery_handle ON delivery (handle, message);
CREATE INDEX IF NOT EXISTS task_session_slug ON task_session (slug);
CREATE INDEX IF NOT EXISTS task_session_branch ON task_session (branch);
CREATE INDEX IF NOT EXISTS counter_board ON counter (id) WHERE id ->> 0 = 'board';
CREATE INDEX IF NOT EXISTS callback_state_address ON callback_state ((doc ->> 'address'));
CREATE INDEX IF NOT EXISTS callback_event_handle ON callback_event ((doc ->> 'handle'), (doc -> 'delivered'));
CREATE INDEX IF NOT EXISTS ask_open ON ask (open, asked, sid);
CREATE INDEX IF NOT EXISTS ask_about ON ask (about, open);
CREATE INDEX IF NOT EXISTS ask_answered ON ask (answered);
CREATE INDEX IF NOT EXISTS usage_slug ON usage (slug);
CREATE INDEX IF NOT EXISTS usage_handle ON usage (handle);
";

/// The functions every statement and trigger here shares, and the triggers.
///
/// `board_set(target, path, value)`: `target` with `path` set to `value`, each object on the way
/// made where it is not one (`UPDATE … SET a.b.c = $x`), and the field removed when `value` is
/// null (`= NONE`).
///
/// `board_bump()`: the marker moved for this transaction, once, on this connection's own row.
/// A transaction-local setting says it moved, so a write of many rows moves it once.
const FUNCTIONS: &str = r"
CREATE OR REPLACE FUNCTION board_set(target jsonb, path text[], value jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE AS $f$
DECLARE
    holder jsonb := CASE WHEN jsonb_typeof(target) = 'object' THEN target ELSE '{}'::jsonb END;
BEGIN
    IF value IS NULL THEN
        RETURN CASE WHEN target IS NULL THEN NULL ELSE target #- path END;
    END IF;
    IF cardinality(path) = 0 THEN
        RETURN value;
    END IF;
    RETURN holder || jsonb_build_object(path[1], board_set(holder -> path[1], path[2:], value));
END
$f$;

CREATE OR REPLACE FUNCTION board_bump() RETURNS void LANGUAGE plpgsql AS $f$
BEGIN
    IF current_setting('board.bumped', true) IS DISTINCT FROM 'yes' THEN
        PERFORM set_config('board.bumped', 'yes', true);
        INSERT INTO counter (id, last) VALUES (jsonb_build_array('board', pg_backend_pid()), 1)
            ON CONFLICT (id) DO UPDATE SET last = counter.last + 1;
    END IF;
END
$f$;

CREATE OR REPLACE FUNCTION board_changed() RETURNS trigger LANGUAGE plpgsql AS $f$
BEGIN
    PERFORM board_bump();
    RETURN NULL;
END
$f$;

CREATE OR REPLACE FUNCTION board_session_changed() RETURNS trigger LANGUAGE plpgsql AS $f$
DECLARE
    revision jsonb := CASE WHEN TG_OP = 'DELETE' THEN OLD.id ELSE NEW.id END;
BEGIN
    INSERT INTO session_revision (id, last) VALUES (revision, 1)
        ON CONFLICT (id) DO UPDATE SET last = session_revision.last + 1;
    RETURN NULL;
END
$f$;

CREATE OR REPLACE FUNCTION board_callback_fields(doc jsonb) RETURNS jsonb
LANGUAGE sql IMMUTABLE AS $f$
    SELECT jsonb_build_array(doc -> 'address', doc -> 'owner', doc -> 'host', doc -> 'chat',
        doc -> 'chat_fallback', doc -> 'chat_resume', doc -> 'confirmed_until', doc -> 'in_process',
        doc -> 'seen', doc -> 'pull', doc -> 'registration_revision')
$f$;

CREATE OR REPLACE FUNCTION board_callback_changed() RETURNS trigger LANGUAGE plpgsql AS $f$
DECLARE
    before jsonb := CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE OLD.doc END;
    after jsonb := CASE WHEN TG_OP = 'DELETE' THEN NULL ELSE NEW.doc END;
BEGIN
    IF board_callback_fields(before) IS DISTINCT FROM board_callback_fields(after) THEN
        PERFORM board_bump();
    END IF;
    RETURN NULL;
END
$f$;

CREATE OR REPLACE FUNCTION board_said() RETURNS trigger LANGUAGE plpgsql AS $f$
DECLARE
    was record;
    newest record;
BEGIN
    IF TG_OP = 'UPDATE' AND (OLD.sender, OLD.sid, OLD.slug, OLD.kind IN ('msg', 'verdict'))
        IS NOT DISTINCT FROM (NEW.sender, NEW.sid, NEW.slug, NEW.kind IN ('msg', 'verdict')) THEN
        RETURN NULL;
    END IF;
    IF TG_OP <> 'INSERT' THEN
        SELECT s.sid, s.slug INTO was FROM said s WHERE s.id = jsonb_build_array(OLD.sender);
        IF FOUND AND was.sid = OLD.sid AND was.slug = OLD.slug THEN
            SELECT m.sid, m.slug INTO newest FROM message m
                WHERE m.sender = OLD.sender AND m.kind IN ('msg', 'verdict')
                ORDER BY m.sid DESC, m.slug DESC LIMIT 1;
            IF FOUND THEN
                UPDATE said SET sid = newest.sid, slug = newest.slug
                    WHERE id = jsonb_build_array(OLD.sender);
            ELSE
                DELETE FROM said WHERE id = jsonb_build_array(OLD.sender);
            END IF;
        END IF;
    END IF;
    IF TG_OP <> 'DELETE' AND NEW.kind IN ('msg', 'verdict') THEN
        INSERT INTO said (id, sid, slug) VALUES (jsonb_build_array(NEW.sender), NEW.sid, NEW.slug)
            ON CONFLICT (id) DO UPDATE SET sid = EXCLUDED.sid, slug = EXCLUDED.slug
            WHERE (said.sid, said.slug) < (EXCLUDED.sid, EXCLUDED.slug);
    END IF;
    RETURN NULL;
END
$f$;
";

/// The tables whose every change moves the marker, as SurrealDB's `board_changed` events do;
/// `session` and `callback_state` move it by triggers of their own.
const MARKED: [&str; 8] = [
    "task",
    "role",
    "link",
    "message",
    "delivery",
    "task_session",
    "chat_event",
    "ask",
];

/// The triggers: the marker on every board input, a session's own revision, a callback
/// registration's fields that a reader reads, and each sender's last word.
fn triggers() -> String {
    let mut sql = String::new();
    for table in MARKED {
        let _ = writeln!(
            sql,
            "CREATE OR REPLACE TRIGGER {table}_made AFTER INSERT OR DELETE ON {table} \
             FOR EACH ROW EXECUTE FUNCTION board_changed();\n\
             CREATE OR REPLACE TRIGGER {table}_changed AFTER UPDATE ON {table} \
             FOR EACH ROW WHEN (OLD.* IS DISTINCT FROM NEW.*) EXECUTE FUNCTION board_changed();"
        );
    }
    sql.push_str(
        "CREATE OR REPLACE TRIGGER session_made AFTER INSERT OR DELETE ON session \
         FOR EACH ROW EXECUTE FUNCTION board_session_changed();
CREATE OR REPLACE TRIGGER session_changed AFTER UPDATE ON session \
         FOR EACH ROW WHEN (OLD.* IS DISTINCT FROM NEW.*) EXECUTE FUNCTION board_session_changed();
CREATE OR REPLACE TRIGGER callback_state_changed AFTER INSERT OR UPDATE OR DELETE ON callback_state \
         FOR EACH ROW EXECUTE FUNCTION board_callback_changed();
CREATE OR REPLACE TRIGGER said AFTER INSERT OR UPDATE OR DELETE ON message \
         FOR EACH ROW EXECUTE FUNCTION board_said();
",
    );
    sql
}

/// `table`'s definition.
fn create(table: &Table) -> String {
    let mut sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (\n    id jsonb PRIMARY KEY",
        table.name
    );
    if table.columns.is_empty() {
        sql.push_str(",\n    doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object')");
    }
    for column in table.columns {
        let _ = write!(sql, ",\n    {} {}", column.name, column.kind.sql());
        if let Some(default) = column.default {
            let _ = write!(sql, " DEFAULT {default}");
        }
        if column.required {
            sql.push_str(" NOT NULL");
        }
        if !column.one_of.is_empty() {
            let values: Vec<String> = column.one_of.iter().map(|v| format!("'{v}'")).collect();
            let _ = write!(sql, " CHECK ({} IN ({}))", column.name, values.join(", "));
        }
        if let Some(json) = column.kind.json() {
            let _ = write!(sql, " CHECK (jsonb_typeof({}) = '{json}')", column.name);
        }
    }
    sql.push_str("\n);\n");
    sql
}

/// The uniques SurrealDB's `UNIQUE` indexes keep.
const UNIQUES: &str = "
CREATE UNIQUE INDEX IF NOT EXISTS task_sid ON task (sid);
CREATE UNIQUE INDEX IF NOT EXISTS task_slug ON task (slug);
CREATE UNIQUE INDEX IF NOT EXISTS ask_sid ON ask (sid);
";

/// This build's steps on PostgreSQL: every table, index, function and trigger, each made if it is
/// not there or replaced, so the steps run again on a board that has them change nothing.
pub(in crate::store) fn define() -> String {
    let mut sql = String::new();
    for table in &TABLES {
        sql.push_str(&create(table));
    }
    sql.push_str(UNIQUES);
    sql.push_str(INDEXES);
    sql.push_str(FUNCTIONS);
    sql.push_str(&triggers());
    sql
}

/// The board's marker: every connection's revision and every session's, summed.
pub(in crate::store) const MARKER: &str =
    "((SELECT coalesce(sum(last), 0) FROM counter WHERE id ->> 0 = 'board')
    + (SELECT coalesce(sum(last), 0) FROM session_revision))";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tables_fields_are_its_typed_columns_and_a_schemaless_one_holds_a_doc() {
        let role = create(table("role").unwrap());
        assert_eq!(
            role,
            "CREATE TABLE IF NOT EXISTS role (
    id jsonb PRIMARY KEY,
    slug text NOT NULL,
    handle text NOT NULL,
    role text NOT NULL CHECK (role IN ('orchestrator', 'executor', 'reviewer', 'reserved')),
    since double precision NOT NULL,
    expires double precision
);
"
        );
        assert_eq!(
            create(table("writer").unwrap()),
            "CREATE TABLE IF NOT EXISTS writer (
    id jsonb PRIMARY KEY,
    doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object')
);
"
        );
        let task = create(table("task").unwrap());
        assert!(task.contains("version bigint DEFAULT 0 NOT NULL"), "{task}");
        assert!(
            task.contains("fields jsonb CHECK (jsonb_typeof(fields) = 'object')"),
            "{task}"
        );
        let ask = create(table("ask").unwrap());
        assert!(
            ask.contains("before jsonb NOT NULL CHECK (jsonb_typeof(before) = 'array')"),
            "{ask}"
        );
        assert!(table("moved").is_none() && table("task_chunk").is_none());
    }

    #[test]
    fn the_board_has_twenty_one_tables_each_named_once() {
        let mut names: Vec<&str> = TABLES.iter().map(|table| table.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 21);
        let defined = define();
        for name in names {
            assert!(
                defined.contains(&format!("CREATE TABLE IF NOT EXISTS {name} (")),
                "{name}"
            );
        }
        for table in MARKED {
            assert!(
                defined.contains(&format!("TRIGGER {table}_changed")),
                "{table}"
            );
        }
    }
}
