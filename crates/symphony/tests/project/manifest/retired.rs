//! The keys the Symphony reads no more (`[agents] transports`, `[board] export`,
//! `[board] disposable`): a file that still writes one loads, the key ignored and said to do
//! nothing, and it is refused nowhere but under another table.

use super::{folder, parsed, refused};
use crate::Tree;
use symphony::lint::project_problems;
use symphony::project::{FILE, Project};

/// A file writing all three, on lines 3, 7 and 8, among settings that are read.
const ALL: &str = "[agents]\npanes = \"hc\"\ntransports = [\"claude-uds\", \"inbox\"]\n\n\
                   [board]\nguide = \"docs/TASKS.md\"\nexport = \"docs/history/tasks.md\"\n\
                   disposable = [\"artifacts/*\", \".target/*\"]\n";

fn says(name: &str) -> String {
    format!(
        "symphony.toml: {name} does nothing: no Symphony reads it any more, and the next release \
         refuses it; remove it"
    )
}

#[test]
fn a_file_that_still_writes_a_retired_key_parses_as_the_file_without_it() {
    let without = parsed("[agents]\npanes = \"hc\"\n\n[board]\nguide = \"docs/TASKS.md\"\n");
    assert_eq!(parsed(ALL), without);
    // each alone is the plain project, whatever it holds: nothing reads the value, so none is
    // checked (a path leading out of the project, a value of another type)
    let plain = parsed("").unwrap();
    for text in [
        "[agents]\ntransports = []\n",
        "[agents]\ntransports = \"claude-uds\"\n",
        "[board]\nexport = \"../tasks.md\"\n",
        "[board]\nexport = 3\n",
        "[board]\ndisposable = [\"artifacts/*\"]\n",
        "[board]\ndisposable = { any = true }\n",
    ] {
        assert_eq!(parsed(text).as_ref(), Ok(&plain), "{text}");
    }
}

#[test]
fn a_retired_key_is_refused_beside_no_other_and_under_no_other_table() {
    // an unknown key beside them is still refused, and alone in the refusal
    assert_eq!(
        refused(&format!("{ALL}starts = [\"y\"]\n")),
        "symphony.toml: [board] starts (line 9) is not a setting the Symphony knows"
    );
    // the names are retired where they stood, and unknown anywhere else
    for (text, named) in [
        ("transports = []\n", "transports (line 1)"),
        ("[board]\ntransports = []\n", "[board] transports (line 2)"),
        ("[agents]\nexport = \"x\"\n", "[agents] export (line 2)"),
        (
            "[project]\ndisposable = []\n",
            "[project] disposable (line 2)",
        ),
        (
            "[agents.launch]\ntransports = []\n",
            "[agents.launch] transports (line 2)",
        ),
    ] {
        assert_eq!(
            refused(text),
            format!("symphony.toml: {named} is not a setting the Symphony knows")
        );
    }
}

#[test]
fn a_project_whose_file_writes_them_loads_runs_and_is_told_each_does_nothing() {
    let tree = Tree::new();
    let root = folder(&tree, "Night Shift");
    assert!(Project::retired(&root).is_empty(), "no file");
    std::fs::write(root.join(FILE), ALL).unwrap();
    let loaded = Project::load(&root).unwrap();
    assert_eq!(loaded.agents.panes, "hc");
    assert_eq!(loaded.board.guide(), "docs/TASKS.md");
    assert!(project_problems(&root).is_empty());
    // the running project is the file's, not the plain one a refused file leaves
    assert_eq!(*Project::of(&root), loaded);
    assert_eq!(Project::problem(&root), None);
    assert_eq!(
        Project::retired(&root),
        [
            says("[agents] transports (line 3)"),
            says("[board] disposable (line 8)"),
            says("[board] export (line 7)"),
        ]
    );
    // (table by table, sorted within each, as unknown keys are named)
    // one key is one line
    std::fs::write(root.join(FILE), "[board]\nexport = \"t.md\"\n").unwrap();
    assert_eq!(Project::retired(&root), [says("[board] export (line 2)")]);
    // a file that writes none, or does not parse, says nothing here
    std::fs::write(root.join(FILE), "[board]\nguide = \"docs/TASKS.md\"\n").unwrap();
    assert!(Project::retired(&root).is_empty());
    std::fs::write(root.join(FILE), "[board\nexport = \"t.md\"\n").unwrap();
    assert!(Project::retired(&root).is_empty());
    assert!(Project::load(&root).is_err());
}

#[test]
fn the_settings_check_and_the_lint_say_it_and_pass() {
    let tree = Tree::new();
    std::fs::write(tree.root.join(FILE), ALL).unwrap();
    let said = [
        says("[agents] transports (line 3)"),
        says("[board] disposable (line 8)"),
        says("[board] export (line 7)"),
    ]
    .map(|line| line + "\n")
    .concat();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = [tree.root.to_str().unwrap().to_string()];
    assert_eq!(symphony::lint::settings(&args, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(err).unwrap(), said);
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("✔ symphony.toml")
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    // the lint goes on past the project file to the documents, of which this tree has none (2)
    assert_eq!(symphony::lint::main(&tree.root, &mut out, &mut err), 2);
    let err = String::from_utf8(err).unwrap();
    assert!(err.starts_with(&said), "{err}");
    assert!(!err.contains("the project file is refused"), "{err}");
    // beside a key that is refused, both still say it, and fail on the other alone
    std::fs::write(tree.root.join(FILE), format!("{ALL}starts = [\"y\"]\n")).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(symphony::lint::main(&tree.root, &mut out, &mut err), 1);
    let err = String::from_utf8(err).unwrap();
    assert!(err.starts_with(&said), "{err}");
    assert!(
        err.contains("[board] starts (line 9) is not a setting"),
        "{err}"
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(symphony::lint::settings(&args, &mut out, &mut err), 1);
    let err = String::from_utf8(err).unwrap();
    assert!(!err.contains("does nothing"), "{err}");
    assert!(
        err.contains("[board] starts (line 9) is not a setting"),
        "{err}"
    );
}
