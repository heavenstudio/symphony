//! What `symphony serve` answers for the schema check.

use serde_json::{Value, json};

fn call(request: Value) -> Value {
    symphony::cli::call(&request)
}

fn ok(request: Value) -> Value {
    let answer = call(request);
    answer
        .get("ok")
        .cloned()
        .unwrap_or_else(|| panic!("no answer: {answer}"))
}

#[test]
fn validate_answers_the_error_lines() {
    let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
    assert_eq!(
        ok(json!({"fn": "validate", "instance": {"a": "x"}, "schema": schema})),
        json!([])
    );
    let found = ok(json!({"fn": "validate", "instance": {"a": 1}, "schema": schema}));
    let lines = found.as_array().unwrap();
    assert_eq!(lines.len(), 1, "{found}");
    assert!(lines[0].as_str().unwrap().starts_with("$.a: "), "{found}");
}

#[test]
fn validate_answers_in_the_order_the_instance_was_written() {
    // As the serve loop reads a request: from its JSON line, members in the order sent.
    let request: Value = serde_json::from_str(
        r#"{"fn": "validate", "instance": {"z": "x", "a": "y"},
            "schema": {"type": "object", "additionalProperties": {"type": "integer"}}}"#,
    )
    .unwrap();
    assert_eq!(
        ok(request),
        json!([
            "$.z: expected integer, got str",
            "$.a: expected integer, got str"
        ])
    );
}

#[test]
fn validate_refuses_a_schema_it_cannot_enforce_as_a_value_error() {
    let answer = call(json!({"fn": "validate", "instance": 1, "schema": {"minItem": 1}}));
    assert_eq!(answer["error"], "value", "{answer}");
    assert!(
        answer["message"].as_str().unwrap().contains("minItem"),
        "{answer}"
    );
}
