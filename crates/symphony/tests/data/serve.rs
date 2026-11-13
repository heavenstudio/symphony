//! What `symphony serve` answers a schema check: none. The check is the library's
//! (`symphony::schema::validate`, tests/after_model.rs and tests/data/cache.rs), which a consumer's
//! code links; `serve` refuses `validate` as a name it lacks, saying where the check is.

use serde_json::{Value, json};

const REFUSED: &str = "unknown function \"validate\": serve has no schema check; link \
                       symphony_schema::validate (symphony::schema) instead";

#[test]
fn validate_is_an_unknown_function_naming_the_library_check() {
    let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
    for request in [
        json!({"fn": "validate", "instance": {"a": 1}, "schema": schema}),
        json!({"fn": "validate", "instance": {"a": "x"}, "schema": schema}),
        json!({"fn": "validate", "instance": 1, "schema": {"minItem": 1}}),
        json!({"fn": "validate"}),
    ] {
        assert_eq!(
            symphony::cli::call(&request),
            json!({"error": "value", "message": REFUSED}),
            "{request}"
        );
    }
    // The same kind and opening as any other name serve lacks.
    let other = symphony::cli::call(&json!({"fn": "validated"}));
    assert_eq!(
        other,
        json!({"error": "value", "message": "unknown function \"validated\""})
    );
}

#[test]
fn a_refused_validate_leaves_the_serve_loop_answering_until_its_input_ends() {
    let input = "{\"fn\": \"validate\", \"instance\": 1, \"schema\": {\"type\": \"integer\"}}\n\
                 {\"fn\": \"split_message\", \"message\": \"a\"}\n";
    let mut out = Vec::new();
    let code = symphony::cli::serve_main(&mut input.as_bytes(), &mut out);
    assert_eq!(code, 0);
    let answers: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        answers,
        [
            json!({"error": "value", "message": REFUSED}),
            json!({"ok": ["a", ""]})
        ]
    );
}
