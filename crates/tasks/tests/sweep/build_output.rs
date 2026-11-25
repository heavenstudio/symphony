//! The build output of work past its executor: a worktree the worktree sweep keeps loses its build
//! folders, and what git ignores in its `build/`, once every task it is tied to (its branch, or a
//! session bound to it, an in-process agent's by its agent id) is in review, done or cancelled and
//! nothing ran there for an hour, merged or not; the worktree of a land that ended too. Never a
//! worktree tied to no task, a task in progress, ready, blocked, in concept or design, an agent that
//! beat within the hour, a build or test the recorder recorded there within the hour, a land that
//! runs, a lock the sweep does not know, or a process working inside it, as read again right before
//! the clear; the release run's checkout only after a week untouched; never a folder a Cargo holds the
//! lock of, never what git tracks, and never one reached through a link.

use crate::common::{GONE, NOW, Seed, session};
use crate::{Repo, dead_pid, git, started, text, write};
use std::cell::Cell;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use symphony_board::sweep::Sweep;
use symphony_board::sweep::holder::RELEASE_LOCK;
use symphony_board::sweep::size;
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
/// runs and the board does not show the agent finished: locked by a live process, this one.
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

/// That `wt` stayed and lost its build folders, each for `why`.
fn assert_cleared(r: &Repo, wt: &Path, folders: &[PathBuf], why: &str, done: &Sweep) {
    let said = text(done);
    r.assert_kept(wt, &said);
    assert_eq!(cleared(done), folders, "{said}");
    assert!(done.cleared.iter().all(|c| c.why == why), "{said}");
    assert!(folders.iter().all(|f| !f.exists()), "{said}");
    assert!(
        wt.join("sim/world.gd").is_file(),
        "the worktree's files stay"
    );
}

/// The folders a sweep cleared.
fn cleared(done: &Sweep) -> Vec<PathBuf> {
    done.cleared.iter().map(|c| c.folder.clone()).collect()
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
    let (wt, folders) = finished(&r);
    let why = "task sim/landed is done; nothing ran there for an hour";
    let held: Vec<u64> = folders.iter().map(|f| size::bytes(f)).collect();
    assert!(held.iter().all(|&bytes| bytes > 0), "{held:?}");

    let dry = sweep_at(&r, LATER, true);
    let said = text(&dry);
    assert_eq!(cleared(&dry), folders, "{said}");
    assert_eq!(
        dry.cleared.iter().map(|c| c.bytes).collect::<Vec<_>>(),
        held,
        "each clear says what its folder holds"
    );
    assert_eq!(dry.freed, held.iter().sum::<u64>(), "{said}");
    assert!(
        said.contains(&format!(
            "would clear build output {} ({why}; {})",
            folders[0].display(),
            size::shown(held[0])
        )) && said.contains("; would clear 2 build folders of idle work; would free "),
        "{said}"
    );
    assert!(
        folders.iter().all(|f| f.is_dir()),
        "a dry run removes nothing"
    );

    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    assert_cleared(&r, &wt, &folders, why, &done);
    assert!(
        said.contains(&format!(
            "cleared build output {} ({why}; {})",
            folders[1].display(),
            size::shown(held[1])
        )) && said.ends_with(&format!(
            "; cleared 2 build folders of idle work; freed {}",
            size::shown(held.iter().sum())
        )),
        "{said}"
    );
    assert!(
        sweep_at(&r, LATER, false).cleared.is_empty(),
        "nothing is left to clear"
    );
}

#[test]
fn work_in_review_loses_its_build_output_merged_or_not() {
    // Past its executor: its reviewer builds in a worktree of its own.
    let r = Repo::with(
        Seed::default()
            .task("sim/merged", "review")
            .task("sim/unmerged", "review"),
    );
    let merged = landed(&r, "merged", "sim/merged");
    let unmerged = r.add("unmerged", Some("sim/unmerged"), "main");
    r.commit(&unmerged, "extends RefCounted\n# in review\n");
    for wt in [&merged, &unmerged] {
        held(&r, wt);
    }
    let [merged_folders, unmerged_folders] = [&merged, &unmerged].map(|wt| built(wt));
    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    let mut gone = cleared(&done);
    symphony_board::order::sort_by(&mut gone, &Ord::cmp);
    let mut all: Vec<PathBuf> = merged_folders
        .iter()
        .chain(&unmerged_folders)
        .cloned()
        .collect();
    symphony_board::order::sort_by(&mut all, &Ord::cmp);
    assert_eq!(gone, all, "{said}");
    for (wt, slug) in [(&merged, "sim/merged"), (&unmerged, "sim/unmerged")] {
        r.assert_kept(wt, &said);
        assert!(
            said.contains(&format!(
                "(task {slug} is review; nothing ran there for an hour;"
            )),
            "{said}"
        );
    }
}

