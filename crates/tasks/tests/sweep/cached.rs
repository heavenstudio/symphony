//! Repeated passes count real status invocations, rather than inferring reuse from elapsed time.

use crate::{Repo, git, text, write};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use symphony_board::sys::Spawning as _;

/// Read the real published cache; a missing file fails the regression's cache precondition.
pub(super) fn saved(tree: &Path) -> serde_json::Value {
    let admin = git(tree, &["rev-parse", "--absolute-git-dir"]);
    let path = Path::new(&admin).join("sf-sweep-verdict.json");
    serde_json::from_slice(
        &fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display())),
    )
    .unwrap()
}

fn traced(r: &Repo, trace: &Path) -> (String, usize) {
    traced_with(r, trace, |_| {})
}

/// Run the native CLI with isolated board/config inputs and count actual native status starts.
pub(super) fn traced_with(
    r: &Repo,
    trace: &Path,
    change: impl FnOnce(&mut Command),
) -> (String, usize) {
    let _ = fs::remove_file(trace);
    let mut command = Command::new(env!("CARGO_BIN_EXE_tasks"));
    command
        .args(["sweep", "--recent-hours", "0"])
        .env_clear()
        .envs(r.vars())
        .env("GIT_TRACE2_EVENT", trace)
        .env("TASKS_SWEEP", "off")
        .current_dir(&r.root);
    change(&mut command);
    let done = command.output_locked().unwrap();
    let out = String::from_utf8(done.stdout).unwrap();
    assert!(
        done.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    (out, status_calls(trace))
}

/// Git trace events distinguish process starts from status-like text in diagnostic output.
pub(super) fn status_calls(trace: &Path) -> usize {
    fs::read_to_string(trace)
        .unwrap()
        .lines()
        .filter(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            event["event"] == "start"
                && event["argv"]
                    .as_array()
                    .is_some_and(|args| args.iter().any(|arg| arg == "status"))
        })
        .count()
}

#[test]
fn a_second_pass_over_300_unchanged_worktrees_runs_no_status() {
    let r = Repo::new();
    let first = r.detached("cached-0");
    write(&first, "sim/new.gd", "keep this source\n");
    let admin = std::path::PathBuf::from(git(&first, &["rev-parse", "--absolute-git-dir"]));
    // These are ordinary detached worktrees. Copy their tiny checkout/admin inputs directly so
    // setting up 300 fixtures does not itself launch 300 checkout processes.
    for n in 1..300 {
        let tree = r.dir.join(format!("cached-{n}"));
        let target = r.root.join(format!(".git/worktrees/cached-{n}"));
        fs::create_dir_all(tree.join("sim")).unwrap();
        fs::create_dir_all(&target).unwrap();
        for file in ["HEAD", "index", "commondir"] {
            fs::copy(admin.join(file), target.join(file)).unwrap();
        }
        fs::write(
            target.join("gitdir"),
            format!("{}\n", tree.join(".git").display()),
        )
        .unwrap();
        fs::write(tree.join(".git"), format!("gitdir: {}\n", target.display())).unwrap();
        fs::copy(first.join(".gitignore"), tree.join(".gitignore")).unwrap();
        fs::copy(first.join("sim/world.gd"), tree.join("sim/world.gd")).unwrap();
        write(&tree, "sim/new.gd", "keep this source\n");
    }
    assert_eq!(r.worktrees().len(), 301);
    let trace = r.dir.join("trace.jsonl");
    let began = std::time::Instant::now();
    let (out, count) = traced(&r, &trace);
    let cold = began.elapsed();
    assert_eq!(count, 300, "every candidate was inspected: {out}");
    assert!(
        out.contains("kept 300 merged for files that did not land"),
        "{out}"
    );
    for number in 0..300 {
        let path = r.root.join(format!(
            ".git/worktrees/cached-{number}/sf-sweep-verdict.json"
        ));
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["verdict"]["left"], serde_json::json!(["sim/new.gd"]));
    }
    let began = std::time::Instant::now();
    let (out, count) = traced(&r, &trace);
    eprintln!(
        "300 fixture worktrees: cold {cold:?}, warm {:?}; warm status calls {count}",
        began.elapsed()
    );
    assert_eq!(
        count, 0,
        "unchanged candidates reused their verdicts: {out}"
    );
    assert!(
        out.contains("kept 300 merged for files that did not land"),
        "{out}"
    );
}

