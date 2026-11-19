//! The board's definitions as each database holds them, one committed file per engine and schema
//! version (`catalogue/ENGINE-VERSION.txt`), and the check a version declared in
//! `schema::COMPATIBLE` must pass: every table of the version before is defined as it was.
//!
//! A catalogue is one line per definition, `TABLE\tKIND\tNAME\tDEFINITION`, sorted: on SurrealDB
//! each table, field, index and event, and the database's own analyzers, functions and parameters
//! (TABLE empty); on PostgreSQL each column, constraint, index and trigger, and the functions the
//! triggers run (TABLE empty). A declared step may add tables, with their own definitions, and
//! definitions of the database's own; a line of a table that existed before, added, removed or
//! changed, or a database-wide line removed or changed, fails it, naming the table.
//!
//! What no catalogue shows is the reviewer's: a rule held in code that writes to an older table,
//! and a step that rewrites rows (docs/CHECKS.md, a schema step that keeps the Symphony writing).
//!
//! This build's own file is written by running this target with `TASKS_CATALOGUE=write`
//! (`TASKS_CATALOGUE=write just test-rs -p symphony-board --test store catalogue`), once its schema
//! changes; the files of the versions before stay as they were committed.

use crate::common::{Backend, HOST, NOW};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use symphony_board::pg::Step;
use symphony_board::schema::{COMPATIBLE, keeps_writes};
use symphony_board::{Database, Opening, Schema, Store};

/// The board on `db` moved to `schema` by a writer of it.
fn moved(db: &Database, schema: Schema) {
    Store::opening_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::write().at(schema),
    )
    .unwrap();
}

/// The engine's name in a catalogue file's.
fn engine(backend: Backend) -> &'static str {
    match backend {
        Backend::SurrealDb => "surrealdb",
        Backend::PostgreSql => "postgresql",
    }
}

/// The committed catalogue of `version` on `backend`.
fn file(backend: Backend, version: i64) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/store/catalogue")
        .join(format!("{}-{version}.txt", engine(backend)))
}

/// One line of a catalogue, its definition quoted as JSON so it stays on its line.
fn line(table: &str, kind: &str, name: &str, definition: &str) -> String {
    format!("{table}\t{kind}\t{name}\t{}", Value::from(definition))
}

/// The definitions `db`'s board holds, as a catalogue.
fn catalogue(db: &Database) -> String {
    let mut lines = BTreeSet::new();
    match db {
        Database::SurrealDb(client) => {
            let info =
                |sql: &str| -> Value { client.query(sql, json!({})).unwrap().pop().unwrap() };
            let mut database = info("RETURN INFO FOR DB;");
            let tables = database.as_object_mut().unwrap().remove("tables").unwrap();
            for (kind, name, definition) in definitions(&database) {
                lines.insert(line("", kind, name, definition));
            }
            for (table, definition) in tables.as_object().unwrap() {
                lines.insert(line(table, "table", table, definition.as_str().unwrap()));
                let of = info(&format!("RETURN INFO FOR TABLE {table};"));
                for (kind, name, definition) in definitions(&of) {
                    lines.insert(line(table, kind, name, definition));
                }
            }
        }
        Database::PostgreSql(client) => {
            let rows = |sql: &str| -> Vec<Vec<String>> {
                let answer = client
                    .request(&[Step::bare(&format!(
                        "SELECT coalesce(jsonb_agg(row ORDER BY row), '[]')::text FROM ({sql}) AS found (row)"
                    ))])
                    .unwrap();
                serde_json::from_str(answer[0].value().unwrap()).unwrap()
            };
            let here = "n.nspname = current_schema()";
            for row in rows(&format!(
                "SELECT jsonb_build_array(c.table_name, 'column', c.column_name, concat_ws(' ', \
                     c.data_type, CASE WHEN c.is_nullable = 'NO' THEN 'NOT NULL' END, \
                     'DEFAULT ' || c.column_default)) \
                 FROM information_schema.columns c JOIN pg_namespace n ON n.nspname = c.table_schema \
                 WHERE {here}"
            ))
            .into_iter()
            .chain(rows(&format!(
                "SELECT jsonb_build_array(r.relname, 'constraint', o.conname, pg_get_constraintdef(o.oid)) \
                 FROM pg_constraint o JOIN pg_class r ON r.oid = o.conrelid \
                 JOIN pg_namespace n ON n.oid = o.connamespace WHERE {here}"
            )))
            .chain(rows(&format!(
                "SELECT jsonb_build_array(r.relname, 'index', i.relname, pg_get_indexdef(i.oid)) \
                 FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid \
                 JOIN pg_class r ON r.oid = x.indrelid JOIN pg_namespace n ON n.oid = r.relnamespace \
                 WHERE {here}"
            )))
            .chain(rows(&format!(
                "SELECT jsonb_build_array(r.relname, 'trigger', t.tgname, pg_get_triggerdef(t.oid)) \
                 FROM pg_trigger t JOIN pg_class r ON r.oid = t.tgrelid \
                 JOIN pg_namespace n ON n.oid = r.relnamespace WHERE NOT t.tgisinternal AND {here}"
            )))
            .chain(rows(&format!(
                "SELECT jsonb_build_array('', 'function', p.proname, pg_get_functiondef(p.oid)) \
                 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace WHERE {here}"
            ))) {
                lines.insert(line(&row[0], &row[1], &row[2], &row[3]));
            }
        }
    }
    lines.into_iter().map(|line| line + "\n").collect()
}

