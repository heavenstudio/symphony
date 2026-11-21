//! The published packages, crates.io's index, the changed-sources check and the release's step,
//! against a workspace and a registry folder of the test's own, a server of the test's own on this
//! machine for curl's reads and a stand-in for the cargo that publishes: nothing here reads
//! crates.io or publishes anything.

use std::io::Read as _;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use serde_json::json;

use super::*;
use crate::test_tree::Tree;

/// A bound no test reaches.
fn later() -> Instant {
    Instant::now() + Duration::from_secs(600)
}

fn bound() -> Bound<'static> {
    Bound::until(later())
}

fn names(packages: &[Package]) -> Vec<&str> {
    packages
        .iter()
        .map(|package| package.name.as_str())
        .collect()
}

/// A workspace of the test's own: `alpha`; `bravo` building with it; `charlie` building with
/// `bravo` and testing with `delta`, which is `publish = false`; each at 1.0.0 with a source of its
/// own.
fn workspace(tree: &Tree) -> PathBuf {
    tree.write(
        "ws/Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"alpha\", \"bravo\", \"charlie\", \"delta\"]\n",
    );
    let member = |name: &str, more: &str, deps: &str| {
        tree.write(
            &format!("ws/{name}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\
                 description = \"{name}\"\nlicense = \"MIT\"\n{more}\n[dependencies]\n{deps}"
            ),
        );
        tree.write(
            &format!("ws/{name}/src/lib.rs"),
            &format!("//! {name}\npub fn {name}() {{}}\n"),
        );
    };
    member("alpha", "", "");
    member(
        "bravo",
        "",
        "alpha = { path = \"../alpha\", version = \"1.0.0\" }\n",
    );
    member(
        "charlie",
        "",
        "bravo = { path = \"../bravo\", version = \"1.0.0\" }\n\n[dev-dependencies]\n\
         delta = { path = \"../delta\" }\n",
    );
    member("delta", "publish = false\n", "");
    tree.path("ws")
}

/// The index file of `name` in the registry folder `registry` listing `versions`.
fn listed(registry: &Path, name: &str, versions: &[&str]) {
    let file = registry.join("index").join(index_path(name));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let lines: String = (versions.iter())
        .map(|version| format!("{}\n", json!({"name": name, "vers": version, "deps": []})))
        .collect();
    fs::write(file, lines).unwrap();
}

/// A stand-in for the cargo that publishes, in `tree`, run as `publish --locked -p NAME …`: it
/// appends each command line, with the build folder it was handed, to `published.log` there; then
/// uploads the packages it is named in that order, listing each in `registry`'s index at 1.0.0,
/// until one that `no_token` names (no token found), `fails` names (an upload that failed) or
/// `there` names (crates.io holds it already), where it stops.
fn stand_in(tree: &Tree, registry: &Path, no_token: &str, fails: &str, there: &str) -> PathBuf {
    let log = tree.path("published.log");
    let script = format!(
        r#"#!/bin/sh
echo "$* target=$CARGO_TARGET_DIR" >> '{log}'
shift 2
for name in "$@"; do
[ "$name" = -p ] && continue
case " {no_token} " in *" $name "*) echo 'error: no token found, please run `cargo login`' >&2; exit 101;; esac
case " {fails} " in *" $name "*) printf 'error: failed to publish %s to registry at https://crates.io\n\nCaused by:\n  the remote server responded with an error (status 503)\n' "$name" >&2; exit 101;; esac
case " {there} " in *" $name "*) echo "error: crate $name@1.0.0 already exists on crates.io index" >&2; exit 101;; esac
dir='{registry}/index/'"$(printf %s "$name" | cut -c1-2)/$(printf %s "$name" | cut -c3-4)"
mkdir -p "$dir" && printf '{{"name":"%s","vers":"1.0.0"}}\n' "$name" >> "$dir/$name"
echo "   Uploaded $name v1.0.0 to registry crates-io"
done
"#,
        log = log.display(),
        registry = registry.display(),
    );
    let path = tree.write("cargo", &script);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The command lines the stand-in ran, one a line.
