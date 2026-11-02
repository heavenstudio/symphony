//! Screen catalogue, film-script and manifest reader cases.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Value, json};
use symphony::screens::{
    check_main, film_document, film_ticks, load_catalogue, problems, read_manifest, script_lines,
};

use crate::common::Tree;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn catalogue() -> Value {
    load_catalogue(&repo()).unwrap()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap()
}

// --- the real catalogue ---

#[test]
fn the_real_catalogue_matches_its_schema() {
    let schema: Value =
        serde_json::from_str(&read("docs/data/schemas/screens.schema.json")).unwrap();
    assert_eq!(
        symphony::schema::validate(&catalogue(), &schema),
        Ok(Vec::new())
    );
}

#[test]
fn the_real_catalogue_has_no_problem() {
    assert_eq!(problems(&catalogue(), &repo()), Vec::<String>::new());
}

fn covered() -> BTreeSet<(String, String, bool)> {
    catalogue()["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e.get("screen")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                e.get("tab")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                e.get("docked").and_then(Value::as_bool).unwrap_or(false),
            )
        })
        .collect()
}

#[test]
fn every_screen_constant_the_menu_state_has_is_in_it() {
    // Derived from view/ui_menu.gd, so a new `SCREEN_` constant fails here until it gets an entry.
    let constant = Regex::new(r"(?m)^const (SCREEN_[A-Z_]+): int").unwrap();
    let source = read("view/ui_menu.gd");
    let constants: BTreeSet<&str> = constant
        .captures_iter(&source)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    assert!(constants.len() > 15, "{constants:?}");
    let have: BTreeSet<String> = covered().into_iter().map(|(screen, _, _)| screen).collect();
    let missing: Vec<&&str> = constants.iter().filter(|c| !have.contains(**c)).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

/// The quoted upper-case labels inside what `pattern`'s first group takes of `source`.
fn labels(source: &str, pattern: &str) -> Vec<String> {
    let text = read(source);
    let body = Regex::new(pattern)
        .unwrap()
        .captures(&text)
        .unwrap_or_else(|| panic!("{pattern} in {source}"))
        .get(1)
        .unwrap()
        .as_str()
        .to_string();
    Regex::new(r#""([A-Z ]+)""#)
        .unwrap()
        .captures_iter(&body)
        .map(|c| c[1].to_string())
        .collect()
}

#[test]
fn every_tab_of_a_tabbed_screen_is_in_it() {
    // The tab lists are read from the screens' own sources: pause, settings, new game and the berth's jobs.
    let tab_labels = r"(?s)const TAB_LABELS: Array\[String\] = \[(.*?)\]";
    let mut wanted: BTreeSet<(String, String)> = BTreeSet::new();
    for tab in labels("view/ui_pause.gd", tab_labels) {
        wanted.insert(("SCREEN_PAUSE".to_string(), tab));
    }
    for tab in labels("view/ui_new_game.gd", tab_labels) {
        wanted.insert(("SCREEN_NEW_GAME".to_string(), tab));
    }
    let settings = Regex::new(r#"(?m)^\t"([A-Z]+)":"#).unwrap();
    let source = read("view/ui_settings.gd");
    let settings_tabs: Vec<String> = settings
        .captures_iter(&source)
        .map(|c| c[1].to_string())
        .collect();
    for tab in &settings_tabs {
        wanted.insert(("SCREEN_SETTINGS".to_string(), tab.clone()));
    }
    assert_eq!(
        settings_tabs.iter().collect::<BTreeSet<_>>().len(),
        2,
        "{settings_tabs:?}"
    );
    let have: BTreeSet<(String, String)> = covered().into_iter().map(|(s, t, _)| (s, t)).collect();
    let missing: Vec<_> = wanted.difference(&have).collect();
    assert!(missing.is_empty(), "{missing:?}");
    for job in labels("view/ui_station.gd", tab_labels) {
        let berth = (format!("SCREEN_{job}"), String::new(), true);
        assert!(covered().contains(&berth), "the berth's {job} tab, docked");
    }
}

#[test]
fn every_ui_key_a_script_presses_is_one_the_router_names() {
    let router = read("view/ui_router.gd");
    let body = router
        .split_once("static func named_keys")
        .unwrap()
        .1
        .split("\n\n\n")
        .next()
        .unwrap();
    let known: BTreeSet<String> = Regex::new(r#""([a-z0-9-]+)""#)
        .unwrap()
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .collect();
    let mut pressed = BTreeSet::new();
    for entry in catalogue()["screens"].as_array().unwrap() {
        for line in script_lines(entry) {
            for key in line
                .get("ui")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                pressed.insert(key.as_str().unwrap().to_string());
            }
        }
    }
    let unknown: Vec<_> = pressed.difference(&known).collect();
    assert!(unknown.is_empty(), "{unknown:?}");
}

// --- what is wrong with a catalogue beyond its schema ---

fn entry(change: Value) -> Value {
    let mut base = json!({"id": "x-y", "title": "X", "seed": 7, "script": [[]]});
    for (key, value) in change.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

fn check(entries: &[Value]) -> Vec<String> {
    problems(
        &json!({"version": 1, "resolution": "1280x800", "screens": entries}),
        &repo(),
    )
}

fn any_says(found: &[String], text: &str) -> bool {
    found.iter().any(|p| p.contains(text))
}

#[test]
fn a_duplicate_id_is_a_problem() {
    let found = check(&[entry(json!({})), entry(json!({}))]);
    assert!(any_says(&found, "twice"), "{found:?}");
}

#[test]
fn an_id_that_is_not_a_slug_is_a_problem() {
    let found = check(&[entry(json!({"id": "Bad_Id"}))]);
    assert!(any_says(&found, "Bad_Id"), "{found:?}");
}

#[test]
fn an_empty_script_is_a_problem() {
    let found = check(&[entry(json!({"script": []}))]);
    assert!(any_says(&found, "no steps"), "{found:?}");
}

#[test]
fn a_step_that_is_neither_keys_nor_a_line_is_a_problem() {
    let found = check(&[entry(json!({"script": [[], 3]}))]);
    assert!(any_says(&found, "step 1"), "{found:?}");
}

#[test]
fn an_unknown_ui_key_is_a_problem() {
    let found = check(&[entry(json!({"script": [["no-such-key"]]}))]);
    assert!(any_says(&found, "no-such-key"), "{found:?}");
}

#[test]
fn a_hold_that_is_not_a_count_is_a_problem() {
    for step in [
        json!({"hold": 0}),
        json!({"hold": "2"}),
        json!({"hold": 2, "press": true}),
    ] {
        let found = check(&[entry(json!({"script": [[], step]}))]);
        assert!(
            any_says(&found, "step 1: hold is a count"),
            "{step}: {found:?}"
        );
    }
    assert_eq!(
        check(&[entry(
            json!({"script": [{"hold": 2, "with": {"fire": true}}]})
        )]),
        Vec::<String>::new()
    );
}

#[test]
fn a_resolution_other_than_the_filming_one_is_a_problem() {
    let found = problems(
        &json!({"version": 1, "resolution": "800x600", "screens": [entry(json!({}))]}),
        &repo(),
    );
    assert!(any_says(&found, "1280x800"), "{found:?}");
    assert!(any_says(&found, "resolution is '800x600'"), "{found:?}");
}

// --- the film script an entry becomes ---

#[test]
fn keys_become_one_ui_line_each() {
    let entry = json!({"script": [["enter"], ["down", "down"]]});
    assert_eq!(
        script_lines(&entry),
        vec![json!({"ui": ["enter"]}), json!({"ui": ["down", "down"]})]
    );
}

#[test]
fn an_empty_key_list_is_an_empty_ui_line_so_the_run_films_the_shell() {
    assert_eq!(
        script_lines(&json!({"script": [[]]})),
        vec![json!({"ui": []})]
    );
}

#[test]
fn an_object_is_a_film_line_as_written() {
    assert_eq!(
        script_lines(&json!({"script": [{"dock": true}]})),
        vec![json!({"dock": true})]
    );
}

#[test]
fn hold_is_that_many_empty_lines() {
    assert_eq!(
        script_lines(&json!({"script": [{"hold": 3}]})),
        vec![json!({}), json!({}), json!({})]
    );
}

#[test]
fn hold_with_a_line_holds_that_line() {
    assert_eq!(
        script_lines(&json!({"script": [{"hold": 2, "with": {"fire": true}}]})),
        vec![json!({"fire": true}), json!({"fire": true})]
    );
}

#[test]
fn the_film_document_is_versioned_and_ticks_count_every_line() {
    let entry = json!({"script": [["enter"], {"hold": 4}]});
    assert_eq!(
        film_document(&entry),
        json!({"version": 1, "inputs": [{"ui": ["enter"]}, {}, {}, {}, {}]})
    );
    assert_eq!(film_ticks(&entry), 5);
}

#[test]
fn a_script_that_never_presses_a_ui_key_still_films_the_shell() {
    // Without any "ui" line `film.gd` films the plain sim, with no menu and no HUD state of a voyage.
    let document = film_document(&json!({"script": [{"hold": 2}]}));
    assert!(
        document["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.get("ui").is_some()),
        "{document}"
    );
}

// --- the manifest ---

#[test]
fn the_manifest_reads_as_written() {
    let tree = Tree::new();
    let manifest = json!({"commit": "c".repeat(40), "date": "2026-10-05",
                          "screens": {"title": {"file": "title.png", "hash": "h", "commit": "c", "date": "d"}}});
    tree.write(
        "manifest.json",
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
    let read = read_manifest(&tree.root).unwrap().unwrap();
    assert_eq!(Value::Object(read), manifest);
}

#[test]
fn a_missing_manifest_reads_as_none() {
    let tree = Tree::new();
    assert!(read_manifest(&tree.root).unwrap().is_none());
}

#[test]
fn a_manifest_that_is_not_an_object_reads_as_none() {
    let tree = Tree::new();
    tree.write("manifest.json", "[1, 2]");
    assert!(read_manifest(&tree.root).unwrap().is_none());
    tree.write("manifest.json", "{not json");
    assert!(read_manifest(&tree.root).unwrap().is_none());
}

// --- `symphony screens --check` ---

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

fn catalogue_tree(screens: &Value) -> Tree {
    let tree = Tree::new();
    tree.write("view/ui_router.gd", &read("view/ui_router.gd"));
    tree.write(
        "docs/data/schemas/screens.schema.json",
        &read("docs/data/schemas/screens.schema.json"),
    );
    let catalogue = json!({"version": 1, "resolution": "1280x800", "screens": screens});
    tree.write("docs/data/screens.json", &catalogue.to_string());
    tree
}

#[test]
fn check_passes_quietly_on_a_good_catalogue() {
    let tree = catalogue_tree(
        &json!([{"id": "one", "title": "One", "seed": 7, "script": [["enter"]], "screen": "SCREEN_TITLE"}]),
    );
    let root = tree.root.to_string_lossy().to_string();
    assert_eq!(
        run(&["--check", "--root", &root]),
        (0, String::new(), String::new())
    );
}

#[test]
fn check_names_a_schema_break_and_a_problem_and_fails() {
    let tree = catalogue_tree(
        &json!([{"id": "one", "title": "One", "seed": 7, "script": [["no-such-key"]]}]),
    );
    let root = tree.root.to_string_lossy().to_string();
    let (code, out, err) = run(&["--check", "--root", &root]);
    assert_eq!((code, out.as_str()), (1, ""));
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 2, "{err}");
    assert!(
        lines[0].starts_with("✖ screens: screens.json $.screens[0]: "),
        "{err}"
    );
    assert!(lines[0].contains("screen"), "{err}");
    assert_eq!(
        lines[1],
        "✖ screens: one: step 0 presses no-such-key, which the router does not name"
    );
}

#[test]
fn check_names_the_problems_in_the_catalogue_s_order() {
    let tree = catalogue_tree(&json!([]));
    tree.write(
        "docs/data/screens.json",
        r#"{"version": 2, "resolution": "1280x800",
            "screens": [{"id": "one", "title": "One", "seed": 7, "script": [["enter"]]}]}"#,
    );
    let root = tree.root.to_string_lossy().to_string();
    let (code, _, err) = run(&["--check", "--root", &root]);
    assert_eq!(code, 1);
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 2, "{err}");
    assert_eq!(lines[0], "✖ screens: screens.json $.version: must be 1");
    assert!(
        lines[1].starts_with("✖ screens: screens.json $.screens[0]: "),
        "{err}"
    );
}

#[test]
fn a_router_that_cannot_be_read_is_one_problem_not_one_per_key() {
    let tree = catalogue_tree(&json!([
        {"id": "one", "title": "One", "seed": 7, "script": [["enter", "up"], ["down"]], "screen": "SCREEN_TITLE"}
    ]));
    std::fs::remove_file(tree.path("view/ui_router.gd")).unwrap();
    let root = tree.root.to_string_lossy().to_string();
    let (code, _, err) = run(&["--check", "--root", &root]);
    assert_eq!(code, 1);
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 1, "{err}");
    assert!(
        lines[0].starts_with("✖ screens: the router's keys cannot be read: [Errno 2] "),
        "{err}"
    );
}

#[test]
fn the_real_catalogue_checks_clean() {
    let root = repo().to_string_lossy().to_string();
    assert_eq!(
        run(&["--check", "--root", &root]),
        (0, String::new(), String::new())
    );
}
