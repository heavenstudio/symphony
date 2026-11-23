//! The shared server's part in the blocks the owner's word ends, on a board and a project of their
//! own: a task blocked on a card goes back once the owner's save decides the card, on main's page
//! or in the worktree of the task that posted it (a subtask's block on its parent's card too), with
//! the verdict named, once; an owner's line that decides nothing leaves it blocked and is told to
//! its orchestrator once; a hold on the owner's approve ends on the approve of its branch's tip
//! alone; a verdict written while no server ran is read at the first look; a move of the owner's
//! in between wins; and a private server leaves them all.

use super::Hub;
use crate::testing::{Tree, board::Fixture, git, repository_holding};
use serde_json::json;
use symphony_board::waits_on::Blocked;
use symphony_board::{NewTask, On};

const DOC: &str = "docs/DECISIONS.md";
const SLUG: &str = "app/x";
/// The branch `SLUG`'s worktree is served under, as a page's scope names it.
const SCOPE: &str = "app~x";
const LAYOUT: &str = "- [ ] **layout** — Wider or narrower? Approve for wider, or reject.\n  \
                      [2026-11-20 04:00] agent — Posted for the board task `app/x`.\n";

fn register(cards: &str) -> String {
    format!("# Decisions\n\n<!-- register:start -->\n{cards}<!-- register:end -->\n")
}

/// A project whose main checkout holds `cards`, checked out off main so no save of its commits.
fn project(tree: &Tree, cards: &str) -> std::path::PathBuf {
    let root = repository_holding(tree, "main", DOC, &register(cards));
    git(&root, &["checkout", "-q", "-b", "review"]);
    root
}

/// The worktree of the branch `SLUG`, which posted the card `layout` in a commit of its own.
fn branch_posting_the_card(tree: &Tree, root: &std::path::Path) -> std::path::PathBuf {
    let worktree = tree.path("wt");
    git(
        root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            SLUG,
            worktree.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(worktree.join(DOC), register(LAYOUT)).unwrap();
    git(&worktree, &["commit", "-qam", "Post layout"]);
    worktree
}

/// The project's server, the shared one when `shared`, its documents read.
fn hub(root: &std::path::Path, shared: bool) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    if shared {
        *hub.decided.get_mut().unwrap() = Some(symphony::decided::Kept::default());
    }
    hub.refresh_docs(None).unwrap();
    hub
}

/// A board whose clock reads two minutes before now, so the owner's saves, dated now, come after
/// what it records; with `SLUG` at `status`, orchestrated by the handle returned, and worked on by
/// an executor when `executing`.
fn board(status: &str, executing: bool) -> (Fixture, String) {
    let f = Fixture::empty();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    f.advance(now - 120.0 - f.now());
    let orch = f.session("orchestrator");
    f.task(SLUG);
    f.store.take_role(SLUG, &orch, "orchestrator").unwrap();
    if executing {
        let worker = f.session("worker");
        f.store.claim(SLUG, &worker).unwrap();
    }
    if status == "review" {
        let worker = f.store.get(SLUG).unwrap().holders("executor")[0].to_string();
        f.store.move_to(SLUG, "review", &worker).unwrap();
    }
    (f, orch)
}

fn block(f: &Fixture, by: &str, on: &str) {
    f.store
        .move_on(
            SLUG,
            "blocked",
            by,
            None,
            Some(On {
                thing: on,
                why: None,
            }),
        )
        .unwrap();
}

fn status(f: &Fixture) -> String {
    f.store.get(SLUG).unwrap().status
}

/// One look of `hub`'s at the board as it is now: whether it moved a task.
fn look(hub: &Hub, f: &Fixture) -> bool {
    hub.keep_decided(&f.store, &f.store.board().unwrap())
}

/// The lines on `SLUG`'s thread that begin `start`.
fn lines(f: &Fixture, start: &str) -> Vec<String> {
    (f.store.thread(SLUG).unwrap().into_iter())
        .map(|m| m.text)
        .filter(|text| text.starts_with(start))
        .collect()
}

/// The owner's save of `payload` on `route`, from the page of `scope` ("" main's).
fn save(root: &std::path::Path, scope: &str, route: &str, payload: serde_json::Value) {
    symphony::save::Saves::start(root)
        .post(scope, route, &payload)
        .unwrap_or_else(|e| panic!("{route}: {} {}", e.status, e.error));
}

