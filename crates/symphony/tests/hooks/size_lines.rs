//! What every agent reads at the start of a session stays within a line: the rules, the guides, each
//! file of a shipped skill and each project page beside an installed one has a line in bytes in
//! `size_lines.toml` beside this file, and one grown past it is red, as is one with no line and a
//! line naming no such file (docs/CHECKS.md, "Size budgets"). It weighs the files git tracks; what
//! a file says is a reviewer's to read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The file of lines, from the workspace's root.
const LINES: &str = "crates/symphony/tests/hooks/size_lines.toml";

/// The rules and the guides, each read whole by a session that works where it stands.
const GUIDES: [&str; 5] = [
    "RULES.md",
    "AGENTS.md",
    "apps/symphony-web/AGENTS.md",
    "apps/symphony-desktop/AGENTS.md",
    "crates/tasks/AGENTS.md",
];

/// Where the shipped skills' sources are, every file of which is held.
const SKILLS: &str = "crates/symphony/shipped/skills/";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Whether the tracked file `path` is one a line is owed for: a guide, a file of a shipped skill,
/// or the project's page beside an installed skill (`.agents/skills/NAME/project.md`).
fn held(path: &str) -> bool {
    let page = path
        .strip_prefix(".agents/skills/")
        .and_then(|rest| rest.strip_suffix("/project.md"))
        .is_some_and(|skill| !skill.is_empty() && !skill.contains('/'));
    GUIDES.contains(&path) || path.starts_with(SKILLS) || page
}

/// The held files `root` tracks, each with its size in bytes.
fn sizes(root: &Path) -> BTreeMap<String, u64> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .split('\0')
        .filter(|path| held(path))
        .map(|path| {
            let size = std::fs::metadata(root.join(path))
                .unwrap_or_else(|err| panic!("{path}: {err}"))
                .len();
            (path.to_string(), size)
        })
        .collect()
}

/// The lines `text` gives, a file's path to its most bytes.
fn lines(text: &str) -> BTreeMap<String, u64> {
    toml::from_str(text).unwrap_or_else(|err| panic!("{LINES}: {err}"))
}

/// What is wrong between the held files' `sizes` and their `lines`, one sentence each: a file past
/// its line, a file with none, and a line for a file that is not held.
fn reds(sizes: &BTreeMap<String, u64>, lines: &BTreeMap<String, u64>) -> Vec<String> {
    let mut reds = Vec::new();
    for (path, size) in sizes {
        match lines.get(path) {
            Some(line) if size > line => reds.push(format!(
                "{path} is {size} bytes, {} past its line of {line}: cut it, or move the \
                 mechanism it retells to its document or module comment, before any raise",
                size - line
            )),
            Some(_) => {}
            None => reds.push(format!(
                "{path} ({size} bytes) has no line: give it one in {LINES}, a tenth above its \
                 size"
            )),
        }
    }
    for path in lines.keys().filter(|path| !sizes.contains_key(*path)) {
        reds.push(format!(
            "{LINES} has a line for {path}, which is no guide, shipped skill file or project \
             page here: take the line out"
        ));
    }
    reds
}

#[test]
fn the_guides_the_rules_and_the_shipped_skills_stay_within_their_lines() {
    let root = workspace();
    let sizes = sizes(&root);
    for guide in GUIDES {
        assert!(sizes.contains_key(guide), "{guide} is read: {sizes:?}");
    }
    assert!(
        sizes.keys().any(|path| path.starts_with(SKILLS))
            && sizes.keys().any(|path| path.starts_with(".agents/")),
        "the shipped skills and the project pages are read: {sizes:?}"
    );
    let lines = lines(&std::fs::read_to_string(root.join(LINES)).unwrap());
    let reds = reds(&sizes, &lines);
    assert!(
        reds.is_empty(),
        "what every session reads grew unseen (docs/CHECKS.md, \"Size budgets\"):\n{}",
        reds.join("\n")
    );
}

