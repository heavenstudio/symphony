//! Relationship settings, table derivation and validation.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use symphony::relationships::{
    Declaration, Settings, Tables, check_main, load, parse_effect, schema_errors, tables,
};

use crate::common::Tree;

fn factions() -> Value {
    json!({
        "version": 1,
        "factions": [
            {"name": "Republic", "colour": "#2f5d9e", "subfactions": [
                {"code": "HOM", "icon": "⚓", "name": "Home Fleet"},
                {"code": "SYN", "icon": "🏴", "name": "Free Syndicate"}]},
            {"name": "Machine", "colour": "#2a9d8f", "subfactions": [
                {"code": "CNS", "icon": "🔷", "name": "Consensus"}]},
            {"name": "Biological", "colour": "#7a3d9e", "subfactions": [
                {"code": "ASM", "icon": "🩸", "name": "Assimilators"}]},
        ],
    })
}

fn data() -> Value {
    json!({
        "version": 1,
        "self": {"destroy": "down strong", "help": "up strong"},
        "destroy": {
            "HOM": {"SYN": "up forces a side"},
            "SYN": {"self": "down medium", "HOM": "up strong"},
            "CNS": {},
            "ASM": {"self": "down slight", "others": {"effect": "up small", "except_factions": ["Machine"]}},
        },
        "help": {
            "HOM": {"SYN": "down forces a side"},
            "SYN": {"HOM": "down forces a side"},
            "CNS": {},
            "ASM": {"others": {"effect": "down small", "except_factions": ["Machine"]}},
        },
    })
}

/// `data()` with `table`'s row for `code` replaced (or removed, for `None`).
fn with_row(table: &str, code: &str, row: Option<Value>) -> Value {
    let mut changed = data();
    let rows = changed[table].as_object_mut().unwrap();
    match row {
        Some(row) => {
            rows.insert(code.to_string(), row);
        }
        None => {
            rows.remove(code);
        }
    }
    changed
}

fn load_from(declaration: &Value, settings: &Value) -> Result<Tables, String> {
    let declaration: Declaration = serde_json::from_value(declaration.clone()).unwrap();
    let settings: Settings = serde_json::from_value(settings.clone()).unwrap();
    load(&declaration, &settings).map_err(|e| e.0)
}

fn loaded() -> Tables {
    load_from(&factions(), &data()).unwrap()
}

fn row(tables: &Tables, table: &str, code: &str) -> Vec<(String, f64)> {
    tables.row(table, code).unwrap().effects.clone()
}

fn pair(code: &str, value: f64) -> (String, f64) {
    (code.to_string(), value)
}

// --- effects ---

#[test]
fn words_map_to_signed_sizes() {
    assert_eq!(parse_effect("up forces a side"), Ok(3.0));
    assert_eq!(parse_effect("down strong"), Ok(-2.0));
    assert_eq!(parse_effect("up medium"), Ok(1.0));
    assert_eq!(parse_effect("down small"), Ok(-0.5));
    assert_eq!(parse_effect("up slight"), Ok(0.25));
}

#[test]
fn unknown_words_are_refused() {
    for bad in ["up", "sideways strong", "down huge", ""] {
        assert!(parse_effect(bad).is_err(), "{bad:?}");
    }
}

// --- load ---

#[test]
fn the_side_itself_comes_first_with_its_default() {
    let tables = loaded();
    assert_eq!(row(&tables, "destroy", "HOM")[0], pair("HOM", -2.0));
    assert_eq!(row(&tables, "destroy", "SYN")[0], pair("SYN", -1.0));
    assert_eq!(row(&tables, "help", "CNS"), vec![pair("CNS", 2.0)]);
}

#[test]
fn others_expands_to_every_unlisted_side_outside_the_excepted_factions() {
    assert_eq!(
        row(&loaded(), "destroy", "ASM"),
        vec![pair("ASM", -0.25), pair("HOM", 0.5), pair("SYN", 0.5)]
    );
}

#[test]
fn an_unknown_code_is_refused() {
    let settings = with_row("destroy", "HOM", Some(json!({"XXX": "up small"})));
    assert!(
        load_from(&factions(), &settings)
            .unwrap_err()
            .contains("XXX")
    );
}

#[test]
fn every_side_needs_a_row_in_both_tables() {
    let problem = load_from(&factions(), &with_row("destroy", "CNS", None)).unwrap_err();
    assert!(
        problem.contains("destroy") && problem.contains("CNS"),
        "{problem}"
    );
    assert!(problem.find("destroy") < problem.find("CNS"), "{problem}");
}

#[test]
fn a_side_cannot_list_itself_by_code() {
    let settings = with_row("help", "HOM", Some(json!({"HOM": "up small"})));
    assert!(
        load_from(&factions(), &settings)
            .unwrap_err()
            .contains("self")
    );
}

#[test]
fn an_unknown_excepted_faction_is_refused() {
    let others = json!({"others": {"effect": "down small", "except_factions": ["Robots"]}});
    let settings = with_row("help", "ASM", Some(others));
    assert!(
        load_from(&factions(), &settings)
            .unwrap_err()
            .contains("Robots")
    );
}

#[test]
fn a_row_keeps_the_order_its_file_names_the_sides_in() {
    let text = r#"{"self": {"destroy": "down strong", "help": "up strong"},
        "destroy": {"HOM": {"SYN": "up small"}, "SYN": {"HOM": "up small"}, "CNS": {},
                    "ASM": {"SYN": "up slight", "HOM": "down slight", "CNS": "up medium"}},
        "help": {"HOM": {}, "SYN": {}, "CNS": {}, "ASM": {}}}"#;
    let declaration: Declaration = serde_json::from_value(factions()).unwrap();
    let tables = load(&declaration, &Settings::parse(text).unwrap()).unwrap();
    assert_eq!(
        row(&tables, "destroy", "ASM"),
        vec![
            pair("ASM", -2.0),
            pair("SYN", 0.25),
            pair("HOM", -0.25),
            pair("CNS", 1.0)
        ]
    );
}

