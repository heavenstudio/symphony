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
    assert_eq!(named("crates/tasks/src/store/tests.rs"), ["tests.rs"]);
    assert_eq!(named("crates/symphony/src/builds_tests.rs"), ["*_tests.rs"]);
    // What tests compare against, this repository's wire fixtures among them.
    assert_eq!(
        named("crates/symphony-api/fixtures/board.json"),
        ["fixtures/"]
    );
    assert_eq!(named("crates/x/testdata/in.txt"), ["testdata/"]);
    assert_eq!(
        named("crates/x/src/snapshots/a.snap"),
        ["snapshots/", "*.snap"]
    );
    assert_eq!(named("justfile"), ["/justfile"]);
    assert_eq!(named("symphony.just"), ["/symphony.just"]);
    // Verify's steps among them: a fix that edits what a step runs changes a check, and so does one
    // that widens the names the docs audit allows after `just`.
    for tool in [
        "tools/verify.sh",
        "tools/size-budget.toml",
        "tools/named-recipes.txt",
        "tools/cargo.sh",
        "tools/seed-clone.rs",
    ] {
        assert_eq!(named(tool), ["/tools/"], "{tool}");
    }
    assert_eq!(named("clippy.toml"), ["clippy.toml"]);
    assert_eq!(named("crates/tasks/.clippy.toml"), [".clippy.toml"]);
    assert_eq!(named("rustfmt.toml"), ["rustfmt.toml"]);
    assert_eq!(named("crates/x/.rustfmt.toml"), [".rustfmt.toml"]);
    assert_eq!(named(".cargo/config.toml"), ["/.cargo/"]);
    let package = [
        "Cargo.toml [lib.test]",
        "Cargo.toml [lib.doctest]",
        "Cargo.toml [lib.harness]",
        "Cargo.toml [test]",
        "Cargo.toml [bench]",
        "Cargo.toml [bin]",
        "Cargo.toml [example]",
        "Cargo.toml [package.autotests]",
        "Cargo.toml [package.autobenches]",
        "Cargo.toml [features]",
        "Cargo.toml [dev-dependencies]",
    ];
    let mut root = package.to_vec();
    root.extend(["/Cargo.toml [workspace.lints]", "Cargo.toml [lints]"]);
    assert_eq!(named("Cargo.toml"), root);
    let mut member = package.to_vec();
    member.push("Cargo.toml [lints]");
    assert_eq!(named("crates/tasks/Cargo.toml"), member);
    assert_eq!(
        named("symphony.toml"),
        [
            "/symphony.toml [verdicts]",
            "/symphony.toml [land]",
            "/symphony.toml [releases]"
        ]
    );
    // The Symphony's own checkers and the gate itself.
    for checker in [
        "crates/symphony-build/src/commands/size_budget.rs",
        "crates/symphony-build/src/commands/crates_check.rs",
        "crates/symphony-build/src/release/crates.rs",
        "crates/symphony/src/docs_audit.rs",
        "crates/symphony-build/src/release.rs",
        "crates/symphony-build/src/release/checks.rs",
        "crates/symphony-build/src/release/steps.rs",
        "crates/symphony/src/cli/commands/size_budget.rs",
        "crates/symphony/src/cli/commands/crates_check.rs",
        "crates/symphony/src/release/crates.rs",
        "crates/symphony/src/release.rs",
        "crates/symphony/src/release/checks.rs",
        "crates/symphony/src/release/steps.rs",
        "crates/symphony/src/nightly.rs",
        "crates/symphony/src/nightly/checks.rs",
        "crates/symphony/src/nightly/steps.rs",
        "crates/tasks/src/land.rs",
        "crates/tasks/src/stamps.rs",
        "crates/tasks/src/red.rs",
        "crates/tasks/src/store/work/red.rs",
    ] {
        assert_eq!(named(checker), [format!("/{checker}")], "{checker}");
    }
    assert_eq!(
        named("crates/tasks/src/land/check_paths.rs"),
        ["/crates/tasks/src/land/"]
    );
    for reader in ["read.rs", "read/tests.rs", "tests.rs"] {
        let matches = named(&format!("crates/tasks/src/stamps/{reader}"));
        assert!(
            matches.contains(&"/crates/tasks/src/stamps/"),
            "{reader}: {matches:?}"
        );
    }
    // The docs audit's modules: what pictures it judges, and the reader of the recipes a text may
    // name, whose lists are under `tools/`.
    for module in ["pictures.rs", "recipes.rs"] {
        assert_eq!(
            named(&format!("crates/symphony/src/docs_audit/{module}")),
            ["/crates/symphony/src/docs_audit/"],
            "{module}"
        );
    }
    assert_eq!(
        named("crates/tasks/src/land/tests.rs"),
        ["tests.rs", "/crates/tasks/src/land/"]
    );
    for product in [
        "crates/tasks/src/sweep.rs",
        "crates/symphony-build/src/release/board.rs",
        "crates/symphony/src/release/board.rs",
        "crates/symphony/src/nightly/board.rs",
        "crates/tasks/tools/cargo.sh",
        "docs/TASKS.md",
        "Cargo.lock",
        "crates/symphony/symphony.toml",
        "crates/x/src/fixture.rs",
    ] {
        assert_eq!(named(product), Vec::<&str>::new(), "{product}");
    }
}