#[test]
fn the_guides_the_skill_files_and_the_project_pages_are_held_and_nothing_else() {
    for path in [
        "RULES.md",
        "AGENTS.md",
        "apps/symphony-web/AGENTS.md",
        "apps/symphony-desktop/AGENTS.md",
        "crates/tasks/AGENTS.md",
        "crates/symphony/shipped/skills/work/SKILL.md",
        "crates/symphony/shipped/skills/work/start.md",
        "crates/symphony/shipped/skills/a-new-skill/SKILL.md",
        ".agents/skills/work/project.md",
        ".agents/skills/a-new-skill/project.md",
    ] {
        assert!(held(path), "{path}");
    }
    for path in [
        "README.md",
        "docs/RULES.md",
        "docs/TASKS.md",
        "apps/symphony-server/README.md",
        "crates/symphony/shipped/symphony.just",
        "crates/symphony/shipped/skills",
        ".agents/skills/work/SKILL.md",
        ".agents/skills/project.md",
        ".agents/skills//project.md",
        ".agents/skills/work/pages/project.md",
        ".claude/skills/work/project.md",
        "crates/symphony/tests/hooks/size_lines.toml",
    ] {
        assert!(!held(path), "{path}");
    }
}

fn map(entries: &[(&str, u64)]) -> BTreeMap<String, u64> {
    entries
        .iter()
        .map(|(path, bytes)| (path.to_string(), *bytes))
        .collect()
}

#[test]
fn a_file_at_or_under_its_line_is_green() {
    let sizes = map(&[("RULES.md", 100), ("AGENTS.md", 0)]);
    let lines = map(&[("RULES.md", 100), ("AGENTS.md", 50)]);
    assert_eq!(reds(&sizes, &lines), Vec::<String>::new());
    assert_eq!(reds(&map(&[]), &map(&[])), Vec::<String>::new());
}

#[test]
fn a_file_grown_past_its_line_is_red_by_its_name_its_bytes_and_its_line() {
    let sizes = map(&[("RULES.md", 101), ("AGENTS.md", 10)]);
    let lines = map(&[("RULES.md", 100), ("AGENTS.md", 50)]);
    assert_eq!(
        reds(&sizes, &lines),
        [
            "RULES.md is 101 bytes, 1 past its line of 100: cut it, or move the mechanism it \
             retells to its document or module comment, before any raise"
        ]
    );
}

#[test]
fn a_held_file_with_no_line_is_red_and_says_to_give_it_one() {
    let sizes = map(&[("crates/symphony/shipped/skills/new/SKILL.md", 700)]);
    assert_eq!(
        reds(&sizes, &map(&[])),
        [
            "crates/symphony/shipped/skills/new/SKILL.md (700 bytes) has no line: give it one in \
             crates/symphony/tests/hooks/size_lines.toml, a tenth above its size"
        ]
    );
}

#[test]
fn a_line_for_a_file_that_is_not_held_is_red() {
    let sizes = map(&[("RULES.md", 10)]);
    let lines = map(&[("RULES.md", 100), ("docs/TASKS.md", 100)]);
    assert_eq!(
        reds(&sizes, &lines),
        [
            "crates/symphony/tests/hooks/size_lines.toml has a line for docs/TASKS.md, which is \
             no guide, shipped skill file or project page here: take the line out"
        ]
    );
}

#[test]
fn every_red_is_told_together() {
    let sizes = map(&[("AGENTS.md", 9), ("RULES.md", 200)]);
    let lines = map(&[("RULES.md", 100), ("gone.md", 1)]);
    let reds = reds(&sizes, &lines);
    assert_eq!(reds.len(), 3, "{reds:?}");
    assert!(reds[0].starts_with("AGENTS.md (9 bytes) has no line"));
    assert!(reds[1].starts_with("RULES.md is 200 bytes, 100 past its line of 100"));
    assert!(reds[2].contains("has a line for gone.md"));
}

#[test]
fn the_lines_are_read_as_paths_and_bytes() {
    assert_eq!(
        lines("# a comment\n\"RULES.md\" = 22_000\n\"a/b.md\" = 5\n"),
        map(&[("RULES.md", 22_000), ("a/b.md", 5)])
    );
}

#[test]
#[should_panic(expected = "size_lines.toml")]
fn a_line_that_is_no_count_of_bytes_is_refused() {
    lines("\"RULES.md\" = \"large\"\n");
}
