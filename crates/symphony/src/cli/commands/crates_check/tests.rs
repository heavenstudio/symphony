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
    let packed = crate::children::output(
        crate::children::command("tar")
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