#[test]
fn an_item_a_test_marks_runs_from_its_first_attribute_to_what_ends_it() {
    let text = r##"use std::fs;
#[cfg(test)]
mod tests;
pub fn f() -> u8 {
    #[cfg(test)]
    let x = 1;
    2
}
#[cfg(all(test, unix))]
#[expect(
    clippy::unwrap_used,
    reason = "a test }"
)]
mod unix {
    fn a() {
    }
}
pub struct S {
    #[cfg(test)]
    seen: u8,
}
#[cfg(test)] use x::{y, z};
#[cfg(not(test))]
fn product() {}
#[cfg(any(test, feature = "testing"))]
pub fn helper<A, B>(a: A, b: B)
where
    A: Into<B>,
{
}
#[ignore] // flaky
#[test]
fn top_level() {
    let s = r#"
}
"#;
    let c = '}';
    let e = '\'';
    let l: &'static str = "}";
    /* } */
}
#[tokio::test]
async fn awaited() {}
const DOC: &str = "#[test] fn not_one() {";
#[cfg(feature = "test")]
fn featured() {}
#[derive(Debug)]
enum E {
    A,
    #[cfg(test)]
    B
}
"##;
    assert_eq!(
        regions(text),
        Some(vec![
            (2, 3),
            (5, 6),
            (9, 17),
            (19, 20),
            (22, 22),
            (25, 30),
            (31, 41),
            (42, 43),
            (50, 52),
        ])
    );
    assert_eq!(regions(""), Some(vec![]));
    // An escaped quote leaves its string open, a line ending escaped is a line still, and an
    // escape of several characters is one character.
    let escaped = r#"#[test]
fn t() {
    let s = "\" }\
    ";
    let u = '\u{7d}';
}
"#;
    assert_eq!(regions(escaped), Some(vec![(1, 6)]), "escapes");
    // Where an item a test marks ends cannot be told: never closed, or code that does not lex.
    for (text, case) in [
        ("const C: char = '\\u{7d\n';\n", "a character never ended"),
        ("#[cfg(test)]\nmod open {\n    fn a() {}\n", "never closed"),
        (
            "#[cfg(test)]\nmod raw {\n    const S: &str = r#\"}\n}\n",
            "a raw string never ended",
        ),
        (
            "fn f() {}\n/* a comment never ended\n",
            "a comment never ended",
        ),
        ("const S: &str = \"never ended\n", "a string never ended"),
    ] {
        assert_eq!(regions(text), None, "{case}");
    }
}

