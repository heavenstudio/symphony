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
        "[agents.launch]\ndefault = \"an agent nobody wrote yet\"\neffort = \"immense\"\n",
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

/// This repository's own project file passes its own check.
#[test]
fn this_repositorys_project_file_passes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (code, _, err) = run(&[root.to_str().unwrap().to_string()]);
    assert_eq!((code, err.as_str()), (0, ""));
}
