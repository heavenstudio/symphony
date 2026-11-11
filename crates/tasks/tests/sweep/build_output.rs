//! The build output of finished work (tools/one-build-folder-for-worktrees): a worktree the
//! worktree sweep keeps loses its build folders once every task it is tied to is done, its HEAD is
//! merged work and no agent there beat within the hour; never a branch with no commits ahead of
//! main, a task in progress, an agent that beat within the hour, or a process working inside it,
//! as read again right before the clear; never a folder a Cargo holds the lock of, and never one
//! reached through a link.

use crate::common::{NOW, Seed};
use crate::{Repo, dead_pid, git, started, text, write};
use std::cell::Cell;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use symphony_board::sweep::Sweep;
use symphony_board::sweep::trigger::now;

/// Two hours on the board's clock: past the hour the build output waits for.
const LATER: f64 = NOW + 7200.0;

/// `wt` built, its build folders holding a compiled file each, as Cargo leaves them.
fn built(wt: &Path) -> [PathBuf; 2] {
    write(wt, ".target/debug/deps/libwork.rlib", "compiled\n");
    write(wt, "symphony/.target/debug/deps/libwork.rlib", "compiled\n");
    [wt.join(".target"), wt.join("symphony/.target")]
}

/// A worktree on a new `branch` from main with a commit on it, merged into main by a merge commit,
/// as work lands here.
fn landed(r: &Repo, name: &str, branch: &str) -> PathBuf {
    let wt = r.add(name, Some(branch), "main");
    r.commit(&wt, &format!("extends RefCounted\n# {name}\n"));
    let message = format!("Merge branch '{branch}'");
    git(&r.root, &["merge", "-q", "--no-ff", "-m", &message, branch]);
    wt
}

/// Kept whole by the worktree sweep, as a Claude Code agent's worktree is while its orchestrator
/// runs: locked by a live process, this one.
fn held(r: &Repo, wt: &Path) {
    let pid = i64::from(std::process::id());
    r.claude_lock(wt, &pid.to_string(), &started(pid));
}

/// The sweep at `now` on the board's clock, every worktree idle and nobody inside.
fn sweep_at(r: &Repo, now: f64, dry_run: bool) -> Sweep {
    r.sweep_with(|o| {
        o.now = Box::new(move || now);
        o.dry_run = dry_run;
    })
}

/// That `wt` stayed, and its build folders with it.
fn assert_untouched(r: &Repo, wt: &Path, folders: &[PathBuf], done: &Sweep) {
    let said = text(done);
    r.assert_kept(wt, &said);
    assert!(done.cleared.is_empty(), "{said}");
    for folder in folders {
        assert!(folder.join("debug/deps/libwork.rlib").is_file(), "{said}");
    }
}

/// The folders a sweep cleared.
fn cleared(done: &Sweep) -> Vec<PathBuf> {
    done.cleared
        .iter()
        .map(|(folder, _)| folder.clone())
        .collect()
}

/// A worktree whose work landed, its task done, kept whole and built: it and its build folders.
fn finished(r: &Repo) -> (PathBuf, [PathBuf; 2]) {
    let wt = landed(r, "landed", "sim/landed");
    held(r, &wt);
    let folders = built(&wt);
    (wt, folders)
}

/// The sweep at [`LATER`], every worktree idle and nobody inside, calling `pause` with each stage
/// it reaches and the path that stage names, canonical.
fn sweep_pausing(r: &Repo, pause: impl Fn(&str, &Path) + 'static) -> Sweep {
    r.sweep_with(|o| {
        o.now = Box::new(|| LATER);
        o.pause = Box::new(move |stage, what| {
            let what = Path::new(what);
            pause(stage, &what.canonicalize().unwrap_or_else(|_| what.into()));
        });
    })
}