fn ran(tree: &Tree) -> Vec<String> {
    fs::read_to_string(tree.path("published.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_member_is_published_unless_it_says_publish_false_or_names_another_registry() {
    let metadata = json!({"packages": [
        {"name": "other", "version": "1.0.0", "publish": null},
        {"name": "bravo", "version": "0.1.0", "publish": ["crates-io"]},
        {"name": "alpha", "version": "1.0.0", "publish": null},
        {"name": "charlie", "version": "2.0.0"},
        {"name": "kept", "version": "0.1.0", "publish": []},
        {"name": "elsewhere", "version": "0.1.0", "publish": ["mine"]},
    ]});
    let found = published(&metadata);
    assert_eq!(
        names(&found),
        ["alpha", "bravo", "charlie", "other"],
        "by name"
    );
    assert_eq!(found[1].version, "0.1.0");
    assert!(published(&json!({})).is_empty());
}

#[test]
fn a_workspace_is_read_by_cargo_metadata_and_a_tree_without_one_publishes_nothing() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let found = packages(&root, bound()).unwrap();
    assert_eq!(
        names(&found),
        ["alpha", "bravo", "charlie"],
        "delta is a test's alone"
    );
    assert_eq!(found[1].said(), "bravo 1.0.0");
    assert!(
        packages(&tree.path("ws/alpha/src"), bound())
            .unwrap()
            .is_empty()
    );
    tree.write("broken/Cargo.toml", "[package\n");
    let broken = packages(&tree.path("broken"), bound()).unwrap_err();
    assert!(broken.starts_with("cargo metadata failed in "), "{broken}");
}

#[test]
fn a_sparse_index_names_a_crate_by_its_length_and_first_letters() {
    assert_eq!(index_path("a"), "1/a");
    assert_eq!(index_path("ab"), "2/ab");
    assert_eq!(index_path("abc"), "3/a/abc");
    assert_eq!(index_path("Serde"), "se/rd/serde");
    assert_eq!(index_path("symphony-env"), "sy/mp/symphony-env");
    let file = "{\"name\":\"x\",\"vers\":\"1.0.0\"}\nnot json\n{\"name\":\"x\",\"vers\":\"1.1.0\",\"yanked\":true}\n";
    assert!(lists(file, "1.0.0"));
    assert!(
        lists(file, "1.1.0"),
        "a yanked version is published all the same"
    );
    assert!(!lists(file, "1.0"));
    assert!(!lists("", "1.0.0"));
}

#[test]
fn a_folder_registry_holds_what_its_index_lists_and_fails_closed_on_what_it_cannot_read() {
    let tree = Tree::new();
    let registry = Registry::Folder(tree.path("registry"));
    assert!(
        !registry.has("alpha", "1.0.0", bound()).unwrap(),
        "no file: none"
    );
    listed(&tree.path("registry"), "alpha", &["0.9.0", "1.0.0"]);
    assert!(registry.has("alpha", "1.0.0", bound()).unwrap());
    assert!(!registry.has("alpha", "1.0.1", bound()).unwrap());
    fs::create_dir_all(tree.path("registry/index/br/av/bravo")).unwrap();
    assert!(
        registry.has("bravo", "1.0.0", bound()).is_err(),
        "a folder is no index file"
    );
    assert_eq!(
        registry.said(),
        format!("the registry {}", tree.path("registry").display())
    );
    assert_eq!(Registry::CratesIo.said(), "crates.io");
}

#[test]
fn two_packages_differ_by_each_file_added_gone_or_changed_but_where_and_how_they_were_packaged() {
    let tree = Tree::new();
    for side in ["ours", "theirs"] {
        tree.write(&format!("{side}/src/lib.rs"), "same\n");
        tree.write(&format!("{side}/Cargo.lock"), &format!("{side}\n"));
        tree.write(
            &format!("{side}/.cargo_vcs_info.json"),
            &format!("{side}\n"),
        );
    }
    assert!(
        differ(&tree.path("ours"), &tree.path("theirs"))
            .unwrap()
            .is_empty()
    );
    tree.write("ours/src/lib.rs", "changed\n");
    tree.write("ours/src/new.rs", "new\n");
    tree.write("theirs/README.md", "old\n");
    tree.write("ours/src/Cargo.lock", "a source of its own below the top\n");
    assert_eq!(
        differ(&tree.path("ours"), &tree.path("theirs")).unwrap(),
        [
            "README.md gone",
            "src/Cargo.lock added",
            "src/lib.rs changed",
            "src/new.rs added"
        ]
    );
    assert!(differ(&tree.path("ours"), &tree.path("none")).is_err());
}

#[test]
fn a_changed_package_is_said_with_its_first_files_and_a_count_of_the_rest() {
    let few = Changed {
        package: "alpha 1.0.0".into(),
        files: vec!["src/lib.rs changed".into()],
    };
    assert_eq!(
        changed_line(&few),
        "alpha 1.0.0: on crates.io, and its packaged sources differ from that version's \
         (src/lib.rs changed): a version is published once, so move its version, its line's as \
         the packages' lines say (docs/workflow/RELEASES.md, \"Packages on crates.io\")"
    );
    let many = Changed {
        package: "alpha 1.0.0".into(),
        files: (0..13).map(|n| format!("f{n} added")).collect(),
    };
    assert!(
        changed_line(&many).contains(
            "(f0 added, f1 added, f2 added, f3 added, f4 added, f5 \
                                      added, f6 added, f7 added, f8 added, f9 added and 3 more)"
        ),
        "{}",
        changed_line(&many)
    );
}

/// `cargo package` of the workspace at `root`'s package `name` into `target`: its `.crate`.
fn packaged(root: &Path, name: &str, target: &Path) -> PathBuf {
    let done = output_until(
        command("cargo")
            .args([
                "package",
                "--no-verify",
                "--exclude-lockfile",
                "--allow-dirty",
                "--offline",
                "--quiet",
                "-p",
                name,
            ])
            .env("CARGO_TARGET_DIR", target)
            .current_dir(root),
        bound(),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    target.join("package").join(format!("{name}-1.0.0.crate"))
}

#[test]
fn a_package_whose_version_the_registry_holds_is_red_once_its_sources_move_and_not_before() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    // Nothing published: each is the next release's, and nothing is packaged.
    let none = check(
        &root,
        &Registry::Folder(registry.clone()),
        &tree.path("target"),
        later(),
    )
    .unwrap();
    assert!(none.changed.is_empty());
    assert_eq!(none.lines.len(), 3, "{:?}", none.lines);
    assert!(
        none.lines[0].starts_with("alpha 1.0.0: not on the registry ")
            && none.lines[0].ends_with(
                " yet: nothing to compare, and a release publishes it while [releases] publish is on"
            ),
        "{:?}",
        none.lines
    );
    assert!(!tree.path("target/package").exists(), "nothing packaged");
    // alpha published as it stands: the same.
    let published = packaged(&root, "alpha", &tree.path("published"));
    fs::create_dir_all(registry.join("crates/alpha")).unwrap();
    fs::copy(&published, registry.join("crates/alpha/alpha-1.0.0.crate")).unwrap();
    listed(&registry, "alpha", &["1.0.0"]);
    let folder = Registry::Folder(registry.clone());
    let same = check(&root, &folder, &tree.path("target"), later()).unwrap();
    assert!(same.changed.is_empty(), "{:?}", same.changed);
    assert!(
        same.lines
            .iter()
            .any(|line| line.starts_with("alpha 1.0.0: on the registry ")
                && line.ends_with(", its packaged sources the same")),
        "{:?}",
        same.lines
    );
    assert!(
        !tree.path("target").join(CHECKED).exists(),
        "its scratch is gone"
    );
    // Its source moved while its version did not: red, naming the file.
    tree.write(
        "ws/alpha/src/lib.rs",
        "//! alpha, changed\npub fn alpha() {}\n",
    );
    let moved = check(&root, &folder, &tree.path("target"), later()).unwrap();
    assert_eq!(
        moved.changed,
        [Changed {
            package: "alpha 1.0.0".into(),
            files: vec!["src/lib.rs changed".into()],
        }]
    );
    // Its version moved too: the next release's to publish, no red.
    let manifest = tree.read("ws/alpha/Cargo.toml").replace("1.0.0", "1.0.1");
    tree.write("ws/alpha/Cargo.toml", &manifest);
    let bumped = check(&root, &folder, &tree.path("target"), later()).unwrap();
    assert!(bumped.changed.is_empty(), "{:?}", bumped.changed);
    assert!(bumped.lines[0].starts_with("alpha 1.0.1: not on the registry "));
}

/// `git ARGS` in the scratch repository `root`, as an author of the test's own.
fn scratch_git(root: &Path, args: &[&str]) -> String {
    let named = ["-c", "user.name=t", "-c", "user.email=t@example.com"];
    git_until(root, &[&named[..], args].concat(), bound()).unwrap()
}

/// [`workspace`] as a repository: one commit on `main`, and the branch `work` checked out on it.
fn repository(tree: &Tree) -> PathBuf {
    let root = workspace(tree);
    tree.write("ws/README.md", "the workspace\n");
    tree.write(
        "ws/charlie/NOTES.md",
        "what charlie carries beside its sources\n",
    );
    scratch_git(&root, &["init", "-q", "-b", "main"]);
    scratch_git(&root, &["add", "-A"]);
    scratch_git(&root, &["commit", "-q", "-m", "first"]);
    scratch_git(&root, &["checkout", "-q", "-b", "work"]);
    root
}

/// Everything written in `root` committed.
fn committed(root: &Path) {
    scratch_git(root, &["add", "-A"]);
    scratch_git(root, &["commit", "-q", "-m", "work"]);
}

/// The published packages the checkout at `root` changes since it left `main`, by name.
fn touched_names(root: &Path) -> Vec<String> {
    let touched = touched(root, "main", bound()).unwrap().unwrap();
    touched.into_iter().map(|package| package.name).collect()
}

#[test]
fn a_branch_touches_the_published_packages_whose_folder_it_changes() {
    let tree = Tree::new();
    let root = repository(&tree);
    // No such branch to compare with, and a folder that is no workspace.
    assert_eq!(touched(&root, "trunk", bound()), Ok(None));
    assert!(touched_names(&root).is_empty(), "nothing changed yet");
    // A file no package holds, and an unpublished member's: none.
    tree.write("ws/README.md", "the workspace, said again\n");
    tree.write(
        "ws/delta/src/lib.rs",
        "//! delta, changed\npub fn delta() {}\n",
    );
    committed(&root);
    assert!(touched_names(&root).is_empty());
    // A committed change of one package, and a change of another not committed yet; a file its
    // package does not carry counts as its sources do.
    tree.write("ws/bravo/tests/it.rs", "#[test]\nfn it() {}\n");
    committed(&root);
    assert_eq!(touched_names(&root), ["bravo"]);
    tree.write(
        "ws/alpha/src/lib.rs",
        "//! alpha, changed\npub fn alpha() {}\n",
    );
    assert_eq!(touched_names(&root), ["alpha", "bravo"]);
    // A file moved out of a package is gone from it, though it is no change where it went.
    scratch_git(&root, &["stash", "-q"]);
    scratch_git(&root, &["mv", "charlie/NOTES.md", "NOTES.md"]);
    assert_eq!(touched_names(&root), ["bravo", "charlie"]);
    scratch_git(&root, &["mv", "NOTES.md", "charlie/NOTES.md"]);
    scratch_git(&root, &["stash", "pop", "-q"]);
    // What main gained since the branch left it is not the branch's.
    scratch_git(&root, &["stash", "-q"]);
    scratch_git(&root, &["checkout", "-q", "main"]);
    tree.write(
        "ws/charlie/src/lib.rs",
        "//! charlie, on main\npub fn charlie() {}\n",
    );
    committed(&root);
    assert!(touched_names(&root).is_empty(), "main against itself");
    scratch_git(&root, &["checkout", "-q", "work"]);
    assert_eq!(touched_names(&root), ["bravo"]);
    // The workspace's own manifest, which every packaged manifest inherits from: all of them.
    let manifest = tree.read("ws/Cargo.toml") + "\n[workspace.package]\nlicense = \"MIT\"\n";
    tree.write("ws/Cargo.toml", &manifest);
    assert_eq!(touched_names(&root), ["alpha", "bravo", "charlie"]);
}

#[test]
fn the_packages_files_change_are_read_by_folder_from_the_workspaces_root() {
    let member = |name: &str, manifest: &str| json!({"name": name, "version": "1.0.0", "manifest_path": manifest});
    let metadata = json!({
        "workspace_root": "/ws",
        "packages": [
            member("deep", "/ws/crates/deep/Cargo.toml"),
            member("deeper", "/ws/crates/deep-er/Cargo.toml"),
            member("outside", "/elsewhere/outside/Cargo.toml"),
        ],
    });
    let names = |files: &[&str]| {
        let changed = changed_by(&metadata, files);
        changed
            .into_iter()
            .map(|package| package.name)
            .collect::<Vec<_>>()
    };
    assert!(names(&[]).is_empty());
    // A folder is matched whole: `crates/deep-er` is not in `crates/deep`.
    assert_eq!(names(&["crates/deep-er/src/lib.rs"]), ["deeper"]);
    assert_eq!(names(&["crates/deep/Cargo.toml", "docs/a.md"]), ["deep"]);
    // A member's manifest is not the workspace's; the workspace's changes every member, the one
    // outside its root too.
    assert_eq!(names(&["Cargo.toml"]), ["deep", "deeper", "outside"]);
    assert!(names(&["elsewhere/outside/src/lib.rs", "Cargo.lock"]).is_empty());
    // A package at the root itself holds every file.
    let rooted = json!({"workspace_root": "/ws", "packages": [member("rooted", "/ws/Cargo.toml")]});
    assert_eq!(changed_by(&rooted, &["src/lib.rs"]).len(), 1);
    assert!(changed_by(&json!({}), &["src/lib.rs"]).is_empty());
}

#[test]
fn a_branchs_check_reads_only_the_packages_it_changes() {
    let tree = Tree::new();
    let root = repository(&tree);
    let registry = tree.path("registry");
    // alpha and bravo published as main holds them.
    for name in ["alpha", "bravo"] {
        let published = packaged(&root, name, &tree.path("published"));
        fs::create_dir_all(registry.join("crates").join(name)).unwrap();
        let file = format!("crates/{name}/{name}-1.0.0.crate");
        fs::copy(&published, registry.join(file)).unwrap();
        listed(&registry, name, &["1.0.0"]);
    }
    let folder = Registry::Folder(registry.clone());
    let reads = Duration::from_secs(60);
    let target = tree.path("target");
    let branch = |default: &str| check_branch(&root, default, &folder, &target, reads, later());
    assert_eq!(branch("trunk"), Ok(Branch::NoBase));
    // A branch that changes none reads nothing: not the registry, which here cannot be read.
    let unreadable = Registry::Folder(tree.path("ws/README.md"));
    let none = check_branch(&root, "main", &unreadable, &target, reads, later());
    assert_eq!(none, Ok(Branch::Checked(Checked::default())));
    // bravo's sources moved while its version did not: red for bravo, and alpha is not read.
    tree.write(
        "ws/bravo/src/lib.rs",
        "//! bravo, changed\npub fn bravo() {}\n",
    );
    committed(&root);
    let Ok(Branch::Checked(moved)) = branch("main") else {
        panic!("{:?}", branch("main"));
    };
    assert!(moved.lines.is_empty(), "{:?}", moved.lines);
    assert_eq!(
        moved.changed,
        [Changed {
            package: "bravo 1.0.0".into(),
            files: vec!["src/lib.rs changed".into()],
        }]
    );
    // Its version moved with them: green, and charlie, changed too, is the next release's.
    let manifest = tree
        .read("ws/bravo/Cargo.toml")
        .replacen("1.0.0", "1.0.1", 1);
    tree.write("ws/bravo/Cargo.toml", &manifest);
    let requires = tree
        .read("ws/charlie/Cargo.toml")
        .replace("version = \"1.0.0\" }", "version = \"1.0.1\" }");
    tree.write("ws/charlie/Cargo.toml", &requires);
    let Ok(Branch::Checked(bumped)) = branch("main") else {
        panic!("{:?}", branch("main"));
    };
    assert!(bumped.changed.is_empty(), "{:?}", bumped.changed);
    assert_eq!(bumped.lines.len(), 2, "{:?}", bumped.lines);
    assert!(bumped.lines[0].starts_with("bravo 1.0.1: not on the registry "));
    assert!(bumped.lines[1].starts_with("charlie 1.0.0: not on the registry "));
}

/// The URL of a port of this machine nothing listens on: a registry that gives no answer.
fn unanswering() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}", listener.local_addr().unwrap())
}

