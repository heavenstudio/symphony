//! Validate local schema constraints and stable diagnostic ordering.

use serde_json::{Value, json};
use symphony_schema::{Schema, SchemaError, validate};

fn errors(instance: &Value, schema: &Value) -> Vec<String> {
    validate(instance, schema).expect("fixture schema is accepted")
}

fn valid(instance: &Value, schema: &Value) -> bool {
    errors(instance, schema).is_empty()
}

fn refused(schema: &Value) -> SchemaError {
    validate(&json!(1), schema).expect_err("invalid fixture schema is refused")
}

// --- types ---

#[test]
fn each_type() {
    for (value, name) in [
        (json!("a"), "string"),
        (json!(1), "integer"),
        (json!(1.5), "number"),
        (json!(true), "boolean"),
        (json!([]), "array"),
        (json!({}), "object"),
        (json!(null), "null"),
    ] {
        assert!(valid(&value, &json!({"type": name})), "{name}");
    }
}

#[test]
fn a_wrong_type_names_the_path() {
    let schema = json!({"type": "object", "properties": {"a": {"type": "object", "properties": {"b": {"type": "string"}}}}});
    let found = errors(&json!({"a": {"b": 3}}), &schema);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("$.a.b"), "{found:?}");
    assert!(found[0].contains("string"), "{found:?}");
}

#[test]
fn booleans_are_not_integers() {
    assert!(!valid(&json!(true), &json!({"type": "integer"})));
}

#[test]
fn type_as_a_list_accepts_any_named_type() {
    let schema = json!({"type": ["array", "null"], "minItems": 2});
    assert!(valid(&json!(null), &schema));
    assert!(valid(&json!([1, 2]), &schema));
    assert!(!valid(&json!([1]), &schema));
    assert!(!valid(&json!("x"), &schema));
}

// --- objects ---

fn object_schema() -> Value {
    json!({"type": "object", "required": ["x"], "properties": {"x": {"type": "integer"}}, "additionalProperties": false})
}

#[test]
fn required_and_additional() {
    let schema = object_schema();
    assert!(valid(&json!({"x": 1}), &schema));
    assert!(errors(&json!({}), &schema)[0].contains('x'));
    assert!(errors(&json!({"x": 1, "y": 2}), &schema)[0].contains('y'));
}

#[test]
fn additional_properties_as_a_schema_and_property_names() {
    let schema = json!({"type": "object", "propertyNames": {"pattern": "^[A-Z]{3}$"}, "additionalProperties": {"type": "string"}});
    assert!(valid(&json!({"ABC": "x"}), &schema));
    assert!(!valid(&json!({"abc": "x"}), &schema));
    assert!(!valid(&json!({"ABC": 1}), &schema));
}

// --- values ---

#[test]
fn enum_pattern_and_const() {
    assert!(valid(&json!("up small"), &json!({"enum": ["up small"]})));
    assert!(!valid(&json!("up huge"), &json!({"enum": ["up small"]})));
    let colour = json!({"type": "string", "pattern": "^#[0-9a-f]{6}$"});
    assert!(valid(&json!("#2f5d9e"), &colour));
    assert!(!valid(&json!("blue"), &colour));
    assert!(!valid(&json!(2), &json!({"const": 1})));
}

#[test]
fn arrays() {
    let schema = json!({"type": "array", "minItems": 1, "items": {"type": "string"}});
    assert!(valid(&json!(["a"]), &schema));
    assert!(!valid(&json!([]), &schema));
    assert!(errors(&json!(["a", 2]), &schema)[0].contains("$[1]"));
}

#[test]
fn a_path_reads_each_step_as_its_container_holds_it() {
    // An object key that reads as a number is still a key, not an index.
    let schema = json!({"type": "object", "additionalProperties": {"type": "array", "items": {"type": "string"}}});
    let found = errors(&json!({"0": ["a", 1]}), &schema);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("$.0[1]: "), "{found:?}");
}

// --- refs ---

#[test]
fn refs_resolve_against_defs() {
    let schema = json!({"$defs": {"code": {"type": "string", "pattern": "^[A-Z]{3}$"}}, "type": "array", "items": {"$ref": "#/$defs/code"}});
    assert!(valid(&json!(["ABC"]), &schema));
    assert!(!valid(&json!(["abc"]), &schema));
}

#[test]
fn a_keyword_the_draft_lacks_is_refused_rather_than_ignored() {
    // The crate enforces all of draft 2020-12, so what it refuses is a keyword the draft does not
    // have: a misspelt one would otherwise check nothing.
    assert!(refused(&json!({"minItem": 1})).0.contains("minItem"));
    let nested = json!({"type": "object", "properties": {"a": {"type": "array", "minItem": 1}}});
    assert!(refused(&nested).0.contains("minItem"));
    let in_defs =
        json!({"$defs": {"code": {"patern": "^[A-Z]{3}$"}}, "items": {"$ref": "#/$defs/code"}});
    assert!(refused(&in_defs).0.contains("patern"));
}

