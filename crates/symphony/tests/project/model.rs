//! The model command from a linked worktree edits the main checkout; a scratch board's limits
//! command reads that edit without a verdict, a restart, or any board write by the editor.

use std::collections::BTreeMap;
use std::path::Path;

use super::Tree;
use super::board::Fixture;
use symphony_board::cli::Env;
use symphony_board::launch::Defaults;

#[path = "../common/layouts.rs"]
mod layouts;

fn model(here: &Path, machine: &Path, args: &[&str]) -> (i32, String, String) {
    let done = symphony_board::sys::output(
        symphony_board::sys::command(env!("CARGO_BIN_EXE_symphony"))
            .arg("model")
            .args(args)
            .env("SYMPHONY_MACHINE_DIR", machine)
            .current_dir(here),
    )
    .unwrap();
    (
        done.status.code().unwrap(),
        String::from_utf8(done.stdout).unwrap(),
        String::from_utf8(done.stderr).unwrap(),
    )
}

#[test]
fn the_model_command_changes_main_and_limits_reads_it_without_a_review() {
    let tree = Tree::new();
    let (main, linked) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let fixture = Fixture::empty();
    let client = fixture.other_database();
    let before = fixture.rows();
    let (code, out, err) = model(&linked.join("docs"), &machine, &["all", "unlisted-model"]);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(out.contains("all model set to \"unlisted-model\""), "{out}");
    assert!(
        !std::fs::read_to_string(linked.join("symphony.toml"))
            .unwrap()
            .contains("unlisted-model")
    );
    let defaults = Defaults::configured(&main).unwrap();
    for role in symphony_board::launch::defaults::ROLES {
        assert_eq!(
            defaults.resolve_as(Some(role), None, None).model,
            "unlisted-model"
        );
    }
    assert_eq!(
        fixture.rows(),
        before,
        "the model edit makes no task or reviewer verdict and writes no board state"
    );
    let env = Env {
        cwd: linked.clone(),
        vars: BTreeMap::from([
            ("TASKS_SERVER".into(), client.addr()),
            ("TASKS_DATABASE".into(), client.database().into()),
            ("SYMPHONY_MACHINE_DIR".into(), machine.display().to_string()),
        ]),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code =
        symphony_board::cli::main(&["limits".into()], &env, &mut &b""[..], &mut out, &mut err);
    let out = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    for role in symphony_board::launch::defaults::ROLES {
        assert!(
            out.lines().any(|line| line.starts_with(role)
                && line.contains("unlisted-model")
                && line.contains("[agents.models]")),
            "{out}"
        );
    }
}

#[test]
fn the_binary_reports_refusals_and_accepts_a_dashed_model_after_double_dash() {
    let tree = Tree::new();
    let (main, _) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let before = std::fs::read_to_string(main.join("symphony.toml")).unwrap();
    for (args, named) in [
        (vec!["worker", "x"], "unknown role"),
        (vec!["executor"], "ROLE and MODEL"),
        (vec!["executor", "--bogus"], "--bogus"),
    ] {
        let (code, out, err) = model(&main, &machine, &args);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains(named), "{err}");
        assert_eq!(
            std::fs::read_to_string(main.join("symphony.toml")).unwrap(),
            before
        );
    }
    let (code, _, err) = model(&main, &machine, &["--", "executor", "-future-model"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        Defaults::configured(&main)
            .unwrap()
            .models
            .executor
            .as_deref(),
        Some("-future-model")
    );
    let (code, out, err) = model(&tree.root, &machine, &["executor", "m"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("no git checkout"), "{err}");
}

#[test]
fn literal_help_flags_and_del_round_trip_through_the_binary_and_settings_readers() {
    let tree = Tree::new();
    let (main, _) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    for value in ["--help", "-h", "bad\u{7f}name", "\u{7f}"] {
        let (code, out, err) = model(&main, &machine, &["executor", "--", value]);
        assert_eq!((code, err.as_str()), (0, ""), "{out}");
        assert!(out.contains("executor model set to"), "{out}");
        let written = std::fs::read_to_string(main.join("symphony.toml")).unwrap();
        let parsed: toml::Table = written.parse().unwrap();
        assert_eq!(parsed["agents"]["models"]["executor"].as_str(), Some(value));
        assert_eq!(
            Defaults::configured(&main)
                .unwrap()
                .models
                .executor
                .as_deref(),
            Some(value)
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            symphony::lint::settings(&[main.display().to_string()], &mut out, &mut err),
            0,
            "{}",
            String::from_utf8_lossy(&err)
        );
    }
    let before = std::fs::read_to_string(main.join("symphony.toml")).unwrap();
    for flag in ["--help", "-h"] {
        let (code, out, err) = model(&main, &machine, &[flag]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.starts_with("usage: symphony model ROLE MODEL"), "{out}");
        assert_eq!(
            std::fs::read_to_string(main.join("symphony.toml")).unwrap(),
            before
        );
    }
}

#[test]
fn a_linked_worktree_reads_the_main_checkouts_local_file_and_has_none_of_its_own() {
    let tree = Tree::new();
    let (main, linked) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let local = main.join(symphony::project::LOCAL_FILE);
    assert_eq!(symphony::project::Project::local_file(&linked), local);
    assert_eq!(symphony::project::Project::local_file(&main), local);
    // Written from the linked worktree, it lands beside the main checkout's project file.
    let (code, out, err) = model(
        &linked.join("docs"),
        &machine,
        &["reviewer", "local-model", "--local"],
    );
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        out,
        format!(
            "{}: reviewer model set to \"local-model\"\n",
            local.display()
        )
    );
    assert!(!linked.join(symphony::project::LOCAL_FILE).exists());
    for root in [&main, &linked] {
        let project = symphony::project::Project::load(root).unwrap();
        assert_eq!(
            project.agents.models.reviewer.as_deref(),
            Some("local-model"),
            "{}",
            root.display()
        );
    }
    // One of the linked worktree's own is read by nobody.
    std::fs::write(
        linked.join(symphony::project::LOCAL_FILE),
        "[agents.models]\nreviewer = \"stray\"\n",
    )
    .unwrap();
    let project = symphony::project::Project::load(&linked).unwrap();
    assert_eq!(
        project.agents.models.reviewer.as_deref(),
        Some("local-model")
    );
    assert_eq!(
        Defaults::configured(&main)
            .unwrap()
            .models
            .reviewer
            .as_deref(),
        Some("local-model")
    );
}

#[test]
fn one_command_switches_main_to_claude_with_fable_and_a_haiku_assessor() {
    let tree = Tree::new();
    let (main, linked) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let file = main.join("symphony.toml");
    let before = "# keep\n[agents]\nlaunch = { default = 'codex', model = 'gpt-6.1-sol', effort = 'high' } # launch\n[agents.models] # roles\nreviewer = 'gpt-6.1-sol' # review\nexecutor = 'gpt-6.1-sol'\norchestrator = 'gpt-6.1-sol'\nassessor = 'gpt-6.1-sol'\n";
    std::fs::write(&file, before).unwrap();
    let args = ["all", "fable", "--agent", "claude", "--assessor", "haiku"];
    let (code, out, err) = model(&linked.join("docs"), &machine, &args);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(out.contains("launch agent set to \"claude\""), "{out}");
    assert!(out.contains("assessor model set to \"haiku\""), "{out}");
    let after = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        after,
        before
            .replace("'codex'", "\"claude\"")
            .replace("'gpt-6.1-sol'", "\"fable\"")
            .replace("assessor = \"fable\"", "assessor = \"haiku\"")
    );
    let defaults = Defaults::configured(&main).unwrap();
    for role in symphony_board::launch::defaults::ROLES {
        let resolved = defaults.resolve_as(Some(role), None, None);
        assert_eq!(resolved.agent, "claude");
        assert_eq!(
            resolved.model,
            if role == "assessor" { "haiku" } else { "fable" }
        );
    }
    let (code, out, err) = model(&main, &machine, &args);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("already"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), after);
}

