//! A release candidate's fix that changes what a generator makes, where the project delivers by
//! pull request (docs/workflow/RELEASES.md, a red's fix): its head keeps the outputs its own sources make,
//! which the candidate takes, so no squash delivers it, and its land says how one is: approved
//! naming that head, the branch merges main once keeping main's side of every generated path,
//! and that merge is delivered, its squash keeping main's outputs while the head approved before
//! it keeps its own. A head that is no fix of the candidate held now (a main task's, one started
//! where the candidate no longer is, one with none held) is told to put them back.

use super::*;
use crate::children::Child;
use symphony_board::land::delivery::{CANDIDATE, RETAINED};
use symphony_board::sys;

/// The project file of a project that delivers by pull request and whose generator makes `dist/`.
fn manifest() -> String {
    format!(
        "{DELIVERS}generators = [{{ name = \"the client\", outputs = \"dist/\", command = \
         [\"true\"] }}]\n"
    )
}

/// A repository delivering by pull request, its host, and the release candidate held at the
/// commit main was at, after which main moved on: a source and the distribution made of it, one
/// file of the candidate's gone and one new. The candidate's commit.
fn held() -> (Repo, PathBuf, String) {
    let r = Repo::new();
    write(&r.root, "dist/app.js", "built of one\n");
    write(&r.root, "dist/assets/one.js", "one\n");
    git(&r.root, &["add", "dist"]);
    project_file(&r, &manifest());
    let bare = remote(&r);
    let candidate = head(&r.root, "main");
    git(&r.root, &["branch", CANDIDATE, &candidate]);
    write(&r.root, "sim/other.gd", "extends Node\n");
    write(&r.root, "dist/app.js", "built of two\n");
    fs::remove_file(r.root.join("dist/assets/one.js")).unwrap();
    write(&r.root, "dist/assets/two.js", "two\n");
    git(&r.root, &["add", "-A"]);
    git(
        &r.root,
        &["commit", "-qm", "main moves on, and its distribution"],
    );
    git(&r.root, &["push", "-q", "origin", "main"]);
    (r, bare, candidate)
}

