//! `[land] check_paths`: what an entry names, how a Rust file's tests and lints are found, and what
//! a branch's commits change of them in a scratch repository.

use super::*;
use std::fs;
use std::path::PathBuf;

fn entry(written: &str) -> Entry {
    Entry::parse(written).unwrap()
}

#[test]
fn an_entry_names_paths_as_a_gitignore_line_does() {
    for (written, path, names) in [
        // A folder anywhere, and what is inside it, never a file of its name.
        ("tests/", "tests/a.rs", true),
        ("tests/", "crates/tasks/tests/store/next.rs", true),
        ("tests/", "crates/tasks/src/tests", false),
        ("tests/", "crates/tasks/src/contests/a.rs", false),
        // A last part anywhere, globbed within it alone.
        ("*_tests.rs", "crates/symphony/src/builds_tests.rs", true),
        ("*_tests.rs", "builds_tests.rs", true),
        ("*_tests.rs", "crates/x/src/tests.rs", false),
        ("*_tests.rs", "a_tests.rs/b.rs", true),
        ("tests.rs", "crates/tasks/src/land/tests.rs", true),
        ("tests.rs", "crates/tasks/src/land/tests.rs.orig", false),
        ("t?sts.rs", "a/tests.rs", true),
        ("t?sts.rs", "a/tsts.rs", false),
        ("*", "anything/at/all", true),
        ("a*b*c", "abxbc", true),
        ("a*b*c", "abxbd", false),
        // Anchored at the root by a slash at its start or in its middle.
        ("/justfile", "justfile", true),
        ("/justfile", "crates/x/justfile", false),
        ("tools/size-budget.toml", "tools/size-budget.toml", true),
        ("tools/size-budget.toml", "x/tools/size-budget.toml", false),
        ("/tools/", "tools/cargo.sh", true),
        ("/tools/", "tools/test-support/script.rs", true),
        ("/tools/", "tools", false),
        ("/crates/*/build.rs", "crates/tasks/build.rs", true),
        ("/crates/*/build.rs", "crates/tasks/src/build.rs", false),
        // A table's entry names its file alone.
        ("/symphony.toml [verdicts]", "symphony.toml", true),
        ("/symphony.toml [verdicts]", "symphony.toml/x", false),
        ("Cargo.toml [workspace.lints]", "crates/x/Cargo.toml", true),
        ("Cargo.toml [workspace.lints]", "Cargo.toml/x.toml", false),
    ] {
        assert_eq!(entry(written).names(path), names, "{written} on {path}");
    }
}

#[test]
fn an_entry_is_read_trimmed_with_its_table_and_one_naming_nothing_inside_is_refused() {
    let table = entry("  /symphony.toml [ releases ]  ");
    assert_eq!(table.written, "/symphony.toml [ releases ]");
    assert_eq!(table.table, Some(vec!["releases".to_string()]));
    assert_eq!(
        entry("/Cargo.toml [workspace.lints]").table,
        Some(vec!["workspace".to_string(), "lints".to_string()])
    );
    assert_eq!(
        entry("tests/[x]").table,
        None,
        "a bracket with no space before it is a name"
    );
    for (written, why) in [
        ("", "which is no path inside the project"),
        ("/", "which is no path inside the project"),
        ("../tests/", "which is no path inside the project"),
        ("a/./b", "which is no path inside the project"),
        ("a//b", "which is no path inside the project"),
        ("/symphony.toml []", "whose table has no TOML table's name"),
        (
            "/symphony.toml [a..b]",
            "whose table has no TOML table's name",
        ),
        ("/tools/ [x]", "a table of a folder, which is no TOML file"),
    ] {
        assert_eq!(
            Entry::parse(written).unwrap_err(),
            format!("[land] check_paths names {written:?}, {why}"),
        );
    }
    assert_eq!(
        parse(&["tests/".into(), "..".into()]).unwrap_err(),
        "[land] check_paths names \"..\", which is no path inside the project"
    );
}

