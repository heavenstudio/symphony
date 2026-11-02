//! What `symphony serve` answers for the schema check, the relationship tables and the screen
//! catalogue.

use std::path::Path;

use serde_json::{Value, json};

use crate::common::Tree;

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

fn repo() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .to_string_lossy()
        .to_string()
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

#[test]
fn relationship_tables_answer_sides_rows_and_groups() {
    let tables = ok(json!({"fn": "relationship_tables", "root": repo()}));
    let sides = tables["sides"].as_array().unwrap();
    assert!(sides.len() > 10, "{tables}");
    let first = &sides[0];
    for field in ["code", "icon", "name", "faction", "colour"] {
        assert!(first[field].is_string(), "{field}: {first}");
    }
    let code = first["code"].as_str().unwrap();
    for table in ["destroy", "help"] {
        let row = tables["rows"][table][code].as_array().unwrap();
        assert_eq!(row[0][0], code, "{table}: the side itself first");
        assert!(row[0][1].is_f64(), "{table}: {row:?}");
        assert!(tables["groups"][table].is_object(), "{table}");
    }
}

#[test]
fn relationship_tables_that_do_not_load_are_a_value_error() {
    let tree = Tree::new();
    let root = tree.root.to_string_lossy().to_string();
    let answer = call(json!({"fn": "relationship_tables", "root": root}));
    assert_eq!(answer["error"], "value", "{answer}");
    assert!(
        answer["message"]
            .as_str()
            .unwrap()
            .contains("factions.json"),
        "{answer}"
    );
}

#[test]
fn a_group_answers_its_effect_its_sides_and_who_it_leaves_out() {
    let tree = Tree::new();
    tree.write(
        "docs/data/factions.json",
        &json!({"factions": [
            {"name": "A", "colour": "#000000", "subfactions": [{"code": "AAA", "icon": "a", "name": "Aa"}]},
            {"name": "B", "colour": "#ffffff", "subfactions": [{"code": "BBB", "icon": "b", "name": "Bb"},
                                                               {"code": "CCC", "icon": "c", "name": "Cc"}]}]})
        .to_string(),
    );
    tree.write(
        "docs/data/relationships.json",
        &json!({"self": {"destroy": "down strong", "help": "up strong"},
                "destroy": {"AAA": {"others": {"effect": "up small"}}, "BBB": {}, "CCC": {}},
                "help": {"AAA": {"others": {"effect": "down small", "except_factions": ["B"]}}, "BBB": {}, "CCC": {}}})
        .to_string(),
    );
    let root = tree.root.to_string_lossy().to_string();
    let tables = ok(json!({"fn": "relationship_tables", "root": root}));
    assert_eq!(
        tables["groups"]["destroy"]["AAA"],
        json!([0.5, ["BBB", "CCC"], []])
    );
    assert_eq!(tables["groups"]["help"]["AAA"], json!([-0.5, [], ["B"]]));
    assert_eq!(
        tables["rows"]["destroy"]["AAA"],
        json!([["AAA", -2.0], ["BBB", 0.5], ["CCC", 0.5]])
    );
}

#[test]
fn screen_problems_read_the_catalogue_under_a_root() {
    assert_eq!(
        ok(json!({"fn": "screen_problems", "root": repo()})),
        json!([])
    );
    let tree = Tree::new();
    tree.write(
        "view/ui_router.gd",
        "static func named_keys(name: String) -> Keys:\n\t\"enter\":\n\n\n",
    );
    tree.write(
        "docs/data/screens.json",
        &json!({"version": 1, "resolution": "1280x800",
                "screens": [{"id": "one", "title": "One", "seed": 7, "script": [["escape"]]}]})
        .to_string(),
    );
    let root = tree.root.to_string_lossy().to_string();
    assert_eq!(
        ok(json!({"fn": "screen_problems", "root": root})),
        json!(["one: step 0 presses escape, which the router does not name"])
    );
}

#[test]
fn film_document_answers_the_script_film_gd_reads() {
    assert_eq!(
        ok(json!({"fn": "film_document", "entry": {"script": [["enter"], {"hold": 2}]}})),
        json!({"version": 1, "inputs": [{"ui": ["enter"]}, {}, {}]})
    );
}

#[test]
fn read_manifest_answers_the_manifest_or_null() {
    let tree = Tree::new();
    let out = tree.root.to_string_lossy().to_string();
    assert_eq!(ok(json!({"fn": "read_manifest", "out": out})), Value::Null);
    tree.write(
        "manifest.json",
        r#"{"commit": "c", "date": "d", "screens": {}}"#,
    );
    assert_eq!(
        ok(json!({"fn": "read_manifest", "out": out})),
        json!({"commit": "c", "date": "d", "screens": {}})
    );
}