#[test]
fn the_others_group_says_its_effect_its_sides_and_who_it_leaves_out() {
    let tables = loaded();
    let group = tables
        .row("destroy", "ASM")
        .unwrap()
        .others
        .clone()
        .unwrap();
    assert_eq!(group.value, 0.5);
    assert_eq!(group.members, ["HOM", "SYN"]);
    assert_eq!(group.excepted, ["Machine"]);
    assert!(tables.row("destroy", "HOM").unwrap().others.is_none());
}

#[test]
fn each_side_carries_its_faction_and_colour() {
    let tables = loaded();
    let side = tables.side("CNS").unwrap();
    assert_eq!(
        (
            side.icon.as_str(),
            side.name.as_str(),
            side.faction.as_str(),
            side.colour.as_str()
        ),
        ("🔷", "Consensus", "Machine", "#2a9d8f")
    );
    assert!(tables.side("XXX").is_none());
}

// --- the real files ---

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/data")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_real_files_match_their_schemas() {
    for name in ["factions.json", "relationships.json"] {
        let path = data_dir().join(name);
        assert_eq!(
            schema_errors(&read_json(&path), &path),
            Ok(Vec::new()),
            "{name}"
        );
    }
}

#[test]
fn the_real_files_load_together() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    tables(&root).unwrap();
}

#[test]
fn a_file_without_a_schema_is_refused() {
    let problem = schema_errors(
        &json!({"version": 1}),
        &data_dir().join("relationships.json"),
    )
    .unwrap_err();
    assert!(problem.0.contains("$schema"), "{problem:?}");
}

// --- `symphony relationships --check` ---

const DOC: &str = "intro\n\n[[relationships:destroy]]\n\nmid\n\n[[relationships:help]]\n\nend\n";

fn with_schema(name: &str, body: Value) -> String {
    let mut object = serde_json::Map::new();
    object.insert(
        "$schema".to_string(),
        json!(format!("schemas/{name}.schema.json")),
    );
    object.extend(body.as_object().unwrap().clone());
    Value::Object(object).to_string()
}

fn tree() -> Tree {
    let tree = Tree::new();
    for schema in ["factions", "relationships"] {
        let name = format!("{schema}.schema.json");
        let text = std::fs::read_to_string(data_dir().join("schemas").join(&name)).unwrap();
        tree.write(&format!("docs/data/schemas/{name}"), &text);
    }
    tree.write(
        "docs/data/factions.json",
        &with_schema("factions", factions()),
    );
    tree.write(
        "docs/data/relationships.json",
        &with_schema("relationships", data()),
    );
    tree.write("docs/factions/RELATIONSHIPS.md", DOC);
    tree
}

fn run(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = check_main(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn check_passes_on_settings_that_load_and_writes_nothing() {
    let tree = tree();
    let root = tree.root.to_string_lossy().to_string();
    let (code, out, _) = run(&["--root", &root, "--check"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "relationships: docs/data/relationships.json and docs/data/factions.json match their schemas\n"
    );
    assert_eq!(tree.read("docs/factions/RELATIONSHIPS.md"), DOC);
}

#[test]
fn the_tables_load_from_the_files_under_a_root() {
    let tree = tree();
    let codes: Vec<String> = tables(&tree.root)
        .unwrap()
        .sides
        .into_iter()
        .map(|side| side.code)
        .collect();
    assert_eq!(codes, ["HOM", "SYN", "CNS", "ASM"]);
}

#[test]
fn a_file_off_its_schema_fails() {
    let tree = tree();
    let mut settings = read_json(&tree.path("docs/data/relationships.json"));
    settings["colour"] = json!("red");
    tree.write("docs/data/relationships.json", &settings.to_string());
    let root = tree.root.to_string_lossy().to_string();
    let (code, _, err) = run(&["--root", &root, "--check"]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with(
            "✖ relationships: files do not match their schemas:\n  relationships.json $"
        ),
        "{err}"
    );
    assert!(err.contains("colour"), "{err}");
}

#[test]
fn a_file_off_its_schema_names_each_problem_once_in_the_file_s_order() {
    let tree = tree();
    tree.write(
        "docs/data/relationships.json",
        r#"{"$schema": "schemas/relationships.schema.json", "version": 2,
            "self": {"destroy": "down strong", "help": "up strong"},
            "destroy": {"HOM": {"SYN": 5}, "SYN": {"self": "down medium", "HOM": "up strong"},
                        "CNS": {}, "ASM": {"self": "down slight"}},
            "help": {"HOM": {"SYN": "down forces a side"}, "SYN": {"HOM": "down forces a side"},
                     "CNS": {}, "ASM": {}}}"#,
    );
    let root = tree.root.to_string_lossy().to_string();
    let (code, out, err) = run(&["--root", &root, "--check"]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert_eq!(
        err,
        "✖ relationships: files do not match their schemas:\n  \
         relationships.json $.version: must be 1\n  \
         relationships.json $.destroy.HOM.SYN: expected string, got int\n"
    );
}

#[test]
fn check_is_required_as_python_s_argparse_required_it() {
    let (code, _, err) = run(&[]);
    assert_eq!(code, 2);
    assert!(
        err.contains("the following arguments are required: --check"),
        "{err}"
    );
}
