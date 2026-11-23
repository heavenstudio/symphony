//! The statements of `store/raw.rs` on PostgreSQL: a board's rows read and set as they are, by
//! table, id and equal fields, each text made of the names it is handed and nothing a caller wrote.
//!
//! A field is named by a path of plain names (`fields._board.red`): on a table of
//! typed columns its first name is the column and the rest a path in that column's `jsonb`; on a
//! SCHEMALESS table, or one the board's schema does not name, the whole path is in `doc`.

use super::Pg;
use super::schema::{Kind, Table, table};
use serde_json::{Map, Value};

/// The rows a statement is about: one by its id (`v -> 'id'`), or a table's, every one or those
/// whose fields equal the given values (`v -> 'w0'`, `v -> 'w1'`, in the fields' order; a null is
/// a field that is not there).
pub(in crate::store) enum Rows<'a> {
    One(&'a str),
    Where(&'a str, &'a Map<String, Value>),
}

/// The table's columns, or none where it holds its fields in `doc`.
fn columns(name: &str) -> Option<&'static Table> {
    table(name).filter(|table| !table.columns.is_empty())
}

fn name<'a>(rows: &Rows<'a>) -> &'a str {
    match rows {
        Rows::One(table) | Rows::Where(table, _) => table,
    }
}

/// `path`'s names as a PostgreSQL text array: `'{a,b}'`.
fn array(path: &[&str]) -> String {
    format!("'{{{}}}'", path.join(","))
}

/// The value of the field `path` in the row `t`, as `jsonb`.
fn value_of(table: &str, path: &str) -> String {
    let path: Vec<&str> = path.split('.').collect();
    match columns(table) {
        Some(_) if path.len() == 1 => format!("to_jsonb(t.{})", path[0]),
        Some(_) => format!("(to_jsonb(t.{}) #> {})", path[0], array(&path[1..])),
        None => format!("(t.doc #> {})", array(&path)),
    }
}

/// The row `t`'s fields, its id left out.
fn fields_of(table: &str) -> &'static str {
    if columns(table).is_some() {
        "(jsonb_strip_nulls(to_jsonb(t)) - 'id')"
    } else {
        "t.doc"
    }
}

/// The row `t`'s fields with its id.
fn listed(table: &str) -> &'static str {
    if columns(table).is_some() {
        "jsonb_strip_nulls(to_jsonb(t))"
    } else {
        "(t.doc || jsonb_build_object('id', t.id))"
    }
}

/// The condition of the rows a statement is about.
fn condition(rows: &Rows) -> String {
    match rows {
        Rows::One(_) => " WHERE t.id = v -> 'id'".into(),
        Rows::Where(table, equal) => {
            let terms: Vec<String> = (equal.iter().enumerate())
                .map(|(at, (field, value))| match value {
                    Value::Null => format!("{} IS NULL", value_of(table, field)),
                    _ => format!("{} = v -> 'w{at}'", value_of(table, field)),
                })
                .collect();
            if terms.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", terms.join(" AND "))
            }
        }
    }
}

/// The value `v -> name` as a column of `kind` holds it.
fn cast(kind: Option<Kind>, name: &str) -> String {
    match kind {
        Some(Kind::Int) => format!("(v ->> '{name}')::bigint"),
        Some(Kind::Float) => format!("(v ->> '{name}')::double precision"),
        Some(Kind::Bool) => format!("(v ->> '{name}')::boolean"),
        Some(Kind::Object | Kind::Array) => format!("(v -> '{name}')"),
        Some(Kind::Text) | None => format!("(v ->> '{name}')"),
    }
}

/// Each column `fields` changes, and what it is set to from `base` (the column as it is, or
/// `NULL` for a row being made), each value its variable (`s0`, `s1`, in the fields' order), a
/// null removing the field.
fn assigned(
    table: &str,
    fields: &Map<String, Value>,
    base: &dyn Fn(&str) -> String,
    id: Option<&str>,
) -> Vec<(String, String)> {
    let typed = columns(table);
    let mut set: Vec<(String, String)> = Vec::new();
    for (at, field) in fields.keys().enumerate() {
        let path: Vec<&str> = field.split('.').collect();
        let (column, inner) = match typed {
            Some(_) => (path[0].to_string(), &path[1..]),
            None => ("doc".to_string(), &path[..]),
        };
        let slot = set.iter().position(|(name, _)| *name == column);
        let was = slot.map_or_else(|| base(&column), |slot| set[slot].1.clone());
        let value = if let Some(id) = id.filter(|_| table == "task" && column == "fields") {
            format!(
                "board_fields({id}, {was}, {}, v -> 's{at}', (v ->> 'board_written')::json -> 's{at}')",
                array(inner)
            )
        } else if inner.is_empty() {
            let kind = typed.and_then(|table| table.columns.iter().find(|c| c.name == column));
            cast(kind.map(|c| c.kind), &format!("s{at}"))
        } else {
            format!("board_set({was}, {}, v -> 's{at}')", array(inner))
        };
        match slot {
            Some(slot) => set[slot].1 = value,
            None => set.push((column, value)),
        }
    }
    if typed.is_none() {
        for (_, value) in &mut set {
            *value = format!("coalesce({value}, '{{}}'::jsonb)");
        }
    }
    set
}

