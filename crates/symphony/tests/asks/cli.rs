//! `CliTest`: `symphony ask list|show|answer`, what a listening agent runs.

use std::collections::BTreeMap;

use symphony::asks::AskStore;

use super::common::Tree;

struct Fixture {
    _tree: Tree,
    env: BTreeMap<String, String>,
    db: std::path::PathBuf,
    open_id: i64,
    done_id: i64,
}

fn fixture() -> Fixture {
    let tree = Tree::new();
    let db = tree.path("asks.db");
    let env = BTreeMap::from([("ASKS_DB".to_string(), db.display().to_string())]);
    let store = AskStore::open(&db).unwrap();
    let open_id = store.ask("docs/A.md", "why five?", "bands", "").unwrap();
    let done_id = store.ask("docs/B.md", "done?", "", "").unwrap();
    store.answer(done_id, "yes", "agent").unwrap();
    Fixture {
        _tree: tree,
        env,
        db,
        open_id,
        done_id,
    }
}

fn run(f: &Fixture, argv: &[&str]) -> (i32, String, String) {
    let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony::asks::main(&argv, &f.env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn list_shows_every_ask_and_open_only_the_open_ones() {
    let f = fixture();
    let (code, out, _) = run(&f, &["list"]);
    assert_eq!(code, 0);
    assert!(out.contains("docs/A.md#bands"), "{out}");
    assert!(out.contains("docs/B.md"), "{out}");
    let (code, out, _) = run(&f, &["list", "--open"]);
    assert_eq!(code, 0);
    assert!(out.contains("why five?"), "{out}");
    assert!(!out.contains("done?"), "{out}");
}

#[test]
fn show_prints_the_thread() {
    let f = fixture();
    let (code, out, _) = run(&f, &["show", &f.done_id.to_string()]);
    assert_eq!(code, 0);
    assert!(out.contains("done?"), "{out}");
    assert!(out.contains("yes"), "{out}");
    assert!(out.contains("answered"), "{out}");
}

#[test]
fn answer_writes_the_answer_as_the_named_agent_by_as_as_the_board_names_it_or_by() {
    for named in ["--as", "--by"] {
        let f = fixture();
        let (code, _, err) = run(
            &f,
            &[
                "answer",
                &f.open_id.to_string(),
                "Three; see [[B#Bands]].",
                named,
                "amber-heron",
            ],
        );
        assert_eq!(code, 0, "{named}: {err}");
        let thread = AskStore::open(&f.db).unwrap().thread(f.open_id).unwrap();
        assert_eq!(thread.status, "answered");
        assert_eq!(thread.answers[0].by, "amber-heron", "{named}");
        assert_eq!(thread.answers[0].text, "Three; see [[B#Bands]].");
    }
}

#[test]
fn a_bad_id_or_empty_answer_fails_and_says_why() {
    let f = fixture();
    let open = f.open_id.to_string();
    for argv in [
        vec!["show", "99"],
        vec!["answer", "99", "x"],
        vec!["answer", open.as_str(), " "],
    ] {
        let (code, _, err) = run(&f, &argv);
        assert_eq!(code, 1, "{argv:?}");
        assert!(!err.trim().is_empty(), "{argv:?}");
    }
}

#[test]
fn an_option_of_another_command_or_of_none_is_refused_as_argparse_refused_it() {
    let f = fixture();
    let open = f.open_id.to_string();
    for (argv, named) in [
        (vec!["show", open.as_str(), "--open"], "--open"),
        (vec!["list", "--by", "amber-heron"], "--by"),
        (vec!["answer", open.as_str(), "x", "--open"], "--open"),
        (vec!["list", "--root", "."], "--root"),
        (vec!["--open", "list"], "--open"),
    ] {
        let (code, out, err) = run(&f, &argv);
        assert_eq!(code, 2, "{argv:?}: {out}{err}");
        assert!(
            err.contains(&format!("error: unrecognized arguments: {named}")),
            "{argv:?}: {err}"
        );
    }
    let thread = AskStore::open(&f.db).unwrap().thread(f.open_id).unwrap();
    assert_eq!(thread.status, "open", "a refused answer writes nothing");
}

#[test]
fn the_root_comes_before_the_command() {
    let f = fixture();
    let (code, out, err) = run(&f, &["--root", ".", "list", "--open"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("why five?"), "{out}");
}
