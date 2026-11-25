//! What `sqlite3 -json` printed, read into rows.

use super::*;

#[derive(Debug, PartialEq, serde::Deserialize)]
struct Row {
    id: i64,
    path: String,
}

/// The rows `printed` holds, read as text: the reader `arrays` used before it read bytes.
fn read_as_text(printed: &str) -> Result<Vec<Row>, AskError> {
    let mut rows = Vec::new();
    for array in serde_json::Deserializer::from_str(printed).into_iter::<Vec<Row>>() {
        rows.extend(array.map_err(|problem| {
            AskError::Store(format!("sqlite3 printed what does not read: {problem}"))
        })?);
    }
    Ok(rows)
}

#[test]
fn every_array_printed_is_read_in_turn_and_nothing_printed_is_no_rows() {
    let printed = "[{\"id\":1,\"path\":\"docs/a.md\"}]\n[{\"id\":2,\"path\":\"é\"},\n{\"id\":3,\"path\":\"\\u00e9\"}]\n";
    let rows = arrays::<Row>(printed).unwrap();
    let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids, [1, 2, 3]);
    assert_eq!((rows[1].path.as_str(), rows[2].path.as_str()), ("é", "é"));
    assert_eq!(arrays::<Row>("").unwrap(), []);
    assert_eq!(arrays::<Row>(" \n\t\n").unwrap(), []);
}

#[test]
fn what_does_not_read_is_refused_in_the_words_and_at_the_place_a_read_of_the_text_gives() {
    let malformed = [
        "[",
        "[{\"id\":1,\"path\":\"a\"}",
        "[{\"id\":1,\"path\":\"a\"}]\n[{\"id\":\"2\",\"path\":\"b\"}]",
        "[{\"id\":1}]",
        "[{\"id\":1,\"path\":\"a\",\"path\":\"b\"}]",
        "[{\"id\":1,\"path\":\"a\\q\"}]",
        "[{\"id\":1,\"path\":\"\\ud800\"}]",
        "[{\"id\":1e400,\"path\":\"a\"}]",
        "[{\"id\":1,\"path\":\"a\"}] x",
        "{\"id\":1,\"path\":\"a\"}",
        "[{\"id\":1,\"path\":\"a\"}]\n\n[{\n  \"id\": 2,\n  \"path\": 3\n}]",
        "Error: in prepare, no such table: asks",
    ];
    for printed in malformed {
        let before = read_as_text(printed).unwrap_err();
        assert!(before.to_string().contains(" at line "), "{before}");
        assert_eq!(arrays::<Row>(printed), Err(before), "{printed:?}");
    }
    let Err(AskError::Store(deep)) = arrays::<Row>(malformed[10]) else {
        panic!("a path that is a number reads");
    };
    assert!(deep.ends_with("at line 5 column 11"), "{deep}");
}