#[test]
fn a_test_attribute_is_a_test_a_bench_or_a_cfg_naming_test() {
    for (attribute, marks) in [
        ("test", true),
        ("bench", true),
        ("tokio::test(flavor = \"\")", true),
        ("rstest", true),
        ("test_case(1)", true),
        ("cfg(test)", true),
        ("cfg(all(test, unix))", true),
        ("cfg(any(unix, test))", true),
        ("cfg( test )", true),
        ("cfg(not(test))", false),
        ("cfg(all(not(test), unix))", false),
        ("cfg(feature = \"\")", false),
        ("cfg(testing)", false),
        ("cfg_attr(test, allow(dead_code))", false),
        ("derive(Debug)", false),
        ("ignore", false),
    ] {
        assert_eq!(testing(attribute), marks, "{attribute}");
    }
    assert!(test_only_file("//! Fixtures.\n#![cfg(test)]\nfn a() {}\n"));
    assert!(test_only_file("#![cfg(any(test, feature = \"x\"))]\n"));
    assert!(!test_only_file("#![cfg(not(test))]\n"));
    assert!(!test_only_file(
        "// #![cfg(test)]\nconst S: &str = \"#![cfg(test)]\";\n"
    ));
    assert!(
        test_only_file("#![cfg(test)]\nconst S: &str = \"never ended\n"),
        "a file that does not lex and says it, counted"
    );
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
                removed: vec!["    1".into()],
            },
            Hunk {
                old: (10, 0),
                new: (11, 2),
                added: vec![
                    (11, "#[expect(clippy::x, reason = \"y\")]".into()),
                    (12, "++ a line that starts with two plusses".into()),
                ],
                removed: vec![],
            },
            Hunk {
                old: (20, 2),
                new: (22, 0),
                added: vec![],
                removed: vec!["gone".into(), "too".into()],
            },
        ]
    );
    for line in [
        "    #[expect(clippy::x, reason = \"y\")]",
        "#![allow(dead_code)]",
        "#![deny(warnings)]",
        "#[warn(clippy::pedantic)]",
        "#![forbid(unsafe_code)]",
        "#[cfg_attr(test, allow(clippy::unwrap_used))]",
        "    allow(dead_code)",
        "#[allow (dead_code)]",
    ] {
        assert!(sets_level(line), "{line}");
    }
    for line in [
        "// #[expect(x)]",
        "    let x = y.expect(\"z\");",
        "#[derive(Debug)]",
        "#[cfg(disallow(x))]",
        "fn deny_list() {}",
    ] {
        assert!(!sets_level(line), "{line}");
    }
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
            "symphony-check-paths-{name}-{}-{}",
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

/// A package with an example its tests run, a default feature and a test's dependency.
const UI: &str = "\
[package]
name = \"ui\"
version = \"0.1.0\"