/// Each definition a SurrealDB `INFO FOR` answer names: its kind, its name and its text.
fn definitions(info: &Value) -> impl Iterator<Item = (&str, &str, &str)> {
    info.as_object()
        .into_iter()
        .flatten()
        .flat_map(|(kind, named)| {
            (named.as_object().into_iter().flatten()).map(move |(name, definition)| {
                (kind.as_str(), name.as_str(), definition.as_str().unwrap())
            })
        })
}

/// A catalogue's lines, by the table each defines ("" for the database's own).
fn by_table(catalogue: &str) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut tables: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for line in catalogue.lines() {
        let table = line.split('\t').next().unwrap();
        tables.entry(table).or_default().insert(line);
    }
    tables
}

/// Whether `after`, a declared version's catalogue, keeps every table of `before`, the version
/// before's, as it was, and every definition of the database's own: each table it changed, and
/// what changed, when not.
fn kept(before: &str, after: &str) -> Result<(), String> {
    let (before, after) = (by_table(before), by_table(after));
    let empty = BTreeSet::new();
    let mut changed = Vec::new();
    for (table, was) in &before {
        let now = after.get(table).unwrap_or(&empty);
        let gone: Vec<&&str> = was.difference(now).collect();
        // The database's own may gain definitions, as tables may be added; a table may not.
        let added: Vec<&&str> = match *table {
            "" => Vec::new(),
            _ => now.difference(was).collect(),
        };
        if !gone.is_empty() || !added.is_empty() {
            let named = if table.is_empty() {
                "the database's own"
            } else {
                table
            };
            changed.push(format!(
                "{named}: removed or changed {gone:?}, added {added:?}"
            ));
        }
    }
    match changed.is_empty() {
        true => Ok(()),
        false => Err(changed.join("\n")),
    }
}

/// A newer build's step on each engine.
struct Twin {
    surreal: &'static str,
    postgres: &'static str,
}

/// Steps that leave every table there before as it was: a table of their own, with its fields and
/// its index, and a definition of the database's own.
const KEEPING: [Twin; 2] = [
    Twin {
        surreal: "DEFINE TABLE IF NOT EXISTS fixture_own SCHEMAFULL;
                  DEFINE FIELD IF NOT EXISTS name ON fixture_own TYPE string;
                  DEFINE INDEX IF NOT EXISTS fixture_own_name ON fixture_own FIELDS name;",
        postgres:
            "CREATE TABLE IF NOT EXISTS fixture_own (id jsonb PRIMARY KEY, name text NOT NULL);
                   CREATE INDEX IF NOT EXISTS fixture_own_name ON fixture_own (name);",
    },
    Twin {
        surreal: "DEFINE ANALYZER IF NOT EXISTS fixture_words TOKENIZERS blank;
                  DEFINE TABLE IF NOT EXISTS fixture_more SCHEMALESS;",
        postgres: "CREATE OR REPLACE FUNCTION fixture_noop() RETURNS void LANGUAGE sql AS 'SELECT';
                   CREATE TABLE IF NOT EXISTS fixture_more (id jsonb PRIMARY KEY, doc jsonb);",
    },
];