#[test]
fn a_registry_that_gives_no_answer_is_said_of_a_branch_and_an_error_of_the_whole_check() {
    let tree = Tree::new();
    let root = repository(&tree);
    let target = tree.path("target");
    tree.write(
        "ws/bravo/src/lib.rs",
        "//! bravo, changed\npub fn bravo() {}\n",
    );
    tree.write(
        "ws/alpha/src/lib.rs",
        "//! alpha, changed\npub fn alpha() {}\n",
    );
    // No bound here is one a test reaches: a read ends by what the registry answers, in one try.
    let reads = Duration::from_secs(60);
    let gone = Registry::at(unanswering(), 1);
    let Ok(Branch::Unread { packages, why }) =
        check_branch(&root, "main", &gone, &target, reads, later())
    else {
        panic!("a registry that gives no answer is no error and no check");
    };
    assert_eq!(packages, ["alpha 1.0.0", "bravo 1.0.0"]);
    assert!(why.starts_with("curl http://127.0.0.1:"), "{why}");
    // The whole check of the same packages, on the same registry: an error, as it fails closed.
    let all = packages_of(&root);
    let whole = compared(&root, &gone, &target, &all, bound(), None);
    assert!(matches!(whole, Err(Unread::NoAnswer(_))), "{whole:?}");
    // An answer that stands as an error is one for a branch too.
    let refusing = Server::new(vec![Answer::Says(400, "")]);
    let registry = Registry::at(refusing.base.clone(), 1);
    let refused = check_branch(&root, "main", &registry, &target, reads, later());
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.ends_with(" answered 400")),
        "{refused:?}"
    );
    // A version the registry lists and gives no answer for is said as well, as nothing is known
    // of it: alpha listed, bravo not, then alpha's download.
    let listing = Server::new(vec![
        Answer::Says(200, "{\"name\":\"alpha\",\"vers\":\"1.0.0\"}"),
        Answer::Says(404, ""),
        Answer::Nothing,
    ]);
    let registry = Registry::at(listing.base.clone(), 1);
    let unserved = check_branch(&root, "main", &registry, &target, reads, later());
    assert!(
        matches!(unserved, Ok(Branch::Unread { .. })),
        "{unserved:?}"
    );
    assert_eq!(listing.heard(), 3);
}