[features]
default = [\"overlays\"]
overlays = []

[dev-dependencies]
tempfile = \"3\"

[[example]]
name = \"wasm_size\"
test = true
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
        ("ui/Cargo.toml", UI),
        ("tools/size-budget.toml", "[symphony]\nbytes = 100\n"),
        ("README.md", "Lamps\n"),
        ("src/lints.rs", "#![deny(missing_docs)]\npub fn l() {}\n"),
        ("src/top.rs", TOP),
        ("src/any.rs", ANY),
        ("src/raw.rs", RAW),
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
    // What tests compare against, and the switches that turn a package's tests off.
    assert_eq!(
        repo.touched(
            "inputs",
            &[
                ("crates/api/fixtures/board.json", "{}\n"),
                ("src/snapshots/jump.snap", "jumped\n"),
                ("rustfmt.toml", "max_width = 200\n"),
                ("symphony.just", "land:\n    true\n"),
            ]
        ),
        [
            "crates/api/fixtures/board.json",
            "rustfmt.toml",
            "src/snapshots/jump.snap",
            "symphony.just",
        ]
    );
    let switched = PACKAGE.replace(
        "version = \"0.1.0\"\n",
        "version = \"0.1.0\"\nautotests = false\n",
    ) + "\n[lib]\ntest = false\n\n[[test]]\nname = \"it\"\nharness = false\n\n[[bench]]\nname = \"b\"\nrequired-features = [\"slow\"]\n\n[[bin]]\nname = \"lamp\"\ntest = false\n";
    assert_eq!(
        repo.touched("switches", &[("lamp/Cargo.toml", &switched)]),
        [
            "lamp/Cargo.toml [lib.test]",
            "lamp/Cargo.toml [test]",
            "lamp/Cargo.toml [bench]",
            "lamp/Cargo.toml [bin]",
            "lamp/Cargo.toml [package.autotests]",
        ]
    );
    // An example's test turned off, a default feature dropped (what it gates leaves the test run)
    // and a test's dependency changed, each by itself.
    for (name, from, to, table) in [
        ("example", "test = true", "test = false", "example"),
        (
            "features",
            "default = [\"overlays\"]",
            "default = []",
            "features",
        ),
        (
            "dev",
            "tempfile = \"3\"",
            "tempfile = \"2\"",
            "dev-dependencies",
        ),
    ] {
        assert_eq!(
            repo.touched(name, &[("ui/Cargo.toml", &UI.replace(from, to))]),
            [format!("ui/Cargo.toml [{table}]")],
            "{name}"
        );
    }
    // A file that is no TOML counts in each table it might hold; a new one in each it holds.
    assert_eq!(
        repo.touched(
            "untoml",
            &[
                ("symphony.toml", "[verdicts\n"),
                (
                    "wick/Cargo.toml",
                    "[package]\nname = \"wick\"\n\n[lib]\ntest = false\n"
                ),
            ]
        ),
        [
            "symphony.toml [verdicts]",
            "symphony.toml [land]",
            "symphony.toml [releases]",
            "wick/Cargo.toml [lib.test]",
        ]
    );
    // An inline test changed, one removed, one added, and a lint allowed.
    let tested = LIB.replace("height(), 3", "height(), 3, \"three\"");
    assert_eq!(
        repo.touched("inline", &[("src/lib.rs", &tested)]),
        ["src/lib.rs (test code at line 16)"]
    );
    let untested = LIB.replace(
        "#[cfg(test)]\nmod inline {\n    #[test]\n    fn three() {\n        assert_eq!(super::height(), 3);\n    }\n}\n",
        "",
    );
    assert_eq!(
        repo.touched("untested", &[("src/lib.rs", &untested)]),
        ["src/lib.rs (test code at line 11)"]
    );
    assert_eq!(
        repo.touched(
            "added",
            &[(
                "src/jump.rs",
                "pub fn jump() {}\n#[cfg(test)]\nmod tests;\n#[cfg(test)]\nfn more() {}\n"
            )]
        ),
        ["src/jump.rs (test code at line 4)"]
    );
    assert_eq!(
        repo.touched(
            "allowed",
            &[(
                "src/jump.rs",
                "#[expect(dead_code, reason = \"later\")]\npub fn jump() {}\n#[cfg(test)]\nmod tests;\n"
            )]
        ),
        ["src/jump.rs (a lint's level set at line 1)"]
    );
    // A lint's level removed, and an `#[ignore]` put above a test.
    assert_eq!(
        repo.touched(
            "unlinted",
            &[
                ("src/lints.rs", "pub fn l() {}\n"),
                ("src/top.rs", &TOP.replace("#[test]", "#[ignore]\n#[test]")),
            ]
        ),
        [
            "src/lints.rs (a lint's level set at line 1)",
            "src/top.rs (test code at line 2)",
        ]
    );
    // A test outside a `#[cfg(test)]` module, an item under `cfg(any(test, …))`, and a test past
    // a raw string holding a `}` where the module's would be.
    assert_eq!(
        repo.touched(
            "marks",
            &[
                ("src/any.rs", &ANY.replace("    1\n", "    2\n")),
                ("src/raw.rs", &RAW.replace("!s.is_empty()", "s.len() > 1")),
                ("src/top.rs", &TOP.replace("1, 1", "1, 2")),
            ]
        ),
        [
            "src/any.rs (test code at line 3)",
            "src/raw.rs (test code at line 9)",
            "src/top.rs (test code at line 4)",
        ]
    );
    // Where a file's tests end cannot be told: the file counts whole.
    assert_eq!(
        repo.touched(
            "unread",
            &[(
                "src/broken.rs",
                "#[cfg(test)]\nmod t {\n    const S: &str = r#\"\n}\n"
            )]
        ),
        ["src/broken.rs (where its tests end cannot be read)"]
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
    // What git cannot read is said, and a path it fails on is never one that is not there.
    assert!(
        touched(&repo.0, "main", "no-such-commit", &own)
            .unwrap_err()
            .starts_with("git merge-base --all main no-such-commit: ")
    );
    let main = Tree::new(&repo.0, "main");
    assert_eq!(main.read("README.md"), Ok(Some("Lamps\n".into())));
    assert_eq!(
        main.read("README.md"),
        Ok(Some("Lamps\n".into())),
        "read once, kept"
    );
    assert_eq!(main.read("no/such/file.rs"), Ok(None));
    assert_eq!(main.read("src/*.rs"), Ok(None), "a path, never a pattern");
    assert_eq!(main.read("src"), Ok(None), "a folder is no file");
    assert!(
        Tree::new(&repo.0, "no-such-commit")
            .read("README.md")
            .unwrap_err()
            .starts_with("git ls-tree -r --full-tree --name-only -z no-such-commit: ")
    );
    // Two merge bases make no one diff of the branch: refused.
    repo.git(&["checkout", "-qb", "cross-a", "main"]);
    let a = repo.commit(&[("a.txt", "a\n")]);
    repo.git(&["checkout", "-qb", "cross-b", "main"]);
    let b = repo.commit(&[("b.txt", "b\n")]);
    repo.git(&["merge", "-q", "--no-edit", &a]);
    repo.git(&["checkout", "-q", "cross-a"]);
    repo.git(&["merge", "-q", "--no-edit", &b]);
    repo.git(&["checkout", "-q", "main"]);
    assert_eq!(
        touched(&repo.0, "cross-a", "cross-b", &defaults()).unwrap_err(),
        "cross-a and cross-b have 2 merge bases, so no one diff says what the branch changes"
    );
}

/// A test outside any `#[cfg(test)]` module.
const TOP: &str = "\
pub fn top() {}
#[test]
fn top_test() {
    assert_eq!(1, 1);
}
";

/// An item under `cfg(any(test, …))`.
const ANY: &str = "\
#[cfg(any(test, feature = \"testing\"))]
pub fn helper() -> u8 {
    1
}
";

/// A test module whose raw string holds a `}` at the start of a line, where the module's own is.
const RAW: &str = "\
pub fn r() {}
#[cfg(test)]
mod tests {
    #[test]
    fn raw() {
        let s = r#\"
}
\"#;
        assert!(!s.is_empty());
    }
}
";