#[test]
fn changed_folder_index_head_and_unreadable_cache_each_force_a_fresh_status() {
    let r = Repo::new();
    let tree = r.detached("kept");
    write(&tree, "sim/new.gd", "keep this source\n");
    let trace = r.dir.join("trace.jsonl");
    assert_eq!(traced(&r, &trace).1, 1);
    assert_eq!(traced(&r, &trace).1, 0);
    write(&tree, "new.txt", "new top-level input\n");
    assert_eq!(traced(&r, &trace).1, 1);
    git(&tree, &["add", "sim/new.gd"]);
    assert_eq!(traced(&r, &trace).1, 1);
    git(&tree, &["commit", "-qm", "new source"]);
    let head = git(&tree, &["rev-parse", "HEAD"]);
    git(&r.root, &["merge", "--ff-only", &head]);
    assert_eq!(traced(&r, &trace).1, 1);
    let admin = git(&tree, &["rev-parse", "--absolute-git-dir"]);
    fs::write(Path::new(&admin).join("sf-sweep-verdict.json"), "broken").unwrap();
    assert_eq!(traced(&r, &trace).1, 1);
}

#[test]
fn an_unchanged_dangling_symlink_reuses_its_verdict_without_following_the_target() {
    let r = Repo::new();
    let tree = r.detached("kept");
    let target = r.dir.join("missing");
    std::os::unix::fs::symlink(&target, tree.join("sim/link")).unwrap();
    let trace = r.dir.join("trace.jsonl");
    assert_eq!(traced(&r, &trace).1, 1);
    let (out, count) = traced(&r, &trace);
    assert_eq!(count, 0);
    r.assert_kept(&tree, &out);
    assert_eq!(fs::read_link(tree.join("sim/link")).unwrap(), target);
}

#[test]
fn removing_the_last_nested_blocker_invalidates_a_kept_verdict() {
    for file in ["sim/deep/new.gd", "artifacts/solar-swift/deep/x.log"] {
        let r = Repo::new();
        let tree = r.detached("kept");
        write(&tree, file, "keep this evidence\n");
        let trace = r.dir.join("trace.jsonl");
        assert_eq!(traced(&r, &trace).1, 1);
        assert_eq!(traced(&r, &trace).1, 0);
        fs::remove_file(tree.join(file)).unwrap();
        if file.starts_with("artifacts/") {
            fs::remove_dir(tree.join("artifacts/solar-swift/deep")).unwrap();
            fs::remove_dir(tree.join("artifacts/solar-swift")).unwrap();
        }
        let (out, count) = traced(&r, &trace);
        assert!(count > 0, "the removed blocker requires a fresh verdict");
        r.assert_removed(&tree, &out);
    }
}

