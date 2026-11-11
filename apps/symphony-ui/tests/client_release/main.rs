//! File comparison controls for the real release build check; no client build is substituted.
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "../../../../crates/symphony/tests/common/tree.rs"]
mod tree;

fn check(built: &Path, committed: &Path) -> Output {
    Command::new("bash")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/client-check.sh"))
        .arg(built)
        .arg(committed)
        .output()
        .unwrap()
}

fn fixtures(tree: &tree::Tree) -> (PathBuf, PathBuf) {
    for directory in ["built", "committed"] {
        tree.write(&format!("{directory}/index.html"), "current shell");
        tree.write(&format!("{directory}/assets/current.wasm"), "current bytes");
    }
    (tree.path("built"), tree.path("committed"))
}

#[test]
fn freshly_built_names_and_bytes_match_while_retained_immutable_assets_are_allowed() {
    let tree = tree::Tree::new();
    let (built, committed) = fixtures(&tree);
    tree.write(
        "committed/assets/previous-hash.js",
        "retained for an open page",
    );
    let done = check(&built, &committed);
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert_eq!(
        done.stdout,
        b"client-check: 2 freshly built files match the committed client\n"
    );
    assert!(done.stderr.is_empty());
}

#[test]
fn a_new_hash_changed_bytes_or_unbuilt_distribution_refuses() {
    let tree = tree::Tree::new();
    let (built, committed) = fixtures(&tree);
    for (path, body) in [
        ("index.html", "changed shell"),
        ("assets/current.wasm", "changed bytes"),
        ("assets/new-hash.js", "new import"),
    ] {
        let file = built.join(path);
        let prior = fs::read(&file).ok();
        fs::write(&file, body).unwrap();
        let done = check(&built, &committed);
        assert_eq!(done.status.code(), Some(1), "{done:?}");
        assert!(done.stdout.is_empty());
        assert_eq!(
            String::from_utf8(done.stderr).unwrap(),
            format!("client-check: rebuild and commit the client asset: {path}\n")
        );
        match prior {
            Some(bytes) => fs::write(file, bytes).unwrap(),
            None => fs::remove_file(file).unwrap(),
        }
    }
    let done = check(&tree.path("not-built"), &committed);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(done.stdout.is_empty());
    let done = check(&committed, &committed);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert_eq!(
        done.stderr,
        b"client-check: build output and committed client must be separate directories\n"
    );
    std::os::unix::fs::symlink(tree.path("outside"), built.join("assets/link")).unwrap();
    let done = check(&built, &committed);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(done.stdout.is_empty());
}
