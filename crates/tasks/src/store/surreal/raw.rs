//! The statements of `store/raw.rs`: a board's rows read and set as they are, by table, id and
//! equal fields, each text made of the names it is handed and nothing a caller wrote in SurrealQL.

use serde_json::{Map, Value};

/// The rows a statement is about: one by its id (`$id`), or a table's, every one or those whose
/// fields equal the given values (`$w0`, `$w1`, in the fields' order; a null is NONE).
pub(in crate::store) enum Rows<'a> {
    One(&'a str),
    Where(&'a str, &'a Map<String, Value>),
}

impl Rows<'_> {
    /// What a statement reads from or changes.
    fn target(&self) -> String {
        match self {
            Rows::One(table) => format!("type::record('{table}', $id)"),
            Rows::Where(table, _) => (*table).to_string(),
        }
    }

    /// The condition after the target, with its leading space, or nothing.
    fn condition(&self) -> String {
        let Rows::Where(_, equal) = self else {
            return String::new();
        };
        let terms: Vec<String> = (equal.iter().enumerate())
            .map(|(at, (field, value))| match value {
                Value::Null => format!("{field} = NONE"),
                _ => format!("{field} = $w{at}"),
            })
            .collect();
        if terms.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", terms.join(" AND "))
        }
    }
}

/// `fields` as a `SET`'s assignments, each value its variable (`$s0`, `$s1`, in the fields'
/// order) and a null NONE, which unsets the field.
fn assignments(fields: &Map<String, Value>) -> String {
    let set: Vec<String> = (fields.iter().enumerate())
        .map(|(at, (field, value))| match value {
            Value::Null => format!("{field} = NONE"),
            _ => format!("{field} = $s{at}"),
        })
        .collect();
    set.join(", ")
}

/// The row whole, its id left out, or NONE; a table's rows whole, ids and all, by id.
pub(in crate::store) fn get(rows: &Rows) -> String {
    match rows {
        Rows::One(_) => format!("RETURN SELECT * OMIT id FROM ONLY {};", rows.target()),
        Rows::Where(..) => format!(
            "RETURN SELECT * FROM {}{} ORDER BY id;",
            rows.target(),
            rows.condition()
        ),
    }
}

/// Only `fields` of the row, or NONE; of a table's rows, by id.
pub(in crate::store) fn fields(rows: &Rows, fields: &[&str]) -> String {
    let fields = fields.join(", ");
    match rows {
        Rows::One(_) => format!("RETURN SELECT {fields} FROM ONLY {};", rows.target()),
        Rows::Where(..) => format!(
            "RETURN SELECT {fields} FROM {}{} ORDER BY id;",
            rows.target(),
            rows.condition()
        ),
    }
}

/// One field of the one row, NONE when either is not there.
pub(in crate::store) fn field(rows: &Rows, field: &str) -> String {
    format!("RETURN {}.{field};", rows.target())
}

/// Whether the one row is there.
pub(in crate::store) fn exists(rows: &Rows) -> String {
    format!("RETURN record::exists({});", rows.target())
}

/// How many rows there are.
pub(in crate::store) fn count(rows: &Rows) -> String {
    format!(
        "RETURN count(SELECT id FROM {}{});",
        rows.target(),
        rows.condition()
    )
}

/// `fields` set on the rows there are: a row that is not there is not made.
pub(in crate::store) fn set(rows: &Rows, fields: &Map<String, Value>) -> String {
    format!(
        "UPDATE {} SET {}{} RETURN NONE;",
        rows.target(),
        assignments(fields),
        rows.condition()
    )
}

/// `field` of the rows there are, moved by `$by`.
pub(in crate::store) fn add(rows: &Rows, field: &str) -> String {
    format!(
        "UPDATE {} SET {field} += $by{} RETURN NONE;",
        rows.target(),
        rows.condition()
    )
}