#[test]
fn a_saved_clean_verdict_rescans_and_reports_new_nested_source_or_artifacts() {
    for file in ["sim/new.gd", "artifacts/solar-swift/x.log"] {
        let r = Repo::new();
        let tree = r.detached("kept");
        let first = r.sweep_with(|o| o.occupied = Box::new(|_| Some(true)));
        r.assert_kept(&tree, &text(&first));
        assert_eq!(saved(&tree)["verdict"]["left"], serde_json::json!([]));
        let folder_time = fs::metadata(&tree).unwrap().modified().unwrap();
        write(&tree, file, "keep this evidence\n");
        if file.starts_with("sim/") {
            assert_eq!(
                fs::metadata(&tree).unwrap().modified().unwrap(),
                folder_time
            );
        }
        let next = r.sweep_now(|options| options.recent_hours = 0.0);
        let out = text(&next);
        r.assert_kept(&tree, &out);
        assert!(
            out.contains(file.split('/').next().unwrap()),
            "new blocker has a retained reason: {out}"
        );
        if file.starts_with("sim/") {
            assert!(out.contains(file), "nested source must be named: {out}");
        }
        assert_eq!(
            fs::read_to_string(tree.join(file)).unwrap(),
            "keep this evidence\n"
        );
        let refreshed = saved(&tree);
        assert_ne!(
            refreshed["verdict"]["left"],
            serde_json::json!([]),
            "{refreshed}"
        );
        let trace = r.dir.join("trace.jsonl");
        // The child has an isolated HOME/config identity, so warm that identity, then prove reuse.
        traced(&r, &trace);
        assert_eq!(traced(&r, &trace).1, 0);
    }
}

#[test]
fn changed_ignore_policies_release_output_after_a_proven_cache_hit() {
    for policy in [
        "shared",
        "global",
        "global-home",
        "global-relative",
        "global-default",
        "included",
        "local",
    ] {
        let r = Repo::new();
        fs::write(r.root.join(".gitignore"), "").unwrap();
        git(&r.root, &["add", ".gitignore"]);
        git(&r.root, &["commit", "-qm", "fixture without ignore rules"]);
        let rules = match policy {
            "shared" => r.root.join(".git/info/exclude"),
            "global" => {
                let path = r.dir.join("global-ignore");
                git(
                    &r.root,
                    &["config", "core.excludesFile", path.to_str().unwrap()],
                );
                path
            }
            "global-home" | "global-relative" => {
                let setting = if policy == "global-home" {
                    "~/global-ignore"
                } else {
                    "../global-ignore"
                };
                git(&r.root, &["config", "core.excludesFile", setting]);
                r.dir.join("global-ignore")
            }
            "global-default" => {
                let path = r.dir.join(".config/git/ignore");
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                path
            }
            "included" => {
                let config = r.dir.join("included-config");
                let rules = r.dir.join("global-ignore");
                fs::write(
                    &config,
                    format!("[core]\nexcludesFile = {}\n", rules.display()),
                )
                .unwrap();
                git(
                    &r.root,
                    &["config", "include.path", config.to_str().unwrap()],
                );
                rules
            }
            _ => r.dir.join("kept/.gitignore"),
        };
        let tree = r.detached("kept");
        write(&tree, ".godot/cache", "regenerable output\n");
        let trace = r.dir.join("trace.jsonl");
        assert_eq!(traced(&r, &trace).1, 1);
        assert_ne!(saved(&tree)["verdict"]["left"], serde_json::json!([]));
        assert_eq!(traced(&r, &trace).1, 0, "warm {policy}");
        fs::write(&rules, "/.godot/\n").unwrap();
        let (out, count) = traced(&r, &trace);
        assert!(
            count > 0,
            "changed {policy} requires a fresh verdict: {out}"
        );
        if policy == "local" {
            r.assert_kept(&tree, &out);
            assert_eq!(
                saved(&tree)["verdict"]["left"],
                serde_json::json!([".gitignore"])
            );
        } else {
            r.assert_removed(&tree, &out);
        }
    }
}