#[test]
fn finished_work_loses_its_build_output_and_keeps_its_worktree() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let head = git(&wt, &["rev-parse", "HEAD"]);
    let why = format!(
        "task sim/landed is done; on sim/landed at {}, merged into main; no agent there for an hour",
        &head[..12]
    );

    let dry = sweep_at(&r, LATER, true);
    assert_eq!(
        dry.cleared,
        folders
            .iter()
            .map(|f| (f.clone(), why.clone()))
            .collect::<Vec<_>>()
    );
    let said = text(&dry);
    assert!(
        said.contains(&format!(
            "would clear build output {} ({why})",
            folders[0].display()
        )) && said.ends_with("; would clear 2 build folders of finished work"),
        "{said}"
    );
    assert!(
        folders.iter().all(|f| f.is_dir()),
        "a dry run removes nothing"
    );

    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    r.assert_kept(&wt, &said);
    assert!(
        said.contains(&format!(
            "cleared build output {} ({why})",
            folders[1].display()
        )) && said.ends_with("; cleared 2 build folders of finished work"),
        "{said}"
    );
    assert!(folders.iter().all(|f| !f.exists()), "{said}");
    assert!(
        wt.join("sim/world.gd").is_file(),
        "the worktree's files stay"
    );
    assert!(
        sweep_at(&r, LATER, false).cleared.is_empty(),
        "nothing is left to clear"
    );
}

#[test]
fn a_branch_with_no_commits_ahead_of_main_is_not_merged_work() {
    // Its task done or not, a branch that never committed is where an agent starts: its HEAD is a
    // commit of main's own line, an ancestor of main like every merged branch's.
    let r = Repo::with(
        Seed::default()
            .task("sim/fresh", "done")
            .task("sim/fast", "done")
            .task("sim/again", "done"),
    );
    let fresh = r.add("fresh", Some("sim/fresh"), "main");

    // One that caught up with main by a fast-forward, after other work landed.
    let fast = r.add("fast", Some("sim/fast"), "main");
    landed(&r, "other", "sim/other");
    git(&fast, &["merge", "-q", "--ff-only", "main"]);

    // One whose earlier work landed, reset to main for its next task, with nothing committed yet.
    let again = landed(&r, "again", "sim/again");
    git(&again, &["reset", "-q", "--hard", "main"]);

    for wt in [&fresh, &fast, &again] {
        held(&r, wt);
    }
    let folders: Vec<[PathBuf; 2]> = [&fresh, &fast, &again].map(|wt| built(wt)).into();
    let done = sweep_at(&r, LATER, false);
    for (wt, folders) in [&fresh, &fast, &again].iter().zip(&folders) {
        assert_untouched(&r, wt, folders, &done);
    }
}

#[test]
fn work_that_never_landed_keeps_its_build_output() {
    // Its task done, but its branch's commit is on no line of main's.
    let r = Repo::with(Seed::default().task("sim/unlanded", "done"));
    let wt = r.add("unlanded", Some("sim/unlanded"), "main");
    r.commit(&wt, "extends RefCounted\n# unlanded\n");
    held(&r, &wt);
    let folders = built(&wt);
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
}

#[test]
fn a_task_in_progress_keeps_the_build_output_of_merged_work() {
    for status in ["in_progress", "review", "blocked", "ready"] {
        let r = Repo::with(Seed::default().task("sim/landed", status));
        let wt = landed(&r, "landed", "sim/landed");
        held(&r, &wt);
        let folders = built(&wt);
        assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
    }
}

#[test]
fn one_task_in_progress_of_two_keeps_it() {
    // Its branch's task is done, but the agent bound there holds a task in progress.
    let r = Repo::with(
        Seed::default()
            .task("sim/landed", "done")
            .task("sim/next", "ready"),
    );
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let handle = r.bind(&wt);
    r.board.store.claim("sim/next", &handle).unwrap();
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
}

#[test]
fn an_agent_that_beat_within_the_hour_keeps_it() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    r.bind(&wt); // its session beat at NOW, whatever its health since

    let within = sweep_at(&r, NOW + 3599.0, false);
    assert_untouched(&r, &wt, &folders, &within);

    let past = sweep_at(&r, NOW + 3601.0, false);
    assert_eq!(past.cleared.len(), 2, "{}", text(&past));
    assert!(folders.iter().all(|f| !f.exists()));
}