#[test]
fn the_default_names_the_tests_the_recipes_the_budget_and_the_project_files_check_tables() {
    let defaults = defaults();
    let named = |path: &str| -> Vec<&str> {
        defaults
            .iter()
            .filter(|e| e.names(path))
            .map(|e| e.written.as_str())
            .collect()
    };
    assert_eq!(named("crates/tasks/tests/store/next.rs"), ["tests/"]);
    assert_eq!(named("crates/tasks/src/land/tests.rs"), ["tests.rs"]);
    assert_eq!(named("crates/symphony/src/builds_tests.rs"), ["*_tests.rs"]);
    assert_eq!(named("justfile"), ["/justfile"]);
    for tool in [
        "tools/size-budget.toml",
        "tools/cargo.sh",
        "tools/seed-clone.rs",
    ] {
        assert_eq!(named(tool), ["/tools/"], "{tool}");
    }
    assert_eq!(named("clippy.toml"), ["clippy.toml"]);
    assert_eq!(named("crates/tasks/.clippy.toml"), [".clippy.toml"]);
    assert_eq!(named(".cargo/config.toml"), ["/.cargo/"]);
    assert_eq!(
        named("Cargo.toml"),
        ["/Cargo.toml [workspace.lints]", "Cargo.toml [lints]"]
    );
    assert_eq!(named("crates/tasks/Cargo.toml"), ["Cargo.toml [lints]"]);
    assert_eq!(
        named("symphony.toml"),
        [
            "/symphony.toml [verdicts]",
            "/symphony.toml [land]",
            "/symphony.toml [releases]"
        ]
    );
    for product in [
        "crates/tasks/src/land.rs",
        "crates/tasks/tools/cargo.sh",
        "docs/TASKS.md",
        "Cargo.lock",
        "crates/symphony/symphony.toml",
    ] {
        assert_eq!(named(product), Vec::<&str>::new(), "{product}");
    }
}

#[test]
fn an_item_under_cfg_test_runs_to_its_closing_brace_or_the_end_of_its_one_line() {
    let text = "\
use std::fs;                              // 1
#[cfg(test)]                              // 2
mod tests;                                // 3
pub fn f() -> u8 {                        // 4
    #[cfg(test)]                          // 5
    let x = 1;                            // 6
    2                                     // 7
}                                         // 8
#[cfg(all(test, unix))]                   // 9
#[expect(                                 // 10
    clippy::unwrap_used,                  // 11
    reason = \"a test\"                   // 12
)]                                        // 13
mod unix {                                // 14
    fn a() {                              // 15
    }                                     // 16
}                                         // 17
pub struct S {                            // 18
    #[cfg(test)]                          // 19
    seen: u8,                             // 20
}                                         // 21
#[cfg(test)] use x::y;                    // 22
#[cfg(not(test))]                         // 23
fn product() {}                           // 24
#[cfg(test)]                              // 25
mod open {                                // 26
";
    let text: String = text
        .lines()
        .map(|l| l.split("//").next().unwrap().trim_end().to_string() + "\n")
        .collect();
    assert_eq!(
        regions(&text),
        [(2, 3), (5, 6), (9, 17), (19, 20), (22, 22), (25, 26)],
        "the last, never closed, runs to the end"
    );
    assert_eq!(regions(""), []);
}

