//! The sweep's rules for finished work (tools/worktree-sweep-on-done): a task still live keeps a
//! worktree its session or its launch is tied to; regenerable output goes with it; and a
//! task just done or cancelled (`--task`) has its closed sessions' worktrees go without the window.

use crate::common::{GONE, Seed};
use crate::{Repo, git, text, write};
use std::path::Path;
use symphony_board::sweep::{Finished, swept};
use symphony_board::{NewLaunch, NewTask};

/// `handle` launched on `slug` in `wt` as `role`, its pane closed if `closed`.
fn launched(r: &Repo, slug: &str, handle: &str, role: &str, wt: &Path, closed: bool) {
    let store = &r.board.store;
    store
        .record_launch(&NewLaunch {
            worktree: wt.to_string_lossy().into_owned(),
            pane: "w1:p1".into(),
            ..NewLaunch::new(slug, handle, role)
        })
        .unwrap();
    if closed {
        store
            .close_task_session(slug, handle, "idle", "just tasks restore")
            .unwrap();
    }
}

/// A session bound to no worktree.
fn unbound(r: &Repo) -> String {
    r.board.session("worker")
}

// -- a task still live keeps the worktrees tied to it ------------------------------------------

#[test]
fn a_lost_session_bound_here_holding_live_work_keeps_it() {
    // Its binding no longer counts (lost), but its task can still be restored there.
    let r = Repo::new();
    let wt = r.detached("lost");
    let handle = r.bind(&wt);
    r.task("sim/jump");
    r.board.store.claim("sim/jump", &handle).unwrap();
    r.board.advance(GONE);
    assert_eq!(r.board.health(&handle).as_str(), "lost");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        said.contains(&format!(
            "task sim/jump is in_progress, held by {handle}, whose session is bound here"
        )),
        "{said}"
    );
}

#[test]
fn a_session_that_left_holds_nothing_so_its_worktree_goes() {
    let r = Repo::new();
    let wt = r.detached("left-work");
    let handle = r.bind(&wt);
    r.task("sim/jump");
    r.board.store.claim("sim/jump", &handle).unwrap();
    r.board.store.leave(&handle).unwrap(); // its roles are released: the task is ready for another
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_live_task_launched_here_keeps_it_whoever_holds_it() {
    let r = Repo::with(Seed::default().task("sim/jump", "review"));
    let wt = r.detached("reviewer");
    let handle = unbound(&r);
    launched(&r, "sim/jump", &handle, "reviewer", &wt, true);
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        said.contains(&format!(
            "task sim/jump is review, launched here for {handle}"
        )),
        "{said}"
    );
}

#[test]
fn once_the_task_launched_here_is_done_the_worktree_goes() {
    let r = Repo::with(Seed::default().task("sim/jump", "done"));
    let wt = r.detached("reviewer");
    launched(&r, "sim/jump", &unbound(&r), "reviewer", &wt, true);
    r.assert_removed(&wt, &r.sweep());
}

// -- a finished worktree's output goes with it ------------------------------------------------

const OUTPUT: [&str; 9] = [
    ".godot/editor/cache",
    ".venv/lib/x.py",
    ".target/debug/tasks",
    ".target.seed.123/debug/deps/libwork.rlib",
    "symphony/.target/debug/tasks",
    "symphony/.target.clear.456/debug/tasks",
    "symphony/build/client.js",
    ".codex/hooks.json",
    ".codex/hooks/callback",
];

#[test]
fn a_finished_worktrees_output_goes_with_it() {
    // A done task's, a cancelled task's and a worktree with no task: one sweep lets each go.
    let r = Repo::with(
        Seed::default()
            .task("sim/landed", "done")
            .task("sim/dropped", "cancelled"),
    );
    let trees = [
        r.merged("landed", "sim/landed"),
        r.merged("dropped", "sim/dropped"),
        r.detached("reviewer"),
    ];
    for wt in &trees {
        for path in OUTPUT {
            write(wt, path, "x\n");
        }
    }
    let said = r.sweep();
    for wt in &trees {
        r.assert_removed(wt, &said);
    }
}

#[test]
fn output_is_not_cleaned_when_the_final_process_scan_keeps_the_tree() {
    let r = Repo::new();
    let wt = r.detached("busy");
    for file in [".target/debug/tasks", ".codex/hooks.json"] {
        write(&wt, file, "active output\n");
    }
    let result = r.sweep_with(|o| o.occupied = Box::new(|_| Some(true)));
    r.assert_kept(&wt, &text(&result));
    for file in [".target/debug/tasks", ".codex/hooks.json"] {
        assert_eq!(
            std::fs::read_to_string(wt.join(file)).unwrap(),
            "active output\n"
        );
    }
}

