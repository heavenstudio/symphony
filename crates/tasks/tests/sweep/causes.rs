//! The worktrees of in-process agents, as this repository's board has them, on scratch
//! repositories: each rule the sweep holds them to.
//!
//! * The land's own pass (`--task SLUG`) visits the worktrees its task's executors' and reviewers'
//!   sessions are bound to, an in-process agent's by its agent id, once none of those is live or
//!   stale, and says what it freed.
//! * Claude Code's lock names the orchestrator's live process, which outlives its agent: such a lock
//!   lets the worktree go once the board shows its agent finished, and keeps it otherwise.
//! * A finished worktree's artifacts folder (the project's, which this repository ignores whole) is
//!   set aside in the main checkout, whatever came there up to its removal, and goes back if git
//!   refuses the removal; a set-aside that cannot be made, or an entry there that cannot be read,
//!   keeps the worktree, saying why; a folder whose `CACHEDIR.TAG` lacks the convention's signature
//!   is evidence like any other; an open task's artifacts keep its worktree.
//! * Build output never keeps a merged worktree, and goes from one whose work is in review or done
//!   once nothing ran there for an hour.
//! * A land's worktree goes once the land is gone; a running land's and the release run's stay,
//!   its checkout locked by a build older than the rename included.

use crate::common::{GONE, NOW, Seed, session};
use crate::{Repo, dead_pid, git, started, text, write};
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use symphony_board::rows::SessionRow;
use symphony_board::sweep::holder::{LEGACY_LOCK, RELEASE_LOCK};
use symphony_board::sweep::{self, Request, size};

const AGENT: &str = "a5a6262f496f70d23";
const SLUG: &str = "tools/landed";

/// A board where `SLUG` is `status`, its executor an in-process subagent (its session tied to its
/// own `agent-ID` worktree by its agent id, no launch record), gone since.
fn subagent_board(status: &str) -> Repo {
    let r = Repo::with(
        Seed::default()
            .session_row(SessionRow {
                agent: Some(AGENT.into()),
                ..session("amber-heron")
            })
            .task(SLUG, status)
            .role(SLUG, "amber-heron", "executor"),
    );
    r.board.advance(GONE);
    r
}

/// The subagent's worktree, on the task's branch with a commit, merged into main by a merge
/// commit, as `just land` merges.
fn landed(r: &Repo) -> PathBuf {
    let wt = r.add(&format!("agent-{AGENT}"), Some(SLUG), "main");
    r.commit(&wt, "extends RefCounted\n# landed\n");
    let message = format!("Merge branch '{SLUG}'");
    git(&r.root, &["merge", "-q", "--no-ff", "-m", &message, SLUG]);
    wt
}

/// This repository's ignore file, committed on main before any worktree is made.
fn ignore_as_here(r: &Repo) {
    write(&r.root, ".gitignore", "/.target/\n/build/\n/artifacts/\n");
    git(
        &r.root,
        &["commit", "-qam", "this repository's ignore file"],
    );
}

/// The line of `said` saying why `wt` was removed.
fn removed(said: &str, wt: &Path) -> String {
    starting(said, &format!("removed {} (", wt.display()))
}

/// The line of a dry run's `said` for `wt`: its size, tasks, merge and verdict.
fn row(said: &str, wt: &Path) -> String {
    starting(said, &format!("worktree {}: ", wt.display()))
}

/// The one line of `said` that starts with `start`.
fn starting(said: &str, start: &str) -> String {
    let found: Vec<&str> = said.lines().filter(|l| l.starts_with(start)).collect();
    assert_eq!(found.len(), 1, "{start}\n{said}");
    found[0].to_string()
}

/// A dry run's lines, every worktree idle and nobody inside.
fn dry(r: &Repo) -> String {
    text(&r.sweep_with(|o| o.dry_run = true))
}

/// The pass a land runs for `SLUG`, from the main checkout.
fn lands_pass(r: &Repo) -> String {
    let request = Request {
        tasks: vec![SLUG],
        ..Request::default()
    };
    sweep::command(&r.board.store, &r.env(&r.root), &request)
        .unwrap()
        .join("\n")
}

