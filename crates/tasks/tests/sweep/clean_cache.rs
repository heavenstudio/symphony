//! Complete clean proofs and fresh native guards, measured in isolated child environments.

use crate::children::Child as _;
use crate::{Repo, cached, git, text, write};
use std::cell::{Cell, RefCell};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Instant;
use symphony_board::sys::Spawning as _;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// No process-global environment mutation in the parallel test runner. Native tracing and policy
/// identity belong to a private child, which invokes exactly this test and owns all its fixtures.
fn isolated(test: &str) -> bool {
    isolated_with("clean_cache", test, |_, _| {})
}

/// Run exactly one test in its own environment, with optional private native-command setup.
pub(super) fn isolated_with(
    module: &str,
    test: &str,
    setup: impl FnOnce(&Path, &mut Command),
) -> bool {
    if std::env::var_os("SYMPHONY_CLEAN_PROOF_CHILD").is_some() {
        return false;
    }
    let path = std::env::temp_dir().join(format!(
        "symphony-clean-proof-{}-{module}-{test}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    let scratch = Scratch(path);
    let mut child = Command::of(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            &format!("{module}::{test}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .cleared()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &scratch.0)
        // The child starts the tests' PostgreSQL, which on macOS refuses to start with no locale
        // ("postmaster became multithreaded during startup").
        .env("LC_ALL", "C")
        .env("SYMPHONY_CLEAN_PROOF_CHILD", "1")
        .env("GIT_TRACE2_EVENT", scratch.0.join("trace.jsonl"));
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        child.env("LLVM_PROFILE_FILE", profile);
    }
    setup(&scratch.0, &mut child);
    let done = child.output_locked().unwrap();
    let transcript = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    eprintln!("{transcript}");
    assert!(done.status.success(), "isolated {test}: {}", done.status);
    let selected = format!("{module}::{test} ... ok");
    assert!(
        transcript.contains(&selected)
            && transcript.contains("test result: ok. 1 passed; 0 failed;"),
        "isolated invocation did not complete its one selected test: {transcript}"
    );
    true
}

#[derive(Default)]
struct TraceCursor {
    offset: u64,
    count: usize,
}

impl TraceCursor {
    fn calls(&mut self) -> usize {
        let mut file = File::open(trace()).unwrap();
        file.seek(SeekFrom::Start(self.offset)).unwrap();
        let mut tail = String::new();
        file.read_to_string(&mut tail).unwrap();
        self.offset += tail.len() as u64;
        for line in tail.lines() {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            if event["event"] == "start"
                && event["argv"]
                    .as_array()
                    .is_some_and(|args| args.iter().any(|arg| arg == "status"))
            {
                self.count += 1;
            }
        }
        self.count
    }
}

/// The native trace file belongs only to the isolated child running this test.
pub(super) fn trace() -> PathBuf {
    std::env::var_os("GIT_TRACE2_EVENT").unwrap().into()
}

fn cli(r: &Repo) -> (String, usize) {
    cached::traced_with(r, &trace(), |command| {
        // The API and CLI must see the same private policy HOME for real cache hits.
        command.env("HOME", std::env::var_os("HOME").unwrap());
    })
}

fn admin(r: &Repo, tree: &Path) -> PathBuf {
    r.root
        .join(".git/worktrees")
        .join(tree.file_name().unwrap())
}

fn empty_cache(r: &Repo, tree: &Path) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(admin(r, tree).join("symphony-sweep-verdict.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(value["verdict"]["left"], serde_json::json!([]));
    assert!(
        value["inventory"]["entries"]
            .as_array()
            .is_some_and(|rows| rows.len() >= 5),
        "real complete proof: {value}"
    );
    value
}

fn extend_trees(r: &Repo, trees: &mut Vec<PathBuf>, count: usize) {
    let first = trees[0].clone();
    let source = admin(r, &first);
    for number in trees.len()..count {
        let tree = r.dir.join(format!("clean-{number}"));
        let target = admin(r, &tree);
        fs::create_dir_all(tree.join("sim")).unwrap();
        fs::create_dir_all(&target).unwrap();
        for file in ["HEAD", "index", "commondir"] {
            fs::copy(source.join(file), target.join(file)).unwrap();
        }
        fs::write(
            target.join("gitdir"),
            format!("{}\n", tree.join(".git").display()),
        )
        .unwrap();
        fs::write(tree.join(".git"), format!("gitdir: {}\n", target.display())).unwrap();
        for file in [".gitignore", "sim/world.gd"] {
            fs::copy(first.join(file), tree.join(file)).unwrap();
        }
        trees.push(tree);
    }
    assert_eq!(r.worktrees().len(), count + 1);
    assert_eq!(trees.len(), count);
}

/// Install a real writer only after the verdict lookup. All candidates reach one mandatory
/// removal-status guard, then the native HEAD guard refuses. Counts at the existing callbacks
/// distinguish lookup from guard calls without instrumenting or replacing production commands.
fn late_writer_pass(r: &Repo, count: usize) -> (usize, usize) {
    fs::write(trace(), "").unwrap();
    let lookups = Rc::new(Cell::new(0));
    let guards = Rc::new(Cell::new(0));
    let checked = Rc::new(Cell::new(0));
    let prior = Cell::new(0);
    let cursor = RefCell::new(TraceCursor::default());
    // The owned pause closure updates counts the caller reads after the sweep.
    let lookup_count = Rc::clone(&lookups);
    let guard_count = Rc::clone(&guards);
    let checked_count = Rc::clone(&checked);
    r.sweep_now(|options| {
        options.recent_hours = 0.0;
        options.pause = Box::new(move |stage, path| {
            let calls = cursor.borrow_mut().calls();
            match stage {
                "remove" => {
                    lookup_count.set(lookup_count.get() + calls - prior.get());
                    prior.set(calls);
                    let target = Path::new(path)
                        .parent()
                        .unwrap()
                        .join("repo/.git/worktrees")
                        .join(Path::new(path).file_name().unwrap());
                    fs::write(target.join("HEAD.lock"), "private writer bytes\n").unwrap();
                }
                "checked" => {
                    guard_count.set(guard_count.get() + calls - prior.get());
                    prior.set(calls);
                    checked_count.set(checked_count.get() + 1);
                }
                _ => {}
            }
        });
    });
    assert_eq!(checked.get(), count, "all first removal guards reached");
    assert_eq!(cached::status_calls(&trace()), lookups.get() + guards.get());
    (lookups.get(), guards.get())
}

#[test]
fn previously_empty_300_trees_reuse_complete_proofs_and_known_blockers_skip_whole_pass_status() {
    if isolated(
        "previously_empty_300_trees_reuse_complete_proofs_and_known_blockers_skip_whole_pass_status",
    ) {
        return;
    }
    let r = Repo::new();
    // Removal refreshes the whole worktree listing per candidate. Sixty exercises the same
    // cache and guard counts without paying that quadratic cost at the CLI's scale.
    const REMOVAL_TREES: usize = 60;
    let mut trees = vec![r.detached("clean-0")];
    extend_trees(&r, &mut trees, REMOVAL_TREES);
    for pass in 0..2 {
        if pass == 1 {
            for tree in &trees {
                fs::remove_file(admin(&r, tree).join("HEAD.lock")).unwrap();
            }
        }
        let began = Instant::now();
        let (lookups, guards) = late_writer_pass(&r, REMOVAL_TREES);
        eprintln!(
            "WHOLE SWEEP: {REMOVAL_TREES} clean, late writer, pass {pass}: {:?}, lookup {lookups}, first guards {guards}, second guards 0, total {}",
            began.elapsed(),
            lookups + guards
        );
        assert_eq!(lookups, if pass == 0 { REMOVAL_TREES } else { 0 });
        assert_eq!(guards, REMOVAL_TREES);
        for tree in &trees {
            empty_cache(&r, tree);
            assert!(tree.is_dir());
            assert_eq!(
                fs::read_to_string(admin(&r, tree).join("HEAD.lock")).unwrap(),
                "private writer bytes\n"
            );
        }
    }
    // Keep the CLI passes at 300, with both the real cached survivors and fresh uncached trees.
    // Known blockers must bypass verdict lookup for either kind of clean candidate.
    extend_trees(&r, &mut trees, 300);
    for tree in &trees[REMOVAL_TREES..] {
        assert!(!admin(&r, tree).join("symphony-sweep-verdict.json").exists());
        fs::write(admin(&r, tree).join("HEAD.lock"), "private writer bytes\n").unwrap();
    }
    for pass in 0..2 {
        let began = Instant::now();
        let (out, calls) = cli(&r);
        eprintln!(
            "WHOLE CLI: 300 clean existing HEAD writers, pass {pass}: {:?}, status {calls}, lookups/guards reached 0",
            began.elapsed()
        );
        assert_eq!(calls, 0, "{out}");
        assert_eq!(
            out.matches("HEAD writer lock exists:").count(),
            300,
            "{out}"
        );
        for tree in &trees {
            assert_eq!(
                fs::read_to_string(admin(&r, tree).join("HEAD.lock")).unwrap(),
                "private writer bytes\n"
            );
        }
        for tree in &trees[..REMOVAL_TREES] {
            empty_cache(&r, tree);
        }
        for tree in &trees[REMOVAL_TREES..] {
            assert!(!admin(&r, tree).join("symphony-sweep-verdict.json").exists());
        }
    }
    let held_path = r.dir.join("held-removal");
    fs::write(&held_path, "private removal bytes\n").unwrap();
    let held = File::options().write(true).open(&held_path).unwrap();
    held.try_lock().unwrap();
    for (number, tree) in trees.iter().enumerate() {
        fs::remove_file(admin(&r, tree).join("HEAD.lock")).unwrap();
        if number < REMOVAL_TREES {
            fs::remove_file(admin(&r, tree).join("symphony-sweep-remove.lock")).unwrap();
        }
        fs::hard_link(
            &held_path,
            admin(&r, tree).join("symphony-sweep-remove.lock"),
        )
        .unwrap();
    }
    for pass in 0..2 {
        let began = Instant::now();
        let (out, calls) = cli(&r);
        eprintln!(
            "WHOLE CLI: 300 clean existing removal locks, pass {pass}: {:?}, status {calls}, lookups/guards reached 0",
            began.elapsed()
        );
        assert_eq!(calls, 0, "{out}");
        assert_eq!(
            out.matches("cannot take worktree removal lock").count(),
            300,
            "{out}"
        );
        for tree in &trees {
            assert!(tree.is_dir());
        }
        for tree in &trees[..REMOVAL_TREES] {
            empty_cache(&r, tree);
        }
        for tree in &trees[REMOVAL_TREES..] {
            assert!(!admin(&r, tree).join("symphony-sweep-verdict.json").exists());
        }
        assert_eq!(
            fs::read_to_string(&held_path).unwrap(),
            "private removal bytes\n"
        );
    }
}

#[test]
fn lock_disappearance_rechecks_nested_source_and_clean_cache_keeps_both_final_status_guards() {
    if isolated(
        "lock_disappearance_rechecks_nested_source_and_clean_cache_keeps_both_final_status_guards",
    ) {
        return;
    }
    let r = Repo::new();
    let tree = r.detached("clean");
    for pass in 0..2 {
        fs::write(trace(), "").unwrap();
        let out = text(&r.sweep_now(|o| {
            o.recent_hours = 0.0;
            o.occupied = Box::new(|_| Some(true));
        }));
        r.assert_kept(&tree, &out);
        empty_cache(&r, &tree);
        assert_eq!(
            cached::status_calls(&trace()),
            if pass == 0 { 3 } else { 2 },
            "both final guards remain: {out}"
        );
    }
    let head = admin(&r, &tree).join("HEAD.lock");
    fs::write(&head, "private HEAD bytes\n").unwrap();
    write(&tree, "sim/new.gd", "source must survive\n");
    let (out, calls) = cli(&r);
    assert_eq!(calls, 0);
    assert!(out.contains("HEAD writer lock exists:"), "{out}");
    assert_eq!(fs::read_to_string(&head).unwrap(), "private HEAD bytes\n");
    fs::remove_file(&head).unwrap();
    let (out, calls) = cli(&r);
    assert_eq!(calls, 1, "disappearance exposes changed proof: {out}");
    assert!(out.contains("sim/new.gd"), "{out}");
    assert_eq!(
        cached::saved(&tree)["verdict"]["left"],
        serde_json::json!(["sim/new.gd"])
    );
    assert_eq!(cli(&r).1, 0);
    r.assert_kept(&tree, &out);
    assert_eq!(
        fs::read_to_string(tree.join("sim/new.gd")).unwrap(),
        "source must survive\n"
    );
    let removal = admin(&r, &tree).join("symphony-sweep-remove.lock");
    fs::write(&removal, "held removal bytes\n").unwrap();
    let held = File::options().write(true).open(&removal).unwrap();
    held.try_lock().unwrap();
    write(&tree, "sim/new.gd", "newer source\n");
    let (out, calls) = cli(&r);
    assert_eq!(calls, 0);
    assert!(out.contains("cannot take worktree removal lock"), "{out}");
    assert_eq!(
        fs::read_to_string(&removal).unwrap(),
        "held removal bytes\n"
    );
    drop(held);
    let (out, calls) = cli(&r);
    assert_eq!(calls, 1, "released lock exposes changed source: {out}");
    assert!(out.contains("sim/new.gd"), "{out}");
    assert_eq!(
        fs::read_to_string(&removal).unwrap(),
        "held removal bytes\n"
    );
}

#[test]
fn clean_proofs_invalidate_for_tracked_edits_and_policy_changes_including_output_namespaces() {
    if isolated(
        "clean_proofs_invalidate_for_tracked_edits_and_policy_changes_including_output_namespaces",
    ) {
        return;
    }
    for change in [
        "content", "mode", "delete", "rename", "ignore", "config", "output",
    ] {
        let r = Repo::new();
        git(&r.root, &["config", "core.filemode", "true"]);
        if change == "output" {
            write(&r.root, ".target/tracked", "tracked source\n");
            git(&r.root, &["add", "-f", ".target/tracked"]);
            git(&r.root, &["commit", "-qm", "tracked namespace fixture"]);
        }
        let tree = r.detached("clean");
        r.sweep_now(|o| {
            o.recent_hours = 0.0;
            o.occupied = Box::new(|_| Some(true));
        });
        empty_cache(&r, &tree);
        let source = tree.join("sim/world.gd");
        match change {
            "content" => {
                let when = fs::metadata(&source).unwrap().modified().unwrap();
                write(&tree, "sim/world.gd", "extends Something!\n");
                crate::set_time(&source, when);
                assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), when);
            }
            "mode" => fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap(),
            "delete" => fs::remove_file(&source).unwrap(),
            "rename" => fs::rename(&source, tree.join("sim/renamed.gd")).unwrap(),
            "ignore" => write(&tree, "sim/.gitignore", "*.tmp\n"),
            "config" => {
                git(&r.root, &["config", "core.ignorecase", "false"]);
                write(&tree, "sim/new.gd", "new source\n");
            }
            _ => write(&tree, ".target/tracked", "changed tracked source\n"),
        }
        let (out, calls) = cli(&r);
        assert_eq!(calls, 1, "fresh {change}: {out}");
        r.assert_kept(&tree, &out);
        let want = match change {
            "ignore" => "sim/.gitignore",
            "config" => "sim/new.gd",
            "output" => ".target/tracked",
            _ => "sim/world.gd",
        };
        assert!(out.contains(want), "{change} reason: {out}");
    }
}

#[test]
fn oversized_empty_directory_proof_rescans_without_saving_an_empty_cache() {
    if isolated("oversized_empty_directory_proof_rescans_without_saving_an_empty_cache") {
        return;
    }
    let r = Repo::new();
    let tree = r.detached("bounded");
    // Empty directories do not appear in native status, but can later receive new source files.
    // This real filesystem exceeds the supported inventory instead of forging a saved cache.
    for number in 0..8193 {
        fs::create_dir(tree.join(format!("empty-{number}"))).unwrap();
    }
    let cache = admin(&r, &tree).join("symphony-sweep-verdict.json");
    assert!(
        !cache.exists(),
        "unexpected preexisting proof {}",
        cache.display()
    );
    for _ in 0..2 {
        fs::write(trace(), "").unwrap();
        let out = text(&r.sweep_now(|options| {
            options.recent_hours = 0.0;
            options.occupied = Box::new(|_| Some(true));
        }));
        assert_eq!(
            cached::status_calls(&trace()),
            4,
            "fresh verdict, bounded retry and both final guards: {out}"
        );
        assert!(
            !cache.exists(),
            "unproven emptiness was persisted at {}",
            cache.display()
        );
        r.assert_kept(&tree, &out);
    }
}
