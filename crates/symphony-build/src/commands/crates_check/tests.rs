use std::fs;

use super::*;
use crate::test_tree::Tree;

/// A workspace of one published package, `alpha` at 1.0.0, in `tree`.
fn workspace(tree: &Tree) -> PathBuf {
    tree.write(
        "ws/Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"alpha\"]\n",
    );
    tree.write(
        "ws/alpha/Cargo.toml",
        "[package]\nname = \"alpha\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\
         description = \"a\"\nlicense = \"MIT\"\n",
    );
    tree.write("ws/alpha/src/lib.rs", "pub fn alpha() {}\n");
    tree.path("ws")
}

/// The check of `root` against the registry folder `registry`: its exit code, stdout and stderr.
fn run(root: &Path, registry: &Path, target: &Path) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = checked(
        root,
        &Registry::Folder(registry.to_path_buf()),
        target,
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_version_the_registry_lacks_is_green_and_one_whose_sources_moved_is_red() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    let (code, out, err) = run(&root, &registry, &tree.path("target"));
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.starts_with("crates-check: alpha 1.0.0: not on the registry ")
            && out.ends_with(
                "crates-check: every published package's version on crates.io holds its sources\n"
            ),
        "{out}"
    );
    // What the registry holds of alpha 1.0.0 is a package of other sources.
    let index = registry.join("index/al/ph/alpha");
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(&index, "{\"name\":\"alpha\",\"vers\":\"1.0.0\"}\n").unwrap();
    tree.write(
        "old/alpha-1.0.0/Cargo.toml",
        "[package]\nname = \"alpha\"\n",
    );
    tree.write("old/alpha-1.0.0/src/lib.rs", "pub fn old() {}\n");
    fs::create_dir_all(registry.join("crates/alpha")).unwrap();
    let packed = symphony::children::output(
        symphony::children::command("tar")
            .arg("-czf")
            .arg(registry.join("crates/alpha/alpha-1.0.0.crate"))
            .arg("-C")
            .arg(tree.path("old"))
            .arg("alpha-1.0.0"),
    )
    .unwrap();
    assert!(packed.status.success(), "{packed:?}");
    let (code, out, err) = run(&root, &registry, &tree.path("target"));
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.starts_with("crates-check: alpha 1.0.0: on crates.io, and its packaged sources differ")
            && err.contains("Cargo.toml changed")
            && err.contains("Cargo.toml.orig added")
            && err.contains("src/lib.rs changed"),
        "{err}"
    );
}

#[test]
fn a_registry_it_cannot_read_is_exit_2_and_a_second_root_is_refused() {
    let tree = Tree::new();
    let root = workspace(&tree);
    fs::create_dir_all(tree.path("registry/index/al/ph/alpha")).unwrap();
    let (code, out, err) = run(&root, &tree.path("registry"), &tree.path("target"));
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(
        err.starts_with("symphony-build crates-check: [Errno "),
        "{err}"
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&["a".into(), "b".into()], &mut out, &mut err), 2);
    assert_eq!(String::from_utf8(err).unwrap(), format!("{USAGE}\n"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&["--root".into()], &mut out, &mut err), 2);
}

/// `git ARGS` in `root`, as an author of the test's own.
fn git(root: &Path, args: &[&str]) {
    let done = symphony::children::output(
        symphony::children::git()
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
}

/// The branch's check of `root` against `default` and the registry folder `registry`: its exit
/// code, stdout and stderr.
fn run_branch(root: &Path, default: &str, registry: &Path) -> (i32, String, String) {
    run_branch_on(root, default, &Registry::Folder(registry.to_path_buf()))
}

/// [`run_branch`] against `registry`, its reads ending before a second try's pause.
fn run_branch_on(root: &Path, default: &str, registry: &Registry) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let target = root.join("../target");
    let reads = Duration::from_secs(1);
    let code = checked_branch(root, default, registry, &target, reads, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_branch_is_checked_for_the_packages_it_changes_and_says_when_that_is_none() {
    let tree = Tree::new();
    let root = workspace(&tree);
    let registry = tree.path("registry");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    git(&root, &["checkout", "-q", "-b", "work"]);
    assert_eq!(
        run_branch(&root, "trunk", &registry),
        (
            0,
            "crates-check: this checkout has no trunk to compare with: nothing checked\n".into(),
            String::new()
        )
    );
    assert_eq!(
        run_branch(&root, "main", &registry),
        (
            0,
            "crates-check: this branch changes no published package: nothing checked\n".into(),
            String::new()
        )
    );
    // It changes alpha, at a version the registry lacks: green, and said as a branch's.
    tree.write(
        "ws/alpha/src/lib.rs",
        "pub fn alpha() {}\npub fn more() {}\n",
    );
    let (code, out, err) = run_branch(&root, "main", &registry);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.starts_with("crates-check: alpha 1.0.0: not on the registry ")
            && out.ends_with(&format!("crates-check: {BRANCH}\n")),
        "{out}"
    );
    // The registry holds that version with the sources main has: red, as the whole check is.
    git(&root, &["stash", "-q"]);
    let packed = symphony::children::output(
        symphony::children::command("cargo")
            .args([
                "package",
                "--no-verify",
                "--exclude-lockfile",
                "--offline",
                "--quiet",
            ])
            .env("CARGO_TARGET_DIR", tree.path("published"))
            .current_dir(&root),
    )
    .unwrap();
    assert!(packed.status.success(), "{packed:?}");
    git(&root, &["stash", "pop", "-q"]);
    fs::create_dir_all(registry.join("crates/alpha")).unwrap();
    fs::copy(
        tree.path("published/package/alpha-1.0.0.crate"),
        registry.join("crates/alpha/alpha-1.0.0.crate"),
    )
    .unwrap();
    let index = registry.join("index/al/ph/alpha");
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(&index, "{\"name\":\"alpha\",\"vers\":\"1.0.0\"}\n").unwrap();
    let (code, out, err) = run_branch(&root, "main", &registry);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert!(
        err.starts_with("crates-check: alpha 1.0.0: on crates.io, and its packaged sources differ")
            && err.contains("src/lib.rs changed"),
        "{err}"
    );
    // A registry it cannot read is exit 2 for a branch too.
    fs::remove_file(&index).unwrap();
    fs::create_dir_all(&index).unwrap();
    let (code, _, err) = run_branch(&root, "main", &registry);
    assert_eq!(code, 2, "{err}");
    // A registry that gives no answer, a port nothing listens on, is said and no red for a branch;
    // the whole check is exit 2 on it, after its three tries.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = Registry::At(format!("http://{}", listener.local_addr().unwrap()));
    drop(listener);
    let (code, out, err) = run_branch_on(&root, "main", &gone);
    assert_eq!((code, out.as_str()), (0, ""), "{err}");
    assert!(
        err.starts_with(
            "crates-check: not checked: crates.io gave no answer (curl http://127.0.0.1:"
        ) && err.ends_with(
            "), so nothing is known of alpha 1.0.0, which this branch changes: `just \
                 crates-check` reads them, and the release run does\n"
        ),
        "{err}"
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = checked(&root, &gone, &tree.path("target"), &mut out, &mut err);
    let err = String::from_utf8(err).unwrap();
    assert_eq!((code, out.is_empty()), (2, true), "{err}");
    assert!(err.ends_with(" (tried 3 times)\n"), "{err}");
    // `--branch` takes the root after it, and no second flag.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        main(&["--branch".into(), "--root".into()], &mut out, &mut err),
        2
    );
    assert_eq!(String::from_utf8(err).unwrap(), format!("{USAGE}\n"));
}
