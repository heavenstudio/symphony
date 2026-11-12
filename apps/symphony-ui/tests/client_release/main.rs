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

/// The build `id` as a folder's builds.json names it: its shell and its wasm.
fn build(id: &str) -> serde_json::Value {
    serde_json::json!({
        "shell": format!("symphony-shell-{id}.js"),
        "files": [format!("symphony-shell-{id}.js"), format!("{id}.wasm")],
    })
}

/// The builds `ids`, newest first, in `directory`: their files and its builds.json, published
/// under the bound of three.
fn holding(tree: &tree::Tree, directory: &str, ids: &[&str]) {
    for id in ids {
        tree.write(
            &format!("{directory}/assets/symphony-shell-{id}.js"),
            &format!("{id} shell"),
        );
        tree.write(
            &format!("{directory}/assets/{id}.wasm"),
            &format!("{id} bytes"),
        );
    }
    let builds: Vec<serde_json::Value> = ids.iter().map(|id| build(id)).collect();
    tree.write(
        &format!("{directory}/builds.json"),
        &serde_json::json!({"keep": 3, "builds": builds}).to_string(),
    );
}

/// A fresh build and the committed client holding it alone.
fn fixtures(tree: &tree::Tree) -> (PathBuf, PathBuf) {
    for directory in ["built", "committed"] {
        tree.write(&format!("{directory}/index.html"), "current page");
        holding(tree, directory, &["current"]);
    }
    (tree.path("built"), tree.path("committed"))
}

fn refused(done: &Output) -> String {
    assert_eq!(done.status.code(), Some(1), "{done:?}");
    assert!(done.stdout.is_empty(), "{done:?}");
    String::from_utf8(done.stderr.clone()).unwrap()
}

#[test]
fn freshly_built_names_and_bytes_match_while_retained_builds_within_the_bound_are_allowed() {
    let tree = tree::Tree::new();
    let (built, committed) = fixtures(&tree);
    let done = check(&built, &committed);
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        "client-check: 3 freshly built files match the committed client, which keeps 1 builds\n"
    );
    assert!(done.stderr.is_empty());
    // Two older builds kept for open pages, and older builds the fresh build's own folder kept
    // from an earlier check, which are not this build's.
    holding(&tree, "committed", &["current", "previous", "older"]);
    holding(&tree, "built", &["current", "stale"]);
    let done = check(&built, &committed);
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        "client-check: 3 freshly built files match the committed client, which keeps 3 builds\n"
    );
}

#[test]
fn a_build_beyond_the_bound_or_a_file_no_kept_build_names_refuses() {
    let tree = tree::Tree::new();
    let (built, committed) = fixtures(&tree);
    holding(
        &tree,
        "committed",
        &["current", "previous", "older", "oldest"],
    );
    assert_eq!(
        refused(&check(&built, &committed)),
        "client-check: the committed client keeps 4 builds, more than the 3 it is published with: run just client-update\n"
    );
    holding(&tree, "committed", &["current", "previous", "older"]);
    // The oldest build's files stay behind once the list drops it.
    assert_eq!(
        refused(&check(&built, &committed)),
        "client-check: no build the committed client keeps names assets/oldest.wasm: run just client-update\n"
    );
    fs::remove_file(committed.join("assets/oldest.wasm")).unwrap();
    fs::remove_file(committed.join("assets/symphony-shell-oldest.js")).unwrap();
    tree.write("committed/assets/chunks/old-dxh789.js", "in a folder");
    assert_eq!(
        refused(&check(&built, &committed)),
        "client-check: no build the committed client keeps names assets/chunks/old-dxh789.js: run just client-update\n"
    );
    fs::remove_dir_all(committed.join("assets/chunks")).unwrap();
    fs::remove_file(committed.join("assets/older.wasm")).unwrap();
    assert_eq!(
        refused(&check(&built, &committed)),
        "client-check: the committed client lacks assets/older.wasm, which a build it keeps names: run just client-update\n"
    );
    fs::remove_dir_all(committed.join("assets")).unwrap();
    holding(&tree, "committed", &["current"]);
    let done = check(&built, &committed);
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    // A committed client that names no builds, or not the fresh one first, is to rebuild.
    for list in [
        None,
        Some(serde_json::json!({"keep": 3, "builds": [build("previous"), build("current")]})),
        Some(serde_json::json!({"keep": 3, "builds": [build("current"), {"shell": 1}]})),
    ] {
        match list {
            Some(list) => fs::write(committed.join("builds.json"), list.to_string()).unwrap(),
            None => fs::remove_file(committed.join("builds.json")).unwrap(),
        }
        assert_eq!(
            refused(&check(&built, &committed)),
            "client-check: rebuild and commit the client asset: builds.json\n"
        );
    }
    // A fresh build that names no build or no bound is no build to hold the committed one to.
    holding(&tree, "committed", &["current"]);
    for list in [
        None,
        Some(serde_json::json!({"builds": [build("current")]})),
        Some(serde_json::json!({"keep": 3, "builds": []})),
    ] {
        match list {
            Some(list) => fs::write(built.join("builds.json"), list.to_string()).unwrap(),
            None => fs::remove_file(built.join("builds.json")).unwrap(),
        }
        let done = check(&built, &committed);
        assert_eq!(done.status.code(), Some(2), "{done:?}");
        assert_eq!(
            String::from_utf8(done.stderr).unwrap(),
            "client-check: the fresh build names no build or bound in its builds.json\n"
        );
    }
}

#[test]
fn a_new_hash_changed_bytes_or_unbuilt_distribution_refuses() {
    let tree = tree::Tree::new();
    let (built, committed) = fixtures(&tree);
    for (path, body) in [
        ("index.html", "changed page"),
        ("assets/current.wasm", "changed bytes"),
    ] {
        let file = built.join(path);
        let prior = fs::read(&file).unwrap();
        fs::write(&file, body).unwrap();
        assert_eq!(
            refused(&check(&built, &committed)),
            format!("client-check: rebuild and commit the client asset: {path}\n")
        );
        fs::write(file, prior).unwrap();
    }
    // A file the fresh build made that the committed client lacks.
    let prior = fs::read(built.join("builds.json")).unwrap();
    tree.write("built/assets/new-hash.js", "new import");
    let mut grown = build("current");
    grown["files"]
        .as_array_mut()
        .unwrap()
        .push("new-hash.js".into());
    fs::write(
        built.join("builds.json"),
        serde_json::json!({"keep": 3, "builds": [grown]}).to_string(),
    )
    .unwrap();
    assert_eq!(
        refused(&check(&built, &committed)),
        "client-check: rebuild and commit the client asset: assets/new-hash.js\n"
    );
    fs::write(built.join("builds.json"), prior).unwrap();
    fs::remove_file(built.join("assets/new-hash.js")).unwrap();
    assert_eq!(check(&built, &committed).status.code(), Some(0));
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