/// The published packages of the workspace at `root`.
fn packages_of(root: &Path) -> Vec<Package> {
    packages(root, bound()).unwrap()
}

#[test]
fn a_check_fails_closed_on_a_registry_it_cannot_read_or_a_package_it_cannot_fetch() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    fs::create_dir_all(registry.join("index/al/ph/alpha")).unwrap();
    let unread = check(
        &root,
        &Registry::Folder(registry.clone()),
        &tree.path("t"),
        later(),
    );
    assert!(unread.is_err(), "{unread:?}");
    fs::remove_dir_all(registry.join("index/al/ph/alpha")).unwrap();
    listed(&registry, "alpha", &["1.0.0"]);
    let unfetched =
        check(&root, &Registry::Folder(registry), &tree.path("t"), later()).unwrap_err();
    assert!(unfetched.contains("alpha-1.0.0.crate"), "{unfetched}");
}

/// What the test's server does with one request it read.
enum Answer {
    /// Closes the connection without a word: curl's empty reply.
    Nothing,
    /// Says nothing and keeps the connection open while the server lives.
    Silent,
    /// Answers this status with this body.
    Says(u16, &'static str),
}

/// A server of the test's own on a free port of this machine: it answers each connection in turn
/// as its answers say, then accepts no more, keeping what it holds open until it is dropped.
struct Server {
    base: String,
    heard: Arc<AtomicUsize>,
    _lives: mpsc::Sender<()>,
}

impl Server {
    fn new(answers: Vec<Answer>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let heard = Arc::new(AtomicUsize::new(0));
        let (lives, dropped) = mpsc::channel::<()>();
        let count = Arc::clone(&heard);
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for answer in answers {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let (mut request, mut byte) = (Vec::new(), [0u8; 1]);
                while !request.ends_with(b"\r\n\r\n")
                    && stream.read(&mut byte).is_ok_and(|read| read == 1)
                {
                    request.push(byte[0]);
                }
                count.fetch_add(1, Ordering::SeqCst);
                match answer {
                    Answer::Nothing => {}
                    Answer::Silent => held.push(stream),
                    Answer::Says(code, body) => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {code} Said\r\nContent-Length: {}\r\n\
                             Connection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                }
            }
            let _ = dropped.recv();
            drop((listener, held));
        });
        Server {
            base,
            heard,
            _lives: lives,
        }
    }

    fn url(&self) -> String {
        format!("{}/sy/mp/symphony-env", self.base)
    }

    /// How many requests it has read.
    fn heard(&self) -> usize {
        self.heard.load(Ordering::SeqCst)
    }
}