fn decide(root: &std::path::Path, scope: &str, state: &str, note: &str) {
    let payload = json!({"path": DOC, "slug": "layout", "state": state, "note": note});
    save(root, scope, "/api/decision", payload);
}

#[test]
fn a_block_on_a_card_ends_once_the_owner_approves_it_on_mains_page() {
    let tree = Tree::new();
    let root = project(&tree, LAYOUT);
    let (f, orch) = board("ready", false);
    block(&f, &orch, "decision:layout");
    let shared = hub(&root, true);
    assert!(!look(&shared, &f), "undecided: nothing moves");
    decide(&root, "", "approved", "");
    assert!(!look(&shared, &f), "the page has not read the save yet");
    // The watcher reads the saved document again; the next look ends the block.
    shared.refresh_docs(None).unwrap();
    hub(&root, false).keep_decided(&f.store, &f.store.board().unwrap());
    assert_eq!(status(&f), "blocked", "a private server leaves it");
    assert!(look(&shared, &f));
    let task = f.store.get(SLUG).unwrap();
    assert_eq!(task.status, "ready");
    assert!(Blocked::of(&task).is_none());
    assert!(!look(&shared, &f));
    assert_eq!(
        lines(&f, "unblocked: "),
        ["unblocked: the owner decided layout (approve); back to ready"]
    );
}

/// The case that asked for it: the card posted on the task's own branch, approved from the branch's
/// page, the save in that worktree and uncommitted.
#[test]
fn a_card_its_own_branch_posted_ends_its_block_once_approved_in_its_worktree() {
    let tree = Tree::new();
    let root = project(&tree, "");
    let worktree = branch_posting_the_card(&tree, &root);
    let (f, orch) = board("in_progress", true);
    block(&f, &orch, "decision:layout");
    decide(&root, SCOPE, "approved", "wider it is");
    let saved = std::fs::read_to_string(worktree.join(DOC)).unwrap();
    assert!(saved.contains("owner · approved — wider it is"), "{saved}");
    assert!(
        !std::fs::read_to_string(root.join(DOC))
            .unwrap()
            .contains("layout")
    );
    let shared = hub(&root, true);
    assert!(look(&shared, &f));
    assert_eq!(status(&f), "in_progress", "its executor still holds it");
    assert_eq!(
        lines(&f, "unblocked: "),
        ["unblocked: the owner decided layout (approve); back to in_progress"]
    );
}

/// The case as a consuming project met it: a subtask's executor posted the card on its parent's
/// branch and blocked the subtask on it, and the owner approved it, with a note naming the option,
/// from that branch's page, the save in the parent's worktree and uncommitted.
#[test]
fn a_subtasks_block_ends_on_a_card_its_parents_branch_posted_answered_in_that_worktree() {
    let tree = Tree::new();
    let root = project(&tree, "");
    branch_posting_the_card(&tree, &root);
    let (f, _) = board("in_progress", true);
    let sub = format!("{SLUG}/layout");
    f.task_with(NewTask {
        parent: Some(SLUG.into()),
        ..NewTask::new(&sub, "Settle the layout", "owner")
    });
    let worker = f.session("worker");
    f.store.claim(&sub, &worker).unwrap();
    let on = On {
        thing: "decision:layout",
        why: Some("wider or narrower"),
    };
    (f.store)
        .move_on(&sub, "blocked", &worker, None, Some(on))
        .unwrap();
    decide(&root, SCOPE, "approved", "wider, as the first option says");
    let shared = hub(&root, true);
    assert!(look(&shared, &f));
    assert_eq!(f.store.get(&sub).unwrap().status, "in_progress");
    assert_eq!(status(&f), "in_progress", "its parent is left as it was");
    let unblocked: Vec<String> = (f.store.thread(&sub).unwrap().into_iter())
        .map(|m| m.text)
        .filter(|text| text.starts_with("unblocked: "))
        .collect();
    assert_eq!(
        unblocked,
        ["unblocked: the owner decided layout (approve); back to in_progress"]
    );
    assert!(lines(&f, "unblocked: ").is_empty());
}

