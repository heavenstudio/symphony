//! Pin schema diagnostic wording and traversal order across keywords and references.

use super::*;

// --- the lines ---
//
// Each problem of a core keyword has the same diagnostic across callers.

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
    let record =
        json!({"type": "object", "properties": {"beam_m": {"type": "integer", "minimum": 0}}});
    assert_eq!(
        errors(&parsed(r#"{"beam_m": 181.0}"#), &record),
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