/// Reads of `tries` tries, each within `each` seconds, with no pause between.
fn reads(tries: u32, each: u64) -> Reads {
    Reads {
        each: Duration::from_secs(each),
        tries,
        pause: Duration::ZERO,
    }
}

#[test]
fn a_read_that_got_no_answer_or_a_servers_error_is_tried_again_and_the_next_answer_stands() {
    let server = Server::new(vec![
        Answer::Nothing,
        Answer::Says(200, "{\"vers\":\"1.0.0\"}"),
    ]);
    assert_eq!(
        fetched_by(reads(3, 60), &server.url(), None, bound()),
        Ok(Some(b"{\"vers\":\"1.0.0\"}".to_vec()))
    );
    assert_eq!(server.heard(), 2);

    let server = Server::new(vec![
        Answer::Says(503, "busy"),
        Answer::Says(429, "slow down"),
        Answer::Says(404, "none"),
    ]);
    assert_eq!(
        fetched_by(reads(3, 60), &server.url(), None, bound()),
        Ok(None)
    );
    assert_eq!(server.heard(), 3);

    let tree = Tree::new();
    let file = tree.path("alpha-1.0.0.crate");
    let server = Server::new(vec![Answer::Says(502, ""), Answer::Says(200, "the crate")]);
    assert_eq!(
        fetched_by(reads(3, 60), &server.url(), Some(&file), bound()),
        Ok(Some(Vec::new()))
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "the crate");
    assert_eq!(server.heard(), 2);
}