#[test]
fn artifacts_of_finished_and_unassigned_worktrees_are_set_aside_and_the_worktree_goes() {
    let r = Repo::with(
        Seed::default()
            .task("sim/landed", "done")
            .task("sim/dropped", "cancelled"),
    );
    // A done task's, a cancelled task's and the branch of no task, each with evidence of its own.
    let trees = ["landed", "dropped", "unassigned"].map(|name| {
        let wt = r.merged(name, &format!("sim/{name}"));
        write(
            &wt,
            "artifacts/solar-swift/x.log",
            &format!("keep the evidence of {name}\n"),
        );
        (name, wt)
    });
    let said = r.sweep();
    for (name, wt) in &trees {
        r.assert_removed(wt, &said);
        let kept_in = swept(&r.root, "artifacts").join(name);
        assert!(
            said.contains(&format!("its artifacts/ moved to {}", kept_in.display())),
            "{name}: {said}"
        );
        assert_eq!(
            std::fs::read_to_string(kept_in.join("solar-swift/x.log")).unwrap(),
            format!("keep the evidence of {name}\n")
        );
    }
}

#[test]
fn a_second_worktree_of_the_same_name_sets_its_artifacts_aside_beside_the_first() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    std::fs::create_dir_all(swept(&r.root, "artifacts").join("landed")).unwrap();
    let wt = r.merged("landed", "sim/landed");
    write(&wt, "artifacts/solar-swift/x.log", "the second\n");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert_eq!(
        std::fs::read_to_string(swept(&r.root, "artifacts").join("landed-2/solar-swift/x.log"))
            .unwrap(),
        "the second\n"
    );
}

#[test]
fn tracked_hooks_edits_and_files_outside_the_regenerable_paths_survive() {
    let r = Repo::new();
    write(&r.root, ".codex/hooks.toml", "tracked = true\n");
    git(&r.root, &["add", ".codex/hooks.toml"]);
    git(&r.root, &["commit", "-qm", "tracked hook source"]);
    let files = [
        ".codex/hooks.toml",
        ".codex/config.toml",
        "symphony/building/new.rs",
        ".target-backup/source.rs",
    ];
    let trees: Vec<_> = files
        .iter()
        .map(|file| {
            let wt = r.detached(&format!("kept-{}", file.replace('/', "-")));
            write(&wt, file, "keep this source\n");
            wt
        })
        .collect();
    let said = r.sweep();
    for (&file, wt) in files.iter().zip(&trees) {
        r.assert_kept(wt, &said);
        assert!(said.contains(file), "{said}");
    }
}

#[test]
fn a_worktree_whose_task_is_not_finished_keeps_its_output_and_names_it() {
    let r = Repo::with(Seed::default().task("sim/parked", "backlog"));
    // A sweep retains a parked task's artifacts as well as a finished task's.
    write(
        &r.root,
        "tests/probe_test.gd",
        "const OUT = \"res://artifacts/solar-swift\"\n",
    );
    git(&r.root, &["add", "tests/probe_test.gd"]);
    git(&r.root, &["commit", "-qm", "test output contract"]);
    let wt = r.merged("parked", "sim/parked");
    write(&wt, "artifacts/solar-swift/probe.json", "x\n");
    write(&wt, "artifacts/slynx-verify.log", "x\n"); // a log goes whatever the task
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    let kept = said
        .lines()
        .find(|l| l.starts_with(&format!("kept {}: merged, but", wt.display())))
        .unwrap_or_else(|| panic!("{said}"));
    assert!(kept.contains("artifacts/solar-swift/"), "{said}");
    assert!(!kept.contains("slynx-verify.log"), "{said}");
}

#[test]
fn a_finished_worktree_still_keeps_a_change_or_an_untracked_file() {
    let r = Repo::with(
        Seed::default()
            .task("sim/untracked", "done")
            .task("sim/changed", "done"),
    );
    let trees = [("untracked", "notes.txt"), ("changed", "sim/world.gd")].map(|(name, path)| {
        let wt = r.merged(name, &format!("sim/{name}"));
        write(&wt, path, "not committed\n");
        write(&wt, "artifacts/solar-swift/probe.json", "x\n");
        (wt, path)
    });
    let said = r.sweep();
    for (wt, path) in &trees {
        r.assert_kept(wt, &said);
        let kept = said
            .lines()
            .find(|l| l.starts_with(&format!("kept {}: ", wt.display())))
            .unwrap_or_else(|| panic!("{said}"));
        assert!(kept.contains(path), "{said}");
        // Its artifacts stay where they are: they are set aside only for a removal.
        assert!(wt.join("artifacts/solar-swift/probe.json").is_file());
    }
    assert!(!swept(&r.root, "artifacts").exists(), "{said}");
}

