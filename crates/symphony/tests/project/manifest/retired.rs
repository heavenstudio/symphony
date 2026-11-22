//! The keys the Symphony reads no more (`[agents] transports`, `[board] export`,
//! `[board] disposable`, `[plugins] loopback`): a file that still writes one is refused as a file
//! writing any key the Symphony does not know is, by its name and line, with nothing said of them
//! apart.

use super::{folder, parsed, refused};
use crate::Tree;
use symphony::lint::project_problems;
use symphony::project::{FILE, Project};

/// A file writing all three, on lines 3, 7 and 8, among settings that are read.
const ALL: &str = "[agents]\npanes = \"hc\"\ntransports = [\"claude-uds\", \"inbox\"]\n\n\
                   [board]\nguide = \"docs/TASKS.md\"\nexport = \"docs/history/tasks.md\"\n\
                   disposable = [\"artifacts/*\", \".target/*\"]\n";

/// What refuses [`ALL`]: the three named table by table, sorted within each.
const REFUSAL: &str = "symphony.toml: [agents] transports (line 3), [board] disposable (line 8), \
                       [board] export (line 7) are not settings the Symphony knows";

#[test]
fn each_retired_key_is_refused_by_name_and_line_whatever_it_holds() {
    for (text, named) in [
        ("[agents]\ntransports = []\n", "[agents] transports"),
        (
            "[agents]\ntransports = \"claude-uds\"\n",
            "[agents] transports",
        ),
        ("[board]\nexport = \"../tasks.md\"\n", "[board] export"),
        ("[board]\nexport = 3\n", "[board] export"),
        (
            "[board]\ndisposable = [\"artifacts/*\"]\n",
            "[board] disposable",
        ),
        (
            "[board]\ndisposable = { any = true }\n",
            "[board] disposable",
        ),
        ("[plugins]\nloopback = true\n", "[plugins] loopback"),
        ("[plugins]\nloopback = false\n", "[plugins] loopback"),
        ("[plugins]\nloopback = \"yes\"\n", "[plugins] loopback"),
    ] {
        assert_eq!(
            refused(text),
            format!("symphony.toml: {named} (line 2) is not a setting the Symphony knows"),
            "{text}"
        );
    }
}

#[test]
fn a_file_writing_all_three_names_each_beside_any_other_unknown_key() {
    assert_eq!(refused(ALL), REFUSAL);
    assert_eq!(
        refused(&format!("{ALL}starts = [\"y\"]\n")),
        "symphony.toml: [agents] transports (line 3), [board] disposable (line 8), \
         [board] export (line 7), [board] starts (line 9) are not settings the Symphony knows"
    );
    // without them the same file is read
    let without = parsed("[agents]\npanes = \"hc\"\n\n[board]\nguide = \"docs/TASKS.md\"\n");
    assert_eq!(without.unwrap().agents.panes, "hc");
}

#[test]
fn a_project_whose_file_writes_them_does_not_load_and_runs_with_them_left_out() {
    let tree = Tree::new();
    let root = folder(&tree, "Night Shift");
    std::fs::write(root.join(FILE), ALL).unwrap();
    assert_eq!(Project::load(&root).unwrap_err(), REFUSAL);
    assert_eq!(project_problems(&root), [REFUSAL]);
    // the running project leaves them out and reads the rest, as with any unknown key
    let running = Project::of(&root);
    assert_eq!(running.agents.panes, "hc");
    assert_eq!(running.board.guide(), "docs/TASKS.md");
    assert_eq!(Project::problem(&root), None);
}

#[test]
fn the_settings_check_and_the_lint_refuse_it_and_say_no_retired_line() {
    let tree = Tree::new();
    std::fs::write(tree.root.join(FILE), ALL).unwrap();
    let args = [tree.root.to_str().unwrap().to_string()];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(symphony::lint::settings(&args, &mut out, &mut err), 1);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        format!("{REFUSAL}\n✖ the project file is refused.\n")
    );
    assert!(out.is_empty());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(symphony::lint::main(&tree.root, &mut out, &mut err), 1);
    let err = String::from_utf8(err).unwrap();
    assert!(err.starts_with(&format!("{REFUSAL}\n")), "{err}");
    assert!(err.contains("the project file is refused"), "{err}");
    assert!(!err.contains("does nothing"), "{err}");
}

/// A file that writes the retired loopback beside the timing that is still read, on line 4.
const LOOPED: &str = "[plugins]\ncrates = []\ntiming = true\nloopback = true\n";

/// What refuses [`LOOPED`].
const LOOPED_REFUSAL: &str =
    "symphony.toml: [plugins] loopback (line 4) is not a setting the Symphony knows";

#[test]
fn the_retired_loopback_is_refused_beside_the_timing_and_the_running_project_keeps_the_timing() {
    let tree = Tree::new();
    let root = folder(&tree, "Night Shift");
    std::fs::write(root.join(FILE), LOOPED).unwrap();
    assert_eq!(Project::load(&root).unwrap_err(), LOOPED_REFUSAL);
    assert_eq!(project_problems(&root), [LOOPED_REFUSAL]);
    let running = Project::of(&root);
    assert_eq!(
        running.plugin_calls,
        symphony::project::PluginCalls { timing: true }
    );
    assert_eq!(Project::problem(&root), None);
    let args = [root.to_str().unwrap().to_string()];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(symphony::lint::settings(&args, &mut out, &mut err), 1);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        format!("{LOOPED_REFUSAL}\n✖ the project file is refused.\n")
    );
    // without it the same file is read
    let without = parsed("[plugins]\ncrates = []\ntiming = true\n").unwrap();
    assert!(without.plugin_calls.recorded());
}
