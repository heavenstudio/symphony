use super::*;
use crate::test_layouts::repository;
use crate::test_tree::Tree;
use symphony_board::launch::Defaults;

const MODELS: &str = "# settings\n[agents]\nlaunch = { default = 'claude', model = 'base' } # launch\n\n[agents.models] # roles\nreviewer = 'r' # review\nexecutor = \"e\"\norchestrator = '''o'''\nassessor = 'a'\n\n[limits]\nlow_volume = 2\n";

fn defaults(text: &str) -> Defaults {
    let tree = Tree::new();
    tree.write("symphony.toml", text);
    Defaults::configured(&tree.root).unwrap()
}

#[test]
fn each_role_changes_only_its_value_and_keeps_comments_and_order() {
    for (role, before) in [
        ("reviewer", "'r'"),
        ("executor", "\"e\""),
        ("orchestrator", "'''o'''"),
        ("assessor", "'a'"),
    ] {
        let after = edited(MODELS, role, "unknown-model").unwrap();
        assert_eq!(after, MODELS.replacen(before, "\"unknown-model\"", 1));
        let defaults = defaults(&after);
        assert_eq!(defaults.model_of(role).1.as_deref(), Some("unknown-model"));
        assert_eq!(
            defaults.resolve_as(Some(role), None, None).model,
            "unknown-model"
        );
    }
}

#[test]
fn all_sets_all_four_roles_without_changing_the_launch_defaults() {
    let after = edited(MODELS, "all", "any-model").unwrap();
    let defaults = defaults(&after);
    for role in ROLES {
        assert_eq!(defaults.models.of(role), Some("any-model"));
    }
    assert_eq!(defaults.model.as_deref(), Some("base"));
    assert!(after.contains("# roles\nreviewer = \"any-model\" # review"));
}

#[test]
fn missing_tables_and_keys_work_in_every_toml_table_form() {
    for before in [
        "",
        "# only a comment",
        "[project]\nid = 'lamp'\n",
        "[agents]\npanes = 'hc'\n[project]\nid = 'lamp'\n",
        "[agents.launch]\nmodel = 'base'\n",
        "[agents.models]",
        "[agents.models] # empty\n[project]\nid = 'lamp'\n",
        "agents = {}\n",
        "agents = { models = {} }\n",
        "agents = { models = { executor = 'e', } }\n",
        "[agents]\nmodels = { executor = 'e' }\n",
        "agents.models.executor = 'e'\n[project]\nid = 'lamp'\n",
        "[agents]\nmodels.executor = 'e'\n",
        "[project]\nid = 'lamp'\n[agents.models]\nexecutor = 'e'\n",
        "[agents.models.extra]\nkey = 'keep'\n",
    ] {
        let after = edited(before, "all", "new").unwrap();
        let defaults = defaults(&after);
        for role in ROLES {
            assert_eq!(defaults.models.of(role), Some("new"), "{after}");
        }
        let old: toml::Table = before.parse().unwrap();
        let new: toml::Table = after.parse().unwrap();
        assert_eq!(old.get("project"), new.get("project"), "{after}");
    }
}

#[test]
fn strings_round_trip_quotes_controls_unicode_empty_and_dashed_names() {
    for model in [
        "",
        " ",
        "-model",
        "m\"\\\n\r\t\0\u{8}\u{c}\u{1f}",
        "\u{7f}",
        "bad\u{7f}name",
        "literal\\u007F",
        "模型🦡",
    ] {
        let after = edited(MODELS, "executor", model).unwrap();
        let file: toml::Table = after.parse().unwrap();
        assert_eq!(file["agents"]["models"]["executor"].as_str(), Some(model));
    }
}

#[test]
fn every_ascii_character_round_trips_in_a_toml_model_string() {
    for character in '\0'..='\u{7f}' {
        let model = format!("before{character}after");
        let after = edited(MODELS, "executor", &model).unwrap();
        let file: toml::Table = after.parse().unwrap();
        assert_eq!(
            file["agents"]["models"]["executor"].as_str(),
            Some(model.as_str())
        );
    }
}

#[test]
fn matching_values_are_unchanged_and_insertions_keep_crlf() {
    assert_eq!(edited(MODELS, "reviewer", "r").unwrap(), MODELS);
    let before = "[agents.models] # roles\r\nexecutor = 'e'\r\n";
    let after = edited(before, "reviewer", "r").unwrap();
    assert_eq!(
        after,
        "[agents.models] # roles\r\nreviewer = \"r\"\r\nexecutor = 'e'\r\n"
    );
}

#[test]
fn unknown_roles_broken_toml_and_wrong_types_are_refused() {
    assert_eq!(
        edited(MODELS, "worker", "m").unwrap_err(),
        "unknown role \"worker\""
    );
    for before in [
        "[agents",
        "agents = 1",
        "[agents]\nmodels = []",
        "[agents.models]\nexecutor = 2",
    ] {
        assert!(
            edited(before, "executor", "m")
                .unwrap_err()
                .contains("symphony.toml")
        );
    }
}