/// Why a read that got no answer on its last try says it failed: any other end fails the test.
fn unanswered(read: Result<Option<Vec<u8>>, Unread>) -> String {
    match read {
        Err(Unread::NoAnswer(why)) => why,
        other => panic!("a read that got no answer: {other:?}"),
    }
}

#[test]
fn a_read_that_fails_each_try_says_the_last_failure_and_its_tries() {
    let server = Server::new(vec![Answer::Nothing, Answer::Nothing, Answer::Nothing]);
    let failed = unanswered(fetched_by(reads(3, 60), &server.url(), None, bound()));
    assert!(
        failed.starts_with(&format!("curl {} failed: curl: (52) ", server.url()))
            && failed.ends_with(" (tried 3 times)"),
        "{failed}"
    );
    assert_eq!(server.heard(), 3);

    // The read the release run met: no answer within a try's time, each time.
    let server = Server::new(vec![Answer::Silent, Answer::Silent]);
    let failed = unanswered(fetched_by(reads(2, 1), &server.url(), None, bound()));
    assert!(
        failed.starts_with(&format!("curl {} ", server.url()))
            && failed.ends_with(" (tried 2 times)"),
        "{failed}"
    );
}

#[test]
fn a_read_is_not_tried_again_after_an_answer_that_stands_its_last_try_its_time_or_a_stop() {
    let server = Server::new(vec![Answer::Says(404, ""), Answer::Says(200, "")]);
    assert_eq!(
        fetched_by(reads(3, 60), &server.url(), None, bound()),
        Ok(None)
    );
    assert_eq!(server.heard(), 1, "a 404 stands");

    let server = Server::new(vec![Answer::Says(400, ""), Answer::Says(200, "")]);
    assert_eq!(
        fetched_by(reads(3, 60), &server.url(), None, bound()),
        Err(Unread::Failed(format!("{} answered 400", server.url())))
    );
    assert_eq!(
        server.heard(),
        1,
        "an answer that is no server's error stands"
    );

    let server = Server::new(vec![Answer::Says(503, ""), Answer::Says(200, "")]);
    assert_eq!(
        fetched_by(reads(1, 60), &server.url(), None, bound()),
        Err(Unread::NoAnswer(format!("{} answered 503", server.url())))
    );
    assert_eq!(server.heard(), 1, "one try is all one try makes");

    let server = Server::new(vec![Answer::Nothing, Answer::Says(200, "")]);
    let paused = Reads {
        pause: Duration::from_secs(600),
        ..reads(3, 60)
    };
    let failed = unanswered(fetched_by(
        paused,
        &server.url(),
        None,
        Bound::until(later()),
    ));
    assert!(
        failed.starts_with(&format!("curl {} failed: curl: (52) ", server.url()))
            && !failed.contains("tried"),
        "a pause past the bound is not waited for: {failed}"
    );
    assert_eq!(server.heard(), 1);

    let server = Server::new(vec![Answer::Silent, Answer::Says(200, "")]);
    let stop = || true;
    let failed = unanswered(fetched_by(
        reads(3, 60),
        &server.url(),
        None,
        Bound::hearing(later(), &stop),
    ));
    assert!(
        failed.starts_with(&format!("curl {} was stopped after ", server.url()))
            && failed.ends_with(" s: the run was told to stop"),
        "a run told to stop tries no more: {failed}"
    );
    assert!(server.heard() <= 1);
}