/// Whether a raw task assignment changes its document as `jsonb` compares it: a number equal in
/// value is no change, and an assignment of none writes nothing. Evaluate this with the pure
/// setter, before `board_fields` records any explicit replacement. A change to another column
/// makes all assignments land.
fn changed(table: &str, fields: &Map<String, Value>) -> Option<String> {
    if table != "task"
        || !fields
            .keys()
            .any(|field| field == "fields" || field.starts_with("fields."))
    {
        return None;
    }
    let next = assigned(table, fields, &|column| format!("t.{column}"), None);
    let before = next
        .iter()
        .map(|(column, _)| format!("t.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    let after = next
        .into_iter()
        .map(|(_, value)| value)
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("({before}) IS DISTINCT FROM ({after})"))
}

/// The row whole, its id left out, or null; a table's rows whole, ids and all, by id.
pub(in crate::store) fn get(rows: &Rows) -> Pg<'static> {
    let table = name(rows);
    Pg::read(match rows {
        Rows::One(_) => format!(
            "(SELECT {} FROM {table} t{})",
            fields_of(table),
            condition(rows)
        ),
        Rows::Where(..) => format!(
            "coalesce((SELECT jsonb_agg({} ORDER BY t.id) FROM {table} t{}), '[]'::jsonb)",
            listed(table),
            condition(rows)
        ),
    })
}

/// Only `fields` of the row, or null; of a table's rows, by id.
pub(in crate::store) fn fields(rows: &Rows, fields: &[&str]) -> Pg<'static> {
    let table = name(rows);
    let mut object = "'{}'::jsonb".to_string();
    for field in fields {
        let path: Vec<&str> = field.split('.').collect();
        object = format!(
            "board_set({object}, {}, {})",
            array(&path),
            value_of(table, field)
        );
    }
    let object = format!("jsonb_strip_nulls({object})");
    Pg::read(match rows {
        Rows::One(_) => format!("(SELECT {object} FROM {table} t{})", condition(rows)),
        Rows::Where(..) => format!(
            "coalesce((SELECT jsonb_agg({object} ORDER BY t.id) FROM {table} t{}), '[]'::jsonb)",
            condition(rows)
        ),
    })
}

/// One field of the one row, null when either is not there.
pub(in crate::store) fn field(rows: &Rows, field: &str) -> Pg<'static> {
    let table = name(rows);
    Pg::read(format!(
        "(SELECT {} FROM {table} t{})",
        value_of(table, field),
        condition(rows)
    ))
}

/// Whether the one row is there.
pub(in crate::store) fn exists(rows: &Rows) -> Pg<'static> {
    Pg::read(format!(
        "EXISTS (SELECT 1 FROM {} t{})",
        name(rows),
        condition(rows)
    ))
}

/// How many rows there are.
pub(in crate::store) fn count(rows: &Rows) -> Pg<'static> {
    Pg::read(format!(
        "(SELECT count(*) FROM {} t{})",
        name(rows),
        condition(rows)
    ))
}

/// `fields` set on the rows there are: a row that is not there is not made.
pub(in crate::store) fn set(rows: &Rows, fields: &Map<String, Value>) -> Pg<'static> {
    let table = name(rows);
    let set: Vec<String> = assigned(table, fields, &|column| format!("t.{column}"), Some("t.id"))
        .into_iter()
        .map(|(column, value)| format!("{column} = {value}"))
        .collect();
    let mut condition = condition(rows);
    if let Some(changed) = changed(table, fields) {
        condition.push_str(if condition.is_empty() {
            " WHERE "
        } else {
            " AND "
        });
        condition.push_str(&changed);
    }
    Pg::script(format!(
        "UPDATE {table} AS t SET {}{condition};",
        set.join(", ")
    ))
}

/// `field` of the rows there are, moved by `by`.
pub(in crate::store) fn add(rows: &Rows, field: &str) -> Pg<'static> {
    let table = name(rows);
    let path: Vec<&str> = field.split('.').collect();
    let (column, value) = match columns(table) {
        Some(typed) if path.len() == 1 => {
            let kind = typed
                .columns
                .iter()
                .find(|c| c.name == path[0])
                .map(|c| c.kind);
            let by = cast(kind, "by");
            (
                path[0].to_string(),
                format!("coalesce(t.{}, 0) + {by}", path[0]),
            )
        }
        typed => {
            let (column, inner) = if typed.is_some() {
                (path[0], &path[1..])
            } else {
                ("doc", &path[..])
            };
            let inner = array(inner);
            let setter = if table == "task" && column == "fields" {
                "board_fields(t.id, "
            } else {
                "board_set("
            };
            (
                column.to_string(),
                format!(
                    "{setter}t.{column}, {inner}, to_jsonb(coalesce((t.{column} #>> {inner})::numeric, 0) \
                     + (v ->> 'by')::numeric))"
                ),
            )
        }
    };
    Pg::script(format!(
        "UPDATE {table} AS t SET {column} = {value}{};",
        condition(rows)
    ))
}

