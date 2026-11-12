//! Symphony names only its own things. Every variable, state folder, format, setting and tool name
//! it carried from Star Foundry, the project it grew in, left at once (the owner's B on the card
//! `symphony-own-names`): what still matches `sf-`, `sf_` or "Star Foundry" in a tracked text file
//! is one of the fences below, each with why it stays, pinned by its count of matching lines, so a
//! new old name anywhere fails here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What an old name looks like, as `git grep -i -E` reads it.
const OLD: &str = "sf[-_]|star.?foundry";

/// This file, which names the old names to fence them.
const HERE: &str = "crates/symphony/tests/hooks/own_names.rs";

/// Each file that may still name an old name, how many of its lines do, and why.
const FENCES: [(&str, usize, &str); 15] = [
    (
        "apps/symphony-server/tests/standalone/main.rs",
        7,
        "the standalone guard: a consuming project's former tool names as executables that fail \
         the test if this workspace runs one",
    ),
    (
        "crates/tasks/tests/launcher/move_state.rs",
        96,
        "the move command's tests: the old folders it moves, as fixtures and expected output",
    ),
    (
        "tools/move-state.sh",
        41,
        "the move command: the old folders it renames into symphony/",
    ),
    (
        "docs/DECISIONS.md",
        9,
        "the card symphony-own-names, a dated record, and the board task slug \
         tools/star-foundry-gates-adapter",
    ),
    (
        "docs/analysis/own-names.md",
        163,
        "the dated inventory of the old names, and the plan the owner's B voided",
    ),
    (
        "docs/analysis/board-design.md",
        8,
        "measurements of Star Foundry's board, read 2026-11-11",
    ),
    (
        "docs/analysis/board-design/sf-board.js",
        2,
        "the data file holding that board as read",
    ),
    (
        "docs/analysis/board-design/a-stuck-lane.html",
        1,
        "a mock-up drawing that board's data file",
    ),
    (
        "docs/analysis/board-design/b-stuck-in-place.html",
        1,
        "a mock-up drawing that board's data file",
    ),
    (
        "docs/analysis/board-design/c-who-acts-next.html",
        1,
        "a mock-up drawing that board's data file",
    ),
    (
        "docs/analysis/board-design/projects-health.html",
        1,
        "a mock-up listing the machine's registered projects by name",
    ),
    (
        "docs/analysis/client-cache.md",
        10,
        "Star Foundry's register, measured as the large one",
    ),
    (
        "docs/analysis/desktop-client.md",
        1,
        "the machine's project registry as read 2026-11-11",
    ),
    (
        "docs/analysis/reports-and-monitoring.md",
        34,
        "readings and measurements of Star Foundry's board, stamps, runs and tools",
    ),
    (
        "docs/analysis/symphony-update.md",
        5,
        "what was read of Star Foundry on 2026-11-11, and the open question on its skill copies",
    ),
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Each tracked text file of `root` with lines matching [`OLD`], and how many do, this file aside.
fn named(root: &Path) -> BTreeMap<String, usize> {
    let done = Command::new("git")
        .current_dir(root)
        .args(["grep", "-I", "-c", "-i", "-E", OLD, "--", "."])
        .arg(format!(":(exclude){HERE}"))
        .output()
        .expect("git runs");
    // git grep answers 1 when no line matches, 2 or more when it fails.
    assert!(
        done.status.code().is_some_and(|code| code <= 1),
        "git grep failed: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let (file, count) = line.rsplit_once(':').expect("FILE:COUNT");
            (file.to_string(), count.parse().expect("a count"))
        })
        .collect()
}

/// What differs between the lines `found` and the fences allow: one line per file.
fn strays(found: &BTreeMap<String, usize>, fenced: &BTreeMap<&str, usize>) -> Vec<String> {
    let mut files: Vec<&str> = found.keys().map(String::as_str).collect();
    files.extend(fenced.keys().copied());
    files.sort_unstable();
    files.dedup();
    files
        .into_iter()
        .filter_map(|file| {
            let (lines, allowed) = (found.get(file).copied(), fenced.get(file).copied());
            match (lines, allowed) {
                (Some(lines), Some(allowed)) if lines == allowed => None,
                (Some(lines), Some(allowed)) => Some(format!(
                    "{file}: {lines} lines name an old name, its fence says {allowed}"
                )),
                (Some(lines), None) => Some(format!(
                    "{file}: {lines} lines name an old name, outside every fence"
                )),
                (None, _) => Some(format!("{file}: fenced, but names no old name any more")),
            }
        })
        .collect()
}

#[test]
fn old_names_stay_inside_their_fences() {
    let root = workspace();
    assert!(root.join(HERE).is_file(), "{HERE} names this file");
    let fenced: BTreeMap<&str, usize> = FENCES
        .iter()
        .map(|(file, lines, _)| (*file, *lines))
        .collect();
    let strays = strays(&named(&root), &fenced);
    assert!(
        strays.is_empty(),
        "name Symphony's own thing instead, or fence the file here with why (and its count):\n{}",
        strays.join("\n")
    );
}

#[test]
fn every_fence_is_one_file_with_lines_and_a_reason() {
    let mut files: Vec<&str> = FENCES.iter().map(|(file, _, _)| *file).collect();
    files.sort_unstable();
    files.dedup();
    assert_eq!(files.len(), FENCES.len(), "one fence per file");
    for (file, lines, why) in FENCES {
        assert!(lines > 0, "{file}: a fence of no lines");
        assert!(!why.trim().is_empty(), "{file}: a fence without its reason");
    }
}

#[test]
fn a_stray_is_a_file_outside_the_fences_a_count_past_one_or_a_fence_left_empty() {
    let found: BTreeMap<String, usize> = [("a.rs", 2), ("b.md", 3), ("c.md", 1)]
        .map(|(file, lines)| (file.to_string(), lines))
        .into();
    let fenced: BTreeMap<&str, usize> = [("b.md", 3), ("c.md", 2), ("d.md", 4)].into();
    assert_eq!(
        strays(&found, &fenced),
        [
            "a.rs: 2 lines name an old name, outside every fence",
            "c.md: 1 lines name an old name, its fence says 2",
            "d.md: fenced, but names no old name any more",
        ]
    );
}

#[test]
fn the_pattern_finds_each_form_of_an_old_name_and_not_symphonys_own() {
    let scratch = super::scratch("own-names");
    let git = |args: &[&str]| {
        let done = Command::new("git")
            .current_dir(&scratch)
            .args(args)
            .output()
            .unwrap();
        assert!(done.status.success(), "{args:?}: {done:?}");
    };
    git(&["init", "-q"]);
    for (file, text) in [
        ("variable.rs", "SF_PORT\n"),
        ("folder.md", "the board in .git/sf-tasks\n"),
        (
            "project.md",
            "Star Foundry's board\nstar-foundry\nStarFoundry\n",
        ),
        (
            "own.md",
            "SYMPHONY_PORT in symphony/tasks, the namespace sf, symphony.board\n",
        ),
    ] {
        std::fs::write(scratch.join(file), text).unwrap();
    }
    std::fs::write(scratch.join("binary.bin"), b"\0sf-tasks\0").unwrap();
    git(&["add", "."]);
    let counted: Vec<(String, usize)> = named(&scratch).into_iter().collect();
    assert_eq!(
        counted,
        [("folder.md", 1), ("project.md", 3), ("variable.rs", 1)]
            .map(|(file, lines)| (file.to_string(), lines))
    );
    let _ = std::fs::remove_dir_all(&scratch);
}
