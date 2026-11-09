//! Native effective-ignore changes invalidate real saved positive and complete empty verdicts.

use crate::{Repo, cached, clean_cache, git, text, write};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use symphony_board::sys::Spawning as _;

/// Each case owns a child environment; parallel tests never mutate the runner's HOME or XDG.
fn isolated(test: &str) -> bool {
    if std::env::var_os("SF_CLEAN_PROOF_CHILD").is_some() {
        return false;
    }
    for case in ["missing", "empty", "nonempty"] {
        clean_cache::isolated_with("policy_cache", test, |home, child| {
            child.env("SF_IGNORE_HOME_CASE", case);
            match case {
                "empty" => child.env("XDG_CONFIG_HOME", ""),
                // Spaces are literal native path bytes, not a value to trim or normalize.
                "nonempty" => child.env("XDG_CONFIG_HOME", home.join(" xdg ")),
                _ => child.env_remove("XDG_CONFIG_HOME"),
            };
        });
    }
    true
}

fn rules() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    match std::env::var("SF_IGNORE_HOME_CASE").unwrap().as_str() {
        "nonempty" => home.join(" xdg /git/ignore"),
        "missing" | "empty" => home.join(".config/git/ignore"),
        other => panic!("unexpected fixture case: {other}"),
    }
}

fn environment(command: &mut Command) {
    command.env("HOME", std::env::var_os("HOME").unwrap());
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(value) => command.env("XDG_CONFIG_HOME", value),
        None => command.env_remove("XDG_CONFIG_HOME"),
    };
}

fn cli(r: &Repo) -> (String, usize) {
    cached::traced_with(r, &clean_cache::trace(), environment)
}

/// The native control cannot refresh the index and accidentally invalidate an unrelated input.
fn native(r: &Repo, tree: &Path) -> String {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "diff.autoRefreshIndex=false",
            "status",
            "--porcelain",
            "--ignored=matching",
            "--untracked-files=all",
        ])
        .current_dir(tree)
        .env_clear()
        .envs(r.vars())
        .env("GIT_OPTIONAL_LOCKS", "0");
    environment(&mut command);
    let done = command.output_locked().unwrap();
    assert!(
        done.status.success(),
        "native status {}: {}",
        done.status,
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap()
}

fn fixture() -> Repo {
    let r = Repo::new();
    fs::write(r.root.join(".gitignore"), "").unwrap();
    git(&r.root, &["add", ".gitignore"]);
    git(
        &r.root,
        &["commit", "-qm", "effective default ignore fixture"],
    );
    // Parent directories exist before warming: only the selected external file changes later.
    fs::create_dir_all(rules().parent().unwrap()).unwrap();
    r
}

#[test]
fn effective_default_ignore_creation_invalidates_saved_positive_verdicts() {
    if isolated("effective_default_ignore_creation_invalidates_saved_positive_verdicts") {
        return;
    }
    let r = fixture();
    let tree = r.detached("kept");
    write(&tree, ".godot/cache", "regenerable output\n");
    assert_eq!(cli(&r).1, 1);
    let before = cached::saved(&tree);
    assert_eq!(
        before["verdict"]["left"],
        serde_json::json!([".godot/cache"])
    );
    assert_eq!(
        before["policy"].get(rules().to_str().unwrap()),
        Some(&serde_json::Value::Null),
        "effective absent ignore is captured: {before}"
    );
    assert_eq!(cli(&r).1, 0, "real saved positive hit");
    assert_eq!(cached::saved(&tree), before);
    fs::write(rules(), "/.godot/\n").unwrap();
    let status = native(&r, &tree);
    assert!(status.contains("!! .godot/"), "effective policy: {status}");
    let (out, calls) = cli(&r);
    // Nonforced native worktree removal launches its own status after our two final guards.
    let trace = fs::read_to_string(clean_cache::trace()).unwrap();
    let native_checks = trace
        .lines()
        .filter(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            event["event"] == "start"
                && event["argv"].as_array().is_some_and(|args| {
                    args.iter().any(|arg| arg == "status")
                        && args.iter().any(|arg| arg == "--ignore-submodules=none")
                })
        })
        .count();
    assert_eq!(native_checks, 1, "native removal protection: {trace}");
    assert_eq!(
        calls, 4,
        "fresh lookup, both guards and native removal: {out}"
    );
    r.assert_removed(&tree, &out);
}

#[test]
fn effective_default_ignore_edits_invalidate_saved_empty_inventory_and_name_retained_files() {
    if isolated(
        "effective_default_ignore_edits_invalidate_saved_empty_inventory_and_name_retained_files",
    ) {
        return;
    }
    let r = fixture();
    fs::write(rules(), "/.godot/\n").unwrap();
    let tree = r.detached("clean");
    write(
        &tree,
        ".godot/cache",
        "source must remain when no longer disposable\n",
    );
    let status = native(&r, &tree);
    assert!(status.contains("!! .godot/"), "effective policy: {status}");
    let mut before = serde_json::Value::Null;
    for pass in 0..2 {
        fs::write(clean_cache::trace(), "").unwrap();
        let out = text(&r.sweep_now(|options| {
            options.recent_hours = 0.0;
            options.occupied = Box::new(|_| Some(true));
        }));
        r.assert_kept(&tree, &out);
        let saved = cached::saved(&tree);
        assert_eq!(saved["verdict"]["left"], serde_json::json!([]));
        assert!(
            saved["inventory"]["entries"].as_array().unwrap().len() >= 5,
            "{saved}"
        );
        assert!(
            saved["policy"][rules().to_str().unwrap()].is_object(),
            "{saved}"
        );
        assert_eq!(
            cached::status_calls(&clean_cache::trace()),
            if pass == 0 { 3 } else { 2 },
            "warm lookup reuses proof, both final guards remain: {out}"
        );
        if pass == 0 {
            before = saved;
        } else {
            assert_eq!(saved, before, "real saved empty proof is unchanged");
        }
    }
    fs::write(rules(), "").unwrap();
    let status = native(&r, &tree);
    assert!(
        status.contains("?? .godot/cache"),
        "effective policy: {status}"
    );
    let (out, calls) = cli(&r);
    assert_eq!(
        calls, 1,
        "effective external policy edit forces lookup: {out}"
    );
    r.assert_kept(&tree, &out);
    assert!(
        out.contains(".godot/cache"),
        "current retained reason: {out}"
    );
    let after = cached::saved(&tree);
    assert_eq!(
        after["inputs"], before["inputs"],
        "only external policy changed"
    );
    assert_eq!(
        after["verdict"]["left"],
        serde_json::json!([".godot/cache"])
    );
    assert_eq!(cli(&r).1, 0, "fresh positive result is reusable");
    assert_eq!(
        fs::read_to_string(tree.join(".godot/cache")).unwrap(),
        "source must remain when no longer disposable\n"
    );
}