#[test]
fn a_diff_is_split_by_the_file_each_part_is_of() {
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
index 1111111..2222222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
-a
+b
diff --git a/src/spaced b/x.rs b/src/spaced b/x.rs
@@ -2,0 +3 @@
+c
diff --git \"a/src/qu\\\"ote.rs\" \"b/src/qu\\\"ote.rs\"
@@ -1 +1 @@
-d
+e
diff --git a/src/link.rs b/src/link.rs
deleted file mode 100644
@@ -1 +0,0 @@
-f
diff --git a/src/one.rs b/src/other.rs
@@ -1 +1 @@
-g
+h
diff --git a/src/link.rs b/src/link.rs
new file mode 120000
@@ -0,0 +1 @@
+target.rs
";
    let parts = split(diff);
    let mut paths: Vec<&str> = parts.keys().map(String::as_str).collect();
    crate::order::sort_by(&mut paths, &Ord::cmp);
    assert_eq!(
        paths,
        ["src/a.rs", "src/link.rs", "src/spaced b/x.rs"],
        "a quoted header, or one naming two paths, gives no part"
    );
    assert_eq!(
        parts["src/a.rs"],
        "diff --git a/src/a.rs b/src/a.rs\nindex 1111111..2222222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-a\n+b\n"
    );
    assert_eq!(
        parts["src/spaced b/x.rs"],
        "diff --git a/src/spaced b/x.rs b/src/spaced b/x.rs\n@@ -2,0 +3 @@\n+c\n"
    );
    // A file made a link: its removal and its addition, one text.
    assert_eq!(
        hunks(&parts["src/link.rs"]),
        [
            Hunk {
                old: (1, 1),
                new: (0, 0),
                added: Vec::new(),
                removed: vec!["f".into()],
            },
            Hunk {
                old: (0, 0),
                new: (1, 1),
                added: vec![(1, "target.rs".into())],
                removed: Vec::new(),
            },
        ]
    );
    assert!(split("").is_empty());
    assert!(
        split("diff --git a/x\n").is_empty(),
        "a header too short to name one"
    );
}

