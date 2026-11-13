//! Real source and external-policy changes between native status and proof collection.

use crate::{
    Repo, cached,
    clean_cache::{isolated_with, trace},
    git, text, write,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn raced(test: &str, mutation: &str) -> bool {
    isolated_with("clean_races", test, |scratch, child| {
        let actual = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join("git"))
            .find(|path| path.is_file())
            .unwrap();
        let wrapper = scratch.join("bin");
        fs::create_dir(&wrapper).unwrap();
        let script = wrapper.join("git");
        fs::write(
            &script,
            format!(
                r#"#!/bin/sh
for arg in "$@"; do
 if [ "$arg" = ls-files ] && [ ! -e "$HOME/collected" ]; then
  printf '%s\n' collected > "$HOME/collected"
  {mutation}
 fi
done
exec "$SYMPHONY_REAL_GIT" "$@"
"#
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        child
            .env(
                "PATH",
                format!("{}:{}", wrapper.display(), std::env::var("PATH").unwrap()),
            )
            .env("SYMPHONY_REAL_GIT", actual);
    })
}

#[test]
fn source_created_after_status_during_inventory_collection_gets_a_fresh_retained_reason() {
    if raced(
        "source_created_after_status_during_inventory_collection_gets_a_fresh_retained_reason",
        "printf '%s\\n' 'concurrent source' > sim/after-status.gd",
    ) {
        return;
    }
    let r = Repo::new();
    let tree = r.detached("clean");
    fs::write(trace(), "").unwrap();
    let out = text(&r.sweep_now(|o| o.recent_hours = 0.0));
    assert_eq!(
        cached::status_calls(&trace()),
        2,
        "one bounded retry after changed collection: {out}"
    );
    assert!(out.contains("sim/after-status.gd"), "fresh reason: {out}");
    r.assert_kept(&tree, &out);
    assert_eq!(
        cached::saved(&tree)["verdict"]["left"],
        serde_json::json!(["sim/after-status.gd"])
    );
    assert_eq!(
        fs::read_to_string(tree.join("sim/after-status.gd")).unwrap(),
        "concurrent source\n"
    );
}

#[test]
fn external_ignore_change_during_collection_cannot_certify_an_old_disposable_cut() {
    if raced(
        "external_ignore_change_during_collection_cannot_certify_an_old_disposable_cut",
        ": > \"$HOME/.config/git/ignore\"",
    ) {
        return;
    }
    let r = Repo::new();
    write(&r.root, ".gitignore", "");
    git(&r.root, &["add", ".gitignore"]);
    git(&r.root, &["commit", "-qm", "external ignore fixture"]);
    let tree = r.detached("clean");
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    write(&home, ".config/git/ignore", "/.godot/\n");
    write(&tree, ".godot/cache", "retained after ignore disappears\n");
    let status = git(
        &tree,
        &[
            "status",
            "--porcelain",
            "--ignored=matching",
            "--untracked-files=all",
        ],
    );
    assert!(
        status.contains("!! .godot/"),
        "native ignored-directory precondition: {status}"
    );
    fs::write(trace(), "").unwrap();
    let out = text(&r.sweep_now(|o| o.recent_hours = 0.0));
    assert_eq!(
        cached::status_calls(&trace()),
        2,
        "one bounded fresh retry for changed external policy: {out}"
    );
    assert!(out.contains(".godot/cache"), "fresh retained reason: {out}");
    r.assert_kept(&tree, &out);
    assert_eq!(
        cached::saved(&tree)["verdict"]["left"],
        serde_json::json!([".godot/cache"])
    );
    assert_eq!(
        fs::read_to_string(tree.join(".godot/cache")).unwrap(),
        "retained after ignore disappears\n"
    );
    assert_eq!(
        fs::read_to_string(home.join(".config/git/ignore")).unwrap(),
        ""
    );
}