/// The one row made, or changed where it is there, with `fields`.
pub(in crate::store) fn put(rows: &Rows, fields: &Map<String, Value>) -> String {
    format!(
        "UPSERT {} SET {} RETURN NONE;",
        rows.target(),
        assignments(fields)
    )
}

/// The one row made with `fields`; an error where it is there already.
pub(in crate::store) fn create(rows: &Rows, fields: &Map<String, Value>) -> String {
    format!(
        "CREATE {} SET {} RETURN NONE;",
        rows.target(),
        assignments(fields)
    )
}

/// The rows removed.
pub(in crate::store) fn delete(rows: &Rows) -> String {
    format!("DELETE {}{} RETURN NONE;", rows.target(), rows.condition())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn one_row_is_named_by_its_id_and_takes_no_condition() {
        let one = Rows::One("task");
        assert_eq!(
            get(&one),
            "RETURN SELECT * OMIT id FROM ONLY type::record('task', $id);"
        );
        assert_eq!(
            fields(&one, &["a", "b.c"]),
            "RETURN SELECT a, b.c FROM ONLY type::record('task', $id);"
        );
        assert_eq!(field(&one, "a.b"), "RETURN type::record('task', $id).a.b;");
        assert_eq!(
            exists(&one),
            "RETURN record::exists(type::record('task', $id));"
        );
        assert_eq!(
            count(&one),
            "RETURN count(SELECT id FROM type::record('task', $id));"
        );
        assert_eq!(
            add(&one, "ts"),
            "UPDATE type::record('task', $id) SET ts += $by RETURN NONE;"
        );
        assert_eq!(
            delete(&one),
            "DELETE type::record('task', $id) RETURN NONE;"
        );
    }

    #[test]
    fn a_set_names_each_value_by_its_place_and_unsets_a_null() {
        let one = Rows::One("session");
        let set_to = object(json!({"a": 1, "gone": null, "b.c": "x"}));
        assert_eq!(
            set(&one, &set_to),
            "UPDATE type::record('session', $id) SET a = $s0, gone = NONE, b.c = $s2 RETURN NONE;"
        );
        assert_eq!(
            put(&one, &set_to),
            "UPSERT type::record('session', $id) SET a = $s0, gone = NONE, b.c = $s2 RETURN NONE;"
        );
        assert_eq!(
            create(&one, &set_to),
            "CREATE type::record('session', $id) SET a = $s0, gone = NONE, b.c = $s2 RETURN NONE;"
        );
    }

    #[test]
    fn a_tables_rows_are_every_one_or_those_with_equal_fields() {
        let none = Map::new();
        let all = Rows::Where("role", &none);
        assert_eq!(get(&all), "RETURN SELECT * FROM role ORDER BY id;");
        assert_eq!(count(&all), "RETURN count(SELECT id FROM role);");
        assert_eq!(delete(&all), "DELETE role RETURN NONE;");
        let equal = object(json!({"slug": "a", "expires": null, "role": "executor"}));
        let some = Rows::Where("role", &equal);
        let condition = " WHERE slug = $w0 AND expires = NONE AND role = $w2";
        assert_eq!(
            get(&some),
            format!("RETURN SELECT * FROM role{condition} ORDER BY id;")
        );
        assert_eq!(
            fields(&some, &["handle"]),
            format!("RETURN SELECT handle FROM role{condition} ORDER BY id;")
        );
        assert_eq!(
            count(&some),
            format!("RETURN count(SELECT id FROM role{condition});")
        );
        assert_eq!(
            set(&some, &object(json!({"since": 2}))),
            format!("UPDATE role SET since = $s0{condition} RETURN NONE;")
        );
        assert_eq!(
            add(&some, "since"),
            format!("UPDATE role SET since += $by{condition} RETURN NONE;")
        );
        assert_eq!(
            delete(&some),
            format!("DELETE role{condition} RETURN NONE;")
        );
    }
}