/// A package of `count` modules, `src/mI.rs` each with one of its own (`src/mI/inner.rs`), every
/// odd one declared under `#[cfg(test)]`, and a manifest with features.
fn modules(count: usize) -> Vec<(String, String)> {
    let lib: String = (0..count)
        .map(|i| match i % 2 {
            0 => format!("pub mod m{i};\n"),
            _ => format!("#[cfg(test)]\nmod m{i};\n"),
        })
        .collect();
    let mut files = vec![
        ("src/lib.rs".into(), lib),
        (
            "Cargo.toml".into(),
            "[package]\nname = \"lamp\"\n\n[features]\nslow = []\n".into(),
        ),
    ];
    for i in 0..count {
        files.push((
            format!("src/m{i}.rs"),
            "pub mod inner;\npub fn f() {}\n".into(),
        ));
        files.push((format!("src/m{i}/inner.rs"), "pub fn g() {}\n".into()));
    }
    files
}

/// What a branch changes of [`modules`]: a feature added, and the first `n` modules' files, a test
/// added to each `src/mI.rs`.
fn changes(n: usize) -> Vec<(String, String)> {
    let mut files = vec![(
        "Cargo.toml".into(),
        "[package]\nname = \"lamp\"\n\n[features]\nslow = []\nfast = []\n".into(),
    )];
    for i in 0..n {
        files.push((
            format!("src/m{i}.rs"),
            "pub mod inner;\npub fn f() { }\n#[test]\nfn t() {}\n".into(),
        ));
        files.push((format!("src/m{i}/inner.rs"), "pub fn g() { }\n".into()));
    }
    files
}

fn pairs(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect()
}

#[test]
fn the_runs_of_git_a_judgement_makes_do_not_grow_with_the_diff() {
    let repo = Repo::new("runs");
    repo.commit(&pairs(&modules(60)));
    let judged = |name: &str, n: usize| -> (usize, Vec<String>) {
        RUNS.set(0);
        let found = repo.touched(name, &pairs(&changes(n)));
        (RUNS.get(), found)
    };
    let (few, small) = judged("small", 2);
    assert_eq!(
        small,
        [
            "Cargo.toml [features]",
            "src/m0.rs (test code at line 3)",
            "src/m1.rs (a test module)",
            "src/m1/inner.rs (a test module)",
        ]
    );
    let (many, large) = judged("large", 60);
    // The manifest, each even module's test and each odd module's two files.
    assert_eq!(large.len(), 1 + 30 + 2 * 30, "{large:#?}");
    for line in [
        "src/m58.rs (test code at line 3)",
        "src/m59.rs (a test module)",
        "src/m59/inner.rs (a test module)",
    ] {
        assert!(large.contains(&line.to_string()), "{line}: {large:#?}");
    }
    assert!(
        many <= few,
        "a diff of 121 files runs git {many} times, one of 5 {few}"
    );
}

#[test]
fn a_file_whose_name_git_quotes_is_read_by_its_own_diff() {
    let repo = Repo::new("quoted");
    repo.commit(&[
        ("src/lib.rs", "pub mod plain;\n"),
        ("src/plain.rs", TOP),
        ("src/q\"uoted.rs", TOP),
    ]);
    assert_eq!(
        repo.touched(
            "quoted",
            &[
                ("src/plain.rs", &TOP.replace("1, 1", "1, 2")),
                ("src/q\"uoted.rs", &TOP.replace("1, 1", "1, 2")),
            ]
        ),
        [
            "src/plain.rs (test code at line 4)",
            "src/q\"uoted.rs (test code at line 4)",
        ]
    );
}

#[test]
fn a_declaration_no_search_for_its_name_finds_is_followed_as_before() {
    let repo = Repo::new("spaced");
    repo.commit(&[
        (
            "src/lib.rs",
            "#[cfg(test)]\nmod  spaced;\n#[cfg(test)]\nmod r#raw;\n#[cfg(test)]\nmod named;\n",
        ),
        ("src/spaced.rs", "pub fn s() {}\n"),
        ("src/raw.rs", "pub fn r() {}\n"),
        ("src/named.rs", "pub fn n() {}\n"),
    ]);
    // Rust reads all three as test-only; a search by `mod NAME` or `NAME.rs` found only the last,
    // and the judgement keeps what it found.
    assert_eq!(
        repo.touched(
            "spaced",
            &[
                ("src/spaced.rs", "pub fn s() { }\n"),
                ("src/raw.rs", "pub fn r() { }\n"),
                ("src/named.rs", "pub fn n() { }\n"),
            ]
        ),
        ["src/named.rs (a test module)"]
    );
}