#[test]
fn every_keyword_of_the_draft_is_enforced() {
    let one_of = json!({"oneOf": [{"type": "integer"}, {"type": "number"}]});
    assert!(
        !valid(&json!(1), &one_of),
        "1 is both an integer and a number"
    );
    assert!(valid(&json!(1.5), &one_of));
    let format = json!({"type": "string", "format": "date"});
    assert!(valid(&json!("2026-10-20"), &format));
    assert!(!valid(&json!("yesterday"), &format));
}

#[test]
fn a_schema_the_draft_does_not_take_is_refused() {
    assert!(refused(&json!({"type": "strin"})).0.contains("strin"));
}

// --- numbers ---

#[test]
fn minimum_rejects_below_boundary_and_nonfinite() {
    let schema = json!({"type": "number", "minimum": 0.1});
    for value in [json!(0), json!(-1), json!(0.09)] {
        assert!(!valid(&value, &schema), "{value}");
    }
    assert!(valid(&json!(0.1), &schema));
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(Value::from(value), Value::Null);
        assert_eq!(
            errors(&Value::from(value), &schema),
            ["$: expected number, got NoneType"]
        );
    }
    for text in ["NaN", "Infinity", "-Infinity"] {
        assert!(serde_json::from_str::<Value>(text).is_err(), "{text}");
    }
    // Arbitrary-precision JSON retains overflow tokens; rejecting them is the validator's job.
    for text in [
        "1e400".to_string(),
        "-1e400".to_string(),
        "1.0e400".to_string(),
        "1.7976931348623159e308".to_string(),
        format!("{}.0", "9".repeat(400)),
    ] {
        let value: Value = serde_json::from_str(&text).unwrap();
        for rule in [&schema, &json!(true), &json!({"anyOf": [true, schema]})] {
            assert_eq!(
                errors(&value, rule),
                ["$: invalid number (must be finite)"],
                "{text}: {rule}"
            );
        }
    }
}

#[test]
fn nonfinite_tokens_name_nested_paths_and_are_refused_in_schemas() {
    let value = parsed(r#"{"z": [1, 1e400], "a": {"0": -1e400}, "text": "1e400"}"#);
    assert_eq!(
        errors(&value, &json!(true)),
        [
            "$.z[1]: invalid number (must be finite)",
            "$.a.0: invalid number (must be finite)"
        ]
    );
    let schema = parsed(r#"{"type": "number", "minimum": 1e400}"#);
    assert_eq!(
        Schema::compile(&schema).err().unwrap().0,
        "$.minimum: invalid number (must be finite)"
    );
    let schema = parsed(r#"{"$defs": {"row": {"maximum": -1e400}}, "minimum": 1e400}"#);
    assert_eq!(
        Schema::compile(&schema).err().unwrap().0,
        "$.$defs.row.maximum: invalid number (must be finite); $.minimum: invalid number (must be finite)"
    );
}

#[test]
fn finite_float_tokens_and_arbitrary_precision_integers_remain_valid() {
    let number = json!({"type": "number"});
    let integer = json!({"type": "integer"});
    for text in ["1e308", "-1e308", "1e-400", "0.0", "1.7976931348623157e308"] {
        assert!(valid(&parsed(text), &number), "{text}");
    }
    // A token written as an integer stays an integer, even beyond f64's range.
    for (text, nonnegative) in [
        (format!("1{}", "0".repeat(400)), true),
        (format!("-1{}", "0".repeat(400)), false),
    ] {
        let value = parsed(&text);
        assert!(valid(&value, &integer), "{text}");
        assert!(valid(&value, &number), "{text}");
        assert_eq!(valid(&value, &json!({"minimum": 0})), nonnegative, "{text}");
    }
}

// --- conditional fields ---

#[test]
fn dependent_required_and_not() {
    let schema = json!({"type": "object", "dependentRequired": {"tip": ["projects"]},
                        "not": {"required": ["tip", "members"]}});
    assert!(valid(&json!({"tip": [1, 2], "projects": true}), &schema));
    assert!(
        errors(&json!({"tip": [1, 2]}), &schema)
            .iter()
            .any(|e| e.contains("projects"))
    );
    assert!(
        errors(
            &json!({"tip": [1, 2], "projects": true, "members": []}),
            &schema
        )
        .iter()
        .any(|e| e.contains("not"))
    );
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).expect("fixture JSON is valid")
}

mod diagnostics;