#[test]
fn restored_executable_mode_or_content_invalidates_a_proven_dirty_cache() {
    for change in ["mode", "content"] {
        let r = Repo::new();
        let tree = r.detached("kept");
        let tracked = tree.join("sim/world.gd");
        let original = fs::read(&tracked).unwrap();
        let permissions = fs::metadata(&tracked).unwrap().permissions();
        if change == "mode" {
            fs::set_permissions(
                &tracked,
                fs::Permissions::from_mode(permissions.mode() | 0o111),
            )
            .unwrap();
        } else {
            fs::write(&tracked, "extends Something!\n").unwrap();
        }
        let dirty_time = fs::metadata(&tracked).unwrap().modified().unwrap();
        let trace = r.dir.join("trace.jsonl");
        assert_eq!(traced(&r, &trace).1, 1);
        assert_eq!(
            saved(&tree)["verdict"]["left"],
            serde_json::json!(["sim/world.gd"])
        );
        assert_eq!(traced(&r, &trace).1, 0);
        if change == "mode" {
            fs::set_permissions(&tracked, permissions).unwrap();
        } else {
            fs::write(&tracked, original).unwrap();
            crate::set_time(&tracked, dirty_time);
            assert_eq!(
                fs::metadata(&tracked).unwrap().modified().unwrap(),
                dirty_time
            );
        }
        let (out, count) = traced(&r, &trace);
        assert!(
            count > 0,
            "restored {change} requires a fresh verdict: {out}"
        );
        r.assert_removed(&tree, &out);
    }
}

#[test]
fn unproven_configuration_rescans_without_losing_positive_retention_reasons() {
    for policy in [
        "conditional",
        "worktree",
        "attributes",
        "fsmonitor",
        "split-index",
    ] {
        let r = Repo::new();
        let tree = r.detached("kept");
        write(&tree, "sim/new.gd", "keep this source\n");
        let trace = r.dir.join("trace.jsonl");
        assert_eq!(traced(&r, &trace).1, 1);
        assert_eq!(
            saved(&tree)["verdict"]["left"],
            serde_json::json!(["sim/new.gd"])
        );
        assert_eq!(traced(&r, &trace).1, 0);
        match policy {
            "conditional" => {
                let included = r.dir.join("conditional-config");
                fs::write(&included, "[core]\nfilemode = false\n").unwrap();
                let admin = git(&tree, &["rev-parse", "--absolute-git-dir"]);
                let key = format!("includeIf.gitdir:{admin}/.path");
                git(&r.root, &["config", &key, included.to_str().unwrap()]);
            }
            "worktree" => {
                git(&r.root, &["config", "extensions.worktreeConfig", "true"]);
            }
            "fsmonitor" => {
                git(&r.root, &["config", "core.fsmonitor", "false"]);
            }
            "split-index" => {
                git(&tree, &["update-index", "--split-index"]);
            }
            _ => {
                write(&r.dir, ".config/git/attributes", "*.gd text\n");
            }
        }
        for _ in 0..2 {
            let (out, count) = traced(&r, &trace);
            assert_eq!(count, 1, "{policy} requires fresh interpretation: {out}");
            r.assert_kept(&tree, &out);
            assert!(out.contains("sim/new.gd"), "{policy}: {out}");
        }
    }
}

#[test]
fn a_candidate_using_another_common_policy_declines_cache_reuse() {
    let r = Repo::new();
    let tree = r.detached("kept");
    write(&tree, "sim/new.gd", "retained source\n");
    let trace = r.dir.join("trace.jsonl");
    assert_eq!(traced(&r, &trace).1, 1);
    assert_eq!(traced(&r, &trace).1, 0);
    let other = r.dir.join("other-common");
    let copied = Command::new("cp")
        .arg("-R")
        .arg(r.root.join(".git"))
        .arg(&other)
        .status_locked()
        .unwrap();
    assert!(copied.success(), "copy common repository: {copied}");
    let admin = r.root.join(".git/worktrees/kept");
    fs::write(admin.join("commondir"), format!("{}\n", other.display())).unwrap();
    for _ in 0..2 {
        let (out, count) = traced(&r, &trace);
        assert_eq!(
            count, 1,
            "a different common config is outside this pass proof: {out}"
        );
        assert!(out.contains("sim/new.gd"), "{out}");
        r.assert_kept(&tree, &out);
    }
}