/// The fix of a red, started at the candidate: a source, and the distribution its sources make,
/// one file of it new. The people, the worktree and its head.
fn fixed(r: &Repo) -> (People, PathBuf, String) {
    let (people, wt, _) = in_review_from(r, SLUG, "sim/jump.gd", CANDIDATE);
    write(&wt, "dist/app.js", "built of the fix\n");
    write(&wt, "dist/assets/fix.js", "the fix\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "the distribution its sources make"]);
    let tip = head(&wt, "HEAD");
    (people, wt, tip)
}

/// What the land told `@executor` last on the thread.
fn asked(r: &Repo) -> String {
    let post = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(
        post.recipient.as_deref(),
        Some("@executor"),
        "{}",
        post.text
    );
    post.text
}

/// The refusal's line, and what it asks of any head that is no fix of the candidate.
fn refused_line(paths: &str, n: usize) -> String {
    let (path, it) = if n == 1 {
        ("path", "it")
    } else {
        ("paths", "them")
    };
    format!(
        "{SLUG} changes {n} generated {path} ({paths}), which a pull request would deliver as the \
         branch built {it}"
    )
}

const BACK: &str = "Put them back as main has them on the branch (`git checkout origin/main -- \
                    PATH`, committed), then have the new head assessed and stamped";

fn show(dir: &Path, at: &str, path: &str) -> Option<String> {
    let object = format!("{at}:{path}");
    let done = sys::output(
        sys::Command::of("git")
            .args(["show", object.as_str()])
            .current_dir(dir),
    )
    .unwrap();
    done.status
        .success()
        .then(|| String::from_utf8(done.stdout).unwrap())
}

fn ancestor(dir: &Path, rev: &str, of: &str) -> bool {
    let done = sys::output(
        sys::Command::of("git")
            .args(["merge-base", "--is-ancestor", rev, of])
            .current_dir(dir),
    )
    .unwrap();
    done.status.success()
}

#[test]
fn a_candidate_fix_keeps_its_outputs_and_is_delivered_by_a_merge_of_main_that_keeps_mains() {
    let (r, bare, candidate) = held();
    let before = head(&bare, "main");
    let (people, wt, fix) = fixed(&r);
    // Assessed as needing its tests alone, green at its head: still no squash delivers it.
    assess(&r, SLUG, &people.assessor, &["tests"], &fix);
    ran(&r, SLUG, &fix, "cargo test -p sim", 0);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        refused_line("dist/app.js, dist/assets/fix.js", 2)
    );
    assert!(host.verbs().is_empty());
    assert_nothing_landed(&r, &bare, &before);
    let told = asked(&r);
    assert!(
        told.contains(&format!(
            "{SLUG} starts at release-candidate ({}), a fix of the release candidate, which \
             takes the outputs its own sources make: keep {} as it is, approved by a fresh \
             reviewer naming it whatever its assessment needs, as the candidate takes only a \
             commit an approve names and a land with none drops it. Then merge origin/main into \
             the branch once, keeping main's side of every generated path (`git restore \
             --source=origin/main --staged --worktree -- dist/` before the merge's commit), and \
             have that head assessed, tested and approved: it is delivered, and the candidate \
             advances to the commit approved before the merge",
            &candidate[..7],
            &fix[..7]
        )),
        "{told}"
    );
    assert!(!told.contains(BACK), "{told}");

    // As it says: the fix approved naming its head, then main merged once, keeping main's side
    // of every generated path, which the merge conflicts in.
    let store = &r.board.store;
    let reviewer = r.board.session("reviewer");
    store.take_role(SLUG, &reviewer, "reviewer").unwrap();
    let verdict = |text: String| store.verdict(SLUG, &reviewer, "approve", &text).unwrap();
    verdict(format!("{fix}: the fix, and the distribution it makes"));
    let merged = sys::output(
        sys::Command::of("git")
            .args(["merge", "-q", "--no-ff", "origin/main"])
            .current_dir(&wt),
    )
    .unwrap();
    assert!(!merged.status.success(), "dist/app.js conflicts");
    git(
        &wt,
        &[
            "restore",
            "--source=origin/main",
            "--staged",
            "--worktree",
            "--",
            "dist/",
        ],
    );
    git(&wt, &["commit", "-q", "--no-edit"]);
    let delivered = head(&wt, "HEAD");
    // Approved, it is done; its new head goes back to review.
    assert_eq!(store.get(SLUG).unwrap().status, "done");
    store.move_to(SLUG, "review", &people.exe).unwrap();
    assess(&r, SLUG, &people.assessor, &["tests", "review"], &delivered);
    ran(&r, SLUG, &delivered, "cargo test -p sim", 0);
    verdict(format!("{delivered}: main merged, its outputs kept"));
    let host = Host::new(&r, &bare);
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap_or_else(|line| panic!("{line}\n{said}"));
    assert_eq!(host.verbs()[..3], ["publish", "arm", "merge"]);
    assert_eq!(host.request("publish")["head"], delivered.as_str());
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    assert_eq!(receipt.head, delivered);
    assert_eq!(receipt.parent, before);
    assert_eq!(
        head(&r.root, &format!("{RETAINED}{SLUG}")),
        delivered,
        "the delivered head is kept"
    );
    // The squash is the fix's source on main, with main's outputs as main had them.
    let squash = head(&bare, "main");
    assert_eq!(receipt.squash, squash);
    let on_main = |path: &str| show(&r.root, &squash, path);
    assert_eq!(on_main("dist/app.js").as_deref(), Some("built of two\n"));
    assert_eq!(on_main("dist/assets/two.js").as_deref(), Some("two\n"));
    assert_eq!(on_main("dist/assets/fix.js"), None);
    assert_eq!(on_main("dist/assets/one.js"), None);
    assert!(on_main("sim/jump.gd").is_some() && on_main("sim/other.gd").is_some());
    // The head approved before the merge is as it was: the outputs its own sources make, beyond
    // the candidate nothing main held, as the candidate's advance requires; the delivered head
    // holds main, which that advance never takes.
    let at_fix = |path: &str| show(&r.root, &fix, path);
    assert_eq!(at_fix("dist/app.js").as_deref(), Some("built of the fix\n"));
    assert_eq!(at_fix("dist/assets/fix.js").as_deref(), Some("the fix\n"));
    assert!(ancestor(&r.root, &candidate, &fix) && ancestor(&r.root, &fix, &delivered));
    assert!(!ancestor(&r.root, &before, &fix));
    assert!(ancestor(&r.root, &before, &delivered));
}

