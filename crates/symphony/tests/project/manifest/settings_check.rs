//! `symphony settings-check`, the settings check a change of settings is held to in a reviewer's
//! place: the project file's shape alone (a table or key the Symphony knows, each value of its
//! type, each required value there), and never a name against a list.

use super::folder;
use crate::Tree;
use symphony::project::FILE;

/// The check on the project at `root` holding `text`: (code, stdout, stderr).
fn check(root: &std::path::Path, text: &str) -> (i32, String, String) {
    std::fs::write(root.join(FILE), text).unwrap();
    run(&[root.to_str().unwrap().to_string()])
}

fn run(args: &[String]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony::lint::settings(args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_model_nobody_has_heard_of_passes_as_any_text_does() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    for text in [
        "",
        "[agents]\nlaunch = { default = \"claude\", model = \"mythos-9\" }\n\n\
         [agents.models]\norchestrator = \"no such model\"\nexecutor = \"fable\"\n\
         reviewer = \"opus-of-next-year\"\nassessor = \"\"\n",
        "[agents.launch]\ndefault = \"claude\"\neffort = \"immense\"\n",
        "[limits]\norchestrator_slots = 3\n\n[releases]\nevery_landings = 7\n",
        "[limits]\norchestrator_tasks = 16\n",
    ] {
        let (code, out, err) = check(&root, text);
        assert_eq!((code, err.as_str()), (0, ""), "{text}");
        assert_eq!(
            out,
            "✔ symphony.toml: every table and key is a setting the Symphony knows, each value \
             of its type\n",
            "{text}"
        );
    }
    // No file at all is the plain project: nothing to refuse.
    std::fs::remove_file(root.join(FILE)).unwrap();
    assert_eq!(run(&[root.to_str().unwrap().to_string()]).0, 0);
}

#[test]
fn recognizable_cross_agent_models_warn_by_key_and_line_but_pass() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    for (text, key, line, model, agent, launch) in [
        (
            "[agents.models]\nexecutor = 'fable'\n",
            "executor",
            2,
            "fable",
            "claude",
            "codex",
        ),
        (
            "[agents]\nlaunch = { default = 'codex' }\n[agents.models]\nreviewer = 'sonnet'\n",
            "reviewer",
            4,
            "sonnet",
            "claude",
            "codex",
        ),
        (
            "agents.launch.default = 'codex'\nagents.models.assessor = 'haiku'\n",
            "assessor",
            2,
            "haiku",
            "claude",
            "codex",
        ),
        (
            "agents = { models = { orchestrator = 'opus' } }\n",
            "orchestrator",
            1,
            "opus",
            "claude",
            "codex",
        ),
        (
            "[agents.launch]\ndefault = 'claude'\n[agents.models]\nexecutor = 'gpt-6.1-sol'\n",
            "executor",
            4,
            "gpt-6.1-sol",
            "codex",
            "claude",
        ),
        (
            "[agents.models]\nexecutor = 'claude-sonnet-4-5'\n",
            "executor",
            2,
            "claude-sonnet-4-5",
            "claude",
            "codex",
        ),
    ] {
        let (code, out, err) = check(&root, text);
        assert_eq!(code, 0, "{err}");
        assert!(out.starts_with("✔ symphony.toml:"), "{out}");
        assert_eq!(
            err,
            format!(
                "warning: {}: [agents.models] {key} (line {line}) model {model:?} names {agent}, but [agents.launch] default is {launch:?}\n",
                root.join(FILE).display()
            )
        );
    }
}

#[test]
fn warnings_report_every_mismatch_and_ignore_compatible_or_unrecognized_names() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    let (code, _, err) = check(
        &root,
        "[agents.models]\nreviewer = 'fable'\nassessor = 'haiku'\n",
    );
    assert_eq!(code, 0);
    assert_eq!(err.lines().count(), 2);
    assert!(err.contains("reviewer (line 2)"), "{err}");
    assert!(err.contains("assessor (line 3)"), "{err}");
    for text in [
        "[agents.models]\nexecutor = 'gpt-6.1-sol'\n",
        "[agents.launch]\ndefault = 'claude'\n[agents.models]\nreviewer = 'fable'\nassessor = 'haiku'\n",
        "[agents.models]\nexecutor = 'a model nobody knows'\nassessor = ''\n",
        "[agents.launch]\ndefault = ''\n[agents.models]\nexecutor = 'gpt-6.1-sol'\n",
    ] {
        let (code, _, err) = check(&root, text);
        assert_eq!((code, err.as_str()), (0, ""), "{text}");
    }
}

