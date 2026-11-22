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
//! and a step that rewrites rows (docs/workflow/CHECKS.md, a schema step that keeps the Symphony writing).
//!
//! An additive step on a contract ([`ADDED`]) has a file of its own,
//! `ENGINE-VERSION.STEPS.txt`, the contract with that many steps, and is held to more than a
//! declared version is ([`additive`]): a table there before gains only an index that is not unique
//! and, on an open table ([`OPEN`]), an optional field of its own; the tables it adds are
//! the ones it declares; and a board with rows in it moved by the step keeps every row as it was
//! ([`step_checked`]).
//!
//! This build's own file is written by running this target with `TASKS_CATALOGUE=write`
//! (`TASKS_CATALOGUE=write just test-rs -p symphony-board --test store catalogue`), once its schema
//! changes; the files of the versions and steps before stay as they were committed.

use crate::common::{Backend, HOST, NOW};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use symphony_board::pg::Step;
use symphony_board::schema::{ADDED, ADDITIONS, COMPATIBLE, OPEN, VERSION, keeps_writes};
use symphony_board::{Database, NewSession, NewTask, Opening, Raw, Schema, Store};

/// The board on `db` moved to `schema` by a writer of it.
fn moved(db: &Database, schema: Schema) {
    writer(db, schema);
}

/// A writer of `schema` on the board on `db`, which it moved there.
fn writer(db: &Database, schema: Schema) -> Store {
    Store::opening_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::write().at(schema),
    )
    .unwrap()
}

/// The engine's name in a catalogue file's.
fn engine(backend: Backend) -> &'static str {
    match backend {
        Backend::SurrealDb => "surrealdb",
        Backend::PostgreSql => "postgresql",
    }
}

