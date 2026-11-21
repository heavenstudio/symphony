//! `tools/check-inputs.sh` on a workspace of the test's own: the digest of what each step of
//! `just verify` and each lane of its tests reads, by which a release candidate's rerun carries a
//! green (docs/workflow/RELEASES.md, "The rerun"). The scripts are this repository's, as they are; the
//! packages are stand-ins of the names the lanes read by, each an empty library with the test
//! targets its lanes select. A change inside the packages a lane builds with changes its digest,
//! one outside them leaves it, a change to the check itself or the toolchain changes every one,
//! and what cannot be derived fails the script, which then tells nothing.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

#[path = "../../symphony/tests/common/tree.rs"]
mod tree;
use tree::Tree;

/// The stand-in packages: each its folder, its name, the packages it depends on by path, and its
/// test targets.
const PACKAGES: [(&str, &str, &[&str], &[&str]); 6] = [
    (
        "crates/tasks",
        "symphony-board",
        &[],
        &["perf", "sweep", "cli", "server", "store", "launch"],
    ),
    (
        "crates/symphony",
        "symphony-app",
        &["crates/tasks"],
        &[
            "after_commit",
            "after_publication",
            "after_worktrees",
            "hooks",
            "picks",
            "project",
        ],
    ),
    (
        "apps/symphony-server",
        "symphony-server",
        &["crates/symphony"],
        &["after_http"],
    ),
    (
        "crates/symphony-build",
        "symphony-build",
        &["crates/symphony"],
        &["release"],
    ),
    (
        "apps/symphony-web-tests",
        "symphony-web-tests",
        &[],
        &["browser", "build"],
    ),
    ("crates/unrelated", "unrelated", &[], &["alone"]),
];

/// `git ARGS` in `dir`, which must succeed.
fn git(dir: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
}

/// The workspace, committed: this repository's scripts and toolchain pin over the stand-in
/// packages, with a document, a client's source and a justfile.
fn workspace() -> Tree {
    let tree = Tree::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in [
        "tools/check-inputs.sh",
        "tools/verify.sh",
        "tools/test-lanes.sh",
        "rust-toolchain.toml",
    ] {
        tree.write(file, &fs::read_to_string(source.join(file)).unwrap());
    }
    let members: Vec<String> = (PACKAGES.iter())
        .map(|(folder, ..)| format!("\"{folder}\""))
        .collect();
    tree.write(
        "Cargo.toml",
        &format!(
            "[workspace]\nresolver = \"2\"\nmembers = [{}]\n",
            members.join(", ")
        ),
    );
    for (folder, name, needs, tests) in PACKAGES {
        let depth = "../".repeat(folder.split('/').count());
        let needs: String = (needs.iter())
            .map(|path| {
                let name = PACKAGES
                    .iter()
                    .find(|package| package.0 == *path)
                    .unwrap()
                    .1;
                format!("{name} = {{ path = \"{depth}{path}\" }}\n")
            })
            .collect();
        tree.write(
            &format!("{folder}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
                 [dev-dependencies]\n{needs}"
            ),
        );
        tree.write(&format!("{folder}/src/lib.rs"), "");
        for test in tests {
            tree.write(&format!("{folder}/tests/{test}.rs"), "");
        }
    }
    tree.write("Cargo.lock", "version = 4\n");
    tree.write("justfile", "verify:\n    @true\n");
    tree.write("docs/README.md", "A document.\n");
    tree.write("apps/symphony-web/src/main.ts", "export {};\n");
    tree.write(
        "crates/symphony-build/src/release/fixtures/tests-nextest-red.log",
        "a log\n",
    );
    git(&tree.root, &["init", "-q", "-b", "main"]);
    commit(&tree, "the workspace");
    tree
}

fn commit(tree: &Tree, message: &str) {
    git(&tree.root, &["add", "-A"]);
    git(&tree.root, &["commit", "-q", "-m", message]);
}

/// The script run in the workspace, with `path` before the search path when given.
fn asked(tree: &Tree, path: Option<&Path>) -> Output {
    let mut command = Command::new("bash");
    command
        .arg("tools/check-inputs.sh")
        .current_dir(&tree.root)
        .env_remove("CARGO")
        .env_remove("GIT_DIR")
        .stdin(Stdio::null());
    if let Some(path) = path {
        let rest = std::env::var("PATH").unwrap();
        command.env("PATH", format!("{}:{rest}", path.display()));
    }
    command.output().unwrap()
}