#[test]
fn a_worktree_touched_within_the_hour_keeps_it() {
    // Its git files or its top folder, by the machine's clock: here it was made, committed in
    // and built, every one of them after `before`, and the sweep finds them as they are.
    let before = now();
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let within = r.sweep_now(|o| o.now = Box::new(move || before + 3599.0));
    assert_untouched(&r, &wt, &folders, &within);

    let after = now();
    let past = r.sweep_now(|o| o.now = Box::new(move || after + 3601.0));
    assert_eq!(past.cleared.len(), 2, "{}", text(&past));
    assert!(folders.iter().all(|f| !f.exists()));
}

#[test]
fn a_commit_made_after_the_judgement_keeps_it() {
    // Right before it clears, the sweep reads the worktree again: a HEAD that moved keeps it.
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let (there, watched) = (wt.clone(), wt.canonicalize().unwrap());
    let reached = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reached);
    let done = r.sweep_with(|o| {
        o.now = Box::new(|| LATER);
        o.pause = Box::new(move |stage, what| {
            if stage == "judged" && Path::new(what).canonicalize().ok() == Some(watched.clone()) {
                seen.set(true);
                write(&there, "sim/world.gd", "extends RefCounted\n# more\n");
                git(&there, &["commit", "-qam", "more"]);
            }
        });
    });
    assert!(
        reached.get(),
        "the sweep judged it finished: {}",
        text(&done)
    );
    assert_untouched(&r, &wt, &folders, &done);
}

#[test]
fn work_landed_by_a_fast_forward_counts_when_it_was_committed_here() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = r.merged("landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let done = sweep_at(&r, LATER, false);
    assert_eq!(done.cleared.len(), 2, "{}", text(&done));
    assert!(folders.iter().all(|f| !f.exists()));
}

#[test]
fn a_worktree_tied_to_no_task_keeps_its_build_output() {
    let r = Repo::new();
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
}

#[test]
fn a_process_working_inside_keeps_it_and_so_does_not_knowing() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let inside = wt.join("crates");
    for cwds in [Some(vec![inside]), None] {
        let scan = cwds.clone();
        let done = r.sweep_with(|o| {
            o.now = Box::new(|| LATER);
            o.cwds = Box::new(move || scan.clone());
        });
        assert_untouched(&r, &wt, &folders, &done);
    }
}

#[test]
fn a_process_found_inside_right_before_the_clear_keeps_it() {
    // The scan it was judged by found nobody there; asked again right before the clear, `lsof`
    // finds a process inside, or cannot tell.
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let (wt, folders) = finished(&r);
    for occupied in [Some(true), None] {
        let done = r.sweep_with(|o| {
            o.now = Box::new(|| LATER);
            o.occupied = Box::new(move |_| occupied);
        });
        assert_untouched(&r, &wt, &folders, &done);
    }
}

#[test]
fn a_task_claimed_right_before_the_clear_keeps_it() {
    // Judged finished, then the agent bound there claims its next task: the board, read again right
    // before the clear, keeps it.
    let r = Repo::with(
        Seed::default()
            .task("sim/landed", "done")
            .task("sim/next", "ready"),
    );
    let (wt, folders) = finished(&r);
    let handle = r.bind(&wt);
    let board = r.board.other();
    let reached = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reached);
    let watched = wt.clone();
    let done = sweep_pausing(&r, move |stage, what| {
        if stage == "judged" && what == watched {
            seen.set(true);
            board.claim("sim/next", &handle).unwrap();
        }
    });
    assert!(
        reached.get(),
        "the sweep judged it finished: {}",
        text(&done)
    );
    assert_untouched(&r, &wt, &folders, &done);
}