#[test]
fn a_branch_with_no_commits_ahead_of_main_loses_it_once_its_task_is_done() {
    // Its task done, a branch that never committed (one at main, one fast-forwarded to it, one reset
    // to it for its next task) holds build output nobody builds on: merged or not, it goes.
    let r = Repo::with(
        Seed::default()
            .task("sim/fresh", "done")
            .task("sim/fast", "done")
            .task("sim/again", "done"),
    );
    let fresh = r.add("fresh", Some("sim/fresh"), "main");
    let fast = r.add("fast", Some("sim/fast"), "main");
    r.commit(&r.root, "extends RefCounted\n# main moved on\n");
    git(&fast, &["merge", "-q", "--ff-only", "main"]);
    let again = landed(&r, "again", "sim/again");
    git(&again, &["reset", "-q", "--hard", "main"]);
    for wt in [&fresh, &fast, &again] {
        held(&r, wt);
    }
    let folders: Vec<[PathBuf; 2]> = [&fresh, &fast, &again].map(|wt| built(wt)).into();
    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    for (wt, folders) in [&fresh, &fast, &again].iter().zip(&folders) {
        r.assert_kept(wt, &said);
        assert!(folders.iter().all(|f| !f.exists()), "{said}");
    }
    assert_eq!(done.cleared.len(), 6, "{said}");
}

#[test]
fn work_that_never_landed_loses_its_build_output_and_keeps_its_commits() {
    // Its task done, its branch's commit on no line of main's: the worktree stays for an
    // orchestrator to decide, its build output does not.
    let r = Repo::with(Seed::default().task("sim/unlanded", "done"));
    let wt = r.add("unlanded", Some("sim/unlanded"), "main");
    r.commit(&wt, "extends RefCounted\n# unlanded\n");
    let head = git(&wt, &["rev-parse", "HEAD"]);
    held(&r, &wt);
    let folders = built(&wt);
    let done = sweep_at(&r, LATER, false);
    assert_cleared(
        &r,
        &wt,
        &folders,
        "task sim/unlanded is done; nothing ran there for an hour",
        &done,
    );
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), head);
}

#[test]
fn a_task_before_review_keeps_the_build_output() {
    const BEFORE: [&str; 5] = ["in_progress", "blocked", "ready", "concept", "design"];
    let seed = BEFORE.iter().fold(Seed::default(), |seed, status| {
        seed.task(&format!("sim/{status}"), status)
    });
    let r = Repo::with(seed);
    // Each landed, kept whole and built, as `finished` makes one.
    let trees = BEFORE.map(|status| {
        let wt = landed(&r, status, &format!("sim/{status}"));
        held(&r, &wt);
        let folders = built(&wt);
        (wt, folders)
    });
    let done = sweep_at(&r, LATER, false);
    for (wt, folders) in &trees {
        assert_untouched(&r, wt, folders, &done);
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
    let (wt, folders) = finished(&r);
    let handle = r.bind(&wt);
    r.board.store.claim("sim/next", &handle).unwrap();
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
}

#[test]
fn an_agent_that_beat_within_the_hour_keeps_it() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let (wt, folders) = finished(&r);
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
    let (wt, folders) = finished(&r);
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
    let (wt, folders) = finished(&r);
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
fn a_lock_taken_after_the_judgement_keeps_it() {
    // Judged idle with no lock, then a land locks it: the listing read again right before the
    // clear keeps it.
    let r = Repo::with(Seed::default().task("sim/unlanded", "done"));
    let wt = r.add("unlanded", Some("sim/unlanded"), "main");
    r.commit(&wt, "extends RefCounted\n# unlanded\n");
    let folders = built(&wt);
    let (root, there, watched) = (r.root.clone(), wt.clone(), wt.canonicalize().unwrap());
    let reached = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reached);
    let done = sweep_pausing(&r, move |stage, what| {
        if stage == "judged" && what == watched {
            seen.set(true);
            let reason = format!("symphony land: sim/unlanded (pid {})", std::process::id());
            let at = there.to_string_lossy().into_owned();
            git(&root, &["worktree", "lock", "--reason", &reason, &at]);
        }
    });
    assert!(reached.get(), "{}", text(&done));
    assert_untouched(&r, &wt, &folders, &done);
}