/// Each name the script tells in the workspace, with its digest.
fn told(tree: &Tree) -> BTreeMap<String, String> {
    let done = asked(tree, None);
    assert!(done.status.success(), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    let told: BTreeMap<String, String> = (said.lines())
        .map(|line| {
            let (name, digest) = line.split_once('\t').unwrap();
            assert!(
                digest.len() == 16 && digest.chars().all(|c| c.is_ascii_hexdigit()),
                "{line:?}"
            );
            (name.to_string(), digest.to_string())
        })
        .collect();
    assert_eq!(
        told.len(),
        said.lines().count(),
        "a name said twice: {said}"
    );
    told
}

/// The names whose digest `after` gives differs from `before`'s, sorted.
fn changed(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<String> {
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    (before.iter())
        .filter(|(name, digest)| after[*name] != **digest)
        .map(|(name, _)| name.clone())
        .collect()
}

/// `file` written with `text` and committed: what the script tells then.
fn after(tree: &Tree, file: &str, text: &str) -> BTreeMap<String, String> {
    tree.write(file, text);
    commit(tree, &format!("change {file}"));
    told(tree)
}

/// The names that read the whole tree, which any change changes.
const TREE: [&str; 6] = [
    "verify/crates-check",
    "verify/docs-audit",
    "verify/lint",
    "verify/tests/doc",
    "verify/tests/hooks",
    "verify/tests/nextest",
];

/// `names` and the whole-tree names, sorted as [`changed`] gives them.
fn with_tree(names: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = names.iter().chain(&TREE).map(|n| n.to_string()).collect();
    all.sort();
    all
}

#[test]
fn every_step_and_lane_is_told_and_the_same_commit_tells_the_same() {
    let tree = workspace();
    let first = told(&tree);
    let names: Vec<&str> = first.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "verify/client-check",
            "verify/crates-check",
            "verify/docs-audit",
            "verify/hosted",
            "verify/lint",
            "verify/nextest-setup",
            "verify/semver-check",
            "verify/size-check",
            "verify/tests/after_commit",
            "verify/tests/after_http",
            "verify/tests/after_publication",
            "verify/tests/after_worktrees",
            "verify/tests/browser",
            "verify/tests/build",
            "verify/tests/cli",
            "verify/tests/doc",
            "verify/tests/fixture",
            "verify/tests/hooks",
            "verify/tests/launch",
            "verify/tests/libraries",
            "verify/tests/nextest",
            "verify/tests/perf",
            "verify/tests/picks",
            "verify/tests/project",
            "verify/tests/release",
            "verify/tests/server",
            "verify/tests/store",
            "verify/tests/sweep",
        ]
    );
    assert_eq!(told(&tree), first);
    // What reads the whole tree has the tree's digest, and nothing narrower has.
    for name in TREE {
        assert_eq!(first[name], first["verify/lint"], "{name}");
    }
    assert_ne!(first["verify/tests/perf"], first["verify/lint"]);
    // A file git does not track is no input: the digest is the commit's.
    tree.write("crates/tasks/untracked.rs", "");
    assert_eq!(told(&tree), first);
    // An empty commit, another commit of the same tree, tells the same.
    git(
        &tree.root,
        &["commit", "-q", "--allow-empty", "-m", "nothing"],
    );
    assert_eq!(told(&tree), first);
}

#[test]
fn a_change_inside_a_lanes_packages_changes_its_digest_and_one_outside_them_leaves_it() {
    let tree = workspace();
    let base = told(&tree);

    // The board's package: every lane that builds with it, by its own targets or by a package
    // that depends on it by path, and nothing that does not.
    let board = after(&tree, "crates/tasks/src/lib.rs", "// changed\n");
    assert_eq!(
        changed(&base, &board),
        with_tree(&[
            "verify/hosted",
            "verify/semver-check",
            "verify/size-check",
            "verify/tests/after_commit",
            "verify/tests/after_http",
            "verify/tests/after_publication",
            "verify/tests/after_worktrees",
            "verify/tests/browser",
            "verify/tests/build",
            "verify/tests/cli",
            "verify/tests/fixture",
            "verify/tests/launch",
            "verify/tests/libraries",
            "verify/tests/perf",
            "verify/tests/picks",
            "verify/tests/project",
            "verify/tests/release",
            "verify/tests/server",
            "verify/tests/store",
            "verify/tests/sweep",
        ])
    );

    // The browser tests alone: their two lanes, and no lane of another package.
    let browser = after(
        &tree,
        "apps/symphony-web-tests/tests/browser.rs",
        "// changed\n",
    );
    assert_eq!(
        changed(&board, &browser),
        with_tree(&["verify/tests/browser", "verify/tests/build"])
    );

    // The client's source, which no package holds: what builds or serves the client.
    let client = after(
        &tree,
        "apps/symphony-web/src/main.ts",
        "export const a = 1;\n",
    );
    assert_eq!(
        changed(&browser, &client),
        with_tree(&[
            "verify/client-check",
            "verify/tests/after_http",
            "verify/tests/browser",
            "verify/tests/build",
            "verify/tests/libraries",
        ])
    );

    // A package no lane names builds with: the steps that read every package, and no lane.
    let unrelated = after(&tree, "crates/unrelated/src/lib.rs", "// changed\n");
    assert_eq!(
        changed(&client, &unrelated),
        with_tree(&["verify/semver-check"])
    );

    // A fixture of the build package that the cli lane's tests read: that lane, declared beside
    // it, with the lanes that build with the package, and no other lane of the board.
    let fixture = after(
        &tree,
        "crates/symphony-build/src/release/fixtures/tests-nextest-red.log",
        "another log\n",
    );
    assert_eq!(
        changed(&unrelated, &fixture),
        with_tree(&[
            "verify/semver-check",
            "verify/tests/cli",
            "verify/tests/fixture",
            "verify/tests/release",
        ])
    );
    let unrelated = fixture;

    // The server's package: what builds with it or runs its binary, never the board's lanes.
    let server = after(&tree, "apps/symphony-server/src/lib.rs", "// changed\n");
    assert_eq!(
        changed(&unrelated, &server),
        with_tree(&[
            "verify/client-check",
            "verify/size-check",
            "verify/tests/after_http",
            "verify/tests/browser",
            "verify/tests/build",
            "verify/tests/libraries",
        ])
    );
}