#[test]
fn a_build_holding_a_lock_in_a_folder_keeps_that_folder_until_it_lets_go() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let (wt, folders) = finished(&r);
    // Each lock Cargo holds while it builds: in a profile's folder, and in a target's.
    for lock in [
        "debug/.cargo-lock",
        "debug/.cargo-build-lock",
        "release/.cargo-artifact-lock",
        "wasm32-unknown-unknown/debug/.cargo-lock",
    ] {
        built(&wt);
        write(&wt, &format!(".target/{lock}"), "");
        let building = File::open(folders[0].join(lock)).unwrap();
        building.lock().unwrap();
        let done = sweep_at(&r, LATER, false);
        assert_eq!(
            cleared(&done),
            [folders[1].clone()],
            "{lock}: {}",
            text(&done)
        );
        assert!(
            folders[0].join("debug/deps/libwork.rlib").is_file(),
            "{lock}"
        );
        building.unlock().unwrap();
    }

    // Let go, it goes: renamed aside while the sweep holds every lock in it, so no Cargo takes one.
    let lock = folders[0].join("debug/.cargo-lock");
    let held_then = Rc::new(Cell::new(None));
    let seen = Rc::clone(&held_then);
    let watched = folders[0].clone();
    let done = sweep_pausing(&r, move |stage, what| {
        if stage == "locked" && what == watched {
            seen.set(Some(File::open(&lock).unwrap().try_lock().is_err()));
        }
    });
    assert_eq!(held_then.get(), Some(true), "{}", text(&done));
    assert_eq!(cleared(&done), [folders[0].clone()], "{}", text(&done));
    assert!(!folders[0].exists());
    let beside: Vec<String> = fs::read_dir(&wt)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".target"))
        .collect();
    assert!(beside.is_empty(), "nothing is left aside: {beside:?}");
}

#[test]
fn a_link_never_leads_the_clear_out_of_the_worktree() {
    let r = Repo::with(
        Seed::default()
            .task("sim/landed", "done")
            .task("sim/linked", "done"),
    );
    // Another checkout's build folders, which a link in each worktree leads to.
    let other = r.dir.join("other");
    let theirs = built(&other);

    // One whose `symphony` is a link to it, and one whose `.target` is.
    let through_symphony = landed(&r, "landed", "sim/landed");
    held(&r, &through_symphony);
    write(
        &through_symphony,
        ".target/debug/deps/libwork.rlib",
        "compiled\n",
    );
    std::os::unix::fs::symlink(&other, through_symphony.join("symphony")).unwrap();
    let through_target = landed(&r, "linked", "sim/linked");
    held(&r, &through_target);
    write(
        &through_target,
        "symphony/.target/debug/deps/libwork.rlib",
        "compiled\n",
    );
    std::os::unix::fs::symlink(&theirs[0], through_target.join(".target")).unwrap();

    let done = sweep_at(&r, LATER, false);
    let mut gone = cleared(&done);
    gone.sort();
    let mut own = vec![
        through_symphony.join(".target"),
        through_target.join("symphony/.target"),
    ];
    own.sort();
    assert_eq!(gone, own, "{}", text(&done));
    assert!(own.iter().all(|folder| !folder.exists()));
    for folder in &theirs {
        assert!(
            folder.join("debug/deps/libwork.rlib").is_file(),
            "{}",
            text(&done)
        );
    }
    assert!(fs::symlink_metadata(through_symphony.join("symphony")).is_ok_and(|m| m.is_symlink()));
    assert!(fs::symlink_metadata(through_target.join(".target")).is_ok_and(|m| m.is_symlink()));
}

#[test]
fn what_a_seed_or_a_clear_left_goes_with_finished_work_once_its_process_is_gone() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let (wt, folders) = finished(&r);
    let (dead, live) = (dead_pid(), std::process::id());
    let aside = |name: String| {
        write(
            &wt,
            &format!("{name}/debug/deps/libwork.rlib"),
            "compiled\n",
        );
        wt.join(name)
    };
    let left = [
        aside(format!(".target.seed.{dead}")),
        aside(format!("symphony/.target.clear.{dead}")),
    ];
    let running = aside(format!(".target.seed.{live}"));

    let done = sweep_at(&r, LATER, false);
    let mut gone = cleared(&done);
    gone.sort();
    let mut expected: Vec<PathBuf> = folders.iter().chain(&left).cloned().collect();
    expected.sort();
    assert_eq!(gone, expected, "{}", text(&done));
    assert!(expected.iter().all(|folder| !folder.exists()));
    assert!(running.join("debug/deps/libwork.rlib").is_file());
}