#[test]
fn a_file_managers_metadata_left_untracked_goes_with_a_merged_worktree() {
    let r = Repo::with(
        Seed::default()
            .task("sim/opened", "done")
            .task("sim/noted", "done"),
    );
    // A project whose ignore file names none of them: in its worktrees each is untracked.
    let ignore = std::fs::read_to_string(r.root.join(".gitignore")).unwrap();
    write(&r.root, ".gitignore", &ignore.replace(".DS_Store\n", ""));
    git(
        &r.root,
        &["commit", "-qam", "ignore no file manager's metadata"],
    );
    let opened = r.merged("opened", "sim/opened");
    for file in [".DS_Store", "sim/.DS_Store", "Thumbs.db", "sim/desktop.ini"] {
        write(&opened, file, "written by a file manager\n");
    }
    assert!(git(&opened, &["status", "--porcelain"]).contains("?? .DS_Store"));
    // Beside one, a file of somebody's own still keeps its worktree, and is the one named.
    let noted = r.merged("noted", "sim/noted");
    write(&noted, ".DS_Store", "written by a file manager\n");
    write(&noted, "sim/notes.txt", "not committed\n");
    let said = r.sweep();
    r.assert_removed(&opened, &said);
    r.assert_kept(&noted, &said);
    assert!(said.contains("sim/notes.txt"), "{said}");
    assert!(!said.contains(".DS_Store"), "{said}");
}

// -- a task just done or cancelled: its closed sessions' worktrees go at once (`--task`) -------

/// sim/jump done, `handle` live and bound to its merged worktree, launched there and closed (or not).
fn finished_jump(closed: bool) -> (Repo, std::path::PathBuf, String) {
    let r = Repo::with(Seed::default().task("sim/jump", "done"));
    let wt = r.merged("sim-jump", "sim/jump");
    let handle = r.bind(&wt);
    launched(&r, "sim/jump", &handle, "executor", &wt, closed);
    (r, wt, handle)
}

#[test]
fn the_closed_sessions_worktree_of_a_finished_task_goes_without_the_window() {
    let (r, wt, handle) = finished_jump(true);
    // Another task's worktree, its session closed too: the finish of sim/jump does not reach it.
    r.board
        .store
        .create(&NewTask::new("sim/dock", "Dock", "owner"))
        .unwrap();
    let dock = r.merged("sim-dock", "sim/dock");
    launched(&r, "sim/dock", &unbound(&r), "executor", &dock, true);
    let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
    assert!(finished.handles.contains(&handle));
    // Touched just now and bound to a live session: only the finish lets it go.
    let said = text(&r.sweep_now(|_| {}));
    r.assert_kept(&wt, &said);
    r.assert_kept(&dock, &said);
    let said = text(&r.sweep_now(|o| o.finished = finished));
    r.assert_removed(&wt, &said);
    // A worktree of another task keeps its window.
    r.assert_kept(&dock, &said);
}

#[test]
fn an_open_session_keeps_its_worktree_when_its_task_finishes() {
    // A busy pane is not closed, so its live session still counts, past the window too.
    let (r, wt, _) = finished_jump(false);
    let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
    assert_eq!(
        finished,
        Finished {
            branches: ["sim/jump", "sim-jump"].map(String::from).into(),
            ..Finished::default()
        },
        "only its branches: no closed launch, and its executor is live"
    );
    r.assert_kept(&wt, &text(&r.sweep_with(|o| o.finished = finished)));
}

#[test]
fn an_unmerged_or_dirty_worktree_of_a_finished_task_stays() {
    let r = Repo::with(
        Seed::default()
            .task("sim/unmerged", "done")
            .task("sim/dirty", "done"),
    );
    for slug in ["sim/unmerged", "sim/dirty"] {
        let wt = r.add(&slug.replace('/', "-"), Some(slug), "main");
        r.commit(&wt, &format!("extends RefCounted\n# {slug}\n"));
        if slug == "sim/dirty" {
            git(&r.root, &["merge", "-q", "--ff-only", slug]);
            write(&wt, "left.txt", "not committed");
        }
        let handle = r.bind(&wt);
        launched(&r, slug, &handle, "executor", &wt, true);
        let finished = Finished::of(&r.board.store, slug).unwrap();
        assert!(finished.handles.contains(&handle), "{slug}");
        r.assert_kept(&wt, &text(&r.sweep_now(|o| o.finished = finished)));
    }
}

#[test]
fn a_task_not_finished_hurries_no_worktree() {
    let r = Repo::with(Seed::default().task("sim/jump", "in_progress"));
    let wt = r.merged("sim-jump", "sim/jump");
    launched(&r, "sim/jump", &r.bind(&wt), "executor", &wt, true);
    assert_eq!(
        Finished::of(&r.board.store, "sim/jump").unwrap(),
        Finished::default()
    );
}

#[test]
fn a_closed_session_of_a_task_under_it_counts_for_the_one_that_finishes() {
    let r = Repo::with(Seed::default().task("sim/jump", "done"));
    r.board
        .store
        .create(&NewTask {
            parent: Some("sim/jump".into()),
            instruction: true,
            ..NewTask::new("sim/jump/do", "Do", "owner")
        })
        .unwrap();
    let wt = r.detached("do");
    let handle = r.bind(&wt);
    launched(&r, "sim/jump/do", &handle, "executor", &wt, true);
    let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
    assert!(finished.worktrees.contains(&wt.canonicalize().unwrap()));
    assert!(finished.handles.contains(&handle));
}
