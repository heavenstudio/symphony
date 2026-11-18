//! The board's tables on PostgreSQL, their indexes, and the triggers that are SurrealDB's events:
//! the board's marker and each sender's last word ([`define`]); and the terms SurrealDB's
//! full-text indexes hold, which triggers keep too ([`WORDS`]).
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

/// The board's tables, including `words` (the full-text indexes, [`WORDS`]) and `copied_fields`
/// (the imported fields' numeric spellings, [`super::copy`]).
pub(in crate::store) const TABLES: [Table; 24] = [
    Table {
        name: "artifact_ledger",
        columns: &[req("revision", Int), req("body", Object)],
    },
    Table {
        name: "copied_fields",
        columns: &[req("original", Text)],
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
    Table {
        name: "words",
        columns: &[
            req("source", Text),
            req("slug", Text),
            req("terms", Object),
            req("length", Int),
        ],
    },
];

/// The board's table named `name`, when it is one: a table a newer schema adds is SCHEMALESS here.
pub(in crate::store) fn table(name: &str) -> Option<&'static Table> {
    TABLES.iter().find(|table| table.name == name)
}

/// The indexes SurrealDB's schema defines that a read here uses, the full-text ones as one index
/// of the terms in `words`, and the marker's own.
const INDEXES: &str = "
CREATE INDEX IF NOT EXISTS words_terms ON words USING gin (terms);
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

-- Patch JSON without converting unchanged siblings through jsonb. Assigned arrays/objects
-- replace their whole subtree, so numeric forms cannot migrate between array positions.
CREATE OR REPLACE FUNCTION board_saved_set(target json, path text[], value json) RETURNS json
LANGUAGE plpgsql IMMUTABLE AS $f$
DECLARE
    holder json := CASE WHEN json_typeof(target) = 'object' THEN target ELSE '{}'::json END;
    child json;
BEGIN
    IF cardinality(path) = 0 THEN RETURN value; END IF;
    child := board_saved_set(holder -> path[1], path[2:], value);
    RETURN (SELECT coalesce(json_object_agg(key, item), '{}'::json) FROM (
        SELECT key, entry.value AS item FROM json_each(holder) AS entry WHERE key <> path[1]
        UNION ALL SELECT path[1], child WHERE child IS NOT NULL
    ) AS entries);
END
$f$;

-- All task-field writes keep imported text at the exact path they assign. Explicit values
-- come from the original JSON variables, before jsonb loses exponent and negative-zero forms.
CREATE OR REPLACE FUNCTION board_fields(task_id jsonb, target jsonb, path text[], value jsonb,
    written json DEFAULT NULL) RETURNS jsonb LANGUAGE plpgsql VOLATILE AS $f$