#[test]
fn a_mismatched_launch_fallback_also_warns_with_its_own_key_and_line() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    let (code, _, err) = check(
        &root,
        "[agents]\nlaunch = { default = 'claude', model = 'gpt-6.1-sol' }\n",
    );
    assert_eq!(code, 0);
    assert_eq!(
        err,
        format!(
            "warning: {}: [agents.launch] model (line 2) model \"gpt-6.1-sol\" names codex, but [agents.launch] default is \"claude\"\n",
            root.join(FILE).display()
        )
    );
}

#[test]
fn a_value_of_the_wrong_type_is_refused_naming_the_file() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    for (text, named) in [
        ("[agents.models]\nassessor = 5\n", "expected a string"),
        ("[agents]\nlaunch = { model = true }\n", "expected a string"),
        ("[agents]\nlaunch = \"opus\"\n", "invalid type: string"),
        ("[releases]\nevery_landings = \"often\"\n", "invalid type"),
        ("[land]\npush = \"yes\"\n", "expected a boolean"),
        ("[limits]\norchestrator_slots = \"three\"\n", "[limits]"),
        (
            "[limits]\norchestrator_tasks = \"sixteen\"\n",
            "[limits] orchestrator_tasks (line 2) is \"sixteen\"; not a whole number",
        ),
        (
            "[limits]\norchestrator_tasks = 0\n",
            "an orchestrator holds at least one task",
        ),
        ("[agents.models\n", "symphony.toml"),
    ] {
        let (code, out, err) = check(&root, text);
        assert_eq!((code, out.as_str()), (1, ""), "{text}: {err}");
        assert!(err.starts_with("symphony.toml: "), "{text}: {err}");
        assert!(err.contains(named), "{text}: {err}");
        assert!(err.ends_with("✖ the project file is refused.\n"), "{err}");
    }
}

#[test]
fn an_agent_no_launch_starts_is_refused_as_a_launch_refuses_it() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    for agent in [
        "an agent nobody wrote yet",
        "gemini",
        "Claude",
        "future-agent",
    ] {
        // A model of another agent beside it is no warning: the file is refused first.
        let text =
            format!("[agents.launch]\ndefault = {agent:?}\n[agents.models]\nexecutor = 'fable'\n");
        let (code, out, err) = check(&root, &text);
        assert_eq!((code, out.as_str()), (1, ""), "{text}");
        assert_eq!(
            err,
            format!(
                "symphony.toml: [agents] launch default {agent:?} is not one of codex, claude\n\
                 ✖ the project file is refused.\n"
            )
        );
    }
    // A blank one names none, as a launch reads it: the board's own agent.
    let (code, _, err) = check(&root, "[agents.launch]\ndefault = \" \"\n");
    assert_eq!((code, err.as_str()), (0, ""));
}

#[test]
fn a_required_value_left_out_is_refused() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    for (text, named) in [
        (
            "[[entrypoints]]\nurl = \"/docs/\"\n",
            "missing field `folder`",
        ),
        (
            "[[entrypoints]]\nfolder = \"docs\"\n",
            "missing field `url`",
        ),
        (
            "[project]\ndefault_branch = \" \"\n",
            "default_branch is empty",
        ),
    ] {
        let (code, _, err) = check(&root, text);
        assert_eq!(code, 1, "{text}: {err}");
        assert!(err.contains(named), "{text}: {err}");
    }
}

#[test]
fn a_table_or_key_the_symphony_does_not_know_is_refused_by_name_and_line() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    let (code, _, err) = check(&root, "[agents.models]\nplanner = \"opus\"\n\n[hooks]\n");
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "symphony.toml: [hooks] (line 4), [agents.models] planner (line 2) are not settings the \
         Symphony knows\n✖ the project file is refused.\n"
    );
}

