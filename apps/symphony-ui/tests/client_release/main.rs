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

#[test]
fn compiler_remapping_retains_arguments_wrappers_streams_status_and_resource_settings() {
    use std::os::unix::fs::PermissionsExt;
    let tree = tree::Tree::new();
    let compiler = tree.write("compiler with spaces", r#"#!/bin/bash
printf '%s\0' "$@" > "$SYMPHONY_TEST_ARGS"
printf '%s|%s|%s|%s' "$CARGO_BUILD_JOBS" "$RUST_TEST_THREADS" "$RUSTFLAGS" "$CARGO_ENCODED_RUSTFLAGS" > "$SYMPHONY_TEST_ENV"
cat
printf 'compiler diagnostic\n' >&2
exit 37
"#);
    let upstream = tree.write(
        "upstream wrapper",
        r#"#!/bin/bash
printf used > "$SYMPHONY_TEST_UPSTREAM"
exec "$@"
"#,
    );
    let workspace = tree.write(
        "workspace wrapper",
        r#"#!/bin/bash
printf used > "$SYMPHONY_TEST_WORKSPACE"
exec "$@"
"#,
    );
    for path in [&compiler, &upstream, &workspace] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let wrapper = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("remap-rustc.sh");
    for (outer, inner) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut command = Command::new("bash");
        command
            .args(["-c", "printf 'compiler input\\n' | \"$@\"", "--"])
            .arg(&wrapper);
        if inner {
            command.arg(&workspace);
        }
        command
            .arg(&compiler)
            .args(["--crate-name", "input with spaces", "-C", "codegen-units=2"])
            .env("SYMPHONY_RELEASE_CARGO_HOME", "/cargo home")
            .env("SYMPHONY_RELEASE_SOURCE", "/source root")
            .env("CARGO_TARGET_DIR", "/target root")
            .env(
                "SYMPHONY_PARENT_RUSTC_WRAPPER",
                if outer {
                    upstream.as_os_str()
                } else {
                    std::ffi::OsStr::new("")
                },
            )
            .env("CARGO_BUILD_JOBS", "2")
            .env("RUST_TEST_THREADS", "6")
            .env("RUSTFLAGS", "-C keep-this")
            .env("CARGO_ENCODED_RUSTFLAGS", "keep-encoded")
            .env("SYMPHONY_TEST_ARGS", tree.path("args"))
            .env("SYMPHONY_TEST_ENV", tree.path("env"))
            .env(
                "SYMPHONY_TEST_UPSTREAM",
                tree.path(&format!("outer-{outer}-{inner}")),
            )
            .env(
                "SYMPHONY_TEST_WORKSPACE",
                tree.path(&format!("inner-{outer}-{inner}")),
            );
        let done = command.output().unwrap();
        assert_eq!(done.status.code(), Some(37), "{done:?}");
        assert_eq!(done.stdout, b"compiler input\n");
        assert_eq!(done.stderr, b"compiler diagnostic\n");
        assert_eq!(tree.read("env"), "2|6|-C keep-this|keep-encoded");
        assert_eq!(
            tree.read("args").split_terminator('\0').collect::<Vec<_>>(),
            [
                "--crate-name",
                "input with spaces",
                "-C",
                "codegen-units=2",
                "--remap-path-prefix=/cargo home=/symphony/cargo",
                "--remap-path-prefix=/source root=/symphony/source",
                "--remap-path-prefix=/target root=/symphony/target",
            ]
        );
        assert_eq!(tree.path(&format!("outer-{outer}-{inner}")).exists(), outer);
        assert_eq!(tree.path(&format!("inner-{outer}-{inner}")).exists(), inner);
    }
}

#[test]
fn immutable_asset_merges_retain_old_bytes_and_refuse_collisions_or_nonregular_sources() {
    let tree = tree::Tree::new();
    tree.write("previous/old-hash.js", "old deferred module");
    tree.write("previous/shared.css", "unchanged style");
    tree.write("built/new-hash.js", "new deferred module");
    tree.write("built/shared.css", "unchanged style");
    fs::create_dir(tree.path("assembled")).unwrap();
    let copy = |source: &str| {
        Command::new("bash")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets-copy.sh"))
            .arg(tree.path(source))
            .arg(tree.path("assembled"))
            .output()
            .unwrap()
    };
    for source in ["previous", "built"] {
        let done = copy(source);
        assert!(done.status.success(), "{done:?}");
        assert!(done.stdout.is_empty());
        assert!(done.stderr.is_empty());
    }
    assert_eq!(
        tree.names("assembled"),
        ["new-hash.js", "old-hash.js", "shared.css"]
    );
    assert_eq!(tree.read("assembled/old-hash.js"), "old deferred module");
    tree.write("built/shared.css", "same URL with different bytes");
    let done = copy("built");
    assert_eq!(done.status.code(), Some(2));
    assert!(
        String::from_utf8(done.stderr)
            .unwrap()
            .contains("immutable asset collision")
    );
    assert_eq!(tree.read("assembled/shared.css"), "unchanged style");
    tree.write("built/shared.css", "unchanged style");
    std::os::unix::fs::symlink(tree.path("outside"), tree.path("built/link.js")).unwrap();
    let done = copy("built");
    assert_eq!(done.status.code(), Some(2));
    assert!(
        String::from_utf8(done.stderr)
            .unwrap()
            .contains("must be a regular file")
    );
    assert!(!tree.path("assembled/link.js").exists());
}

#[test]
fn a_release_with_no_owned_repository_refuses_before_metadata_or_parent_tools() {
    let tree = tree::Tree::new();
    let root = tree.path("containing/child");
    let script = root.join("apps/symphony-ui/bundle.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bundle.sh"),
        &script,
    )
    .unwrap();
    tree.write("containing/child/Cargo.toml", "[workspace]\n");
    tree.write("containing/.git/HEAD", "hostile parent");
    let guard = "#!/bin/sh\nprintf reached > \"$SYMPHONY_TEST_FORBIDDEN\"\nexit 91\n";
    for path in [
        "containing/tools/rs-bin.sh",
        "containing/child/tools/cargo.sh",
    ] {
        let path = tree.write(path, guard);
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let done = Command::new("bash")
        .arg(script)
        .arg(tree.path("output"))
        .env("CARGO_TARGET_DIR", tree.path("target"))
        .env("SYMPHONY_TEST_FORBIDDEN", tree.path("forbidden"))
        .env("GIT_DIR", tree.path("containing/.git"))
        .output()
        .unwrap();
    assert_eq!(done.status.code(), Some(2));
    assert!(done.stdout.is_empty());
    assert!(
        String::from_utf8(done.stderr)
            .unwrap()
            .contains("requires its own repository")
    );
    assert!(!tree.path("forbidden").exists());
    assert!(!tree.path("target").exists());
    assert!(!tree.path("output").exists());
}