#[test]
fn the_lands_pass_removes_its_subagents_worktree_and_names_the_bytes() {
    let r = subagent_board("done");
    let wt = landed(&r);
    write(&wt, ".target/debug/deps/libwork.rlib", "compiled\n");
    let bytes = size::bytes(&wt);
    // Just landed: no window for its own worktree.
    let said = lands_pass(&r);
    r.assert_removed(&wt, &said);
    let shown = size::shown(bytes);
    assert!(
        removed(&said, &wt).ends_with(&format!(", merged into main, clean, no lock; {shown})")),
        "{said}"
    );
    assert!(
        said.contains("removed 1 of 2 worktrees") && said.contains(&format!("; freed {shown}")),
        "{said}"
    );
    // Its branch, merged and held by no worktree now, goes with it.
    assert_eq!(r.branch(SLUG), "", "{said}");
    assert!(
        said.contains(&format!(
            "deleted branch {SLUG} (merged into main, its worktree removed)"
        )),
        "{said}"
    );
}

#[test]
fn the_lands_pass_leaves_a_worktree_its_subagent_still_works_in() {
    // The same subagent, its session live: the land's pass visits its worktree, on the task's
    // branch, and keeps it, its session bound there.
    let r = Repo::with(
        Seed::default()
            .session_row(SessionRow {
                agent: Some(AGENT.into()),
                ..session("amber-heron")
            })
            .task(SLUG, "done")
            .role(SLUG, "amber-heron", "executor"),
    );
    let wt = landed(&r);
    let said = lands_pass(&r);
    r.assert_kept(&wt, &said);
    assert!(said.contains("removed 0 of 2 worktrees"), "{said}");
}

#[test]
fn a_finished_subagents_worktree_locked_by_its_live_orchestrator_goes() {
    let r = subagent_board("done");
    let wt = landed(&r);
    let orchestrator = i64::from(std::process::id());
    r.claude_lock(&wt, &orchestrator.to_string(), &started(orchestrator));
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(
        removed(&said, &wt).contains(&format!(
            "Claude Code's lock of pid {orchestrator}, the session that started agent-{AGENT} and \
             outlives it: its session amber-heron is neither live nor stale, and task {SLUG} is done"
        )),
        "{said}"
    );
}

#[test]
fn a_live_orchestrators_lock_keeps_an_agent_the_board_does_not_show_finished() {
    let orchestrator = i64::from(std::process::id());
    // Its session live (bound, so kept before its lock is read), its task in review, or no
    // session the board knows of: the lock is all there is, and it holds.
    for case in ["live", "review", "unknown agent", "cancelled but live"] {
        let r = match case {
            "live" => Repo::with(
                Seed::default()
                    .session_row(SessionRow {
                        agent: Some(AGENT.into()),
                        ..session("amber-heron")
                    })
                    .task(SLUG, "done")
                    .role(SLUG, "amber-heron", "executor"),
            ),
            "review" => subagent_board("review"),
            "unknown agent" => Repo::with(Seed::default().task(SLUG, "done")),
            _ => Repo::with(
                Seed::default()
                    .session_row(SessionRow {
                        agent: Some(AGENT.into()),
                        ..session("amber-heron")
                    })
                    .task(SLUG, "cancelled")
                    .role(SLUG, "amber-heron", "executor"),
            ),
        };
        let wt = landed(&r);
        r.claude_lock(&wt, &orchestrator.to_string(), &started(orchestrator));
        let said = dry(&r);
        let line = row(&said, &wt);
        let expected = match case {
            "live" | "cancelled but live" => "kept: session amber-heron is bound there",
            "review" => "kept: task tools/landed is review",
            _ => "and the board does not show that agent finished",
        };
        assert!(line.contains(expected), "{case}: {said}");
        let said = r.sweep();
        r.assert_kept(&wt, &said);
    }
}

