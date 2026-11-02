//! Validate every repository settings file against its declared schema.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use symphony::schema::{Schema, SchemaError, validate};

fn errors(instance: &Value, schema: &Value) -> Vec<String> {
    validate(instance, schema).unwrap_or_else(|e| panic!("the schema was refused: {e}"))
}

fn valid(instance: &Value, schema: &Value) -> bool {
    errors(instance, schema).is_empty()
}

fn refused(schema: &Value) -> SchemaError {
    match validate(&json!(1), schema) {
        Err(problem) => problem,
        Ok(found) => panic!("{schema} was taken as a schema (errors: {found:?})"),
    }
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

// --- the lines ---
//
// The crate judges; each problem of a core keyword is written in Python's terms, in the lines
// `content --check`, `ship_art --check` and `just relationships` print.

#[test]
fn a_wrong_type_reads_expected_and_got_with_python_s_type_names() {
    let nested = json!({"type": "object", "properties": {"a": {"type": "object", "properties": {"b": {"type": "string"}}}}});
    assert_eq!(
        errors(&json!({"a": {"b": 3}}), &nested),
        ["$.a.b: expected string, got int"]
    );
    for (value, kind, got) in [
        (json!(null), "object", "NoneType"),
        (json!(1.5), "integer", "float"),
        (json!(true), "integer", "bool"),
        (json!([]), "object", "list"),
        (json!({}), "array", "dict"),
        (json!("x"), "number", "str"),
    ] {
        assert_eq!(
            errors(&value, &json!({"type": kind})),
            [format!("$: expected {kind}, got {got}")]
        );
    }
    assert_eq!(
        errors(&json!("x"), &json!({"type": ["array", "null"]})),
        ["$: expected ['array', 'null'], got str"]
    );
}

#[test]
fn a_wrong_value_reads_as_python_wrote_it() {
    let at_least = json!({"type": "number", "minimum": 0.1});
    assert_eq!(errors(&json!(0), &at_least), ["$: must be at least 0.1"]);
    assert_eq!(
        errors(&json!(-1), &json!({"minimum": 0})),
        ["$: must be at least 0"]
    );
    assert_eq!(errors(&json!(2), &json!({"const": 1})), ["$: must be 1"]);
    assert_eq!(
        errors(&json!("b"), &json!({"const": "a"})),
        ["$: must be 'a'"]
    );
    assert_eq!(
        errors(
            &json!("up huge"),
            &json!({"enum": ["up small", "down small"]})
        ),
        ["$: 'up huge' is not one of 'up small', 'down small'"]
    );
    assert_eq!(
        errors(
            &json!("blue"),
            &json!({"type": "string", "pattern": "^#[0-9a-f]{6}$"})
        ),
        ["$: 'blue' does not match ^#[0-9a-f]{6}$"]
    );
    assert_eq!(
        errors(&json!([]), &json!({"type": "array", "minItems": 1})),
        ["$: needs at least 1 item(s)"]
    );
}

#[test]
fn an_object_s_problems_read_as_python_wrote_them() {
    let schema = object_schema();
    assert_eq!(errors(&json!({}), &schema), ["$: missing required 'x'"]);
    assert_eq!(
        errors(&json!({"x": 1, "y": 2}), &schema),
        ["$: 'y' is not allowed here"]
    );
    let mut two = errors(&json!({"x": 1, "z": 3, "y": 2}), &schema);
    two.sort();
    assert_eq!(
        two,
        ["$: 'y' is not allowed here", "$: 'z' is not allowed here"]
    );
    let nested = json!({"type": "object", "properties": {"a": schema}});
    assert_eq!(
        errors(&json!({"a": {"x": 1, "y": 2}}), &nested),
        ["$.a: 'y' is not allowed here"]
    );
    let names = json!({"type": "object", "propertyNames": {"pattern": "^[A-Z]{3}$"}});
    assert_eq!(
        errors(&json!({"ABC": 1, "abc": 2}), &names),
        ["$.abc (name): 'abc' does not match ^[A-Z]{3}$"]
    );
}

#[test]
fn a_conditional_field_s_problems_read_as_python_wrote_them() {
    let schema = json!({"type": "object", "dependentRequired": {"tip": ["projects"]},
                        "not": {"required": ["tip", "members"]}});
    assert_eq!(
        errors(&json!({"tip": [1, 2]}), &schema),
        ["$: 'tip' requires 'projects'"]
    );
    assert_eq!(
        errors(
            &json!({"tip": [1, 2], "projects": true, "members": []}),
            &schema
        ),
        ["$: must not match forbidden schema"]
    );
    // Reached through a `$ref`, the trigger is still the one the instance holds.
    let referred = json!({"$defs": {"mount": {"type": "object", "dependentRequired": {"arc": ["bearing"], "tip": ["projects"]}}},
                          "type": "object", "properties": {"a": {"$ref": "#/$defs/mount"}}});
    assert_eq!(
        errors(&json!({"a": {"tip": 1}}), &referred),
        ["$.a: 'tip' requires 'projects'"]
    );
}

#[test]
fn a_value_in_a_line_is_written_as_python_wrote_it() {
    assert_eq!(
        errors(
            &json!(0),
            &json!({"const": {"a": [1, 1.5, true, null, "it's"]}})
        ),
        [r#"$: must be {'a': [1, 1.5, True, None, "it's"]}"#]
    );
    assert_eq!(
        errors(&json!("x"), &json!({"enum": ["a\\b", "é\n", "a'b\"c"]})),
        [r#"$: 'x' is not one of 'a\\b', 'é\n', 'a\'b"c'"#]
    );
}

#[test]
fn a_keyword_python_did_not_know_keeps_the_crate_s_line() {
    let found = errors(
        &json!("yesterday"),
        &json!({"type": "string", "format": "date"}),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("$: ") && found[0].contains("date"),
        "{found:?}"
    );
}

// --- the verdicts and the order ---
//
// Draft 2020-12 counts 1.0 an integer and checks every keyword of a node; the crate takes an integer
// as Python's `int` (1.0 is a float), stops at a node's wrong type, and goes member by member in the
// file's order.

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn an_integer_is_written_without_a_fraction_as_python_read_it() {
    let integer = json!({"type": "integer"});
    for text in ["1.0", "181.0", "1e2", "-3.0"] {
        assert_eq!(
            errors(&parsed(text), &integer),
            ["$: expected integer, got float"],
            "{text}"
        );
    }
    assert!(valid(&parsed("181"), &integer));
    assert_eq!(
        errors(&parsed("1.0"), &json!({"type": ["integer", "null"]})),
        ["$: expected ['integer', 'null'], got float"]
    );
    for text in ["1", "1.0", "1.5"] {
        assert!(valid(&parsed(text), &json!({"type": "number"})), "{text}");
    }
    let hull =
        json!({"type": "object", "properties": {"beam_m": {"type": "integer", "minimum": 0}}});
    assert_eq!(
        errors(&parsed(r#"{"beam_m": 181.0}"#), &hull),
        ["$.beam_m: expected integer, got float"]
    );
    // Under `not` too: 1.0 is not an integer, so it does not match the forbidden schema.
    let not_integer = json!({"not": {"type": "integer"}});
    assert!(valid(&parsed("1.0"), &not_integer));
    assert_eq!(
        errors(&parsed("1"), &not_integer),
        ["$: must not match forbidden schema"]
    );
}

#[test]
fn a_wrong_type_is_the_one_line_python_wrote_for_its_node() {
    for (value, schema, line) in [
        (
            json!(4),
            json!({"type": "string", "enum": ["a", "b"]}),
            "$: expected string, got int",
        ),
        (
            json!(5),
            json!({"type": "string", "const": "a"}),
            "$: expected string, got int",
        ),
        (
            json!(5),
            json!({"type": "string", "pattern": "^a$"}),
            "$: expected string, got int",
        ),
        (
            json!({"a": 1}),
            json!({"type": "array", "minItems": 2, "items": {"type": "string"}}),
            "$: expected array, got dict",
        ),
        (
            json!(1.5),
            json!({"type": "integer", "minimum": 2}),
            "$: expected integer, got float",
        ),
    ] {
        assert_eq!(errors(&value, &schema), [line], "{schema}");
    }
    let row =
        json!({"type": "object", "additionalProperties": {"type": "string", "enum": ["up small"]}});
    assert_eq!(
        errors(&json!({"ARM": 5}), &row),
        ["$.ARM: expected string, got int"]
    );
}

#[test]
fn a_not_is_checked_before_its_node_s_type_as_python_did() {
    assert_eq!(
        errors(&json!(3), &json!({"type": "string", "not": {"const": 3}})),
        [
            "$: must not match forbidden schema",
            "$: expected string, got int"
        ]
    );
}

#[test]
fn a_ref_s_lines_survive_its_node_s_wrong_type_ordered_with_the_node_s_own() {
    // A wrong type hides none of a `$ref`'s lines. The crate names a keyword reached through a
    // `$ref` by where it is written, so its line is ordered as the node's own.
    let referred = json!({"$defs": {"e": {"enum": ["a"]}},
                          "type": "object", "properties": {"a": {"$ref": "#/$defs/e", "type": "string"}}});
    assert_eq!(
        errors(&json!({"a": 5}), &referred),
        ["$.a: expected string, got int", "$.a: 5 is not one of 'a'"]
    );
}

#[test]
fn lines_come_member_by_member_in_the_file_s_order() {
    let two =
        json!({"type": "object", "properties": {"a": {"type": "string"}, "z": {"type": "string"}}});
    assert_eq!(
        errors(&parsed(r#"{"z": 1, "a": 2}"#), &two),
        [
            "$.z: expected string, got int",
            "$.a: expected string, got int"
        ]
    );
    assert_eq!(
        errors(&parsed(r#"{"y": 2, "x": "s", "z": 3}"#), &object_schema()),
        [
            "$: 'y' is not allowed here",
            "$.x: expected integer, got str",
            "$: 'z' is not allowed here"
        ]
    );
    let nested = json!({"type": "object", "properties": {
        "b": {"type": "object", "properties": {"k": {"type": "string"}, "j": {"type": "string"}}},
        "a": {"type": "string"}}});
    assert_eq!(
        errors(&parsed(r#"{"b": {"k": 1, "j": 2}, "a": 3}"#), &nested),
        [
            "$.b.k: expected string, got int",
            "$.b.j: expected string, got int",
            "$.a: expected string, got int"
        ]
    );
}

#[test]
fn a_node_s_own_lines_come_before_its_members_and_items() {
    let object = json!({"type": "object", "required": ["r"], "dependentRequired": {"t": ["u"]},
                        "properties": {"t": {"type": "string"}}});
    assert_eq!(
        errors(&parsed(r#"{"t": 1}"#), &object),
        [
            "$: 't' requires 'u'",
            "$: missing required 'r'",
            "$.t: expected string, got int"
        ]
    );
    let array = json!({"type": "array", "minItems": 3, "items": {"type": "string"}});
    assert_eq!(
        errors(&parsed(r#"[1, "a"]"#), &array),
        [
            "$: needs at least 3 item(s)",
            "$[0]: expected string, got int"
        ]
    );
    let named = json!({"type": "object", "propertyNames": {"pattern": "^[a-z]+$"},
                       "additionalProperties": {"type": "string"}});
    assert_eq!(
        errors(&parsed(r#"{"B": 1, "c": 2}"#), &named),
        [
            "$.B (name): 'B' does not match ^[a-z]+$",
            "$.B: expected string, got int",
            "$.c: expected string, got int"
        ]
    );
}

#[test]
fn two_fields_requiring_one_missing_field_each_name_their_own() {
    let schema = json!({"type": "object", "dependentRequired": {"a": ["x"], "b": ["x"]}});
    assert_eq!(
        errors(&parsed(r#"{"b": 2, "a": 1}"#), &schema),
        ["$: 'a' requires 'x'", "$: 'b' requires 'x'"]
    );
    let two = json!({"type": "object", "dependentRequired": {"a": ["x", "y"]}});
    assert_eq!(
        errors(&json!({"a": 1}), &two),
        ["$: 'a' requires 'x'", "$: 'a' requires 'y'"]
    );
}

#[test]
fn a_pattern_s_end_takes_one_final_newline_as_python_s_did() {
    let code = json!({"type": "string", "pattern": "^[A-Z]{3}$"});
    assert!(valid(&json!("ABC\n"), &code));
    assert_eq!(
        errors(&json!("ABC\nD"), &code),
        [r"$: 'ABC\nD' does not match ^[A-Z]{3}$"]
    );
    assert_eq!(
        errors(&json!("ABC\n\n"), &code),
        [r"$: 'ABC\n\n' does not match ^[A-Z]{3}$"]
    );
    let either = json!({"type": "string", "pattern": "^([A-Z]{3}|self|others)$"});
    assert!(valid(&json!("self\n"), &either));
    // A `$` escaped or in a class is a dollar sign.
    let dollar = json!({"type": "string", "pattern": r"^a\$[$]$"});
    assert!(valid(&json!("a$$"), &dollar));
    assert!(!valid(&json!("a\n$"), &dollar));
}

#[test]
fn a_misspelt_keyword_is_the_only_one_named() {
    assert_eq!(
        refused(&json!({"minItem": 1})).0,
        "unsupported keyword(s): minItem"
    );
    assert_eq!(
        refused(&json!({"maxItem": 2, "minItem": 1})).0,
        "unsupported keyword(s): maxItem, minItem"
    );
    let nested =
        json!({"type": "object", "properties": {"mounts": {"type": "array", "minItem": 1}}});
    assert_eq!(
        refused(&nested).0,
        "unsupported keyword(s): minItem (at $.properties.mounts)"
    );
    let role = json!({"type": "object", "additionalProperties": false,
                      "properties": {"role": {"type": "string", "minLenght": 1}}});
    assert_eq!(
        refused(&role).0,
        "unsupported keyword(s): minLenght (at $.properties.role)"
    );
    let in_defs = json!({"$schema": "https://json-schema.org/draft/2020-12/schema", "$id": "x.schema.json",
                         "$defs": {"row": {"type": "object", "propertyNames": {"pattern": "^[A-Z]{3}$"},
                                           "items": {"minItem": 1}}},
                         "type": "object", "additionalProperties": {"$ref": "#/$defs/row"}});
    assert_eq!(
        refused(&in_defs).0,
        "unsupported keyword(s): minItem (at $.$defs.row.items)"
    );
}

// --- the repository's own files ---

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every `.json` under `dir`, recursively, sorted.
fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

// The verdict on every file the repository checks against a schema (`just relationships`,
// `screens --check`, `ship_art --check`, `content --check`, the hull outline check): all valid.
// Split three ways, so the harness runs them side by side; the hull outlines, 12 MB of notes, also
// share their files out.

/// Each file of `files` that names its schema by a relative `$schema`, checked against it; the names
/// of the schemas used.
fn check_named(files: &[PathBuf]) -> BTreeSet<String> {
    let mut compiled: BTreeMap<PathBuf, Schema> = BTreeMap::new();
    let mut used = BTreeSet::new();
    for path in files {
        let data = read_json(path);
        let Some(named) = data.get("$schema").and_then(Value::as_str) else {
            continue;
        };
        if named.starts_with("https://") {
            continue; // a schema itself, checked against the draft when it is used
        }
        let schema_path = path.parent().unwrap().join(named).canonicalize().unwrap();
        let schema = compiled
            .entry(schema_path.clone())
            .or_insert_with(|| Schema::compile(&read_json(&schema_path)).unwrap());
        let found = schema.errors(&data);
        assert!(found.is_empty(), "{}: {found:?}", path.display());
        used.insert(
            schema_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
    }
    used
}

const OUTLINES: &str = "docs/data/hull-outlines";

#[test]
fn every_settings_file_matches_the_schema_it_names_as_python_found() {
    let outlines = repo().join(OUTLINES);
    let files: Vec<PathBuf> = json_files(&repo().join("docs/data"))
        .into_iter()
        .filter(|path| !path.starts_with(&outlines))
        .collect();
    let mut used = check_named(&files);
    used.insert("hull-outline-notes.schema.json".to_string()); // the next test's
    for schema in json_files(&repo().join("docs/data/schemas")) {
        let name = schema.file_name().unwrap().to_string_lossy().to_string();
        assert!(used.contains(&name), "no file is checked against {name}");
    }
}

#[test]
fn every_hull_outline_note_matches_its_schema_as_python_found() {
    let files = json_files(&repo().join(OUTLINES));
    assert!(files.len() > 100, "{} notes", files.len());
    let used: BTreeSet<String> = std::thread::scope(|scope| {
        let shares: Vec<_> = files
            .chunks(files.len().div_ceil(4))
            .map(|share| scope.spawn(|| check_named(share)))
            .collect();
        shares
            .into_iter()
            .flat_map(|share| share.join().unwrap())
            .collect()
    });
    assert_eq!(
        used,
        BTreeSet::from(["hull-outline-notes.schema.json".to_string()])
    );
}

#[test]
fn every_content_file_matches_its_folder_s_schema_as_python_found() {
    for (folder, name) in [
        ("classes", "class"),
        ("factions", "faction"),
        ("subfactions", "subfaction"),
        ("hulls", "hull"),
        ("hull_stats", "hull_stats"),
        ("class_stats", "class_stats"),
        ("role_stats", "role_stats"),
        ("gear", "gear"),
    ] {
        let schema_path = repo().join(format!("content/schemas/{name}.schema.json"));
        let schema = Schema::compile(&read_json(&schema_path)).unwrap();
        let files = json_files(&repo().join("content").join(folder));
        assert!(!files.is_empty(), "content/{folder} holds no file");
        for path in files {
            let found = schema.errors(&read_json(&path));
            assert!(found.is_empty(), "{}: {found:?}", path.display());
        }
    }
}

#[test]
fn gear_schema_types_and_component_icon_keys_agree_in_both_directions() {
    let catalogue = read_json(&repo().join("symphony/crates/symphony/src/component_icons.json"));
    let schema = read_json(&repo().join("content/schemas/gear.schema.json"));
    let icons = catalogue["gear"].as_array().unwrap();
    let types = schema["properties"]["type"]["enum"].as_array().unwrap();
    let icon_keys: BTreeSet<_> = icons
        .iter()
        .map(|icon| icon["key"].as_str().unwrap())
        .collect();
    let type_keys: BTreeSet<_> = types.iter().map(|value| value.as_str().unwrap()).collect();
    assert!(!icon_keys.is_empty());
    assert_eq!(icon_keys.len(), icons.len(), "duplicate gear icon keys");
    assert_eq!(type_keys.len(), types.len(), "duplicate gear schema types");
    assert_eq!(
        icon_keys, type_keys,
        "gear icons and content schema types must match"
    );
}
