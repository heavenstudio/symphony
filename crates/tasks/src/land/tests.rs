//! What a land reads without a repository or a board: the project's settings, the approval on a
//! thread and the commits it names. The landing itself is `tests/sweep/land.rs`'s, on scratch
//! repositories and boards.

use super::*;
use std::collections::BTreeMap;

fn task(status: &str) -> Task {
    Task {
        branch: None,
        id: 1,
        slug: "tools/lamp".into(),
        title: "A lamp".into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: BTreeMap::new(),
    }
}

/// A thread of `(kind, sender, text)` lines, one a second from 100.
fn thread(lines: &[(&str, &str, &str)]) -> Vec<Message> {
    lines
        .iter()
        .zip(1..)
        .map(|((kind, sender, text), id)| Message {
            id,
            slug: "tools/lamp".into(),
            sender: (*sender).into(),
            recipient: None,
            kind: (*kind).into(),
            text: (*text).into(),
            #[expect(clippy::cast_precision_loss, reason = "a handful of ids")]
            ts: 100.0 + id as f64,
        })
        .collect()
}

#[test]
fn a_project_with_no_settings_lands_on_main_with_no_check_and_pushes() {
    assert_eq!(Settings::parse("").unwrap(), Settings::default());
    let settings = Settings::default();
    assert_eq!(
        (
            settings.default_branch.as_str(),
            settings.check.is_empty(),
            settings.worktrees,
            settings.generators.is_empty(),
            settings.push
        ),
        ("main", true, None, true, true)
    );
}

#[test]
fn the_settings_a_land_reads_and_the_rest_of_the_file_left_alone() {
    let settings = Settings::parse(
        r#"
[project]
name = "Lamps"
default_branch = "trunk"

[agents]
panes = "hc"
worktrees = "../lamps-wt"

[verdicts]
check = ["just", "verify"]
land = ["just", "land"]

[land]
push = false
generators = [
  { name = "the shades", outputs = "shades/", command = ["just", "shades"] },
  { name = "the index", outputs = "index.md", command = ["just", "index"], when_changed = true },
]

[board]
on_review = ["just", "review"]
"#,
    )
    .unwrap();
    assert_eq!(
        settings,
        Settings {
            default_branch: "trunk".into(),
            check: vec!["just".into(), "verify".into()],
            worktrees: Some("../lamps-wt".into()),
            generators: vec![
                Generator {
                    name: "the shades".into(),
                    outputs: "shades/".into(),
                    command: vec!["just".into(), "shades".into()],
                    when_changed: false,
                },
                Generator {
                    name: "the index".into(),
                    outputs: "index.md".into(),
                    command: vec!["just".into(), "index".into()],
                    when_changed: true,
                },
            ],
            push: false,
        }
    );
}

#[test]
fn a_generators_outputs_read_as_the_symphony_reads_them() {
    for (written, read) in [
        ("gen/", "gen/"),
        ("./gen/", "gen/"),
        ("gen/./index.md", "gen/index.md"),
        ("a//b/", "a/b/"),
        ("index.md", "index.md"),
    ] {
        let text = format!(
            "[land]\ngenerators = [{{ name = \"x\", outputs = {written:?}, command = [\"true\"] }}]\n"
        );
        let settings = Settings::parse(&text).unwrap();
        assert_eq!(settings.generators[0].outputs, read, "{written}");
    }
}

#[test]
fn a_generator_or_branch_a_land_cannot_use_is_refused_by_name() {
    let refused = |text: &str| Settings::parse(text).unwrap_err();
    assert_eq!(
        refused("[project]\ndefault_branch = \" \"\n"),
        "symphony.toml: [project] default_branch is empty"
    );
    assert_eq!(
        refused("[land]\ngenerators = [{ name = \"x\", outputs = \"out/\", command = [] }]\n"),
        "symphony.toml: [land] generator x has no command"
    );
    assert_eq!(
        refused("[land]\ngenerators = [{ name = \"x\", outputs = \"out/\", command = [\"\"] }]\n"),
        "symphony.toml: [land] generator x has no command"
    );
    for outputs in ["", ".", "./", "/abs", "../up", "a/../b"] {
        let text = format!(
            "[land]\ngenerators = [{{ name = \"x\", outputs = {outputs:?}, command = [\"true\"] }}]\n"
        );
        assert_eq!(
            refused(&text),
            format!(
                "symphony.toml: [land] generator x writes {outputs:?}, not a path inside the project"
            )
        );
    }
    let misspelt = refused(
        "[land]\ngenerators = [{ name = \"x\", outputs = \"o/\", command = [\"true\"], when = 1 }]\n",
    );
    assert!(
        misspelt.starts_with("symphony.toml: ") && misspelt.contains("unknown field `when`"),
        "{misspelt}"
    );
    assert!(
        refused("[land]\npush = \"yes\"\n").starts_with("symphony.toml: "),
        "push is a boolean"
    );
}