#[test]
fn a_change_to_the_check_itself_or_outside_the_packages_changes_every_digest() {
    let tree = workspace();
    let mut before = told(&tree);
    let every: Vec<String> = before.keys().cloned().collect();
    let toolchain = tree.read("rust-toolchain.toml");
    let verify = tree.read("tools/verify.sh");
    for (file, text) in [
        ("tools/verify.sh", format!("{verify}# changed\n")),
        (
            "tools/check-inputs.sh",
            format!("{}# changed\n", tree.read("tools/check-inputs.sh")),
        ),
        ("tools/nextest.sh", "# the runner pin\n".to_string()),
        ("justfile", "verify:\n    @false\n".to_string()),
        ("Cargo.lock", "version = 4\n# changed\n".to_string()),
        ("rust-toolchain.toml", format!("{toolchain}# changed\n")),
        (
            "Cargo.toml",
            format!("{}# changed\n", tree.read("Cargo.toml")),
        ),
        ("docs/README.md", "Another document.\n".to_string()),
        ("symphony.toml", "[project]\nid = \"scratch\"\n".to_string()),
    ] {
        let now = after(&tree, file, &text);
        assert_eq!(changed(&before, &now), every, "{file}");
        before = now;
    }
}

#[test]
fn another_toolchain_installed_changes_every_digest() {
    let tree = workspace();
    let before = told(&tree);
    // A compiler that says another version, before the installed one on the search path.
    let stand = Tree::new();
    let rustc = stand.write("rustc", "#!/bin/sh\necho 'rustc 0.0.0 (stand-in)'\n");
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let done = asked(&tree, Some(&stand.root));
    assert!(done.status.success(), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert_eq!(said.lines().count(), before.len());
    for line in said.lines() {
        let (name, digest) = line.split_once('\t').unwrap();
        assert_ne!(before[name], digest, "{name}");
    }
}

#[test]
fn what_cannot_be_derived_fails_the_script_which_tells_nothing() {
    // A lane names a package the workspace does not hold.
    let tree = workspace();
    fs::remove_dir_all(tree.root.join("crates/symphony-build")).unwrap();
    let manifest = tree.read("Cargo.toml");
    tree.write(
        "Cargo.toml",
        &manifest.replace("\"crates/symphony-build\", ", ""),
    );
    commit(&tree, "a package gone");
    let done = asked(&tree, None);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(done.stdout.is_empty(), "it tells nothing: {done:?}");
    let said = String::from_utf8_lossy(&done.stderr);
    assert!(
        said.contains("no package symphony-build in the workspace")
            && said.ends_with("check-inputs: what a step reads does not resolve\n"),
        "{said}"
    );

    // A lane's test target is in no package.
    let tree = workspace();
    fs::remove_file(tree.root.join("crates/tasks/tests/perf.rs")).unwrap();
    commit(&tree, "a target gone");
    let done = asked(&tree, None);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(done.stdout.is_empty(), "{done:?}");
    assert!(
        String::from_utf8_lossy(&done.stderr).contains("no test target perf in the workspace"),
        "{done:?}"
    );

    // The manifest does not read.
    let tree = workspace();
    tree.write("Cargo.toml", "[workspace\n");
    commit(&tree, "a manifest that does not read");
    let done = asked(&tree, None);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(done.stdout.is_empty(), "{done:?}");
    assert!(
        String::from_utf8_lossy(&done.stderr).ends_with("check-inputs: cargo metadata failed\n"),
        "{done:?}"
    );
}