#[test]
fn a_live_claude_lock_naming_another_agent_keeps_it() {
    // A lock the board cannot tie to this worktree: its agent's own worktree is another.
    let r = subagent_board("done");
    let wt = landed(&r);
    let orchestrator = i64::from(std::process::id());
    r.lock(
        &wt,
        &format!(
            "claude agent agent-ac953cb051c3dd046 (pid {orchestrator} start {})",
            started(orchestrator)
        ),
    );
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        starting(&said, &format!("kept {}: ", wt.display()))
            .contains("which is not this worktree's"),
        "{said}"
    );
}

#[test]
fn a_finished_worktrees_artifacts_are_set_aside_and_the_worktree_goes() {
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/docs-app.port", "4321\n");
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");

    // A dry run says where they would go, and moves nothing.
    let dry = r.sweep_with(|o| o.dry_run = true);
    let kept_in = sweep::swept(&r.root, "artifacts").join(format!("agent-{AGENT}"));
    assert!(
        text(&dry).contains(&format!(
            "its artifacts/ would move to {}",
            kept_in.display()
        )),
        "{}",
        text(&dry)
    );
    assert!(wt.join("artifacts/docs-app.port").is_file());
    assert!(!kept_in.exists());

    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(
        said.contains(&format!("its artifacts/ moved to {}", kept_in.display())),
        "{said}"
    );
    assert_eq!(
        fs::read_to_string(kept_in.join("docs-app.port")).unwrap(),
        "4321\n"
    );
    assert_eq!(
        fs::read_to_string(kept_in.join("tools-landed/shot.png")).unwrap(),
        "a picture\n"
    );
}

#[test]
fn artifacts_set_aside_go_back_when_git_refuses_the_removal() {
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");
    let kept_in = sweep::swept(&r.root, "artifacts").join(format!("agent-{AGENT}"));
    let (there, watched) = (wt.clone(), wt.canonicalize().unwrap());
    let reached = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reached);
    // Set aside, then an untracked file appears: git refuses the removal.
    let done = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, what| {
            if stage == "aside" && Path::new(what).canonicalize().ok() == Some(watched.clone()) {
                seen.set(true);
                write(&there, "notes.txt", "written meanwhile\n");
            }
        });
    });
    assert!(reached.get(), "{}", text(&done));
    r.assert_kept(&wt, &text(&done));
    assert_eq!(
        fs::read_to_string(wt.join("artifacts/tools-landed/shot.png")).unwrap(),
        "a picture\n"
    );
    assert!(!kept_in.exists(), "{}", text(&done));
}

#[test]
fn an_artifact_written_after_the_checks_is_set_aside_not_removed() {
    // A finished worktree with no artifacts when it was judged, only an empty folder for them (a
    // file new in its top folder would make it touched within the hour): one written after the
    // last check that could see it goes aside, never with the worktree.
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    fs::create_dir_all(wt.join("artifacts/tools-landed")).unwrap();
    let kept_in = sweep::swept(&r.root, "artifacts").join(format!("agent-{AGENT}"));
    let (there, watched) = (wt.clone(), wt.canonicalize().unwrap());
    let reached = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reached);
    let done = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, what| {
            if stage == "unlocked" && Path::new(what).canonicalize().ok() == Some(watched.clone()) {
                seen.set(true);
                write(&there, "artifacts/tools-landed/late.png", "written late\n");
            }
        });
    });
    let said = text(&done);
    assert!(reached.get(), "{said}");
    r.assert_removed(&wt, &said);
    assert!(
        removed(&said, &wt).contains(&format!("its artifacts/ moved to {}", kept_in.display())),
        "{said}"
    );
    assert_eq!(
        fs::read_to_string(kept_in.join("tools-landed/late.png")).unwrap(),
        "written late\n"
    );
}