/// Steps that each change a table there before, or a definition of the database's own, with the
/// table each check must name: a field or a column added, an assertion changed, an index added,
/// an event or a trigger added, and the analyzer or a function a trigger runs changed.
const CHANGING: [(Twin, &str); 5] = [
    (
        Twin {
            surreal: "DEFINE FIELD IF NOT EXISTS extra ON task TYPE option<string>;",
            postgres: "ALTER TABLE task ADD COLUMN IF NOT EXISTS extra text;",
        },
        "task: ",
    ),
    (
        Twin {
            surreal: "DEFINE FIELD OVERWRITE priority ON task TYPE int ASSERT $value >= 0;",
            postgres: "ALTER TABLE task ADD CONSTRAINT task_priority_known CHECK (priority >= 0);",
        },
        "task: ",
    ),
    (
        Twin {
            surreal: "DEFINE INDEX IF NOT EXISTS message_kind ON message FIELDS kind;",
            postgres: "CREATE INDEX IF NOT EXISTS message_kind ON message (kind);",
        },
        "message: ",
    ),
    (
        Twin {
            surreal: "DEFINE EVENT OVERWRITE fixture_moves ON task WHEN $before.status != $after.status \
                      THEN { THROW 'refused: no move' };",
            postgres: "CREATE OR REPLACE TRIGGER fixture_moves AFTER UPDATE ON task \
                       FOR EACH ROW EXECUTE FUNCTION board_changed();",
        },
        "task: ",
    ),
    (
        Twin {
            surreal: "DEFINE ANALYZER OVERWRITE task_words TOKENIZERS blank FILTERS lowercase;",
            postgres: "CREATE OR REPLACE FUNCTION board_bump() RETURNS void LANGUAGE plpgsql AS \
                       $f$ BEGIN END $f$;",
        },
        "the database's own: ",
    ),
];

/// A board on `backend` made at this build's schema and moved on by `steps`, one migration of them
/// all, and the catalogue of each: before and after.
fn moved_by(backend: Backend, steps: &[&Twin]) -> (String, String) {
    let db = backend.database();
    moved(&db, Schema::this_build());
    let before = catalogue(&db);
    let mut schema = Schema::this_build();
    for step in steps {
        schema = schema.then_each(step.surreal, step.postgres);
    }
    moved(&db, schema.keeping_writes());
    (before, catalogue(&db))
}

on_both! {
    fn this_builds_catalogue_is_its_versions_file_and_a_declared_version_keeps_the_one_before(
        backend: Backend
    ) {
        let db = backend.database();
        moved(&db, Schema::this_build());
        let made = catalogue(&db);
        let version = Schema::this_build().version();
        let path = file(backend, version);
        if std::env::var("TASKS_CATALOGUE").as_deref() == Ok("write") {
            std::fs::write(&path, &made).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|why| {
            panic!(
                "{}: {why}: a schema's definitions are committed once it changes: \
                 TASKS_CATALOGUE=write just test-rs -p symphony-board --test store catalogue",
                path.display()
            )
        });
        // Each way, as a check of one way takes the tables and the database's lines it adds.
        if let Err(changed) = kept(&committed, &made).and_then(|()| kept(&made, &committed)) {
            panic!(
                "{} is not the board this build makes ({changed}); write it again with \
                 TASKS_CATALOGUE=write just test-rs -p symphony-board --test store catalogue",
                path.display()
            );
        }
        // A declared version keeps every table of the one before as that version's file has it.
        if keeps_writes(version) {
            let before = std::fs::read_to_string(file(backend, version - 1)).unwrap();
            if let Err(changed) = kept(&before, &made) {
                panic!("schema {version} is declared in COMPATIBLE but changes: {changed}");
            }
        }
        for (listed, _) in COMPATIBLE {
            assert!(*listed <= version);
        }
    }

    fn the_check_takes_a_step_of_new_tables_and_refuses_every_change_to_one_there_before(
        backend: Backend
    ) {
        // Each step that keeps the tables there before, alone and two at once.
        for steps in [vec![&KEEPING[0]], vec![&KEEPING[1]], vec![&KEEPING[0], &KEEPING[1]]] {
            let (before, after) = moved_by(backend, &steps);
            assert_ne!(before, after, "the step defined something");
            kept(&before, &after).unwrap();
        }
        // Each that changes one, alone and after a step that keeps them, in one move.
        for (step, named) in &CHANGING {
            for steps in [vec![step], vec![&KEEPING[0], step]] {
                let (before, after) = moved_by(backend, &steps);
                let changed = kept(&before, &after).unwrap_err();
                assert!(changed.starts_with(named), "{}: {changed}", step.surreal);
                assert_eq!(changed.lines().count(), 1, "{changed}");
            }
        }
        // A table there before and gone is a change too; one that was not there is new.
        let (before, _) = moved_by(backend, &[&KEEPING[0]]);
        let without: String = (before.lines())
            .filter(|line| !line.starts_with("ask\t"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert!(kept(&before, &without).unwrap_err().starts_with("ask: "));
        kept(&without, &before).unwrap();
    }
}