#[test]
fn it_reads_one_folder_and_refuses_any_other_command_line() {
    for args in [vec!["a", "b"], vec!["--root", "a"], vec!["--lint"]] {
        let args: Vec<String> = args.into_iter().map(String::from).collect();
        let (code, out, err) = run(&args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        assert!(
            err.starts_with("usage: symphony settings-check [DIR]\n"),
            "{err}"
        );
    }
}

#[test]
fn a_local_settings_file_is_judged_with_the_project_file() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    let local = root.join(symphony::project::LOCAL_FILE);
    let committed = "[agents.models]\nexecutor = \"committed-model\"\n";
    // One that holds preferences alone passes, and is said.
    std::fs::write(
        &local,
        "[agents.models]\nexecutor = \"local-model\"\n\n[limits]\nmachine_agents = 4\n",
    )
    .unwrap();
    let (code, out, err) = check(&root, committed);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "✔ symphony.toml: every table and key is a setting the Symphony knows, each value of \
         its type\n✔ symphony.local.toml: every key is a preference a local file may override, \
         each value of its type\n"
    );
    // And the project the Symphony loads takes its model.
    let project = symphony::project::Project::load(&root).unwrap();
    assert_eq!(
        project.agents.models.executor.as_deref(),
        Some("local-model")
    );
    assert_eq!(project.agents.models.reviewer, None);
    for (text, named) in [
        (
            "[land]\ncheck = false\n",
            "[land] check (line 2) is not read from a local file",
        ),
        (
            "[verdicts]\ncheck = [\"true\"]\n",
            "[verdicts] check (line 2) is not read from a local file",
        ),
        (
            "[board]\non_land = [\"true\"]\n",
            "[board] on_land (line 2) is not read from a local file",
        ),
        (
            "[limits]\nwip = 3\n",
            "[limits] wip (line 2) is not a setting the Symphony knows",
        ),
        (
            "[agents.models]\nexecutor = 4\n",
            "[agents.models] executor (line 2) is 4; not text",
        ),
        (
            "[theme]\n",
            "[theme] (line 1) is not a setting a local file overrides",
        ),
    ] {
        std::fs::write(&local, text).unwrap();
        let (code, out, err) = check(&root, committed);
        assert_eq!((code, out.as_str()), (1, ""), "{text}: {err}");
        assert!(
            err.starts_with(&format!("{}: {named}", local.display())),
            "{text}: {err}"
        );
        assert!(
            err.ends_with("✖ the local settings file is refused.\n"),
            "{err}"
        );
        // Refused, none of it is read: the project loads on the committed settings.
        let project = symphony::project::Project::load(&root).unwrap();
        assert_eq!(
            project.agents.models.executor.as_deref(),
            Some("committed-model")
        );
    }
    // Both files refused, both are said.
    let (code, _, err) = check(&root, "[hooks]\n");
    assert_eq!(code, 1);
    assert!(err.contains("✖ the project file is refused.\n"), "{err}");
    assert!(
        err.contains("✖ the local settings file is refused"),
        "{err}"
    );
}

#[test]
fn the_running_symphony_reads_the_local_file_again_when_it_changes() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    let local = root.join(symphony::project::LOCAL_FILE);
    assert_eq!(symphony::project::Project::local_file(&root), local);
    std::fs::write(root.join(FILE), "[agents.models]\nexecutor = \"opus\"\n").unwrap();
    let executor = || {
        symphony::project::Project::of(&root)
            .agents
            .models
            .executor
            .clone()
    };
    assert_eq!(executor().as_deref(), Some("opus"), "no local file");
    std::fs::write(&local, "[agents.models]\nexecutor = \"fable\"\n").unwrap();
    assert_eq!(executor().as_deref(), Some("fable"), "it appeared");
    // Refused, it is left out and the project still loads, on the committed model.
    std::fs::write(&local, "[land]\ncheck = false\n").unwrap();
    assert_eq!(executor().as_deref(), Some("opus"));
    assert_eq!(symphony::project::Project::problem(&root), None);
    assert!(symphony::project::Project::of(&root).land.check);
    std::fs::remove_file(&local).unwrap();
    assert_eq!(executor().as_deref(), Some("opus"), "gone");
}

#[test]
fn a_local_settings_file_git_tracks_is_refused() {
    let tree = Tree::new();
    let root = folder(&tree, "lamps");
    let git = |args: &[&str]| {
        let done =
            symphony_board::sys::output(symphony_board::sys::git().arg("-C").arg(&root).args(args))
                .unwrap();
        assert!(done.status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    let local = root.join(symphony::project::LOCAL_FILE);
    std::fs::write(&local, "[limits]\nlow_volume = 2\n").unwrap();
    // In a repository and untracked, it passes.
    assert_eq!(check(&root, "").0, 0);
    git(&["add", "-f", symphony::project::LOCAL_FILE]);
    let root = root.canonicalize().unwrap();
    let (code, out, err) = check(&root, "");
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert_eq!(
        err,
        format!(
            "{}: git tracks it, and a local settings file is one machine's alone: remove it \
             from the index (git rm --cached symphony.local.toml) and ignore it\n✖ the local \
             settings file is refused.\n",
            root.join(symphony::project::LOCAL_FILE).display()
        )
    );
    // The lint says the same.
    assert_eq!(symphony::lint::project_problems(&root).len(), 1);
}

/// This repository's own project file passes its own check.
#[test]
fn this_repositorys_project_file_passes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (code, _, err) = run(&[root.to_str().unwrap().to_string()]);
    assert_eq!((code, err.as_str()), (0, ""));
}