#[test]
fn a_projects_own_artifacts_folder_is_the_one_set_aside() {
    // Its manifest names another folder: that one is the evidence set aside, in the main
    // checkout's; what is ignored under the default's name is a file like any other.
    let r = subagent_board("done");
    write(
        &r.root,
        "symphony.toml",
        "[project]\nartifacts = \"evidence\"\n",
    );
    write(
        &r.root,
        ".gitignore",
        "/.target/\n/build/\n/evidence/\n/artifacts/\n",
    );
    git(&r.root, &["add", "symphony.toml", ".gitignore"]);
    git(
        &r.root,
        &["commit", "-qm", "a project with its own artifacts folder"],
    );
    let wt = landed(&r);
    write(&wt, "evidence/tools-landed/shot.png", "a picture\n");
    let other = r.merged("other", "sim/other");
    write(&other, "artifacts/shot.png", "not evidence here\n");
    let kept_in = sweep::swept(&r.root, "evidence").join(format!("agent-{AGENT}"));

    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(
        removed(&said, &wt).contains(&format!("its evidence/ moved to {}", kept_in.display())),
        "{said}"
    );
    assert_eq!(
        fs::read_to_string(kept_in.join("tools-landed/shot.png")).unwrap(),
        "a picture\n"
    );
    r.assert_kept(&other, &said);
    assert_eq!(
        starting(&said, &format!("kept {}: ", other.display())),
        format!(
            "kept {}: merged, but holds files that did not land: artifacts/",
            other.display()
        )
    );
    assert!(!sweep::swept(&r.root, "artifacts").exists(), "{said}");
}

#[test]
fn a_set_aside_that_cannot_be_made_keeps_the_worktree_and_says_why() {
    // The main checkout's folder for what is set aside is a file: nothing can go below it.
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");
    let swept = sweep::swept(&r.root, "artifacts");
    write(&r.root, "artifacts/swept", "a file, not a folder\n");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    let line = starting(&said, &format!("kept {}: ", wt.display()));
    assert!(
        line.starts_with(&format!(
            "kept {}: {} cannot be moved to {}: ",
            wt.display(),
            wt.join("artifacts").display(),
            swept.join(format!("agent-{AGENT}")).display()
        )),
        "{said}"
    );
    assert_eq!(
        fs::read_to_string(wt.join("artifacts/tools-landed/shot.png")).unwrap(),
        "a picture\n"
    );
    assert_eq!(
        fs::read_to_string(&swept).unwrap(),
        "a file, not a folder\n"
    );
}

#[test]
fn a_folder_with_a_cache_tag_that_is_not_signed_is_set_aside_with_its_pictures() {
    // A tool, or a fixture copied in, left a CACHEDIR.TAG without the convention's signature beside
    // a review's pictures: evidence all the same, set aside; Cargo's build folder, signed, goes.
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/tools-landed/CACHEDIR.TAG", "not a cache\n");
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");
    write(&wt, "artifacts/tools-landed/after/shot.png", "after\n");
    write(
        &wt,
        "artifacts/tools-landed/target/CACHEDIR.TAG",
        "Signature: 8a477f597d28d172789f06886806bc55\n# made by cargo\n",
    );
    write(
        &wt,
        "artifacts/tools-landed/target/debug/app",
        "\u{7f}ELF\n",
    );
    let kept_in = sweep::swept(&r.root, "artifacts").join(format!("agent-{AGENT}"));
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    for (file, text) in [
        ("shot.png", "a picture\n"),
        ("after/shot.png", "after\n"),
        ("CACHEDIR.TAG", "not a cache\n"),
    ] {
        assert_eq!(
            fs::read_to_string(kept_in.join("tools-landed").join(file)).unwrap(),
            text,
            "{file}: {said}"
        );
    }
    assert!(!kept_in.join("tools-landed/target").exists(), "{said}");
}

/// Folder `dir` listable but not searchable while it lives: the records of what it holds cannot be
/// read. Searchable again when dropped, so the test's folder can be removed whatever happened.
struct Unsearchable(PathBuf);

impl Unsearchable {
    fn new(dir: PathBuf) -> Unsearchable {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o600)).unwrap();
        Unsearchable(dir)
    }
}

