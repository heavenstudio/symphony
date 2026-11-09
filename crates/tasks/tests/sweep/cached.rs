//! Repeated passes count real status invocations, rather than inferring reuse from elapsed time.

use crate::{Repo, git, text, write};
use std::fs;
use std::path::Path;
use std::process::Command;
use symphony_board::sys::Spawning as _;

fn traced(r: &Repo, trace: &Path) -> (String, usize) {
    let _ = fs::remove_file(trace);
    let done = Command::new(env!("CARGO_BIN_EXE_tasks"))
        .args(["sweep", "--recent-hours", "0"])
        .env_clear()
        .envs(r.vars())
        .env("GIT_TRACE2_EVENT", trace)
        .env("TASKS_SWEEP", "off")
        .current_dir(&r.root)
        .output_locked()
        .unwrap();
    let out = String::from_utf8(done.stdout).unwrap();
    assert!(
        done.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let count = fs::read_to_string(trace)
        .unwrap()
        .lines()
        .filter(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            event["event"] == "start"
                && event["argv"]
                    .as_array()
                    .is_some_and(|args| args.iter().any(|arg| arg == "status"))
        })
        .count();
    (out, count)
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
fn a_cached_clean_verdict_never_authorizes_removal_after_a_nested_artifact() {
    let r = Repo::new();
    let tree = r.detached("kept");
    fs::create_dir_all(tree.join("artifacts/solar-swift")).unwrap();
    let first = r.sweep_with(|o| o.occupied = Box::new(|_| Some(true)));
    r.assert_kept(&tree, &text(&first));
    write(&tree, "artifacts/solar-swift/x.log", "keep this evidence\n");
    let next = r.sweep_now(|_| {});
    r.assert_kept(&tree, &text(&next));
    assert_eq!(
        fs::read_to_string(tree.join("artifacts/solar-swift/x.log")).unwrap(),
        "keep this evidence\n"
    );
}