#[test]
fn a_reject_ends_it_with_the_verdict_named_for_its_executor() {
    let tree = Tree::new();
    let root = project(&tree, "");
    branch_posting_the_card(&tree, &root);
    let (f, orch) = board("in_progress", false);
    block(&f, &orch, "decision:layout");
    decide(&root, SCOPE, "rejected", "neither: keep it as it is");
    let shared = hub(&root, true);
    assert!(look(&shared, &f));
    // Work in progress whose executor let go goes back to ready, as a task block's does.
    assert_eq!(status(&f), "ready");
    assert_eq!(
        lines(&f, "unblocked: "),
        ["unblocked: the owner decided layout (reject); back to ready"]
    );
}

#[test]
fn a_comment_leaves_it_blocked_and_is_told_to_its_orchestrator_once() {
    let tree = Tree::new();
    let root = project(&tree, LAYOUT);
    let (f, orch) = board("ready", false);
    block(&f, &orch, "decision:layout");
    let message = json!({"path": DOC, "slug": "layout", "text": "Which screens does it change?"});
    save(&root, "", "/api/message", message);
    let shared = hub(&root, true);
    for _ in 0..2 {
        assert!(!look(&shared, &f));
    }
    // A server started again asks the board, which says it once.
    assert!(!look(&hub(&root, true), &f));
    assert_eq!(status(&f), "blocked");
    let told = lines(&f, "the owner on card layout");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].ends_with(
            "(docs/DECISIONS.md#layout), deciding nothing, so it stays blocked on \
             decision:layout: Which screens does it change?"
        ),
        "{told:?}"
    );
    let heard = f.store.inbox(&orch).unwrap();
    assert!(heard.iter().any(|m| m.text == told[0]), "{heard:?}");
    // Then the owner decides: it ends.
    decide(&root, "", "approved", "");
    shared.refresh_docs(None).unwrap();
    assert!(look(&shared, &f));
    assert_eq!(status(&f), "ready");
}

#[test]
fn a_hold_on_the_owners_approve_ends_on_his_approve_of_the_tip_alone() {
    let tree = Tree::new();
    let root = project(&tree, "");
    let worktree = branch_posting_the_card(&tree, &root);
    let rev =
        |name: &str| symphony_board::sweep::git::rev(&root, name).expect("the commit is there");
    let (posted, forked) = (rev(SLUG), rev("main"));
    let (f, orch) = board("review", true);
    // The default landing's hold: blocked from review, on the owner's approve.
    f.store
        .hold_blocked_on(
            SLUG,
            &orch,
            Some(On {
                thing: "check:owner",
                why: None,
            }),
        )
        .unwrap();
    let shared = hub(&root, true);
    assert!(!look(&shared, &f));
    f.store
        .say(SLUG, "owner", &format!("approve {forked}"), None)
        .unwrap();
    assert!(!look(&shared, &f), "an approve of another commit");
    // A commit after the owner's approve of the tip it had: the approve names no tip now.
    f.store
        .say(SLUG, "owner", &format!("approve {posted}"), None)
        .unwrap();
    std::fs::write(worktree.join("NOTE.md"), "more\n").unwrap();
    git(&worktree, &["add", "NOTE.md"]);
    git(&worktree, &["commit", "-qm", "More"]);
    assert!(!look(&shared, &f), "the tip moved past the approve");
    assert_eq!(status(&f), "blocked");
    let tip = rev(SLUG);
    f.store
        .say(SLUG, "owner", &format!("approve {tip}"), None)
        .unwrap();
    assert!(look(&shared, &f));
    assert_eq!(status(&f), "review");
    assert_eq!(
        lines(&f, "unblocked: "),
        [format!(
            "unblocked: the owner approved {}; back to review",
            &tip[..7]
        )]
    );
}