/// The committed catalogue of `version` with `steps` additive steps on it on `backend`.
fn file(backend: Backend, version: i64, steps: i64) -> PathBuf {
    let name = match steps {
        0 => format!("{}-{version}.txt", engine(backend)),
        steps => format!("{}-{version}.{steps}.txt", engine(backend)),
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/store/catalogue")
        .join(name)
}

/// The committed catalogue of `version` with every additive step it had, the last of its files.
fn last_file(backend: Backend, version: i64) -> PathBuf {
    let mut steps = 0;
    while file(backend, version, steps + 1).exists() {
        steps += 1;
    }
    file(backend, version, steps)
}

/// The text of a committed catalogue, which is there.
fn committed(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|why| {
        panic!(
            "{}: {why}: a schema's definitions are committed once it changes: \
             TASKS_CATALOGUE=write just test-rs -p symphony-board --test store catalogue",
            path.display()
        )
    })
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

/// Whether `after`'s catalogue is `before`'s with an additive step: the tables it adds, when it
/// is; each table it changed as no additive step may, and how, when not. A table of its own takes
/// any definition and the database's own may gain definitions, as for a declared version
/// ([`kept`]); a table there before gains only what [`may_add`] says, and loses and changes
/// nothing.
fn additive(before: &str, after: &str) -> Result<BTreeSet<String>, String> {
    let (before, after) = (by_table(before), by_table(after));
    let empty = BTreeSet::new();
    let mut changed = Vec::new();
    for (table, was) in &before {
        let now = after.get(table).unwrap_or(&empty);
        let gone: Vec<&&str> = was.difference(now).collect();
        let added: Vec<&&str> = (now.difference(was))
            .filter(|line| !table.is_empty() && !may_add(table, line))
            .collect();
        if !gone.is_empty() || !added.is_empty() {
            let named = if table.is_empty() {
                "the database's own"
            } else {
                table
            };
            changed.push(format!(
                "{named}: removed or changed {gone:?}, added what no additive step may {added:?}"
            ));
        }
    }
    match changed.is_empty() {
        true => Ok((after.keys())
            .filter(|table| !table.is_empty() && !before.contains_key(*table))
            .map(|table| table.to_string())
            .collect()),
        false => Err(changed.join("\n")),
    }
}

/// Whether `line`, a definition added to `table`, a table there before, is one an additive step
/// may add: an index that is not unique, on any table; and on an open table, a field (a column)
/// at the top of its rows that may be none (null), with no default, assertion, value or other
/// rule of its own, which a build without the step neither writes nor reads, and keeps.
fn may_add(table: &str, line: &str) -> bool {
    let mut parts = line.splitn(4, '\t').skip(1);
    let (kind, name) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let definition: String = serde_json::from_str(parts.next().unwrap_or("")).unwrap_or_default();
    let open = OPEN.contains(&table);
    let top = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    match kind {
        "indexes" => !definition.contains(" UNIQUE"),
        "index" => definition.starts_with("CREATE INDEX "),
        "fields" => {
            open && top
                && definition.contains(" TYPE none | ")
                && [
                    " ASSERT ",
                    " DEFAULT ",
                    " VALUE ",
                    " READONLY",
                    " COMPUTED ",
                    " REFERENCE",
                ]
                .iter()
                .all(|rule| !definition.contains(rule))
        }
        "column" => {
            open && top && !definition.contains("NOT NULL") && !definition.contains("DEFAULT")
        }
        _ => false,
    }
}

/// A board on `db` at `schema`, with rows in the tables a board's work writes, by a writer of it.
fn populated(db: &Database, schema: Schema) {
    let store = writer(db, schema);
    let handle = |kind: &str| store.register(&NewSession::new(kind)).unwrap().row.handle;
    let (orch, exe) = (handle("orchestrator"), handle("worker"));
    store
        .create(&NewTask {
            links: vec!["docs/TASKS.md".into()],
            ..NewTask::new("contract/first", "Kept as it was", &orch)
        })
        .unwrap();
    store
        .create(&NewTask {
            depends: vec!["contract/first".into()],
            ..NewTask::new("contract/second", "Kept too", &orch)
        })
        .unwrap();
    store.claim("contract/first", &exe).unwrap();
    store.say("contract/first", &exe, "on it", None).unwrap();
    store
        .take_role("contract/second", &orch, "orchestrator")
        .unwrap();
}

/// Each row of every table of `catalogue` on `db` but the ones a move writes (`meta`, its
/// records, and `writer`, a writer's line), each without its fields that are none: a field a step
/// added is none on every row it did not write, a column of it null. PostgreSQL keeps the board's
/// marker as a counter row per backend, which a writer's line folds into one keeping their sum
/// (`store/postgres/writers.rs`): the marker is its sum.
fn rows(db: &Database, catalogue: &str) -> BTreeMap<String, Vec<String>> {
    let raw = Raw::of(db);
    let tables = by_table(catalogue);
    (tables.keys())
        .filter(|table| !["", "meta", "writer"].contains(table))
        .map(|table| {
            let read = raw.table(table).get().unwrap();
            let mut marker = None;
            let mut rows: Vec<String> = (read.as_array().into_iter().flatten())
                .filter_map(|row| {
                    let mut row = row.clone();
                    if *table == "counter" && row["id"][0] == "board" {
                        let last = row["last"].as_i64().unwrap();
                        *marker.get_or_insert(0) += last;
                        return None;
                    }
                    if let Some(fields) = row.as_object_mut() {
                        fields.retain(|_, value| !value.is_null());
                    }
                    Some(row.to_string())
                })
                .collect();
            rows.extend(marker.map(|sum| json!({"id": ["board"], "last": sum}).to_string()));
            rows.sort();
            (table.to_string(), rows)
        })
        .collect()
}

/// A board on `backend` with rows in it at `before`, moved by a writer to `after`, an additive
/// step on: refused unless its catalogue is the one before with an additive step ([`additive`]),
/// the step defines just the tables it declares, `declared`, and every row of each table there
/// before is as it was.
fn step_checked(
    backend: Backend,
    before: Schema,
    after: Schema,
    declared: &[&str],
) -> Result<(), String> {
    let db = backend.database();
    populated(&db, before);
    let was = catalogue(&db);
    let rows_were = rows(&db, &was);
    assert!(
        rows_were["task"].len() == 2,
        "the board has rows: {rows_were:?}"
    );
    moved(&db, after);
    let tables = additive(&was, &catalogue(&db))?;
    let declared: BTreeSet<String> = declared.iter().map(|table| table.to_string()).collect();
    if tables != declared {
        return Err(format!(
            "the step declares the tables {declared:?} and defines {tables:?}"
        ));
    }
    let rows_now = rows(&db, &was);
    let changed: Vec<&String> = (rows_were.iter())
        .filter(|(table, rows)| rows_now.get(*table) != Some(rows))
        .map(|(table, _)| table)
        .collect();
    match changed.is_empty() {
        true => Ok(()),
        false => Err(format!(
            "rows changed in {changed:?}: an additive step writes no row ({})",
            (changed.iter())
                .map(|table| format!(
                    "{table}: {:?} then {:?}",
                    rows_were[*table], rows_now[*table]
                ))
                .collect::<Vec<_>>()
                .join("; ")
        )),
    }
}

/// A newer build's step on each engine.
#[derive(Clone, Copy)]
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

/// Additive steps, each with the tables it declares: a table of its own, its fields and its index;
/// a definition of the database's own and a table; an optional field on an open table, of two
/// types; and an index that is not unique.
const ADDITIVE: [(Twin, &[&str]); 5] = [
    (KEEPING[0], &["fixture_own"]),
    (KEEPING[1], &["fixture_more"]),
    (
        Twin {
            surreal: "DEFINE FIELD IF NOT EXISTS fixture_note ON task TYPE option<string>;",
            postgres: "ALTER TABLE task ADD COLUMN IF NOT EXISTS fixture_note text;",
        },
        &[],
    ),
    (
        Twin {
            surreal: "DEFINE FIELD IF NOT EXISTS fixture_until ON role TYPE option<float>;",
            postgres: "ALTER TABLE role ADD COLUMN IF NOT EXISTS fixture_until double precision;",
        },
        &[],
    ),
    (CHANGING[2].0, &[]),
];

/// Steps no build may take as additive, each with the start of its refusal: a field on a closed
/// table, a field a row must have, one with a default or an assertion, a field retyped, an
/// assertion changed, a field removed, a unique index, an event, the analyzer changed, a step that
/// writes rows, and a step that declares other tables than it defines.
const NOT_ADDITIVE: [(Twin, &[&str], &str); 12] = [
    (
        Twin {
            surreal: "DEFINE FIELD IF NOT EXISTS fixture_note ON meta TYPE option<string>;",
            postgres: "ALTER TABLE meta ADD COLUMN IF NOT EXISTS fixture_note text;",
        },
        &[],
        "meta: ",
    ),
    (
        Twin {
            surreal: "DEFINE FIELD IF NOT EXISTS fixture_note ON task TYPE string DEFAULT '';",
            postgres: "ALTER TABLE task ADD COLUMN IF NOT EXISTS fixture_note text NOT NULL DEFAULT '';",
        },
        &[],
        "task: ",
    ),
    (
        Twin {
            surreal: "DEFINE FIELD IF NOT EXISTS fixture_note ON task TYPE option<string> DEFAULT 'x';",
            postgres: "ALTER TABLE task ADD COLUMN IF NOT EXISTS fixture_note text DEFAULT 'x';",
        },
        &[],
        "task: ",
    ),
    (
        Twin {
            surreal: "DEFINE FIELD IF NOT EXISTS fixture_note ON task TYPE option<string> \
                      ASSERT $value = NONE OR string::len($value) < 9;",
            postgres: "ALTER TABLE task ADD COLUMN IF NOT EXISTS fixture_note text \
                       CHECK (length(fixture_note) < 9);",
        },
        &[],
        "task: ",
    ),
    (
        Twin {
            surreal: "DEFINE FIELD OVERWRITE title ON task TYPE option<string>;",
            postgres: "ALTER TABLE task ALTER COLUMN title DROP NOT NULL;",
        },
        &[],
        "task: ",
    ),
    (CHANGING[1].0, &[], "task: "),
    (
        Twin {
            surreal: "REMOVE FIELD expires ON role;",
            postgres: "ALTER TABLE role DROP COLUMN expires;",
        },
        &[],
        "role: ",
    ),
    (
        Twin {
            surreal: "DEFINE INDEX IF NOT EXISTS fixture_unique ON task FIELDS slug UNIQUE;",
            postgres: "CREATE UNIQUE INDEX IF NOT EXISTS fixture_unique ON task (slug);",
        },
        &[],
        "task: ",
    ),
    (CHANGING[3].0, &[], "task: "),
    (CHANGING[4].0, &[], "the database's own: "),
    (
        Twin {
            surreal: "UPDATE task SET priority = 9 RETURN NONE;",
            postgres: "UPDATE task SET priority = 9;",
        },
        &[],
        "rows changed in [",
    ),
    (
        KEEPING[0],
        &[],
        "the step declares the tables {} and defines {\"fixture_own\"}",
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
    fn this_builds_catalogue_is_its_file_and_each_of_its_additive_steps_is_one(
        backend: Backend
    ) {
        let db = backend.database();
        moved(&db, Schema::this_build());
        let made = catalogue(&db);
        let (version, steps) = (VERSION, ADDITIONS);
        assert_eq!(
            (Schema::this_build().version(), Schema::this_build().additions()),
            (version, steps)
        );
        let path = file(backend, version, steps);
        if std::env::var("TASKS_CATALOGUE").as_deref() == Ok("write") {
            std::fs::write(&path, &made).unwrap();
        }
        let committed_now = committed(&path);
        // Each way, as a check of one way takes the tables and the database's lines it adds.
        if let Err(changed) = kept(&committed_now, &made).and_then(|()| kept(&made, &committed_now)) {
            panic!(
                "{} is not the board this build makes ({changed}); write it again with \
                 TASKS_CATALOGUE=write just test-rs -p symphony-board --test store catalogue",
                path.display()
            );
        }
        // A declared version keeps every table of the one before as that version's last file has
        // it: its contract before any additive step on it.
        if keeps_writes(version) {
            let before = committed(&last_file(backend, version - 1));
            if let Err(changed) = kept(&before, &committed(&file(backend, version, 0))) {
                panic!("schema {version} is declared in COMPATIBLE but changes: {changed}");
            }
        }
        for (listed, _) in COMPATIBLE {
            assert!(*listed <= version);
        }
        // Each additive step on the contract is one: a board one step before, moved by it, and
        // its file, the one before with it.
        for step in 1..=steps {
            let addition = &ADDED[usize::try_from(step - 1).unwrap()];
            let (before, after) = (Schema::of_this_contract(step - 1), Schema::of_this_contract(step));
            if let Err(why) = step_checked(backend, before, after, addition.tables) {
                panic!("schema {version}'s additive step {step} ({}) is not one: {why}", addition.why);
            }
            let (was, now) = (file(backend, version, step - 1), file(backend, version, step));
            if let Err(why) = additive(&committed(&was), &committed(&now)) {
                panic!("{} is not {} with an additive step: {why}", now.display(), was.display());
            }
        }
        assert!(
            !file(backend, version, steps + 1).exists(),
            "a file of a step this build does not have"
        );
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

    fn an_additive_step_adds_tables_and_optional_fields_on_open_tables_and_writes_no_row(
        backend: Backend
    ) {
        let this = Schema::this_build;
        let step = |schema: Schema, twin: &Twin, tables: &[&str]| {
            schema.then_additive(twin.surreal, twin.postgres, tables)
        };
        for (twin, tables) in &ADDITIVE {
            let after = step(this(), twin, tables);
            assert_eq!((after.version(), after.additions()), (VERSION, ADDITIONS + 1));
            step_checked(backend, this(), after, tables)
                .unwrap_or_else(|why| panic!("{}: {why}", twin.surreal));
        }
        // Two at once, by a build two steps on.
        let (first, second) = (&ADDITIVE[0], &ADDITIVE[2]);
        let after = step(step(this(), &first.0, first.1), &second.0, second.1);
        step_checked(backend, this(), after, &["fixture_own"]).unwrap();
        // And none of the rest, each refused naming what it changed.
        for (twin, tables, named) in &NOT_ADDITIVE {
            let why = step_checked(backend, this(), step(this(), twin, tables), tables)
                .expect_err(twin.surreal);
            assert!(why.starts_with(named), "{}: {why}", twin.surreal);
        }
        let (row_writer, _, _) = &NOT_ADDITIVE[10];
        let why = step_checked(backend, this(), step(this(), row_writer, &[]), &[]).unwrap_err();
        assert!(why.contains("\"task\""), "{why}");
        let (field, _) = &ADDITIVE[2];
        let why = step_checked(backend, this(), step(this(), field, &["task"]), &["task"]).unwrap_err();
        assert_eq!(why, "the step declares the tables {\"task\"} and defines {}");
    }
}

#[test]
fn an_additive_line_is_an_optional_top_field_on_an_open_table_or_an_index_not_unique() {
    let line = |table: &str, kind: &str, name: &str, definition: &str| {
        may_add(table, &self::line(table, kind, name, definition))
    };
    let field = |name: &str, type_and_rules: &str| {
        line(
            "task",
            "fields",
            name,
            &format!("DEFINE FIELD {name} ON task TYPE {type_and_rules} PERMISSIONS FULL"),
        )
    };
    assert!(field("note", "none | string"));
    assert!(field("note", "none | object FLEXIBLE"));
    assert!(field("note", "none | array<string>"));
    // Not one a row must have, nor one with a rule of its own, nor one inside another field.
    assert!(!field("note", "string"));
    assert!(!field("note", "array<string>"));
    for rule in [
        "DEFAULT 'x'",
        "ASSERT $value != 'x'",
        "VALUE 'x'",
        "READONLY",
        "COMPUTED 'x'",
        "REFERENCE",
    ] {
        assert!(!field("note", &format!("none | string {rule}")), "{rule}");
    }
    assert!(!field("fields.note", "none | string"));
    assert!(!field("notes[*]", "none | string"));
    assert!(!field("", "none | string"));
    // Only on an open table: a closed one reads its rows as no field more than it knows.
    assert!(!line(
        "meta",
        "fields",
        "note",
        "DEFINE FIELD note ON meta TYPE none | string PERMISSIONS FULL"
    ));
    assert!(!line("workflow", "column", "note", "text"));
    for table in OPEN {
        assert!(line(table, "column", "note", "text"), "{table}");
    }
    // A column that may be null, with no default.
    assert!(line("task", "column", "note", "jsonb"));
    assert!(!line("task", "column", "note", "text NOT NULL"));
    assert!(!line("task", "column", "note", "text DEFAULT 'x'::text"));
    // An index that is not unique, on any table.
    assert!(line(
        "counter",
        "indexes",
        "by_last",
        "DEFINE INDEX by_last ON counter FIELDS last"
    ));
    assert!(!line(
        "task",
        "indexes",
        "by_slug",
        "DEFINE INDEX by_slug ON task FIELDS slug UNIQUE"
    ));
    assert!(line(
        "meta",
        "index",
        "by",
        "CREATE INDEX by ON public.meta USING btree (version)"
    ));
    assert!(!line(
        "task",
        "index",
        "by",
        "CREATE UNIQUE INDEX by ON public.task USING btree (slug)"
    ));
    // Nothing else: an event, a trigger, a constraint, a table's own line.
    assert!(!line(
        "task",
        "events",
        "moves",
        "DEFINE EVENT moves ON task WHEN true THEN {}"
    ));
    assert!(!line(
        "task",
        "trigger",
        "moves",
        "CREATE TRIGGER moves AFTER UPDATE ON public.task"
    ));
    assert!(!line(
        "task",
        "constraint",
        "note_check",
        "CHECK ((note <> 'x'::text))"
    ));
    assert!(!line(
        "task",
        "table",
        "task",
        "DEFINE TABLE task TYPE NORMAL SCHEMAFULL"
    ));
    assert!(!line("task", "fields", "note", "not json"));
}

#[test]
fn an_additive_catalogue_loses_and_changes_nothing_and_says_the_tables_it_adds() {
    let catalogue = |lines: &[(&str, &str, &str, &str)]| -> String {
        (lines.iter())
            .map(|(table, kind, name, definition)| line(table, kind, name, definition) + "\n")
            .collect()
    };
    let before = catalogue(&[
        ("", "analyzers", "words", "DEFINE ANALYZER words"),
        (
            "task",
            "fields",
            "slug",
            "DEFINE FIELD slug ON task TYPE string",
        ),
    ]);
    let with = |more: &[(&str, &str, &str, &str)]| {
        let mut lines = vec![
            ("", "analyzers", "words", "DEFINE ANALYZER words"),
            (
                "task",
                "fields",
                "slug",
                "DEFINE FIELD slug ON task TYPE string",
            ),
        ];
        lines.extend_from_slice(more);
        catalogue(&lines)
    };
    assert_eq!(additive(&before, &before), Ok(BTreeSet::new()));
    assert_eq!(
        additive(
            &before,
            &with(&[
                ("", "functions", "more", "DEFINE FUNCTION fn::more() {}"),
                ("fixture", "table", "fixture", "DEFINE TABLE fixture"),
                (
                    "fixture",
                    "fields",
                    "x",
                    "DEFINE FIELD x ON fixture TYPE string"
                ),
                (
                    "task",
                    "fields",
                    "note",
                    "DEFINE FIELD note ON task TYPE none | string"
                ),
            ])
        ),
        Ok(BTreeSet::from(["fixture".to_string()]))
    );
    // A line gone or changed, of a table or of the database's own, is refused, naming it.
    let gone = catalogue(&[("", "analyzers", "words", "DEFINE ANALYZER words")]);
    assert!(
        additive(&before, &gone)
            .unwrap_err()
            .starts_with("task: removed or changed")
    );
    let changed = catalogue(&[
        (
            "",
            "analyzers",
            "words",
            "DEFINE ANALYZER words FILTERS lowercase",
        ),
        (
            "task",
            "fields",
            "slug",
            "DEFINE FIELD slug ON task TYPE string",
        ),
    ]);
    assert!(
        additive(&before, &changed)
            .unwrap_err()
            .starts_with("the database's own: ")
    );
    let required = with(&[(
        "task",
        "fields",
        "note",
        "DEFINE FIELD note ON task TYPE string",
    )]);
    let why = additive(&before, &required).unwrap_err();
    assert!(
        why.starts_with("task: removed or changed [], added what no additive step may"),
        "{why}"
    );
}