BEGIN
    IF written IS NULL AND (target #> path) IS NOT DISTINCT FROM value THEN
        RETURN target;
    END IF;
    IF cardinality(path) = 0 AND value IS NULL THEN
        UPDATE copied_fields SET original = '{}' WHERE id = task_id;
    ELSE
        UPDATE copied_fields SET original = board_saved_set(original::json, path,
            coalesce(written, value::json))::text WHERE id = task_id;
    END IF;
    IF cardinality(path) = 0 THEN RETURN value; END IF;
    RETURN board_set(target, path, value);
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

/// What SurrealDB's full-text indexes hold (`task_words`, [`crate::schema::ANALYZERS`]): each
/// task's title, brief and slug and each thread line's text, a row of `words` each, its `terms`
/// counted and their number its `length`, kept by the triggers `board_words` names in each
/// table's fields it reads. The search's twins read it ([`super::search`]).
///
/// `board_terms(input)`: the analyzer, each term with where it is. SurrealDB's tokenizers split a
/// text at each change of class (a letter, a number, ASCII punctuation one character a term),
/// dropping the rest; its filters then lower each term and make it ASCII (`deunicode`). Here a
/// letter is PostgreSQL's (`[[:alpha:]]`, Unicode's alphabetic, as Rust's `is_alphabetic`, a newer
/// Unicode's letters aside), a number Rust's own ([`number_class`]), and a term outside
/// ASCII is decomposed (NFKD) with its accents dropped and the Latin letters that have no
/// decomposition spelt as `deunicode` spells them; another script is kept as it is, where
/// `deunicode` spells it in Latin letters.
const WORDS: &str = r"
CREATE OR REPLACE FUNCTION board_terms(input text) RETURNS TABLE (term text, at bigint)
LANGUAGE sql IMMUTABLE STRICT AS $f$
SELECT term, at FROM (SELECT CASE WHEN octet_length(m[1]) = char_length(m[1]) THEN m[1]
    ELSE translate(regexp_replace(normalize(LATIN, NFKD), '[̀-ͯ]', '', 'g'), '⁄', '/') END AS term, at
    FROM regexp_matches(lower(input COLLATE pg_c_utf8), '[[:alpha:]]+|[NUMBERS]+|[!-/:-@[-`{-~]', 'g')
        WITH ORDINALITY AS t (m, at)) t
WHERE term <> ''
$f$;

CREATE OR REPLACE FUNCTION board_words() RETURNS trigger LANGUAGE plpgsql AS $f$
BEGIN
    IF TG_OP <> 'INSERT' THEN
        DELETE FROM words WHERE id IN (SELECT jsonb_build_array(f, OLD.id) FROM unnest(TG_ARGV) f);
    END IF;
    IF TG_OP <> 'DELETE' THEN
        INSERT INTO words (id, source, slug, terms, length)
        SELECT jsonb_build_array(f, NEW.id), f, NEW.slug,
            coalesce(jsonb_object_agg(term, n) FILTER (WHERE n > 0), '{}'), coalesce(sum(n), 0)
        FROM unnest(TG_ARGV) f LEFT JOIN LATERAL
            (SELECT term, count(*) AS n FROM board_terms(to_jsonb(NEW) ->> f) GROUP BY term) w ON true
        GROUP BY f;
    END IF;
    RETURN NULL;
END
$f$;
";

/// Each table `words` holds the terms of, and its fields that it reads.
const WORDED: [(&str, &[&str]); 2] = [("task", &["title", "body", "slug"]), ("message", &["text"])];

/// Latin exceptions to NFKD with accents removed, grouped by their ASCII spelling. The data
/// is from deunicode 1.6.2 (BSD-3-Clause, https://github.com/kornelski/deunicode), the analyzer SurrealDB
/// 3.3 uses: each named Latin letter whose spelling differs from NFKD, plus the unit sign µ.
/// Apply these to the original letters, before decomposition: e.g. ǿ is o, ŉ is 'n (not ʼn).
/// Its license is kept in `latin.LICENSE`. No database extension or full transliteration
/// library is linked into the board.
fn latin() -> String {
    let (mut from, mut to) = (String::new(), String::new());
    let mut expanded = Vec::new();
    for line in include_str!("latin.tsv").lines() {
        let (ascii, letters) = line
            .split_once('\t')
            .expect("Latin mapping is tab separated");
        if ascii.len() == 1 {
            from.push_str(letters);
            to.extend(std::iter::repeat_n(
                ascii.chars().next().unwrap(),
                letters.chars().count(),
            ));
        } else {
            expanded.push((ascii, letters));
        }
    }
    let quoted = |s: &str| s.replace('\'', "''");
    let mut folded = format!("translate(m[1], '{}', '{}')", quoted(&from), quoted(&to));
    for (ascii, letters) in expanded {
        for c in letters.chars() {
            folded = format!("replace({folded}, '{c}', '{}')", quoted(ascii));
        }
    }
    folded
}

/// The tokenizer's numeric class from the pinned toolchain, kept as data for schema construction.
/// The full regeneration test checks every Unicode scalar when the toolchain changes.
const NUMBER_RANGES: &str = concat!(
    "\u{30}-\u{39}\u{b2}-\u{b3}\u{b9}\u{bc}-\u{be}\u{660}-\u{669}\u{6f0}-\u{6f9}\u{7c0}-\u{7c9}",
    "\u{966}-\u{96f}\u{9e6}-\u{9ef}\u{9f4}-\u{9f9}\u{a66}-\u{a6f}\u{ae6}-\u{aef}\u{b66}-\u{b6f}",
    "\u{b72}-\u{b77}\u{be6}-\u{bf2}\u{c66}-\u{c6f}\u{c78}-\u{c7e}\u{ce6}-\u{cef}\u{d58}-\u{d5e}",
    "\u{d66}-\u{d78}\u{de6}-\u{def}\u{e50}-\u{e59}\u{ed0}-\u{ed9}\u{f20}-\u{f33}\u{1040}-",
    "\u{1049}\u{1090}-\u{1099}\u{1369}-\u{137c}\u{17e0}-\u{17e9}\u{17f0}-\u{17f9}\u{1810}-",
    "\u{1819}\u{1946}-\u{194f}\u{19d0}-\u{19da}\u{1a80}-\u{1a89}\u{1a90}-\u{1a99}\u{1b50}-",
    "\u{1b59}\u{1bb0}-\u{1bb9}\u{1c40}-\u{1c49}\u{1c50}-\u{1c59}\u{2070}\u{2074}-\u{2079}",
    "\u{2080}-\u{2089}\u{2150}-\u{215f}\u{2189}\u{2460}-\u{249b}\u{24ea}-\u{24ff}\u{2776}-",
    "\u{2793}\u{2cfd}\u{3192}-\u{3195}\u{3220}-\u{3229}\u{3248}-\u{324f}\u{3251}-\u{325f}",
    "\u{3280}-\u{3289}\u{32b1}-\u{32bf}\u{a620}-\u{a629}\u{a830}-\u{a835}\u{a8d0}-\u{a8d9}",
    "\u{a900}-\u{a909}\u{a9d0}-\u{a9d9}\u{a9f0}-\u{a9f9}\u{aa50}-\u{aa59}\u{abf0}-\u{abf9}",
    "\u{ff10}-\u{ff19}\u{10107}-\u{10133}\u{10175}-\u{10178}\u{1018a}-\u{1018b}\u{102e1}-",
    "\u{102fb}\u{10320}-\u{10323}\u{104a0}-\u{104a9}\u{10858}-\u{1085f}\u{10879}-\u{1087f}",
    "\u{108a7}-\u{108af}\u{108fb}-\u{108ff}\u{10916}-\u{1091b}\u{109bc}-\u{109bd}\u{109c0}-",
    "\u{109cf}\u{109d2}-\u{109ff}\u{10a40}-\u{10a48}\u{10a7d}-\u{10a7e}\u{10a9d}-\u{10a9f}",
    "\u{10aeb}-\u{10aef}\u{10b58}-\u{10b5f}\u{10b78}-\u{10b7f}\u{10ba9}-\u{10baf}\u{10cfa}-",
    "\u{10cff}\u{10d30}-\u{10d39}\u{10d40}-\u{10d49}\u{10e60}-\u{10e7e}\u{10f1d}-\u{10f26}",
    "\u{10f51}-\u{10f54}\u{10fc5}-\u{10fcb}\u{11052}-\u{1106f}\u{110f0}-\u{110f9}\u{11136}-",
    "\u{1113f}\u{111d0}-\u{111d9}\u{111e1}-\u{111f4}\u{112f0}-\u{112f9}\u{11450}-\u{11459}",
    "\u{114d0}-\u{114d9}\u{11650}-\u{11659}\u{116c0}-\u{116c9}\u{116d0}-\u{116e3}\u{11730}-",
    "\u{1173b}\u{118e0}-\u{118f2}\u{11950}-\u{11959}\u{11bf0}-\u{11bf9}\u{11c50}-\u{11c6c}",
    "\u{11d50}-\u{11d59}\u{11da0}-\u{11da9}\u{11de0}-\u{11de9}\u{11f50}-\u{11f59}\u{11fc0}-",
    "\u{11fd4}\u{16130}-\u{16139}\u{16a60}-\u{16a69}\u{16ac0}-\u{16ac9}\u{16b50}-\u{16b59}",
    "\u{16b5b}-\u{16b61}\u{16d70}-\u{16d79}\u{16e80}-\u{16e96}\u{1ccf0}-\u{1ccf9}\u{1d2c0}-",
    "\u{1d2d3}\u{1d2e0}-\u{1d2f3}\u{1d360}-\u{1d378}\u{1d7ce}-\u{1d7ff}\u{1e140}-\u{1e149}",
    "\u{1e2f0}-\u{1e2f9}\u{1e4f0}-\u{1e4f9}\u{1e5f1}-\u{1e5fa}\u{1e8c7}-\u{1e8cf}\u{1e950}-",
    "\u{1e959}\u{1ec71}-\u{1ecab}\u{1ecad}-\u{1ecaf}\u{1ecb1}-\u{1ecb4}\u{1ed01}-\u{1ed2d}",
    "\u{1ed2f}-\u{1ed3d}\u{1f100}-\u{1f10c}\u{1fbf0}-\u{1fbf9}",
);

/// Expand the UTF-8 endpoints and range separators into PostgreSQL's hexadecimal class.
fn number_class() -> String {
    let mut class = String::new();
    for number in NUMBER_RANGES.chars() {
        if number == '-' {
            class.push('-');
        } else {
            let _ = write!(class, "\\x{:x}", u32::from(number));
        }
    }
    class
}

#[cfg(test)]
fn numbers() -> String {
    let mut class = String::new();
    let mut from = None;
    for at in 0..=0x11_0000 {
        let number = char::from_u32(at).is_some_and(|c| c.is_numeric() && !c.is_alphabetic());
        match from {
            None if number => from = Some(at),
            Some(start) if !number => {
                let _ = write!(class, "\\x{start:x}");
                if at - 1 > start {
                    let _ = write!(class, "-\\x{:x}", at - 1);
                }
                from = None;
            }
            _ => {}
        }
    }
    class
}

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
/// registration's fields that a reader reads, each sender's last word, and the terms of each
/// text searched.
fn triggers() -> String {
    let mut sql = String::new();
    for (table, fields) in WORDED {
        // A row's terms are counted again when a field read changes, or the task it is of.
        let slug = (!fields.contains(&"slug")).then_some(&"slug");
        let read = |row: &str| {
            let read: Vec<String> = (fields.iter().chain(slug))
                .map(|field| format!("{row}.{field}"))
                .collect();
            read.join(", ")
        };
        let named: Vec<String> = fields.iter().map(|field| format!("'{field}'")).collect();
        let (named, old, new) = (named.join(", "), read("OLD"), read("NEW"));
        let _ = writeln!(
            sql,
            "CREATE OR REPLACE TRIGGER {table}_words AFTER INSERT OR DELETE ON {table} \
             FOR EACH ROW EXECUTE FUNCTION board_words({named});\n\
             CREATE OR REPLACE TRIGGER {table}_words_changed AFTER UPDATE ON {table} \
             FOR EACH ROW WHEN (({old}) IS DISTINCT FROM ({new})) \
             EXECUTE FUNCTION board_words({named});"
        );
    }
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
        "CREATE OR REPLACE FUNCTION board_uncopied_fields() RETURNS trigger LANGUAGE plpgsql AS $f$
BEGIN
    DELETE FROM copied_fields WHERE id = OLD.id;
    RETURN NULL;
END
$f$;
CREATE OR REPLACE TRIGGER task_uncopied_fields AFTER DELETE ON task
    FOR EACH ROW EXECUTE FUNCTION board_uncopied_fields();
CREATE OR REPLACE TRIGGER session_made AFTER INSERT OR DELETE ON session \
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
    static ANALYZED: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        WORDS
            .replace("NUMBERS", &number_class())
            .replace("LATIN", &latin())
    });
    sql.push_str(&ANALYZED);
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
        assert_eq!(
            create(table("artifact_ledger").unwrap()),
            "CREATE TABLE IF NOT EXISTS artifact_ledger (
    id jsonb PRIMARY KEY,
    revision bigint NOT NULL,
    body jsonb NOT NULL CHECK (jsonb_typeof(body) = 'object')
);
"
        );
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

    /// The numbers are Rust's, ranges of them and single ones, and no letter among them.
    #[test]
    fn the_analyzers_numbers_are_rusts_that_are_no_letters() {
        let class = numbers();
        assert_eq!(
            number_class(),
            class,
            "regenerate numeric-class data for this toolchain"
        );
        assert!(
            class.starts_with("\\x30-\\x39\\xb2-\\xb3\\xb9\\xbc-\\xbe\\x660-\\x669"),
            "{class}"
        );
        let mut held = 0;
        for range in class.split("\\x").filter(|at| !at.is_empty()) {
            let at = u32::from_str_radix(range.trim_end_matches('-'), 16).unwrap();
            let c = char::from_u32(at).unwrap();
            assert!(c.is_numeric() && !c.is_alphabetic(), "{c:?}");
            held += 1;
        }
        assert!(held > 200, "{held}");
        for letter in ['Ⅳ', 'a', 'µ'] {
            assert!(!class.contains(&format!("\\x{:x}", u32::from(letter))));
        }
    }

    #[test]
    fn the_boards_tables_words_and_copied_fields_are_each_named_once() {
        let mut names: Vec<&str> = TABLES.iter().map(|table| table.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 24);
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
        assert!(defined.contains(
            "TRIGGER task_words_changed AFTER UPDATE ON task FOR EACH ROW WHEN \
             ((OLD.title, OLD.body, OLD.slug) IS DISTINCT FROM \
             (NEW.title, NEW.body, NEW.slug)) EXECUTE FUNCTION \
             board_words('title', 'body', 'slug');"
        ));
        assert!(defined.contains(
            "WHEN ((OLD.text, OLD.slug) IS DISTINCT FROM (NEW.text, NEW.slug)) \
             EXECUTE FUNCTION board_words('text');"
        ));
        assert!(!defined.contains("NUMBERS") && defined.contains("|[\\x30-\\x39"));
    }
}