#[test]
fn a_diffs_hunks_say_their_lines_and_what_they_add() {
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
index 1..2 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -3 +3 @@ fn f() {
-    1
+    2
@@ -10,0 +11,2 @@ mod tests {
+#[expect(clippy::x, reason = \"y\")]
+++ a line that starts with two plusses
@@ -20,2 +22,0 @@
-gone
-too
";
    assert_eq!(
        hunks(diff),
        [
            Hunk {
                old: (3, 1),
                new: (3, 1),
                added: vec![(3, "    2".into())],
            },
            Hunk {
                old: (10, 0),
                new: (11, 2),
                added: vec![
                    (11, "#[expect(clippy::x, reason = \"y\")]".into()),
                    (12, "++ a line that starts with two plusses".into()),
                ],
            },
            Hunk {
                old: (20, 2),
                new: (22, 0),
                added: vec![],
            },
        ]
    );
    assert!(allows("    #[expect(clippy::x, reason = \"y\")]"));
    assert!(allows("#![allow(dead_code)]"));
    assert!(!allows("// #[expect(x)]"));
    assert_eq!(within((10, 0), &[(5, 20)]), None, "a place between lines");
    assert_eq!(within((4, 3), &[(5, 20)]), Some(5));
    assert_eq!(within((21, 3), &[(5, 20)]), None);
}

#[test]
fn a_modules_file_is_where_rust_looks_for_it() {
    assert_eq!(
        declared("crates/x/src/lib.rs", "land", None),
        ["crates/x/src/land.rs", "crates/x/src/land/mod.rs"]
    );
    assert_eq!(
        declared("crates/x/src/land.rs", "tests", None),
        [
            "crates/x/src/land/tests.rs",
            "crates/x/src/land/tests/mod.rs"
        ]
    );
    assert_eq!(
        declared("crates/x/src/bin/tool.rs", "helper", None),
        [
            "crates/x/src/bin/tool/helper.rs",
            "crates/x/src/bin/tool/helper/mod.rs",
            "crates/x/src/bin/helper.rs",
            "crates/x/src/bin/helper/mod.rs"
        ],
        "a binary's root may declare beside itself"
    );
    assert_eq!(
        declared(
            "crates/symphony/src/lib.rs",
            "fixture",
            Some("../../../tools/test-support/script_board.rs")
        ),
        ["tools/test-support/script_board.rs"]
    );
    let text = "\
pub mod land;
#[cfg(test)]
#[path = \"unanswered_scratch.rs\"]
mod scratch;
pub(crate) mod r#async;
mod inline {
modest;
";
    assert_eq!(
        declarations(text),
        [
            (1, "land".to_string(), None),
            (
                4,
                "scratch".to_string(),
                Some("unanswered_scratch.rs".to_string())
            ),
            (5, "async".to_string(), None),
        ]
    );
}

/// A scratch repository, removed when dropped.
struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!(
            "sf-check-paths-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let repo = Repo(dir);
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let done = crate::sys::output(
            crate::sys::command("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
                .arg(&self.0)
                .args(args),
        )
        .unwrap();
        assert!(done.status.success(), "git {args:?}: {done:?}");
        String::from_utf8(done.stdout).unwrap().trim().to_string()
    }

    /// `files` written (an empty text removes one) and committed: the commit.
    fn commit(&self, files: &[(&str, &str)]) -> String {
        for (path, text) in files {
            let file = self.0.join(path);
            if text.is_empty() {
                fs::remove_file(&file).unwrap();
            } else {
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(&file, text).unwrap();
            }
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "a change"]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// What the branch `name`, made from main with `files` committed, changes of the defaults'
    /// paths and the Rust files' tests and lints.
    fn touched(&self, name: &str, files: &[(&str, &str)]) -> Vec<String> {
        self.git(&["checkout", "-qb", name, "main"]);
        let tip = self.commit(files);
        self.git(&["checkout", "-q", "main"]);
        touched(&self.0, "main", &tip, &defaults()).unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const LIB: &str = "\
pub mod jump;
#[cfg(test)]
mod fixture;
#[cfg(test)]
#[path = \"../support/helper.rs\"]
mod helper;

pub fn height() -> u8 {
    3
}

#[cfg(test)]
mod inline {
    #[test]
    fn three() {
        assert_eq!(super::height(), 3);
    }
}
";

const PROJECT: &str = "\
[project]
name = \"Lamps\"

[verdicts]
check = [\"just\", \"verify\"]

[board]
guide = \"docs/TASKS.md\"
";

const WORKSPACE: &str = "\
[workspace]
members = [\"lamp\"]

[workspace.lints.clippy]
pedantic = \"warn\"
";

const PACKAGE: &str = "\
[package]
name = \"lamp\"
version = \"0.1.0\"

[lints]
workspace = true
";

#[test]
fn a_branch_changes_the_entries_paths_its_tables_and_its_rust_tests_and_lints() {
    let repo = Repo::new("touched");
    repo.commit(&[
        ("src/lib.rs", LIB),
        (
            "src/jump.rs",
            "pub fn jump() {}\n#[cfg(test)]\nmod tests;\n",
        ),
        ("src/jump/tests.rs", "#[test]\nfn jumps() {}\n"),
        ("src/fixture.rs", "pub fn lamp() {}\nmod deeper;\n"),
        ("src/fixture/deeper.rs", "pub fn deeper() {}\n"),
        ("support/helper.rs", "pub fn help() {}\n"),
        ("tests/it.rs", "#[test]\nfn it() {}\n"),
        ("symphony.toml", PROJECT),
        ("Cargo.toml", WORKSPACE),
        ("lamp/Cargo.toml", PACKAGE),
        ("tools/size-budget.toml", "[symphony]\nbytes = 100\n"),
        ("README.md", "Lamps\n"),
    ]);
    let none: Vec<String> = Vec::new();
    // What the product does, read and written: nothing of the checks.
    assert_eq!(
        repo.touched(
            "product",
            &[
                ("src/lib.rs", &LIB.replace("    3\n", "    4\n")),
                (
                    "src/jump.rs",
                    "pub fn jump() { }\n#[cfg(test)]\nmod tests;\n"
                ),
                ("README.md", "Lamps, lit\n"),
                (
                    "symphony.toml",
                    &PROJECT.replace("docs/TASKS.md", "docs/BOARD.md")
                ),
                (
                    "Cargo.toml",
                    &WORKSPACE.replace("[\"lamp\"]", "[\"lamp\", \"wick\"]")
                ),
                ("lamp/Cargo.toml", &PACKAGE.replace("0.1.0", "0.2.0")),
            ]
        ),
        none
    );
    // A test, the budget, the lints, a check table and the recipes, each by its path.
    assert_eq!(
        repo.touched(
            "paths",
            &[
                ("tests/it.rs", ""),
                ("src/jump/tests.rs", "#[test]\n#[ignore]\nfn jumps() {}\n"),
                ("tools/size-budget.toml", "[symphony]\nbytes = 200\n"),
                ("symphony.toml", &PROJECT.replace("verify", "lint-rs")),
                ("justfile", "verify:\n    true\n"),
                ("Cargo.toml", &WORKSPACE.replace("warn", "allow")),
                (
                    "lamp/Cargo.toml",
                    "[package]\nname = \"lamp\"\nversion = \"0.1.0\"\n"
                ),
            ]
        ),
        [
            "Cargo.toml [workspace.lints]",
            "justfile",
            "lamp/Cargo.toml [lints]",
            "src/jump/tests.rs",
            "symphony.toml [verdicts]",
            "tests/it.rs",
            "tools/size-budget.toml",
        ]
    );
    // Test-only modules by their declarations: under `#[cfg(test)]`, through `#[path]`, and one a
    // test-only module declares.
    assert_eq!(
        repo.touched(
            "modules",
            &[
                ("src/fixture.rs", "pub fn lamp() { }\nmod deeper;\n"),
                ("src/fixture/deeper.rs", "pub fn deeper() { }\n"),
                ("support/helper.rs", "pub fn help() { }\n"),
            ]
        ),
        [
            "src/fixture.rs (a test module)",
            "src/fixture/deeper.rs (a test module)",
            "support/helper.rs (a test module)",
        ]
    );
    // An inline test changed, one removed, one added, and a lint allowed.
    let tested = LIB.replace("height(), 3", "height(), 3, \"three\"");
    assert_eq!(
        repo.touched("inline", &[("src/lib.rs", &tested)]),
        ["src/lib.rs (#[cfg(test)] code at line 16)"]
    );
    let untested = LIB.replace(
        "#[cfg(test)]\nmod inline {\n    #[test]\n    fn three() {\n        assert_eq!(super::height(), 3);\n    }\n}\n",
        "",
    );
    assert_eq!(
        repo.touched("untested", &[("src/lib.rs", &untested)]),
        ["src/lib.rs (#[cfg(test)] code at line 11)"]
    );
    assert_eq!(
        repo.touched(
            "added",
            &[(
                "src/jump.rs",
                "pub fn jump() {}\n#[cfg(test)]\nmod tests;\n#[cfg(test)]\nfn more() {}\n"
            )]
        ),
        ["src/jump.rs (#[cfg(test)] code at line 4)"]
    );
    assert_eq!(
        repo.touched(
            "allowed",
            &[(
                "src/jump.rs",
                "#[expect(dead_code, reason = \"later\")]\npub fn jump() {}\n#[cfg(test)]\nmod tests;\n"
            )]
        ),
        ["src/jump.rs (a lint allowed at line 1)"]
    );
    // A change main made since the branch left it is none of the branch's.
    repo.git(&["checkout", "-qb", "later", "main"]);
    let tip = repo.commit(&[("README.md", "Lamps, later\n")]);
    repo.git(&["checkout", "-q", "main"]);
    repo.commit(&[("tests/it.rs", "#[test]\nfn it() { }\n")]);
    assert_eq!(touched(&repo.0, "main", &tip, &defaults()).unwrap(), none);
    // A project's own entries, a table of a TOML file among them.
    let own = parse(&["/README.md".into(), "/symphony.toml [board]".into()]).unwrap();
    assert_eq!(
        touched(&repo.0, "main", &tip, &own).unwrap(),
        ["README.md".to_string()]
    );
    // What git cannot read is said.
    assert!(
        touched(&repo.0, "main", "no-such-commit", &own)
            .unwrap_err()
            .starts_with("git merge-base main no-such-commit: ")
    );
}