fn status(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code << 8)
}

fn ended(code: i32, stderr: &str) -> Output {
    Output {
        status: status(code),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn an_upload_is_told_done_already_there_without_a_token_or_failed_by_cargos_line() {
    assert_eq!(upload(&ended(0, "warning: x\n")), Upload::Done);
    assert_eq!(
        upload(&ended(
            101,
            "error: crate a@1.0.0 already exists on crates.io index\n"
        )),
        Upload::There
    );
    assert_eq!(
        upload(&ended(
            101,
            "error: crate version `1.0.0` is already uploaded\n"
        )),
        Upload::There
    );
    assert_eq!(
        upload(&ended(
            101,
            "error: no token found, please run `cargo login`\n"
        )),
        Upload::NoToken
    );
    assert_eq!(
        upload(&ended(
            101,
            "   Packaging a v1.0.0\nerror: failed to publish a\n\nCaused by:\n  503\n"
        )),
        Upload::Failed("error: failed to publish a".into())
    );
    assert_eq!(
        upload(&ended(1, "\nsomething else\n\n")),
        Upload::Failed("something else".into())
    );
}

#[test]
fn a_publication_uploads_what_crates_io_lacks_with_one_command_and_skips_what_it_holds() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    listed(&registry, "alpha", &["1.0.0"]);
    let cargo = stand_in(&tree, &registry, "", "", "");
    let log = tree.path("logs/publish.log");
    let folder = Registry::Folder(registry.clone());
    let done = publish(
        &root,
        "release-2026-11-12",
        &folder,
        cargo.as_os_str(),
        &log,
        bound(),
    );
    assert_eq!(
        done,
        Publication {
            release: "release-2026-11-12".into(),
            uploaded: vec!["bravo 1.0.0".into(), "charlie 1.0.0".into()],
            skipped: vec!["alpha 1.0.0".into()],
            ..Publication::default()
        }
    );
    let target = root.join(".target");
    assert_eq!(
        ran(&tree),
        [format!(
            "publish --locked -p bravo -p charlie target={}",
            target.display()
        )],
        "one command, naming only what the index lacks"
    );
    let said = fs::read_to_string(&log).unwrap();
    assert!(
        said.starts_with(
            "$ cargo publish --locked -p bravo -p charlie\n   Uploaded bravo v1.0.0 to registry \
             crates-io\n   Uploaded charlie v1.0.0"
        ),
        "{said}"
    );
    assert_eq!(
        done.lines(),
        [
            "published bravo 1.0.0, charlie 1.0.0",
            "on crates.io already: alpha 1.0.0"
        ]
    );
    // Again: everything is there, so nothing more is run.
    let again = publish(
        &root,
        "release-2026-11-12",
        &folder,
        cargo.as_os_str(),
        &log,
        bound(),
    );
    assert_eq!(
        again.skipped,
        ["alpha 1.0.0", "bravo 1.0.0", "charlie 1.0.0"]
    );
    assert!(again.uploaded.is_empty());
    assert_eq!(ran(&tree).len(), 1, "no upload more");
}

#[test]
fn with_no_token_nothing_more_is_tried_and_the_release_stays_green() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    let cargo = stand_in(&tree, &registry, "alpha bravo charlie", "", "");
    let done = publish(
        &root,
        "release-2026-11-12",
        &Registry::Folder(registry),
        cargo.as_os_str(),
        &tree.path("publish.log"),
        bound(),
    );
    assert!(!done.failed, "no token is no red");
    assert_eq!(done.left, ["alpha 1.0.0", "bravo 1.0.0", "charlie 1.0.0"]);
    assert_eq!(done.why.as_deref(), Some(NO_TOKEN));
    assert!(done.uploaded.is_empty());
    assert_eq!(ran(&tree).len(), 1, "{:?}", ran(&tree));
    let lines = done.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("not published: no token: "),
        "{lines:?}"
    );
    assert!(
        lines[0].ends_with(": alpha 1.0.0, bravo 1.0.0, charlie 1.0.0"),
        "{lines:?}"
    );
}

#[test]
fn a_failed_upload_is_red_naming_cargos_line_and_the_index_read_again_says_what_went_up() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    // cargo uploads alpha and bravo, then fails at charlie.
    let cargo = stand_in(&tree, &registry, "", "charlie", "");
    let log = tree.path("publish.log");
    let folder = Registry::Folder(registry.clone());
    let done = publish(
        &root,
        "release-2026-11-12",
        &folder,
        cargo.as_os_str(),
        &log,
        bound(),
    );
    assert!(done.failed);
    assert_eq!(done.uploaded, ["alpha 1.0.0", "bravo 1.0.0"]);
    assert_eq!(done.left, ["charlie 1.0.0"]);
    assert_eq!(
        done.why,
        Some(format!(
            "cargo publish failed (error: failed to publish charlie to registry at \
             https://crates.io; log {})",
            log.display()
        ))
    );
    assert_eq!(ran(&tree).len(), 1);
    let lines = done.lines();
    assert_eq!(lines[0], "published alpha 1.0.0, bravo 1.0.0");
    assert!(lines[1].ends_with("): charlie 1.0.0"), "{lines:?}");
    assert_eq!(
        lines[2],
        "the release stays tagged; `just release --refresh`, or the next release, publishes what \
         is left"
    );
    // The next publication names only what is left.
    let cargo = stand_in(&tree, &registry, "", "", "");
    let again = publish(&root, "r", &folder, cargo.as_os_str(), &log, bound());
    assert_eq!(
        (again.uploaded, again.failed),
        (vec!["charlie 1.0.0".to_string()], false)
    );
    assert!(
        ran(&tree)[1].starts_with("publish --locked -p charlie target="),
        "{:?}",
        ran(&tree)
    );
}

