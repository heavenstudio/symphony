//! What every agent reads at the start of a session stays within a line: the rules, the guides, each
//! file of a shipped skill and each project page beside an installed one has a line in bytes in
//! `size_lines.toml` beside this file, and one grown past it is red, as is one with no line and a
//! line naming no such file (docs/CHECKS.md, "Size budgets"). Each line is set under a bare key
//! made from its file's path ([`key`]), so a gate decision's raise can name it, and every line is
//! read here as the gate reads it. It weighs the files git tracks; what a file says is a
//! reviewer's to read.

use crate::support::repo as workspace;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use symphony_board::gates::Limit;

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

/// Whether the tracked file `path` is one a line is owed for: a guide, a file of a shipped skill,
/// or the project's page beside an installed skill (`.agents/skills/NAME/project.md`).
fn held(path: &str) -> bool {
    let page = path
        .strip_prefix(".agents/skills/")
        .and_then(|rest| rest.strip_suffix("/project.md"))
        .is_some_and(|skill| !skill.is_empty() && !skill.contains('/'));
    GUIDES.contains(&path) || path.starts_with(SKILLS) || page
}

/// The key `path`'s line is set under: the path with its `.md` dropped and each character other
/// than an ASCII letter, a digit, `_` or `-` made `-`, a leading one dropped
/// (`apps/symphony-web/AGENTS.md` is `apps-symphony-web-AGENTS`). A bare TOML key, which a raise
/// names as `size_lines.toml:KEY`, where the gate refuses a path.
fn key(path: &str) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    let key: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    key.trim_start_matches('-').to_string()
}