#[test]
fn work_landed_by_a_fast_forward_counts_too() {
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
fn a_worktree_tied_to_no_task_keeps_it_whatever_its_lock() {
    // Commits not in main keep each worktree, and nothing ties it to the board: the owner's, or one
    // a project made by hand, whose rebuild has no seed. No lock, or one whose process is gone,
    // keeps its build all the same.
    let r = Repo::new();
    let open = r.add("open", Some("sim/open"), "main");
    let dead = r.add("dead", Some("sim/dead"), "main");
    for wt in [&open, &dead] {
        r.commit(wt, "extends RefCounted\n# no task\n");
    }
    r.claude_lock(
        &dead,
        &dead_pid().to_string(),
        &started(i64::from(std::process::id())),
    );
    let folders = [&open, &dead].map(|wt| built(wt));
    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    assert!(done.cleared.is_empty(), "{said}");
    for (wt, folders) in [&open, &dead].iter().zip(&folders) {
        r.assert_kept(wt, &said);
        assert!(folders.iter().all(|f| f.is_dir()), "{said}");
    }
}

#[test]
fn an_in_process_agents_worktree_is_tied_to_its_task_by_its_session() {
    // A subagent's worktree is on a branch of its own name, its session registered at the main
    // checkout: its agent id ties the worktree to the task its session holds.
    const AGENT: &str = "a5a6262f496f70d23";
    let r = Repo::with(
        Seed::default()
            .session_row(symphony_board::rows::SessionRow {
                agent: Some(AGENT.into()),
                ..session("amber-heron")
            })
            .task("sim/landed", "review")
            .role("sim/landed", "amber-heron", "executor"),
    );
    r.board.advance(GONE);
    let wt = r.add(
        &format!("agent-{AGENT}"),
        Some(&format!("worktree-agent-{AGENT}")),
        "main",
    );
    r.commit(&wt, "extends RefCounted\n# review\n");
    held(&r, &wt);
    let folders = built(&wt);
    let done = sweep_at(&r, NOW + GONE + 7200.0, false);
    assert_cleared(
        &r,
        &wt,
        &folders,
        "task sim/landed is review; nothing ran there for an hour",
        &done,
    );
}

/// A build or test the board's recorder recorded in `checkout`, finished at `ended`, in the work
/// folder of `r`'s repository.
fn recorded(r: &Repo, checkout: &Path, ended: f64) {
    use symphony_board::cli::machine_work::recorded::{self, Phase, Record, Step};
    let id = format!("{:032x}", ended.to_bits());
    let folder = recorded::in_common(&r.root.join(".git")).join(recorded::OPERATIONS);
    fs::create_dir_all(&folder).unwrap();
    let record = Record {
        version: 1,
        operation_id: id.clone(),
        parent_id: None,
        phase: Phase::Test,
        label: "just test-rs".into(),
        state: Step::Finished,
        handle: None,
        task: None,
        checkout: checkout.to_string_lossy().into_owned(),
        branch: None,
        commit: None,
        machine_id: None,
        host: "here".into(),
        pid: 1,
        child_pid: Some(2),
        queued_at: ended - 60.0,
        started_at: Some(ended - 60.0),
        ended_at: Some(ended),
        beat_at: ended,
        exit_code: Some(0),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    };
    recorded::write(&folder.join(format!("{id}.json")), &record).unwrap();
}

#[test]
fn a_build_or_test_recorded_there_within_the_hour_keeps_it() {
    // A reviewer building in its executor's worktree: no session of its is bound there, but the
    // recorder says where its test ran.
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let (wt, folders) = finished(&r);
    recorded(&r, &wt.join("crates/tasks"), LATER - 600.0);
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
    // An hour after it ended, nothing runs there.
    let done = sweep_at(&r, LATER + 3600.0, false);
    assert_cleared(
        &r,
        &wt,
        &folders,
        "task sim/landed is done; nothing ran there for an hour",
        &done,
    );
}

#[test]
fn what_git_ignores_in_build_goes_and_what_it_tracks_stays() {
    // The fixture ignores `build/*` but `build/.gdignore`, which it tracks, as a Godot project does.
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    write(&r.root, "build/.gdignore", "");
    git(&r.root, &["add", "-f", "build/.gdignore"]);
    git(&r.root, &["commit", "-qm", "the export folder"]);
    let (wt, folders) = finished(&r);
    write(&wt, "build/web/index.html", "exported\n");
    write(&wt, "build/web/game.wasm", "exported\n");
    let exported = size::bytes(&wt.join("build/web"));
    let held: u64 = folders.iter().map(|f| size::bytes(f)).sum();
    let why = "task sim/landed is done; nothing ran there for an hour";

    let dry = sweep_at(&r, LATER, true);
    let said = text(&dry);
    assert_eq!(
        cleared(&dry),
        [folders[0].clone(), folders[1].clone(), wt.join("build")],
        "{said}"
    );
    assert_eq!(
        dry.cleared[2].bytes, exported,
        "what git ignores there: {said}"
    );
    assert_eq!(dry.freed, held + exported, "{said}");

    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    assert_eq!(cleared(&done), cleared(&dry), "{said}");
    assert_eq!(done.freed, dry.freed, "{said}");
    assert!(done.cleared.iter().all(|c| c.why == why), "{said}");
    assert!(!wt.join("build/web").exists(), "{said}");
    assert!(wt.join("build/.gdignore").is_file(), "a tracked file stays");
    assert_eq!(
        git(&wt, &["status", "--porcelain"]),
        "",
        "nothing tracked changed"
    );
    assert!(
        sweep_at(&r, LATER, false).cleared.is_empty(),
        "nothing git ignores is left there"
    );
}

#[test]
fn a_land_that_runs_keeps_it_and_one_gone_does_not() {
    let r = Repo::new();
    let running = r.detached("land-running");
    let stopped = r.detached("land-stopped");
    // A commit of its own keeps each worktree: what is judged here is its build output.
    for wt in [&running, &stopped] {
        git(
            wt,
            &["commit", "-q", "--allow-empty", "-m", "the land's merge"],
        );
    }
    let live = std::process::id();
    r.lock(&running, &format!("symphony land: sim/a (pid {live})"));
    r.lock(
        &stopped,
        &format!("symphony land: sim/b (pid {})", dead_pid()),
    );
    let [running_folders, stopped_folders] = [&running, &stopped].map(|wt| built(wt));
    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    assert_eq!(cleared(&done), stopped_folders, "{said}");
    assert!(
        done.cleared.iter().all(
            |c| c.why.starts_with("the worktree of a land that ended (pid ")
                && c.why.ends_with("); nothing ran there for an hour")
        ),
        "{said}"
    );
    assert!(running_folders.iter().all(|f| f.is_dir()), "{said}");
}

#[test]
fn the_release_runs_checkout_keeps_its_build_a_week() {
    let r = Repo::new();
    let checkout = r.detached("release");
    r.lock(&checkout, RELEASE_LOCK);
    let folders = built(&checkout);
    // Touched hours ago, by the machine's clock: the release run builds on it run after run.
    let hours_ago = r.sweep_with(|o| o.now = Box::new(now));
    assert_untouched(&r, &checkout, &folders, &hours_ago);
    // Untouched for a week: no release run runs there now.
    Repo::age(&checkout, 7.0 * 24.0 + 1.0);
    let done = r.sweep_now(|o| o.now = Box::new(now));
    let said = text(&done);
    r.assert_kept(&checkout, &said);
    assert_eq!(cleared(&done), folders, "{said}");
    assert!(folders.iter().all(|f| !f.exists()), "{said}");
    assert!(
        done.cleared
            .iter()
            .all(|c| c.why == "the release run's checkout; nothing touched it for 7 days"),
        "{said}"
    );
}

#[test]
fn a_lock_the_sweep_does_not_know_keeps_it() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    r.lock(&wt, "on a removable disk");
    let folders = built(&wt);
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
}

#[test]
fn a_process_working_inside_keeps_it_and_so_does_not_knowing() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let (wt, folders) = finished(&r);
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
    symphony_board::order::sort_by(&mut gone, &Ord::cmp);
    let mut own = vec![
        through_symphony.join(".target"),
        through_target.join("symphony/.target"),
    ];
    symphony_board::order::sort_by(&mut own, &Ord::cmp);
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
    symphony_board::order::sort_by(&mut gone, &Ord::cmp);
    let mut expected: Vec<PathBuf> = folders.iter().chain(&left).cloned().collect();
    symphony_board::order::sort_by(&mut expected, &Ord::cmp);
    assert_eq!(gone, expected, "{}", text(&done));
    assert!(expected.iter().all(|folder| !folder.exists()));
    assert!(running.join("debug/deps/libwork.rlib").is_file());
}