#[test]
fn a_head_that_is_no_fix_of_the_candidate_held_now_is_told_to_put_its_outputs_back() {
    // A main task's branch, which holds the candidate and main's own commits beyond it.
    let (r, bare, _) = held();
    let before = head(&bare, "main");
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    write(&wt, "dist/app.js", "built of the task\n");
    git(&wt, &["commit", "-qam", "its own build"]);
    let tip = head(&wt, "HEAD");
    assess(&r, SLUG, &people.assessor, &[], &tip);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(result.unwrap_err(), refused_line("dist/app.js", 1));
    assert!(host.verbs().is_empty());
    assert_nothing_landed(&r, &bare, &before);
    let told = asked(&r);
    assert!(told.ends_with(BACK), "{told}");
    assert!(!told.contains(CANDIDATE), "{told}");

    // A fix started where the candidate was, which moved on since (another red's fix landed):
    // it no longer holds the candidate, so nothing promises the candidate takes it.
    let (r, bare, candidate) = held();
    let (people, _, fix) = fixed(&r);
    assess(&r, SLUG, &people.assessor, &[], &fix);
    let tree = head(&r.root, &format!("{candidate}^{{tree}}"));
    let moved = git(
        &r.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &candidate,
            "-m",
            "another red's fix",
        ],
    );
    git(&r.root, &["branch", "-f", CANDIDATE, &moved]);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        refused_line("dist/app.js, dist/assets/fix.js", 2)
    );
    assert!(host.verbs().is_empty());
    let told = asked(&r);
    assert!(
        told.ends_with(&format!(
            "{BACK}. A fix of the release candidate starts at release-candidate, now at {}, \
             which this head does not hold: rebased there while nothing approved it, its land \
             says how its outputs are delivered; approved already, it lands as any task and its \
             land drops the candidate",
            &moved[..7]
        )),
        "{told}"
    );

    // The candidate ended (released or dropped): its branch is gone, and the head is any task's.
    git(&r.root, &["branch", "-D", CANDIDATE]);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        refused_line("dist/app.js, dist/assets/fix.js", 2)
    );
    assert!(host.verbs().is_empty());
    let told = asked(&r);
    assert!(told.ends_with(BACK), "{told}");
    assert!(!told.contains(CANDIDATE), "{told}");
}

#[test]
fn a_branch_at_the_candidate_with_nothing_of_its_own_is_no_fix_of_it() {
    // The candidate held where main still is, a branch that adds nothing to it: what the remote
    // default holds beyond the candidate decides nothing, as there is none, and there is no fix.
    let r = Repo::new();
    write(&r.root, "dist/app.js", "built of one\n");
    git(&r.root, &["add", "dist"]);
    project_file(&r, &manifest());
    let bare = remote(&r);
    // The candidate on a line main's history does not hold (a head a pull request delivered),
    // whose outputs differ from main's.
    let main = head(&r.root, "main");
    let wt = r.add("line", None, &main);
    write(&wt, "dist/app.js", "built of the line\n");
    git(&wt, &["commit", "-qam", "a delivered head"]);
    let line = head(&wt, "HEAD");
    git(&r.root, &["branch", CANDIDATE, &line]);
    let (people, branch, _) = in_review_from(&r, SLUG, "sim/jump.gd", CANDIDATE);
    git(&branch, &["reset", "-q", "--hard", &line]);
    assess(&r, SLUG, &people.assessor, &[], &line);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(result.unwrap_err(), refused_line("dist/app.js", 1));
    let told = asked(&r);
    assert!(told.ends_with(BACK), "{told}");
    assert!(!told.contains(CANDIDATE), "{told}");
}