impl Drop for Unsearchable {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn an_artifact_that_cannot_be_read_keeps_the_worktree_and_says_why() {
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");
    write(
        &wt,
        "artifacts/tools-landed/target/CACHEDIR.TAG",
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    );
    let folder = wt.join("artifacts/tools-landed");
    let kept_in = sweep::swept(&r.root, "artifacts").join(format!("agent-{AGENT}"));
    let locked = Unsearchable::new(folder.clone());
    let why = format!(
        "{} cannot be read: {}",
        folder.join("shot.png").display(),
        std::io::Error::from_raw_os_error(libc::EACCES)
    );
    let dry = dry(&r);
    assert!(row(&dry, &wt).ends_with(&format!("kept: {why}")), "{dry}");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert_eq!(
        starting(&said, &format!("kept {}: ", wt.display())),
        format!("kept {}: {why}", wt.display())
    );
    assert!(!kept_in.exists(), "{said}");
    // Readable again, the next sweep sets the picture aside and the worktree goes.
    drop(locked);
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert_eq!(
        fs::read_to_string(kept_in.join("tools-landed/shot.png")).unwrap(),
        "a picture\n"
    );
}

#[test]
fn artifacts_keep_a_worktree_whose_work_is_not_finished() {
    // Its task parked, neither live nor finished: what its artifacts hold may still be named on
    // its thread, so they keep the worktree, named.
    let r = subagent_board("backlog");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert_eq!(
        starting(&said, &format!("kept {}: ", wt.display())),
        format!(
            "kept {}: merged, but holds files that did not land: artifacts/",
            wt.display()
        )
    );
    assert!(!sweep::swept(&r.root, "artifacts").exists(), "{said}");
}

#[test]
fn build_output_does_not_keep_a_merged_worktree() {
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, ".target/debug/deps/libwork.rlib", "compiled\n");
    write(&wt, "build/client/index.html", "built\n");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
}

#[test]
fn build_output_of_work_in_review_or_awaiting_its_land_goes_after_an_hour() {
    for status in ["review", "done"] {
        let r = subagent_board(status);
        let wt = r.add(&format!("agent-{AGENT}"), Some(SLUG), "main");
        r.commit(&wt, "extends RefCounted\n# in review\n");
        write(&wt, ".target/debug/deps/libwork.rlib", "compiled\n");
        let done = r.sweep_with(|o| o.now = Box::new(|| NOW + 7200.0));
        let said = text(&done);
        r.assert_kept(&wt, &said);
        assert_eq!(done.cleared.len(), 1, "{status}: {said}");
        assert_eq!(
            done.cleared[0].why,
            format!("task {SLUG} is {status}; nothing ran there for an hour")
        );
        assert!(!wt.join(".target").exists(), "{status}: {said}");
    }
}

#[test]
fn a_lands_worktree_goes_once_its_land_is_gone_and_the_release_runs_stays() {
    // Each where the land and the release run make them, beside the main checkout, and the run's
    // checkout as a build older than the rename left it, not moved yet.
    let r = Repo::new();
    let land = r.detached("symphony-wt/land-tools-x");
    let gone = dead_pid();
    r.lock(&land, &format!("symphony land: tools/x (pid {gone})"));
    write(&land, ".target/debug/deps/libwork.rlib", "compiled\n");
    let running = r.detached("symphony-wt/land-tools-y");
    r.lock(
        &running,
        &format!("symphony land: tools/y (pid {})", std::process::id()),
    );
    let release = r.detached("symphony-wt/release");
    r.lock(&release, RELEASE_LOCK);
    write(&release, "artifacts/release/report.json", "{}\n");
    let legacy = r.detached("symphony-wt/nightly");
    r.lock(&legacy, LEGACY_LOCK);
    write(&legacy, "artifacts/nightly/report.json", "{}\n");
    let said = r.sweep();
    r.assert_removed(&land, &said);
    assert!(
        removed(&said, &land).contains(&format!(
            "the land's lock, pid {gone}, names a land that ended"
        )),
        "{said}"
    );
    for (wt, why) in [
        (&running, "a land runs there"),
        (&release, "the release run's checkout"),
        (&legacy, "the release run's checkout"),
    ] {
        r.assert_kept(wt, &said);
        assert!(
            starting(&said, &format!("kept {}: ", wt.display())).contains(why),
            "{said}"
        );
    }
    assert!(release.join("artifacts/release/report.json").is_file());
    assert!(legacy.join("artifacts/nightly/report.json").is_file());
}