#[test]
fn an_approval_stands_when_it_is_the_last_verdict_and_left_the_task_done() {
    let lines = thread(&[
        ("system", "amber", "amber claimed it"),
        ("verdict", "slate", "changes: the hash moved"),
        ("msg", "amber", "fixed at 4b5e7b0"),
        ("verdict", "slate", "approve: Approve at 4b5e7b0."),
        ("msg", "amber", "thanks"),
    ]);
    assert_eq!(
        approval(&task("done"), &lines).unwrap(),
        Approval {
            by: "slate".into(),
            at: 104.0,
            text: "approve: Approve at 4b5e7b0.".into(),
        }
    );
    // A bare `approve` is one too; `approved` is not.
    let bare = thread(&[("verdict", "slate", "approve")]);
    assert!(approval(&task("done"), &bare).is_ok());
    let not = thread(&[("verdict", "slate", "approved: fine")]);
    assert_eq!(
        approval(&task("done"), &not).unwrap_err(),
        "tools/lamp is done with no approving verdict from a reviewer"
    );
}

#[test]
fn no_approval_stands_on_open_work_on_work_closed_since_or_closed_another_way() {
    let approved = thread(&[("verdict", "slate", "approve: good")]);
    for status in ["ready", "in_progress", "review", "blocked"] {
        assert_eq!(
            approval(&task(status), &approved).unwrap_err(),
            format!("tools/lamp is {status}, with no approving verdict standing")
        );
    }
    let landed = thread(&[
        ("verdict", "slate", "approve: good"),
        (
            "verdict",
            "amber",
            "landed tools/lamp on main at 3e0c66192471\nmore",
        ),
    ]);
    assert_eq!(
        approval(&task("done"), &landed).unwrap_err(),
        "tools/lamp was closed after its approval: landed tools/lamp on main at 3e0c66192471"
    );
    let done = thread(&[("verdict", "amber", "done: integrated in 3e0c661")]);
    assert_eq!(
        approval(&task("done"), &done).unwrap_err(),
        "tools/lamp is done with no approving verdict from a reviewer"
    );
    assert_eq!(
        approval(&task("done"), &[]).unwrap_err(),
        "tools/lamp is done with no approving verdict from a reviewer"
    );
}

#[test]
fn the_words_that_may_name_a_commit() {
    assert_eq!(
        named("Approve at 4b5e7b0, merged with main 4f4c8fd (tip 4b5e7b0a1c2d)."),
        ["4b5e7b0", "4f4c8fd", "4b5e7b0a1c2d"]
    );
    // Forty digits at most.
    let full = "a".repeat(39) + "1";
    assert_eq!(named(&format!("{full} {full}0")), [full.as_str()]);
    // Too short, a letter past f, upper case, no digit, part of a longer word.
    for text in [
        "abc123",
        "4b5e7g0",
        "4B5E7B0",
        "defaced",
        "x4b5e7b0",
        "4b5e7b0_tip",
    ] {
        assert_eq!(named(text), Vec::<&str>::new(), "{text:?}");
    }
}

#[test]
fn counts_and_names_read_as_words() {
    assert_eq!(commits(1), "1 commit");
    assert_eq!(commits(3), "3 commits");
    assert_eq!(and(&[]), "");
    assert_eq!(and(&["the shades"]), "the shades");
    assert_eq!(and(&["a", "b", "c"]), "a, b and c");
    assert_eq!(short("4b5e7b0a1c2d"), "4b5e7b0");
    assert_eq!(short("4b5"), "4b5");
    assert!(is_verdict("approve", "approve"));
    assert!(is_verdict("approve: yes", "approve"));
    assert!(!is_verdict("approves", "approve"));
}

#[test]
fn two_names_of_one_place_are_the_same_and_others_are_not() {
    let dir = std::env::temp_dir().join(format!("sf-land-same-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("real/inner")).unwrap();
    std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
    let real = dir.join("real/land");
    // The land's folder need not be there yet; its parent is resolved.
    assert!(same(&dir.join("link/land"), &real));
    assert!(same(&dir.join("real/inner/../land"), &real));
    assert!(same(&dir.join("gone/land"), &dir.join("gone/land")));
    assert!(!same(&dir.join("gone/land"), &dir.join("gone/other")));
    assert!(!same(&dir.join("link/other"), &real));
    fs::remove_dir_all(&dir).unwrap();
}