/// The line a file of `size` bytes is given: its size plus a tenth, rounded up to a multiple of 100.
fn line_for(size: u64) -> u64 {
    (size + size.div_ceil(10)).div_ceil(100) * 100
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

/// The lines `text` gives, a key to its most bytes.
fn lines(text: &str) -> BTreeMap<String, u64> {
    toml::from_str(text).unwrap_or_else(|err| panic!("{LINES}: {err}"))
}

/// What is wrong between the held files' `sizes` and their `lines`, one sentence each: a file past
/// its line, a file with none, two files of one key, and a line for no held file.
fn reds(sizes: &BTreeMap<String, u64>, lines: &BTreeMap<String, u64>) -> Vec<String> {
    let mut reds = Vec::new();
    let mut keys: BTreeMap<String, &str> = BTreeMap::new();
    for (path, &size) in sizes {
        let key = key(path);
        if let Some(first) = keys.insert(key.clone(), path) {
            reds.push(format!(
                "{first} and {path} are both {key} in {LINES}, where one line cannot hold two \
                 files: rename one"
            ));
            continue;
        }
        match lines.get(&key) {
            Some(&line) if size > line => reds.push(format!(
                "{path} is {size} bytes, {} past its line of {line} ({key} in {LINES}): cut it, \
                 or move the mechanism it retells to its document or module comment, before any \
                 raise",
                size - line
            )),
            Some(_) => {}
            None => reds.push(format!(
                "{path} ({size} bytes) has no line: give it one in {LINES}, {key} = {}, its size \
                 plus a tenth",
                line_for(size)
            )),
        }
    }
    for key in lines.keys().filter(|key| !keys.contains_key(*key)) {
        reds.push(format!(
            "{LINES} has a line {key}, the key of no guide, shipped skill file or project page \
             here: take it out"
        ));
    }
    reds
}

/// The `lines` of the file `text` a gate decision could not raise, one sentence each: each must be
/// a limit `just tasks gate` reads as `LINES:KEY`, at the number read here.
fn unraisable(text: &str, lines: &BTreeMap<String, u64>) -> Vec<String> {
    (lines.iter())
        .filter_map(|(key, &line)| {
            match Limit::parse(&format!("{LINES}:{key}")).and_then(|limit| limit.read(text)) {
                Ok(read) if read == line => None,
                Ok(read) => Some(format!(
                    "a gate reads {key} as {read}, not its line of {line}"
                )),
                Err(why) => Some(format!("a gate decision cannot raise {key}: {why}")),
            }
        })
        .collect()
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
    let text = std::fs::read_to_string(root.join(LINES)).unwrap();
    let lines = lines(&text);
    let mut reds = reds(&sizes, &lines);
    reds.extend(unraisable(&text, &lines));
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

#[test]
fn a_key_is_its_path_made_a_bare_key_a_raise_can_name() {
    for (path, expected) in [
        ("RULES.md", "RULES"),
        ("apps/symphony-web/AGENTS.md", "apps-symphony-web-AGENTS"),
        (
            ".agents/skills/work/project.md",
            "agents-skills-work-project",
        ),
        (
            "crates/symphony/shipped/skills/apply-review/rewrite-brief.md",
            "crates-symphony-shipped-skills-apply-review-rewrite-brief",
        ),
        (
            "crates/symphony/shipped/skills/x/data.json",
            "crates-symphony-shipped-skills-x-data-json",
        ),
        ("a b+c_d.md", "a-b-c_d"),
        ("notes.md.md", "notes-md"),
    ] {
        assert_eq!(key(path), expected, "{path}");
        assert!(
            Limit::parse(&format!("{LINES}:{expected}")).is_ok(),
            "{path}"
        );
    }
}

#[test]
fn a_line_is_the_size_plus_a_tenth_rounded_up_to_a_hundred() {
    for (size, line) in [
        (0, 0),
        (1, 100),
        (90, 100),
        (91, 200),
        (715, 800),
        (730, 900),
        (1_000, 1_100),
        (20_714, 22_800),
    ] {
        assert_eq!(line_for(size), line, "{size}");
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
    let sizes = map(&[("RULES.md", 100), ("apps/symphony-web/AGENTS.md", 0)]);
    let lines = map(&[("RULES", 100), ("apps-symphony-web-AGENTS", 50)]);
    assert_eq!(reds(&sizes, &lines), Vec::<String>::new());
    assert_eq!(reds(&map(&[]), &map(&[])), Vec::<String>::new());
}

#[test]
fn a_file_grown_past_its_line_is_red_by_its_name_its_bytes_and_its_line() {
    let sizes = map(&[("RULES.md", 101), ("AGENTS.md", 10)]);
    let lines = map(&[("RULES", 100), ("AGENTS", 50)]);
    assert_eq!(
        reds(&sizes, &lines),
        ["RULES.md is 101 bytes, 1 past its line of 100 (RULES in \
             crates/symphony/tests/hooks/size_lines.toml): cut it, or move the mechanism it \
             retells to its document or module comment, before any raise"]
    );
}

#[test]
fn a_held_file_with_no_line_is_red_and_says_to_give_it_one() {
    let sizes = map(&[("crates/symphony/shipped/skills/new/SKILL.md", 700)]);
    assert_eq!(
        reds(&sizes, &map(&[])),
        [
            "crates/symphony/shipped/skills/new/SKILL.md (700 bytes) has no line: give it one in \
             crates/symphony/tests/hooks/size_lines.toml, crates-symphony-shipped-skills-new-SKILL \
             = 800, its size plus a tenth"
        ]
    );
}

#[test]
fn a_line_for_no_held_file_is_red() {
    let sizes = map(&[("RULES.md", 10)]);
    let lines = map(&[("RULES", 100), ("docs-TASKS", 100), ("RULES.md", 100)]);
    assert_eq!(
        reds(&sizes, &lines),
        [
            "crates/symphony/tests/hooks/size_lines.toml has a line RULES.md, the key of no \
             guide, shipped skill file or project page here: take it out",
            "crates/symphony/tests/hooks/size_lines.toml has a line docs-TASKS, the key of no \
             guide, shipped skill file or project page here: take it out",
        ]
    );
}

#[test]
fn two_files_of_one_key_are_red() {
    let sizes = map(&[
        ("crates/symphony/shipped/skills/a-b/c.md", 10),
        ("crates/symphony/shipped/skills/a/b-c.md", 10),
    ]);
    let lines = map(&[("crates-symphony-shipped-skills-a-b-c", 100)]);
    assert_eq!(
        reds(&sizes, &lines),
        [
            "crates/symphony/shipped/skills/a-b/c.md and crates/symphony/shipped/skills/a/b-c.md \
             are both crates-symphony-shipped-skills-a-b-c in \
             crates/symphony/tests/hooks/size_lines.toml, where one line cannot hold two files: \
             rename one"
        ]
    );
}

#[test]
fn every_red_is_told_together() {
    let sizes = map(&[("AGENTS.md", 9), ("RULES.md", 200)]);
    let lines = map(&[("RULES", 100), ("gone", 1)]);
    let reds = reds(&sizes, &lines);
    assert_eq!(reds.len(), 3, "{reds:?}");
    assert!(reds[0].starts_with("AGENTS.md (9 bytes) has no line"));
    assert!(reds[1].starts_with("RULES.md is 200 bytes, 100 past its line of 100"));
    assert!(reds[2].contains("has a line gone,"));
}

#[test]
fn each_line_is_one_a_gate_decision_raises() {
    let text = "# RULES = 9\nRULES = 22_800\napps-symphony-web-AGENTS = 16_400\n";
    assert_eq!(unraisable(text, &lines(text)), Vec::<String>::new());
    assert_eq!(
        unraisable(text, &map(&[("RULES", 22_900)])),
        ["a gate reads RULES as 22800, not its line of 22900"]
    );
    let quoted = "\"RULES.md\" = 22_800\n";
    let refused = unraisable(quoted, &lines(quoted));
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].starts_with("a gate decision cannot raise RULES.md: a raise names its limit"),
        "{refused:?}"
    );
    let unread = "\"RULES\" = 22_800\n";
    let refused = unraisable(unread, &lines(unread));
    assert_eq!(
        refused,
        [format!(
            "a gate decision cannot raise RULES: no line of {LINES} sets RULES"
        )]
    );
}

#[test]
fn the_lines_are_read_as_keys_and_bytes() {
    assert_eq!(
        lines("# a comment\nRULES = 22_000\na-b = 5\n"),
        map(&[("RULES", 22_000), ("a-b", 5)])
    );
}

#[test]
#[should_panic(expected = "size_lines.toml")]
fn a_line_that_is_no_count_of_bytes_is_refused() {
    lines("RULES = \"large\"\n");
}
