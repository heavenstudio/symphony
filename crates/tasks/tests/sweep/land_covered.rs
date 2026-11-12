//! A task whose work landed with another task's branch, closed as covered by that landing
//! (`just tasks done SLUG NOTE --covered-by OTHER`, `covering::close`) in a scratch repository: the
//! landing's commit is read on main; work of the task's own main lacks refuses, where the board
//! records its executors working (a branch named as its slug, a launch's branch, an in-process
//! executor's branch, detached HEAD or changes no commit has), and not in the main checkout; and
//! the land finds no approve on a covered task, whatever approve came before.

use super::*;
use symphony_board::Beat;
use symphony_board::cli::Env;
use symphony_board::covering::close;

/// The task whose fix landed with `SLUG`'s branch.
const COVERED: &str = "app/covered";

/// `SLUG`'s branch approved and landed: the people, and the merge on main.
fn landed_first(r: &Repo) -> (People, String) {
    manifest(r, "");
    branch(r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(r, SLUG, "good");
    landed(r, SLUG, &people.orch).1.unwrap();
    let merge = head(&r.root, "main");
    (people, merge)
}

/// `COVERED`, filed by the owner and claimed by `exe`.
fn claimed(r: &Repo, exe: &str) {
    r.task(COVERED);
    r.board.store.claim(COVERED, exe).unwrap();
}

/// `tasks ARGS` from the main checkout on the repository's board: the exit code, what it printed
/// and what it said on stderr.
fn run(r: &Repo, args: &[&str]) -> (i32, String, String) {
    let env = Env {
        vars: r.vars(),
        cwd: r.root.clone(),
    };
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn its_executor_closes_it_from_the_command_line_naming_the_merge_on_main() {
    let r = Repo::new();
    let (people, merge) = landed_first(&r);
    claimed(&r, &people.exe);
    let (code, out, err) = run(
        &r,
        &[
            "done",
            COVERED,
            "its fix landed with sim/jump",
            "--covered-by",
            SLUG,
            "--as",
            &people.exe,
        ],
    );
    assert_eq!((code, out.as_str()), (0, "app/covered is done\n"), "{err}");
    let task = r.board.store.get(COVERED).unwrap();
    assert_eq!(task.status, "done");
    let thread = r.board.store.thread(COVERED).unwrap();
    let line = thread.last().unwrap();
    assert_eq!(
        (line.kind.as_str(), line.sender.as_str(), line.text.clone()),
        (
            "verdict",
            people.exe.as_str(),
            format!(
                "covered by {SLUG}, whose landing at {} holds its work; closed by {}: its fix \
                 landed with sim/jump",
                &merge[..12],
                people.exe
            )
        )
    );
    // No landing goes by it, and main stays where it is.
    let (_, result) = landed(&r, COVERED, &people.orch);
    assert_eq!(
        result,
        Err(format!(
            "{COVERED} is done with no approving verdict from a reviewer"
        ))
    );
    assert_eq!(head(&r.root, "main"), merge);
    // Nor does it go back to review.
    let (code, _, err) = run(&r, &["move", COVERED, "review", "--as", &people.exe]);
    assert_eq!(code, 1);
    assert!(
        err.contains("only an approved task no landing has closed goes back to review"),
        "{err}"
    );
}

#[test]
fn a_branch_of_its_own_with_commits_main_lacks_refuses_naming_them() {
    let r = Repo::new();
    let (people, merge) = landed_first(&r);
    claimed(&r, &people.exe);
    let wt = branch(&r, COVERED, "app/covered.gd", "extends Node\n");
    write(&wt, "app/more.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "more on it"]);
    let shorts: Vec<String> = ["HEAD~1", "HEAD"]
        .iter()
        .map(|at| git(&wt, &["rev-parse", "--short", at]))
        .collect();
    let refused = close(
        &r.board.store,
        Some(&r.env(&r.root)),
        COVERED,
        SLUG,
        &people.exe,
        "landed with it",
    );
    assert_eq!(
        refused.map(|t| t.status),
        Err(symphony_board::Error::Refused(format!(
            "{COVERED} has a branch of its own, {COVERED}, with 2 commits main does not hold ({} \
             work on {COVERED}, {} more on it): that work needs its own review and land, not a \
             close as covered by {SLUG}",
            shorts[0], shorts[1]
        )))
    );
    assert_eq!(r.board.store.get(COVERED).unwrap().status, "in_progress");
    assert_eq!(head(&r.root, "main"), merge);
    // Once main holds what it has, its branch is no work of its own.
    git(&r.root, &["merge", "-q", "--ff-only", COVERED]);
    close(
        &r.board.store,
        Some(&r.env(&r.root)),
        COVERED,
        SLUG,
        &people.exe,
        "landed with it",
    )
    .unwrap();
}

/// The branch a launch of its executor recorded (`tasks launch`, `restore`) counts as its own too,
/// whatever it is named; one main holds whole does not refuse.
#[test]
fn the_branch_its_executors_launch_recorded_counts_as_its_own() {
    let r = Repo::new();
    let (people, _) = landed_first(&r);
    claimed(&r, &people.exe);
    let wt = branch(&r, "launched-x", "app/covered.gd", "extends Node\n");
    let store = &r.board.store;
    store
        .record_launch(&NewLaunch {
            worktree: wt.to_string_lossy().into_owned(),
            branch: "launched-x".into(),
            pane: "w1:p1".into(),
            ..NewLaunch::new(COVERED, &people.exe, "executor")
        })
        .unwrap();
    let short = git(&wt, &["rev-parse", "--short", "HEAD"]);
    let refused = close(
        store,
        Some(&r.env(&r.root)),
        COVERED,
        SLUG,
        &people.exe,
        "x",
    );
    assert_eq!(
        refused.map(|t| t.status),
        Err(symphony_board::Error::Refused(format!(
            "{COVERED} has a branch of its own, launched-x, with 1 commit main does not hold \
             ({short} work on launched-x): that work needs its own review and land, not a close \
             as covered by {SLUG}"
        )))
    );
    // A branch that holds nothing main lacks (the covering task's own, say) is no work of its own.
    git(&wt, &["reset", "-q", "--hard", "main"]);
    close(
        store,
        Some(&r.env(&r.root)),
        COVERED,
        SLUG,
        &people.exe,
        "x",
    )
    .unwrap();
}

/// `handle`'s tool calls run in `wt`, on `branch` ("" detached), as an in-process agent's do: the
/// binding the beat hook keeps for it, the place `just tasks agents` shows.
fn works_in(r: &Repo, handle: &str, wt: &Path, branch: &str) {
    let store = &r.board.store;
    assert!(store.bind_agent("agent-a1b2", handle, false).unwrap());
    let place = Some((wt.to_str().unwrap(), branch));
    (store.agent_beat_in("agent-a1b2", &Beat::default(), place, None)).unwrap();
}

/// `COVERED` closed by `exe` from the main checkout, as covered by `SLUG`.
fn closing(r: &Repo, exe: &str) -> Result<String, symphony_board::Error> {
    let env = r.env(&r.root);
    close(
        &r.board.store,
        Some(&env),
        COVERED,
        SLUG,
        exe,
        "landed with it",
    )
    .map(|t| t.status)
}

/// An in-process executor makes no launch record: the branch its binding places it on is read,
/// and refuses while main lacks a commit of it.
#[test]
fn an_in_process_executors_worktree_agent_branch_counts_as_its_own() {
    let r = Repo::new();
    let (people, _) = landed_first(&r);
    claimed(&r, &people.exe);
    let wt = branch(
        &r,
        "worktree-agent-a1b2",
        "app/covered.gd",
        "extends Node\n",
    );
    works_in(&r, &people.exe, &wt, "worktree-agent-a1b2");
    let short = git(&wt, &["rev-parse", "--short", "HEAD"]);
    assert_eq!(
        closing(&r, &people.exe),
        Err(symphony_board::Error::Refused(format!(
            "{COVERED} has a branch of its own, worktree-agent-a1b2, with 1 commit main does not \
             hold ({short} work on worktree-agent-a1b2): that work needs its own review and land, \
             not a close as covered by {SLUG}"
        )))
    );
    assert_eq!(r.board.store.get(COVERED).unwrap().status, "in_progress");
    git(
        &r.root,
        &["merge", "-q", "--ff-only", "worktree-agent-a1b2"],
    );
    assert_eq!(closing(&r, &people.exe), Ok("done".into()));
}

/// Commits on a detached HEAD where the executor works are no branch's, and are read all the same.
#[test]
fn commits_on_a_detached_head_where_its_executor_works_count_as_its_own() {
    let r = Repo::new();
    let (people, _) = landed_first(&r);
    claimed(&r, &people.exe);
    let wt = r.detached("agent-detached");
    write(&wt, "app/covered.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "the fix, detached"]);
    works_in(&r, &people.exe, &wt, "");
    let short = git(&wt, &["rev-parse", "--short", "HEAD"]);
    assert_eq!(
        closing(&r, &people.exe),
        Err(symphony_board::Error::Refused(format!(
            "{COVERED}'s executor {} works in {}, on a detached HEAD, with 1 commit main does \
             not hold ({short} the fix, detached): that work needs its own review and land, not \
             a close as covered by {SLUG}",
            people.exe,
            wt.display()
        )))
    );
    git(&r.root, &["merge", "-q", "--ff-only", &head(&wt, "HEAD")]);
    assert_eq!(closing(&r, &people.exe), Ok("done".into()));
}

/// Changes no commit holds where the executor works refuse, naming them; once they are gone the
/// task closes.
#[test]
fn changes_no_commit_has_where_its_executor_works_refuse_naming_them() {
    let r = Repo::new();
    let (people, _) = landed_first(&r);
    claimed(&r, &people.exe);
    let wt = r.add("agent-dirty", Some("worktree-agent-dirty"), "main");
    write(&wt, "sim/world.gd", "extends Node\n");
    write(&wt, "covered.gd", "extends Node\n");
    works_in(&r, &people.exe, &wt, "worktree-agent-dirty");
    assert_eq!(
        closing(&r, &people.exe),
        Err(symphony_board::Error::Refused(format!(
            "{COVERED}'s executor {} works in {}, which holds changes no commit has (2 paths: \
             sim/world.gd, covered.gd): commit them for their own review and land, or drop \
             them, before a close as covered by {SLUG}",
            people.exe,
            wt.display()
        )))
    );
    assert_eq!(r.board.store.get(COVERED).unwrap().status, "in_progress");
    git(&wt, &["checkout", "--", "sim/world.gd"]);
    fs::remove_file(wt.join("covered.gd")).unwrap();
    assert_eq!(closing(&r, &people.exe), Ok("done".into()));
}

/// The main checkout is every session's, so changes there are not read as the task's, though
/// its executor's calls ran there last.
#[test]
fn changes_in_the_main_checkout_are_not_read_as_its_own() {
    let r = Repo::new();
    let (people, _) = landed_first(&r);
    claimed(&r, &people.exe);
    write(&r.root, "stray.gd", "extends Node\n");
    works_in(&r, &people.exe, &r.root, "main");
    assert_eq!(closing(&r, &people.exe), Ok("done".into()));
}

#[test]
fn a_landing_whose_commit_main_does_not_hold_covers_nothing() {
    let r = Repo::new();
    manifest(&r, "");
    let side = branch(&r, "sim/side", "side.gd", "extends Node\n");
    let off_main = head(&side, "HEAD");
    // A landing line naming a commit main lacks, and one naming no commit git knows.
    let exe = r.board.session("worker");
    for (other, commit) in [("sim/one", off_main.as_str()), ("sim/two", "0123456789ab")] {
        r.task(other);
        r.board.store.claim(other, &exe).unwrap();
        r.board.store.move_to(other, "review", &exe).unwrap();
        r.board
            .store
            .landed(
                other,
                &exe,
                &format!("landed {other} on main at {}", &commit[..12]),
                true,
                &["review"],
            )
            .unwrap();
    }
    claimed(&r, &exe);
    for (other, commit) in [("sim/one", &off_main[..12]), ("sim/two", "0123456789ab")] {
        assert_eq!(
            close(
                &r.board.store,
                Some(&r.env(&r.root)),
                COVERED,
                other,
                &exe,
                "x"
            )
            .map(|t| t.status),
            Err(symphony_board::Error::Refused(format!(
                "{other}'s landing names {commit}, which main does not hold: {COVERED} is closed \
                 as covered only by a landing on main"
            )))
        );
    }
    assert_eq!(r.board.store.get(COVERED).unwrap().status, "in_progress");
}

#[test]
fn outside_a_checkout_of_its_repository_it_is_refused() {
    let r = Repo::new();
    let (people, _) = landed_first(&r);
    claimed(&r, &people.exe);
    let refusal = format!(
        "{COVERED} is closed as covered only from a checkout of its repository, which says \
         whether {SLUG}'s landing is on the default branch and what {COVERED}'s own branch holds"
    );
    let outside = r.env(&r.dir);
    for env in [None, Some(&outside)] {
        assert_eq!(
            close(&r.board.store, env, COVERED, SLUG, &people.exe, "x").map(|t| t.status),
            Err(symphony_board::Error::Refused(refusal.clone()))
        );
    }
    assert_eq!(r.board.store.get(COVERED).unwrap().status, "in_progress");
}

/// An approve the task had before it went back to review stands no more once it is closed as
/// covered: the land refuses it, saying what closed it after, and moves main nowhere.
#[test]
fn the_land_refuses_a_covered_task_whatever_approve_came_before() {
    let r = Repo::new();
    let (people, merge) = landed_first(&r);
    claimed(&r, &people.exe);
    let store = &r.board.store;
    store.move_to(COVERED, "review", &people.exe).unwrap();
    store.take_role(COVERED, &people.rev, "reviewer").unwrap();
    store
        .verdict(COVERED, &people.rev, "approve", "good")
        .unwrap();
    store.move_to(COVERED, "review", &people.rev).unwrap();
    close(
        store,
        Some(&r.env(&r.root)),
        COVERED,
        SLUG,
        &people.exe,
        "landed with it",
    )
    .unwrap();
    let (_, result) = landed(&r, COVERED, &people.orch);
    assert_eq!(
        result,
        Err(format!(
            "{COVERED} was closed after its approval: covered by {SLUG}, whose landing at {} \
             holds its work; closed by {}: landed with it",
            &merge[..12],
            people.exe
        ))
    );
    assert_eq!(head(&r.root, "main"), merge);
    assert_eq!(store.get(COVERED).unwrap().status, "done");
}