#[test]
fn a_linked_worktree_edits_only_the_main_file_and_replaces_it_whole() {
    let tree = Tree::new();
    let (main, linked) = repository(&tree.root);
    std::fs::write(main.join("symphony.toml"), MODELS).unwrap();
    let file = main.join("symphony.toml");
    let mut open = std::fs::File::open(&file).unwrap();
    let line = change(
        &linked.join("docs"),
        "executor",
        "future-model",
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(
        line,
        format!("{}: executor model set to \"future-model\"", file.display())
    );
    let mut old = String::new();
    std::io::Read::read_to_string(&mut open, &mut old).unwrap();
    assert_eq!(old, MODELS);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        edited(MODELS, "executor", "future-model").unwrap()
    );
    assert!(
        !std::fs::read_to_string(linked.join("symphony.toml"))
            .unwrap()
            .contains("future-model")
    );
    assert!(
        change(&main, "executor", "future-model", &BTreeMap::new())
            .unwrap()
            .contains("already")
    );
}

#[test]
fn a_failed_edit_leaves_the_main_file_whole_and_missing_files_are_not_created() {
    let tree = Tree::new();
    let (main, _) = repository(&tree.root);
    let file = main.join("symphony.toml");
    let broken = "[agents.models]\nassessor = 3\n";
    std::fs::write(&file, broken).unwrap();
    assert!(change(&main, "all", "m", &BTreeMap::new()).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
    std::fs::remove_file(&file).unwrap();
    assert!(change(&main, "executor", "m", &BTreeMap::new()).is_err());
    assert!(!file.exists());
    assert!(
        change(&tree.root, "executor", "m", &BTreeMap::new())
            .unwrap_err()
            .contains("no git checkout")
    );
}

#[test]
fn an_unasked_command_cannot_write_outside_its_checkout() {
    let tree = Tree::new();
    let (main, _) = repository(&tree.root);
    let file = main.join("symphony.toml");
    let before = std::fs::read_to_string(&file).unwrap();
    let env = BTreeMap::from([(symphony_board::confine::MARKER.into(), "1".into())]);
    let problem = change(&main, "executor", "m", &env).unwrap_err();
    assert!(
        problem.contains("not a place a recipe an agent runs unasked writes"),
        "{problem}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
}

#[test]
fn argument_refusals_name_the_problem_before_reading_any_file() {
    for args in [
        vec![],
        vec!["executor"],
        vec!["executor", "m", "extra"],
        vec!["worker", "m"],
        vec!["executor", "--bogus"],
    ] {
        let args: Vec<String> = args.into_iter().map(str::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(super::main(&args, &BTreeMap::new(), &mut out, &mut err), 2);
        assert!(out.is_empty());
        assert!(String::from_utf8(err).unwrap().starts_with(USAGE));
    }
}

#[test]
fn local_writes_the_local_file_beside_the_main_project_file_and_never_the_project_file() {
    let tree = Tree::new();
    let (main, linked) = repository(&tree.root);
    std::fs::write(main.join("symphony.toml"), MODELS).unwrap();
    let local = main.join("symphony.local.toml");
    let none = BTreeMap::new();
    // No local file yet: it is made, holding the one table.
    let line = change_in(&linked.join("docs"), "executor", "fable", &none, true).unwrap();
    assert_eq!(
        line,
        format!("{}: executor model set to \"fable\"", local.display())
    );
    assert_eq!(
        std::fs::read_to_string(&local).unwrap(),
        "agents.models.executor = \"fable\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(main.join("symphony.toml")).unwrap(),
        MODELS,
        "the committed file is unchanged"
    );
    assert!(!linked.join("symphony.local.toml").exists());
    // Every reader takes it: the committed executor is "e", the local one holds.
    let defaults = Defaults::configured(&main).unwrap();
    assert_eq!(defaults.models.executor.as_deref(), Some("fable"));
    assert_eq!(defaults.models.reviewer.as_deref(), Some("r"));
    assert!(
        change_in(&main, "executor", "fable", &none, true)
            .unwrap()
            .contains("already")
    );
    // A file with comments keeps them, and `all` sets the four.
    std::fs::write(
        &local,
        "# mine\n[agents.models] # roles\nexecutor = 'x' # e\n",
    )
    .unwrap();
    change_in(&main, "all", "opus", &none, true).unwrap();
    let text = std::fs::read_to_string(&local).unwrap();
    assert!(
        text.starts_with("# mine\n[agents.models] # roles\n"),
        "{text}"
    );
    assert!(text.contains("executor = \"opus\" # e\n"), "{text}");
    let defaults = Defaults::configured(&main).unwrap();
    for role in ROLES {
        assert_eq!(defaults.models.of(role), Some("opus"));
    }
}

#[test]
fn local_refuses_a_local_file_its_readers_would_leave_out_and_names_that_file() {
    let tree = Tree::new();
    let (main, _) = repository(&tree.root);
    let local = main.join("symphony.local.toml");
    let none = BTreeMap::new();
    for (before, said) in [
        (
            "[land]\ncheck = false\n",
            "[land] check (line 2) is not read from a local file",
        ),
        (
            "[agents.models]\nexecutor = 2\n",
            "symphony.local.toml: [agents.models] executor (line 2) is 2; not text",
        ),
        ("[agents", "symphony.local.toml: TOML parse error"),
    ] {
        std::fs::write(&local, before).unwrap();
        let problem = change_in(&main, "executor", "m", &none, true).unwrap_err();
        assert!(problem.contains(said), "{before}: {problem}");
        assert!(!problem.contains("symphony.toml:"), "{before}: {problem}");
        assert_eq!(std::fs::read_to_string(&local).unwrap(), before);
    }
    // Under an unasked recipe it is confined as the project file's edit is.
    std::fs::remove_file(&local).unwrap();
    let env = BTreeMap::from([(symphony_board::confine::MARKER.into(), "1".into())]);
    assert!(change_in(&main, "executor", "m", &env, true).is_err());
    assert!(!local.exists());
}