#[test]
fn the_editor_warns_on_changes_and_noops_without_refusing_or_touching_other_settings() {
    let tree = Tree::new();
    let (main, _) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let file = main.join("symphony.toml");
    std::fs::write(&file, "[agents.models]\nexecutor = 'gpt-6.1-sol'\n").unwrap();
    for _ in 0..2 {
        let (code, out, err) = model(&main, &machine, &["executor", "fable"]);
        assert_eq!(code, 0, "{out}: {err}");
        assert_eq!(
            err,
            format!(
                "warning: {}: [agents.models] executor (line 2) model \"fable\" names claude, but [agents.launch] default is \"codex\"\n",
                file.display()
            )
        );
    }
    let (code, _, err) = model(&main, &machine, &["executor", "unknown-model"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let before = std::fs::read_to_string(&file).unwrap();
    let (code, out, err) = model(&main, &machine, &["all", "fable", "--agent", "future"]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("unknown agent \"future\""), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
}

#[test]
fn local_composes_with_agent_and_assessor_and_a_dashed_model_after_the_delimiter() {
    let tree = Tree::new();
    let (main, linked) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let file = main.join("symphony.toml");
    let committed = "# keep\n[agents]\nlaunch = { default = 'codex', model = 'gpt-6.1-sol', effort = 'high' } # launch\n[agents.models] # roles\nreviewer = 'gpt-6.1-sol' # review\n";
    std::fs::write(&file, committed).unwrap();
    let local = main.join(symphony::project::LOCAL_FILE);
    // The whole switch, on this machine alone, from a linked worktree, the flag in the middle.
    let args = [
        "all",
        "--local",
        "fable",
        "--agent",
        "claude",
        "--assessor",
        "haiku",
    ];
    let (code, out, err) = model(&linked.join("docs"), &machine, &args);
    // The local file alone names its agent, so nothing is warned of, and nothing refused.
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        out,
        format!(
            "{}: all model set to \"fable\"; launch agent set to \"claude\"; assessor model set to \"haiku\"\n",
            local.display()
        )
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        committed,
        "the committed file, its comments and its effort are untouched"
    );
    let written: toml::Table = std::fs::read_to_string(&local).unwrap().parse().unwrap();
    assert_eq!(
        written["agents"]["launch"]["default"].as_str(),
        Some("claude")
    );
    assert_eq!(written["agents"]["launch"]["model"].as_str(), Some("fable"));
    let defaults = Defaults::configured(&main).unwrap();
    for role in symphony_board::launch::defaults::ROLES {
        let resolved = defaults.resolve_as(Some(role), None, None);
        assert_eq!(resolved.agent, "claude");
        assert_eq!(
            resolved.model,
            if role == "assessor" { "haiku" } else { "fable" }
        );
        // The committed effort goes on, as the local file leaves it out.
        assert_eq!(resolved.effort.as_deref(), Some("high"));
    }
    // Run again it changes nothing.
    let after = std::fs::read_to_string(&local).unwrap();
    let (code, out, err) = model(&main, &machine, &args);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("already"), "{out}");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), after);
    // After the delimiter every word is a value: a model named as the flag is, and a dashed one.
    for (args, model_text) in [
        (vec!["--local", "executor", "--", "--agent"], "--agent"),
        (vec!["reviewer", "--local", "--", "-dashed"], "-dashed"),
        (vec!["--local", "--", "assessor", "--local"], "--local"),
    ] {
        let role = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .unwrap()
            .to_string();
        let (code, out, err) = model(&main, &machine, &args);
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}: {out}");
        assert_eq!(
            Defaults::configured(&main).unwrap().models.of(&role),
            Some(model_text),
            "{args:?}"
        );
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), committed);
    // The two flags stay refused with one role, with --local as without, and nothing is written.
    let before = std::fs::read_to_string(&local).unwrap();
    let (code, out, err) = model(
        &main,
        &machine,
        &["executor", "m", "--local", "--agent", "claude"],
    );
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("only available with all"), "{err}");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), before);
}