/// The owner's approve of the card the task's branch posted ends the hold too, once a commit
/// carries it: the line saved in the worktree alone counts for nothing, as the land carries commits.
#[test]
fn a_hold_on_the_owners_approve_ends_on_his_approve_of_the_card_its_branch_posted_committed() {
    let tree = Tree::new();
    let root = project(&tree, "");
    let worktree = branch_posting_the_card(&tree, &root);
    let (f, orch) = board("review", true);
    f.store
        .hold_blocked_on(
            SLUG,
            &orch,
            Some(On {
                thing: "check:owner",
                why: None,
            }),
        )
        .unwrap();
    // The owner approves the card on the branch's page: saved in its worktree, no commit yet.
    decide(&root, SCOPE, "approved", "wider it is");
    let saved = std::fs::read_to_string(worktree.join(DOC)).unwrap();
    assert!(saved.contains("owner · approved — wider it is"), "{saved}");
    assert!(
        !look(&hub(&root, true), &f),
        "an owner's line no commit carries"
    );
    assert_eq!(status(&f), "blocked");
    git(
        &worktree,
        &["commit", "-qam", "The owner's approve of layout"],
    );
    let tip = symphony_board::sweep::git::rev(&root, SLUG).expect("the branch is there");
    assert!(look(&hub(&root, true), &f));
    assert_eq!(status(&f), "review");
    assert_eq!(
        lines(&f, "unblocked: "),
        [format!(
            "unblocked: the owner approved card layout, at {}; back to review",
            &tip[..7]
        )]
    );
}

/// A hold whose task has no branch of its name (a landing of another branch's) names no tip: it
/// stays, and the look says nothing went wrong.
#[test]
fn a_hold_whose_task_has_no_branch_stays_held() {
    let tree = Tree::new();
    let root = project(&tree, "");
    let (f, orch) = board("review", true);
    f.store
        .hold_blocked_on(
            SLUG,
            &orch,
            Some(On {
                thing: "check:owner",
                why: None,
            }),
        )
        .unwrap();
    let main = symphony_board::sweep::git::rev(&root, "main").unwrap();
    f.store
        .say(SLUG, "owner", &format!("approve {main}"), None)
        .unwrap();
    let mut kept = symphony::decided::Kept::default();
    let board = f.store.board().unwrap();
    let looked = kept.look(&f.store, &root, &board, &Default::default(), 0.0);
    assert_eq!(looked, symphony::decided::Looked::default());
    assert_eq!(status(&f), "blocked");
}

/// No server ran when the owner's line was written (by hand, or a save of another build's that
/// moved nothing): the first look reads it.
#[test]
fn a_verdict_written_while_no_server_ran_is_read_at_the_first_look() {
    let tree = Tree::new();
    let root = project(&tree, LAYOUT);
    let (f, orch) = board("ready", false);
    block(&f, &orch, "decision:layout");
    let stamp = symphony::clock::stamp_at((f.now() + 60.0) as libc::time_t);
    let answered = format!("{LAYOUT}  [{stamp}] owner · dismissed — not needed now\n");
    std::fs::write(root.join(DOC), register(&answered)).unwrap();
    assert!(look(&hub(&root, true), &f));
    assert_eq!(
        lines(&f, "unblocked: "),
        ["unblocked: the owner decided layout (dismiss); back to ready"]
    );
}

#[test]
fn an_owners_move_in_between_wins() {
    let tree = Tree::new();
    let root = project(&tree, LAYOUT);
    let (f, orch) = board("ready", false);
    block(&f, &orch, "decision:layout");
    decide(&root, "", "approved", "");
    // He moved it himself before any server looked.
    f.store.move_to(SLUG, "review", "owner").unwrap();
    let shared = hub(&root, true);
    assert!(!look(&shared, &f));
    assert_eq!(status(&f), "review");
    // Blocked on the same card again later: the owner's earlier verdict decides nothing of the new
    // block.
    f.advance(600.0);
    block(&f, &orch, "decision:layout");
    assert!(!look(&shared, &f));
    assert_eq!(status(&f), "blocked");
    assert!(lines(&f, "unblocked: ").is_empty());
}

#[test]
fn other_blocks_stay_whatever_the_cards_say() {
    let tree = Tree::new();
    let root = project(&tree, LAYOUT);
    decide(&root, "", "approved", "");
    let shared = hub(&root, true);
    for on in ["decision:lamp", "check:tests", "parked"] {
        let (f, orch) = board("ready", false);
        block(&f, &orch, on);
        assert!(!look(&shared, &f), "{on}");
        assert_eq!(status(&f), "blocked", "{on}");
    }
}