#[test]
fn a_version_crates_io_had_already_leaves_the_rest_to_the_next_publication_and_is_no_red() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    // The index read before it lacked bravo, which crates.io holds: cargo stops there.
    let cargo = stand_in(&tree, &registry, "", "", "bravo");
    let done = publish(
        &root,
        "release-2026-11-12",
        &Registry::Folder(registry),
        cargo.as_os_str(),
        &tree.path("publish.log"),
        bound(),
    );
    assert!(!done.failed);
    assert_eq!(done.uploaded, ["alpha 1.0.0"]);
    assert_eq!(done.left, ["bravo 1.0.0", "charlie 1.0.0"]);
    assert_eq!(
        done.lines(),
        [
            "published alpha 1.0.0",
            "not published: cargo found a version on crates.io already, which the index read \
             before it lacked: the next publication finishes what is left: bravo 1.0.0, charlie \
             1.0.0"
        ]
    );
}

#[test]
fn a_publication_is_red_where_it_cannot_read_its_packages_or_the_registry_or_nothing_went_up() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    fs::create_dir_all(registry.join("index/al/ph/alpha")).unwrap();
    let cargo = stand_in(&tree, &registry, "", "", "");
    let folder = Registry::Folder(registry);
    let log = tree.path("publish.log");
    let unread = publish(&root, "r", &folder, cargo.as_os_str(), &log, bound());
    assert!(unread.failed);
    assert_eq!(unread.left.len(), 3);
    assert!(
        unread.why.as_deref().unwrap().contains(" cannot be read: "),
        "{unread:?}"
    );
    tree.write("ws/Cargo.toml", "[workspace\n");
    let broken = publish(&root, "r", &folder, cargo.as_os_str(), &log, bound());
    assert!(broken.failed);
    assert!(
        broken
            .why
            .as_deref()
            .unwrap()
            .starts_with("its packages cannot be read: cargo metadata failed"),
        "{broken:?}"
    );
    assert!(ran(&tree).is_empty(), "no upload: {:?}", ran(&tree));
    // A cargo that ends well and uploads nothing: the index says so, red.
    workspace(&tree);
    fs::remove_dir_all(tree.path("registry")).unwrap();
    let quiet = publish(&root, "r", &folder, OsStr::new("true"), &log, bound());
    assert!(quiet.failed);
    assert_eq!(quiet.left.len(), 3);
    assert!(
        quiet
            .why
            .as_deref()
            .unwrap()
            .starts_with("cargo publish ended well, but the index does not list them (log "),
        "{quiet:?}"
    );
    let nothing = publish(
        &tree.path("none"),
        "r",
        &folder,
        cargo.as_os_str(),
        &log,
        bound(),
    );
    assert_eq!(nothing.lines(), ["no package to publish"]);
}

#[test]
fn a_publication_off_says_so_and_is_no_red() {
    let off = Publication::not("release-2026-11-12", OFF, false);
    assert_eq!(off.lines(), ["not published: [releases] publish is off"]);
    let json = serde_json::to_value(&off).unwrap();
    assert_eq!(
        json,
        json!({"release": "release-2026-11-12", "why": OFF, "failed": false}),
        "what is empty is left out of the record"
    );
}

#[test]
fn the_notes_name_each_version_the_registry_lacks_when_tagged() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    let folder = Registry::Folder(registry.clone());
    listed(&registry, "alpha", &["1.0.0"]);
    let said = notes(&root, &folder, bound());
    assert_eq!(
        said,
        [format!(
            "Publishes to {}: bravo 1.0.0, charlie 1.0.0.",
            folder.said()
        )]
    );
    listed(&registry, "bravo", &["1.0.0"]);
    listed(&registry, "charlie", &["1.0.0"]);
    assert_eq!(
        notes(&root, &folder, bound()),
        [format!(
            "Publishes nothing: {} holds each package's version.",
            folder.said()
        )]
    );
    fs::remove_file(registry.join("index/ch/ar/charlie")).unwrap();
    fs::create_dir_all(registry.join("index/ch/ar/charlie")).unwrap();
    let unread = notes(&root, &folder, bound());
    assert!(
        unread[0].contains(" could not be read when it was tagged (")
            && unread[0].ends_with("): the step after its push publishes what it lacks."),
        "{unread:?}"
    );
    assert!(notes(&tree.path("none"), &folder, bound()).is_empty());
}