/// What the settings check says of the project at `root`: (code, stderr).
fn checked(root: &Path) -> (i32, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony::lint::settings(&[root.to_str().unwrap().to_string()], &mut out, &mut err);
    (code, String::from_utf8(err).unwrap())
}

#[test]
fn a_cross_agent_warning_judges_the_settings_in_force_and_names_the_file_supplying_each() {
    let tree = Tree::new();
    let (main, linked) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let file = main.join("symphony.toml");
    let local = main.join(symphony::project::LOCAL_FILE);
    let committed =
        "[agents]\nlaunch = { default = 'codex' }\n[agents.models]\nexecutor = 'gpt-6.1-sol'\n";
    std::fs::write(&file, committed).unwrap();
    assert_eq!(checked(&main), (0, String::new()), "committed alone agrees");

    // A local model alone: codex, the committed agent, would now start on fable.
    let model_only = format!(
        "warning: {}: [agents.models] executor (line 1) model \"fable\" names claude, but [agents.launch] default is \"codex\" in {}\n",
        local.display(),
        file.display()
    );
    for again in [false, true] {
        let (code, out, err) = model(
            &linked.join("docs"),
            &machine,
            &["executor", "fable", "--local"],
        );
        assert_eq!(code, 0, "{out}: {err}");
        assert_eq!(out.contains("already"), again, "{out}");
        assert_eq!(err, model_only, "an edit and a no-op warn alike");
    }
    // The settings check says the same, and refuses nothing.
    assert_eq!(checked(&main), (0, model_only.clone()));
    // A linked worktree's check reads its own project file with the main checkout's local file
    // over it: its file names no agent, so the board's own is in force and no file is named.
    assert_eq!(
        checked(&linked),
        (
            0,
            format!(
                "warning: {}: [agents.models] executor (line 1) model \"fable\" names claude, but [agents.launch] default is \"codex\"\n",
                local.display()
            )
        )
    );

    // The local agent alone: the committed model is now the other agent's.
    std::fs::write(&local, "[agents.launch]\ndefault = \"claude\"\n").unwrap();
    let agent_only = format!(
        "warning: {}: [agents.models] executor (line 4) model \"gpt-6.1-sol\" names codex, but [agents.launch] default is \"claude\" in {}\n",
        file.display(),
        local.display()
    );
    assert_eq!(checked(&main), (0, agent_only.clone()));
    // An edit of the project file is judged with the local file over it, a no-op too.
    let (code, _, err) = model(&main, &machine, &["executor", "gpt-6.1-sol"]);
    assert_eq!((code, err.as_str()), (0, agent_only.as_str()));

    // Overridden back into agreement: the local agent with a local model of its own.
    let (code, _, err) = model(&main, &machine, &["executor", "fable", "--local"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(checked(&main), (0, String::new()));
    // And a committed pairing that disagrees, set right by the local agent alone.
    std::fs::write(
        &file,
        "[agents]\nlaunch = { default = 'codex' }\n[agents.models]\nexecutor = 'fable'\n",
    )
    .unwrap();
    std::fs::write(&local, "[agents.launch]\ndefault = \"claude\"\n").unwrap();
    assert_eq!(checked(&main), (0, String::new()));
    std::fs::remove_file(&local).unwrap();
    assert!(checked(&main).1.contains("names claude"), "committed alone");

    // The combined flags: the local file names agent and models together, so nothing is warned
    // of, though the committed file still pairs codex with its own.
    std::fs::write(&file, committed).unwrap();
    let args = [
        "all",
        "fable",
        "--agent",
        "claude",
        "--assessor",
        "haiku",
        "--local",
    ];
    for _ in 0..2 {
        let (code, _, err) = model(&main, &machine, &args);
        assert_eq!((code, err.as_str()), (0, ""));
    }
    // A local launch model of the other agent is named by its own key and line, in the one file.
    std::fs::write(
        &local,
        "[agents.launch]\ndefault = \"claude\"\nmodel = \"gpt-6.1-sol\"\n\n[agents.models]\nexecutor = \"anything-at-all\"\n",
    )
    .unwrap();
    assert_eq!(
        checked(&main),
        (
            0,
            format!(
                "warning: {}: [agents.launch] model (line 3) model \"gpt-6.1-sol\" names codex, but [agents.launch] default is \"claude\"\n",
                local.display()
            )
        ),
        "any other text is taken unjudged, and the committed executor it overrides is not named"
    );
    // A local file that is refused is read by nobody: the committed pairing alone is judged.
    std::fs::write(
        &local,
        "[agents.models]\nexecutor = \"fable\"\n\n[land]\ncheck = false\n",
    )
    .unwrap();
    let (code, err) = checked(&main);
    assert_eq!(code, 1);
    assert!(!err.contains("warning:"), "{err}");
}