#[test]
fn a_listed_file_git_cannot_give_is_refused_never_read_as_none() {
    let repo = Repo::new("ungiven");
    repo.commit(&[("README.md", "Lamps\n")]);
    // A submodule's commit, and a file whose object the repository lacks.
    let index = |info: &str| repo.git(&["update-index", "--add", "--cacheinfo", info]);
    index("160000,1111111111111111111111111111111111111111,src/sub.rs");
    index("100644,2222222222222222222222222222222222222222,src/gone.rs");
    let tree = repo.git(&["write-tree", "--missing-ok"]);
    let commit = repo.git(&["commit-tree", &tree, "-m", "broken"]);
    let broken = Tree::new(&repo.0, &commit);
    assert!(
        (broken.read("src/sub.rs").unwrap_err()).starts_with("git cat-file could not read in "),
    );
    assert_eq!(
        broken.read("src/gone.rs"),
        Err(format!(
            "git cat-file --batch: {commit}:src/gone.rs gives no file"
        ))
    );
    assert_eq!(broken.read("README.md"), Ok(Some("Lamps\n".into())));
}

#[test]
fn the_walk_up_a_module_stops_where_it_did() {
    let repo = Repo::new("walk");
    // A chain of modules ten deep under one `#[cfg(test)]`: followed eight files up, no further.
    let chain: Vec<String> = (0..10)
        .map(|depth| {
            let folders: String = (0..depth).map(|d| format!("d{d}/")).collect();
            format!("src/{folders}d{depth}.rs")
        })
        .collect();
    let mut files: Vec<(String, String)> = (chain.iter().enumerate())
        .map(|(depth, path)| (path.clone(), format!("pub mod d{};\n", depth + 1)))
        .collect();
    files.extend(
        [
            (
                "src/lib.rs",
                "#[cfg(test)]\nmod d0;\n#[cfg(test)]\nmod gated;\npub mod only;\npub mod selfish;\n\
                 pub mod support_tests;\n#[cfg(not(test))]\nmod twice;\n#[cfg(test)]\nmod twice;\n",
            ),
            ("src/gated/mod.rs", "pub fn g() {}\n"),
            ("src/only.rs", "#![cfg(test)]\npub fn o() {}\n"),
            (
                "src/selfish.rs",
                "#[path = \"selfish.rs\"]\nmod selfish;\npub fn s() {}\n",
            ),
            ("src/support_tests.rs", "pub mod helper;\n"),
            ("src/support_tests/helper.rs", "pub fn h() {}\n"),
            ("src/twice.rs", "pub fn t() {}\n"),
            ("mod.rs", "pub fn m() {}\n"),
        ]
        .map(|(path, text)| (path.to_string(), text.to_string())),
    );
    repo.commit(&pairs(&files));
    let changed: Vec<(String, String)> = [
        (chain[8].as_str(), "pub mod d9;\npub fn f() {}\n"),
        (chain[9].as_str(), "pub mod d10;\npub fn f() {}\n"),
        ("src/gated/mod.rs", "pub fn g() { }\n"),
        ("src/only.rs", "#![cfg(test)]\npub fn o() { }\n"),
        (
            "src/selfish.rs",
            "#[path = \"selfish.rs\"]\nmod selfish;\npub fn s() { }\n",
        ),
        ("src/support_tests/helper.rs", "pub fn h() { }\n"),
        ("src/twice.rs", "pub fn t() { }\n"),
        ("mod.rs", "pub fn m() { }\n"),
    ]
    .map(|(path, text)| (path.to_string(), text.to_string()))
    .to_vec();
    assert_eq!(
        repo.touched("walk", &pairs(&changed)),
        [
            // Its gate eight files up; the next one's is nine, past the walk.
            format!("{} (a test module)", chain[8]),
            // A folder's module, by the folder's name.
            "src/gated/mod.rs (a test module)".into(),
            // Its own file says so.
            "src/only.rs (a test module)".into(),
            // Declared by a file an entry names, though not under a test's attribute.
            "src/support_tests/helper.rs (a test module)".into(),
            // Declared twice by one file, once for tests.
            "src/twice.rs (a test module)".into(),
        ]
    );
}