/// The one row made with `fields`, or, when `upsert`, changed where it is there.
fn made(rows: &Rows, fields: &Map<String, Value>, upsert: bool) -> Pg<'static> {
    let table = name(rows);
    let new = assigned(table, fields, &|_| "NULL".into(), None);
    let mut columns = vec!["id".to_string()];
    let mut values = vec!["v -> 'id'".to_string()];
    for (column, value) in new {
        columns.push(column);
        values.push(value);
    }
    if self::columns(table).is_none() && columns.len() == 1 {
        columns.push("doc".into());
        values.push("'{}'::jsonb".into());
    }
    let mut sql = format!(
        "INSERT INTO {table} AS t ({}) VALUES ({})",
        columns.join(", "),
        values.join(", ")
    );
    if upsert {
        let changed: Vec<String> =
            assigned(table, fields, &|column| format!("t.{column}"), Some("t.id"))
                .into_iter()
                .map(|(column, value)| format!("{column} = {value}"))
                .collect();
        if changed.is_empty() {
            sql.push_str(" ON CONFLICT (id) DO NOTHING");
        } else {
            sql.push_str(&format!(
                " ON CONFLICT (id) DO UPDATE SET {}",
                changed.join(", ")
            ));
            if let Some(changed) = self::changed(table, fields) {
                sql.push_str(&format!(" WHERE {changed}"));
            }
        }
    }
    sql.push(';');
    Pg::script(sql)
}

/// The one row made, or changed where it is there, with `fields`.
pub(in crate::store) fn put(rows: &Rows, fields: &Map<String, Value>) -> Pg<'static> {
    made(rows, fields, true)
}

/// The one row made with `fields`; an error where it is there already.
pub(in crate::store) fn create(rows: &Rows, fields: &Map<String, Value>) -> Pg<'static> {
    made(rows, fields, false)
}

/// The rows removed.
pub(in crate::store) fn delete(rows: &Rows) -> Pg<'static> {
    Pg::script(format!(
        "DELETE FROM {} AS t{};",
        name(rows),
        condition(rows)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn text(statement: Pg) -> String {
        match statement {
            Pg::Read(sql) | Pg::Script(sql, None) => sql.into_owned(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_typed_tables_field_is_its_column_and_a_path_in_it() {
        let one = Rows::One("task");
        assert_eq!(
            text(field(&one, "fields._board.red")),
            "(SELECT (to_jsonb(t.fields) #> '{_board,red}') FROM task t WHERE t.id = v -> 'id')"
        );
        assert_eq!(
            text(set(
                &one,
                &object(json!({"title": "x", "priority": 1, "fields._board.red": null}))
            )),
            "UPDATE task AS t SET title = (v ->> 's0'), priority = (v ->> 's1')::bigint, \
             fields = board_fields(t.id, t.fields, '{_board,red}', v -> 's2', \
             (v ->> 'board_written')::json -> 's2') WHERE t.id = v -> 'id' \
             AND (t.title, t.priority, t.fields) IS DISTINCT FROM ((v ->> 's0'), \
             (v ->> 's1')::bigint, board_set(t.fields, '{_board,red}', v -> 's2'));"
        );
        assert_eq!(
            text(add(&one, "updated")),
            "UPDATE task AS t SET updated = coalesce(t.updated, 0) + (v ->> 'by')::double precision \
             WHERE t.id = v -> 'id';"
        );
    }

    #[test]
    fn a_schemaless_tables_fields_are_paths_in_its_doc() {
        let lease = Rows::One("callback_lease");
        assert_eq!(
            text(put(&lease, &object(json!({"until": 5, "asked": 1})))),
            "INSERT INTO callback_lease AS t (id, doc) VALUES (v -> 'id', \
             coalesce(board_set(board_set(NULL, '{until}', v -> 's0'), '{asked}', v -> 's1'), '{}'::jsonb)) \
             ON CONFLICT (id) DO UPDATE SET doc = \
             coalesce(board_set(board_set(t.doc, '{until}', v -> 's0'), '{asked}', v -> 's1'), '{}'::jsonb);"
        );
        assert_eq!(
            text(create(&lease, &Map::new())),
            "INSERT INTO callback_lease AS t (id, doc) VALUES (v -> 'id', '{}'::jsonb);"
        );
        let equal = object(json!({"handle": "a", "gone": null}));
        assert_eq!(
            text(count(&Rows::Where("callback_event", &equal))),
            "(SELECT count(*) FROM callback_event t WHERE (t.doc #> '{handle}') = v -> 'w0' \
             AND (t.doc #> '{gone}') IS NULL)"
        );
    }
}
