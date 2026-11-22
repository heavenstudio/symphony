//! `tasks land` on a task a custom workflow holds (docs/WORKFLOW.md, "External effects"), in
//! scratch repositories on a board of their own: it lands by the prepare and finalize edges of the
//! default landing's adapter and by nothing else, refuses before any merge a task no such edge
//! leaves, binds the head its approve read, withdraws a reservation that went stale, and finishes
//! a land interrupted between its merge and its finalize by the same intent.

use super::gates::{BUDGET, GATE, decide, raise};
use super::stamps::told;
use super::*;
use crate::common::pair;
use std::collections::BTreeMap;
use symphony_api::workflow::{InputValue, OperationRequest, Pair};
use symphony_board::Store;
use symphony_board::land::adapter::DefaultLand;
use symphony_board::stamps::Catalogue;
use symphony_board::workflow::effect::{DEFAULT_LAND, Intent, Stage};
use symphony_board::workflow::{Followed, Host, agent_session, follow, local_owner};
use symphony_plugin::execution::Principal;

/// The workflow whose landing is the default landing's.
const WORKFLOW: &str = include_str!("../fixtures/landing_workflow.toml");

/// The project file on main: `check` and the workflow, followed on the board.
fn workflowed(r: &Repo) {
    manifest(r, &format!("\n{WORKFLOW}"));
    let followed = follow(&r.board.store, &r.root).unwrap();
    assert!(matches!(followed, Followed::Activated(_)), "{followed:?}");
}

fn at(r: &Repo, slug: &str) -> (Pair, u64) {
    let position = r.board.store.workflow_position(slug).unwrap().unwrap();
    (position.at(), position.revision)
}

/// `edge` taken on `slug` by `principal`, its `head` input given where `head` is.
fn take(store: &Store, slug: &str, principal: &Principal, edge: &str, head: Option<&str>) {
    let next = store
        .workflow_actions(slug, principal, &Host::local())
        .unwrap();
    let mut inputs = BTreeMap::new();
    if let Some(head) = head {
        inputs.insert("head".to_string(), InputValue::Revision(head.to_string()));
    }
    let request = OperationRequest {
        project: store.database().database().into(),
        task: slug.into(),
        edge: edge.into(),
        expected: next.snapshot,
        input_revisions: BTreeMap::new(),
        idempotency_key: format!("{slug}:{edge}:{}", head.unwrap_or("")),
        inputs,
    };
    store
        .apply_operation(principal, &Host::local(), &request)
        .unwrap_or_else(|why| panic!("{edge} on {slug}: {why:?}"));
}

/// `slug` filed under the workflow with its branch, orchestrated, built and submitted by its
/// executor: the people, the worktree and its tip. It awaits review.
fn submitted(r: &Repo, slug: &str) -> (People, PathBuf, String) {
    let wt = branch(r, slug, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let store = &r.board.store;
    let people = People {
        orch: r.board.session("orchestrator"),
        exe: r.board.session("worker"),
        rev: r.board.session("reviewer"),
    };
    r.task(slug);
    store.take_role(slug, &people.orch, "orchestrator").unwrap();
    let exe = agent_session(&people.exe);
    take(store, slug, &exe, "claim", None);
    take(store, slug, &exe, "submit", None);
    assert_eq!(at(r, slug).0, pair("verify", "awaiting"));
    (people, wt, tip)
}

/// [`submitted`], then approved by its reviewer, which read `read` (its tip when none).
fn approved_by_edge(r: &Repo, slug: &str, read: Option<&str>) -> (People, PathBuf, String) {
    let (people, wt, tip) = submitted(r, slug);
    let rev = agent_session(&people.rev);
    take(
        &r.board.store,
        slug,
        &rev,
        "approve",
        Some(read.unwrap_or(&tip)),
    );
    assert_eq!(at(r, slug), (pair("deliver", "approved"), 4));
    (people, wt, tip)
}

fn adapter(r: &Repo) -> DefaultLand<'_> {
    DefaultLand {
        store: &r.board.store,
        env: Box::leak(Box::new(r.env(&r.root))),
        main: &r.root,
        folder: r.root.join(".git/symphony/tasks/land"),
        branch: "main",
    }
}

/// `slug`'s prepare edge taken for the default landing as the land takes it, bound to `tip`: its
/// intent, at workflow revision 5.
fn prepared(r: &Repo, slug: &str, orch: &str, tip: &str) -> Intent {
    let store = &r.board.store;
    let principal = agent_session(orch);
    let land = adapter(r);
    let next = store
        .workflow_actions(slug, &principal, &Host::local())
        .unwrap();
    let request = OperationRequest {
        project: store.database().database().into(),
        task: slug.into(),
        edge: "prepare_land".into(),
        expected: next.snapshot,
        input_revisions: BTreeMap::new(),
        idempotency_key: format!("land:{tip}:4"),
        inputs: BTreeMap::from([("head".to_string(), InputValue::Revision(tip.into()))]),
    };
    let bindings = land.bindings(slug, tip).unwrap();
    store
        .prepare_effect(&principal, &Host::local(), &land, &request, &bindings)
        .unwrap()
}

/// The merge of `tip` a land killed after it moved main left: main at one merge commit of it, the
/// main checkout with it, and the land's record of it. The merge.
fn merged_as_a_land_left_it(r: &Repo, slug: &str, tip: &str) -> String {
    git(
        &r.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("Merge branch '{slug}'"),
            tip,
        ],
    );
    let merge = head(&r.root, "main");
    let folder = r.root.join(".git/symphony/tasks/land");
    fs::create_dir_all(&folder).unwrap();
    Record {
        slug: slug.into(),
        tip: tip.into(),
        merge: merge.clone(),
        checked: merge.clone(),
        regenerated: Vec::new(),
    }
    .write(&folder)
    .unwrap();
    merge
}

fn intent(r: &Repo, slug: &str) -> Intent {
    r.board.store.effect_intent(slug, 5).unwrap().unwrap()
}

// -- nothing lands past the graph -------------------------------------------------------------------

#[test]
fn a_managed_task_at_a_pair_no_prepare_edge_leaves_is_refused_before_anything_is_merged() {
    let r = Repo::new();
    workflowed(&r);
    let (people, _wt, _tip) = submitted(&r, SLUG);
    let before = head(&r.root, "main");
    // Awaiting review (the board's review), then accepted by the owner (the board's done): at
    // neither does a prepare edge of the default landing leave its pair.
    for (state, status) in [("awaiting", "review"), ("accepted", "done")] {
        if state == "accepted" {
            take(&r.board.store, SLUG, &local_owner(), "accept", None);
        }
        assert_eq!(r.board.store.get(SLUG).unwrap().status, status);
        let (said, result) = landed(&r, SLUG, &people.orch);
        let line = result.unwrap_err();
        assert!(
            line.starts_with(&format!(
                "explicit_action_required: a custom workflow holds {SLUG}"
            )),
            "{line}"
        );
        assert!(
            line.ends_with(&format!(
                "it is at verify/{state}, which no prepare_land edge of the adapter default_land \
                 leaves, so nothing is merged"
            )),
            "{line}"
        );
        assert_eq!(said, "", "nothing was merged, checked or pushed");
        assert_eq!(head(&r.root, "main"), before);
        assert_eq!(runs(&r), Vec::<String>::new());
        assert_eq!(at(&r, SLUG).0, pair("verify", state));
        assert!(!r.board.said(SLUG).iter().any(|l| l.starts_with("landed ")));
    }
}

#[test]
fn a_managed_task_is_not_delivered_by_a_pull_request() {
    let r = Repo::new();
    manifest(
        &r,
        &format!("\n[land]\ndelivery = [\"host\", \"deliver\"]\n\n{WORKFLOW}"),
    );
    follow(&r.board.store, &r.root).unwrap();
    let (people, _wt, _tip) = approved_by_edge(&r, SLUG, None);
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    assert!(
        line.contains(
            "a land delivered by a pull request ([land] delivery) takes no workflow edge yet"
        ),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(at(&r, SLUG), (pair("deliver", "approved"), 4));
}

// -- prepare, dispatch, finalize --------------------------------------------------------------------

#[test]
fn an_approved_managed_task_lands_by_its_prepare_and_finalize_edges() {
    let r = Repo::new();
    workflowed(&r);
    let (people, _wt, tip) = approved_by_edge(&r, SLUG, None);
    let base = head(&r.root, "main");
    let policy = head(&r.root, "main:symphony.toml");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    let merge = head(&r.root, "main");
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, by its workflow ({SLUG}@5); not pushed: no origin remote",
            &merge[..12]
        )
    );
    // One merge commit of the bound head, checked once, as any land's.
    assert_eq!(head(&r.root, "main^1"), base);
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(runs(&r), [format!("{SLUG} {}", people.orch)]);
    assert!(
        said.contains(&format!(
            "land {SLUG}: prepared {SLUG}@5 by its edge prepare_land, bound to its head {}",
            &tip[..7]
        )),
        "{said}"
    );
    // The graph says where it is, and the intent what happened.
    assert_eq!(at(&r, SLUG), (pair("deliver", "landed"), 6));
    let task = r.board.store.get(SLUG).unwrap();
    assert_eq!(task.status, "done");
    let intent = intent(&r, SLUG);
    assert_eq!(intent.stage, Stage::Finalized);
    assert_eq!(intent.adapter, DEFAULT_LAND);
    assert_eq!(intent.receipt.as_deref(), Some(merge.as_str()));
    assert_eq!(intent.finalized.as_deref(), Some("sim/jump@6"));
    assert_eq!(intent.recovery, None);
    let bound = &intent.bindings;
    assert_eq!(bound.head, tip);
    assert_eq!(bound.task, SLUG);
    assert_eq!(bound.policy, policy);
    assert_eq!(bound.destination, "refs/heads/main");
    assert_eq!(bound.pull, None);
    assert!(
        Path::new(&bound.repository).ends_with(".git"),
        "{}",
        bound.repository
    );
    assert_eq!(bound.evidence.len(), 64);
    // Its thread: prepared, finalized with the merge read back, then the land's own line.
    let thread = r.board.said(SLUG);
    assert!(
        thread
            .iter()
            .any(|l| l.starts_with("prepared prepare_land for default_land"))
    );
    assert!(
        thread.iter().any(|l| l.contains(&format!(
            "finalized {SLUG}@5 by default_land ({SLUG}@6): {merge}"
        ))),
        "{thread:?}"
    );
    assert_eq!(last(&r, SLUG), line);
    // Its record is dropped and its worktree gone.
    assert_eq!(
        Record::read(&r.root.join(".git/symphony/tasks/land"), SLUG),
        None
    );
    assert!(!r.root.join(".git/symphony/tasks/land/worktree").exists());
    // Landed: a land again merges nothing and says so.
    let (_, again) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        again.unwrap_err(),
        format!(
            "{SLUG} landed already, at {} ({SLUG}@5): nothing to land",
            &merge[..7]
        )
    );
    assert_eq!(head(&r.root, "main"), merge);
}

#[test]
fn a_head_its_approve_did_not_read_is_refused_before_it_is_prepared() {
    let r = Repo::new();
    workflowed(&r);
    let (people, wt, read) = approved_by_edge(&r, SLUG, None);
    write(&wt, "sim/more.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "more, after the approve"]);
    let tip = head(&wt, "HEAD");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        result.unwrap_err(),
        format!(
            "{SLUG}'s approve ({SLUG}@4) read {}, and its tip is {}: the head reviewed is the \
             head landed",
            &read[..7],
            &tip[..7]
        )
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(at(&r, SLUG), (pair("deliver", "approved"), 4));
    assert_eq!(r.board.store.effect_intent(SLUG, 5).unwrap(), None);
    let told = last(&r, SLUG);
    assert!(told.starts_with("land refused: "), "{told}");
    assert!(
        told.contains("A fresh reviewer approves the tip by the workflow's approve edge"),
        "{told}"
    );
    // A prepare naming the tip, sent past the land, binds a head the receipt does not hold: the
    // land that follows it dispatches nothing under it.
    let intent = prepared(&r, SLUG, &people.orch, &read);
    assert_eq!(intent.bindings.head, read);
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    assert!(
        line.starts_with(&format!("stale_input: {SLUG}@5 is withdrawn")),
        "{line}"
    );
    assert!(
        line.contains(&format!("its head was {read} when prepared and is {tip}")),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before);
}

#[test]
fn a_reservation_that_went_stale_is_withdrawn_and_nothing_is_merged() {
    let r = Repo::new();
    workflowed(&r);
    let (people, _wt, tip) = approved_by_edge(&r, SLUG, None);
    prepared(&r, SLUG, &people.orch, &tip);
    // Cancelled by an edge out before the land dispatched it.
    let orch = agent_session(&people.orch);
    take(&r.board.store, SLUG, &orch, "send_back", None);
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    // It awaits review again: no prepare edge leaves that pair, so nothing is asked of the intent.
    assert!(
        line.contains("it is at verify/awaiting, which no prepare_land edge"),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(runs(&r), Vec::<String>::new());
    // The intent itself refuses a dispatch, and is withdrawn.
    let why = (r.board.store)
        .dispatch_effect(&adapter(&r), SLUG, 5)
        .unwrap_err()
        .to_string();
    assert!(why.contains("moved on to verify/awaiting"), "{why}");
    assert_eq!(intent(&r, SLUG).stage, Stage::Withdrawn);
}

#[test]
fn a_land_killed_between_its_merge_and_its_finalize_is_finished_by_the_same_intent() {
    let r = Repo::new();
    workflowed(&r);
    let (people, _wt, tip) = approved_by_edge(&r, SLUG, None);
    prepared(&r, SLUG, &people.orch, &tip);
    r.board
        .store
        .dispatch_effect(&adapter(&r), SLUG, 5)
        .unwrap();
    let merge = merged_as_a_land_left_it(&r, SLUG, &tip);
    assert_eq!(at(&r, SLUG), (pair("deliver", "landing"), 5));
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, by its workflow ({SLUG}@5), finishing a land \
             interrupted after its merge; not pushed: no origin remote",
            &merge[..12]
        )
    );
    assert!(
        said.contains("finishing the same operation (sim/jump@5), merging nothing"),
        "{said}"
    );
    // Nothing merged again, nothing checked again.
    assert_eq!(head(&r.root, "main"), merge);
    assert_eq!(runs(&r), Vec::<String>::new());
    assert_eq!(at(&r, SLUG), (pair("deliver", "landed"), 6));
    let intent = intent(&r, SLUG);
    assert_eq!(intent.receipt.as_deref(), Some(merge.as_str()));
    assert_eq!(intent.recovery, None);
    assert_eq!(last(&r, SLUG), line);
}

#[test]
fn a_dispatched_land_whose_merge_never_happened_is_dispatched_and_merged_again() {
    let r = Repo::new();
    workflowed(&r);
    let (people, _wt, tip) = approved_by_edge(&r, SLUG, None);
    prepared(&r, SLUG, &people.orch, &tip);
    r.board
        .store
        .dispatch_effect(&adapter(&r), SLUG, 5)
        .unwrap();
    let (said, result) = landed(&r, SLUG, &people.orch);
    result.unwrap();
    assert!(
        said.contains("is dispatched, and nothing of it is on main: dispatching it again"),
        "{said}"
    );
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(at(&r, SLUG), (pair("deliver", "landed"), 6));
}

#[test]
fn a_policy_changed_between_the_prepare_and_the_merge_records_the_merge_and_the_recovery() {
    let r = Repo::new();
    workflowed(&r);
    let (people, _wt, tip) = approved_by_edge(&r, SLUG, None);
    let bound = prepared(&r, SLUG, &people.orch, &tip).bindings.policy;
    r.board
        .store
        .dispatch_effect(&adapter(&r), SLUG, 5)
        .unwrap();
    // The request is in flight; main gains another landing policy before the merge is made.
    let text = fs::read_to_string(r.root.join("symphony.toml")).unwrap();
    write(
        &r.root,
        "symphony.toml",
        &format!("# another policy\n{text}"),
    );
    git(&r.root, &["commit", "-qam", "another landing policy"]);
    let now = head(&r.root, "main:symphony.toml");
    assert_ne!(bound, now);
    let merge = merged_as_a_land_left_it(&r, SLUG, &tip);
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert!(
        line.ends_with(&format!(
            "; RECOVERY REQUIRED: the landing policy was {bound} when prepared and {now} when \
             the effect happened"
        )),
        "{line}"
    );
    // The merge happened, so it is recorded and the edge taken; the requirement is on the record.
    assert_eq!(at(&r, SLUG), (pair("deliver", "landed"), 6));
    let intent = intent(&r, SLUG);
    assert_eq!(intent.receipt.as_deref(), Some(merge.as_str()));
    assert_eq!(intent.bindings.policy, bound);
    assert!(
        intent
            .recovery
            .unwrap()
            .starts_with("the landing policy was ")
    );
    let thread = r.board.store.thread(SLUG).unwrap();
    let told = thread
        .iter()
        .find(|m| m.text.contains("RECOVERY REQUIRED"))
        .unwrap();
    assert_eq!(told.recipient.as_deref(), Some("@orchestrator"));
}

#[test]
fn a_branch_that_changes_its_own_stamps_lands_with_no_recovery_owed() {
    let r = Repo::new();
    workflowed(&r);
    let (people, wt, _) = submitted(&r, SLUG);
    // The branch brings a catalogue under which it would need `tests` too: the main checkout's
    // head names it once the merge is made, and the land is judged by the one it was prepared
    // under, which the merge's first parent holds.
    let text = fs::read_to_string(wt.join("symphony.toml")).unwrap();
    write(
        &wt,
        "symphony.toml",
        &format!("{text}\n[stamps.review]\n\n[stamps.tests]\ndefault = true\n"),
    );
    git(&wt, &["commit", "-qam", "tests needed by default"]);
    let tip = head(&wt, "HEAD");
    let rev = agent_session(&people.rev);
    take(&r.board.store, SLUG, &rev, "approve", Some(&tip));
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert!(!line.contains("RECOVERY REQUIRED"), "{line}");
    let merge = head(&r.root, "main");
    assert_eq!(head(&r.root, "main^2"), tip);
    // The head's catalogue is the branch's now, and names another need than the one bound.
    let (before, after) = (
        Catalogue::committed(&r.root, "main^1").unwrap().defaults(),
        Catalogue::of(&r.root).unwrap().defaults(),
    );
    assert_eq!(before, ["review"]);
    assert_eq!(after, ["tests", "review"]);
    assert_eq!(at(&r, SLUG), (pair("deliver", "landed"), 6));
    let intent = intent(&r, SLUG);
    assert_eq!(intent.stage, Stage::Finalized);
    assert_eq!(intent.receipt.as_deref(), Some(merge.as_str()));
    assert_eq!(intent.recovery, None);
    let thread = r.board.said(SLUG);
    assert!(
        !thread.iter().any(|l| l.contains("RECOVERY REQUIRED")),
        "{thread:?}"
    );
}

#[test]
fn a_land_killed_after_its_finalize_says_its_landed_line_once() {
    let r = Repo::new();
    workflowed(&r);
    let (people, _wt, tip) = approved_by_edge(&r, SLUG, None);
    prepared(&r, SLUG, &people.orch, &tip);
    let land = adapter(&r);
    r.board.store.dispatch_effect(&land, SLUG, 5).unwrap();
    let merge = merged_as_a_land_left_it(&r, SLUG, &tip);
    let orch = agent_session(&people.orch);
    (r.board.store)
        .finalize_effect(&orch, &Host::local(), &land, SLUG, 5)
        .unwrap();
    assert_eq!(at(&r, SLUG), (pair("deliver", "landed"), 6));
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, by its workflow ({SLUG}@5), finishing a land \
             interrupted after its finalize",
            &merge[..12]
        )
    );
    assert_eq!(last(&r, SLUG), line);
    assert_eq!(
        Record::read(&r.root.join(".git/symphony/tasks/land"), SLUG),
        None
    );
    let (_, again) = landed(&r, SLUG, &people.orch);
    assert!(again.unwrap_err().contains("landed already"));
    assert_eq!(head(&r.root, "main"), merge);
}

#[test]
fn a_withdrawn_intent_lands_nothing_until_an_edge_leaves_its_pair() {
    let r = Repo::new();
    workflowed(&r);
    let (people, wt, tip) = approved_by_edge(&r, SLUG, None);
    prepared(&r, SLUG, &people.orch, &tip);
    // The branch moves under the reservation: the head bound is not the head there.
    write(&wt, "sim/more.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "more, after the prepare"]);
    let before = head(&r.root, "main");
    let (_, first) = landed(&r, SLUG, &people.orch);
    assert!(
        first
            .unwrap_err()
            .starts_with("stale_input: sim/jump@5 is withdrawn")
    );
    let told = last(&r, SLUG);
    assert!(
        told.contains("Nothing was merged. Take an edge out of its pair"),
        "{told}"
    );
    let (said, again) = landed(&r, SLUG, &people.orch);
    let line = again.unwrap_err();
    assert!(
        line.starts_with("sim/jump@5 is withdrawn (its head was "),
        "{line}"
    );
    assert!(line.ends_with("then land it again"), "{line}");
    assert_eq!(said, "");
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(runs(&r), Vec::<String>::new());
}

/// A managed task lands a raise its gate decision gave only carried at the number decided, as the
/// default land does: a tip setting it to another is refused to its executor before the prepare
/// edge is taken, and a decision changes the evidence the land binds.
#[test]
fn a_raise_the_tip_does_not_carry_refuses_a_managed_task_before_its_prepare_edge() {
    let r = Repo::new();
    workflowed(&r);
    write(&r.root, BUDGET, "symphony = 100_000\n");
    git(&r.root, &["add", BUDGET]);
    git(&r.root, &["commit", "-qm", "the budget"]);
    let (people, wt, _) = submitted(&r, SLUG);
    write(&wt, BUDGET, "symphony = 120_000\n");
    git(&wt, &["commit", "-qam", "past the line"]);
    let tip = head(&wt, "HEAD");
    take(
        &r.board.store,
        SLUG,
        &agent_session(&people.rev),
        "approve",
        Some(&tip),
    );
    let unbound = adapter(&r).bindings(SLUG, &tip).unwrap().evidence;
    decide(&r, SLUG, raise(&people.orch, 100_000, 110_000));
    assert_ne!(
        adapter(&r).bindings(SLUG, &tip).unwrap().evidence,
        unbound,
        "a decision is evidence the land binds"
    );
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        result.unwrap_err(),
        format!(
            "{GATE} raised 100,000 -> 110,000 by {}, but the tip {} sets it to 120,000, which no \
             decision gave: a raise lands at the number decided",
            people.orch,
            &tip[..7]
        )
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(at(&r, SLUG), (pair("deliver", "approved"), 4));
    assert_eq!(r.board.store.effect_intent(SLUG, 5).unwrap(), None);
    assert_eq!(told(&r, SLUG).1.as_deref(), Some("@executor"));
    // Carried at the number decided, another lands by its edges, its line naming the decision.
    let slug = "sim/run";
    let (people, wt, _) = submitted(&r, slug);
    write(&wt, BUDGET, "symphony = 110_000\n");
    git(&wt, &["commit", "-qam", "at the line decided"]);
    let tip = head(&wt, "HEAD");
    take(
        &r.board.store,
        slug,
        &agent_session(&people.rev),
        "approve",
        Some(&tip),
    );
    decide(&r, slug, raise(&people.orch, 100_000, 110_000));
    let (said, result) = landed(&r, slug, &people.orch);
    result.unwrap();
    assert!(
        said.contains(&format!(
            "land {slug}: its gate decisions: {GATE} raised 100,000 -> 110,000 by {}, carried at \
             {}\n",
            people.orch,
            &tip[..7]
        )),
        "{said}"
    );
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(at(&r, slug).0, pair("deliver", "landed"));
}

// -- a guard's decision revoked before the land ------------------------------------------------------

/// The key of the project's approval authority, `approvals`.
fn authority_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[7; 32])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `payload` signed by the authority's key, as an observation or an attestation.
fn signed_by_authority(observation: bool, payload: String) -> symphony_api::workflow::Signed {
    use ed25519_dalek::Signer;
    let message = symphony_board::workflow::guard::signed_message(observation, &payload);
    symphony_api::workflow::Signed {
        signature: hex(&authority_key().sign(&message).to_bytes()),
        payload,
    }
}

/// [`WORKFLOW`] with its approve guarded by `approved`, which the project's `approvals` authority
/// attests, its key committed, and the project `lamp` its attestations name.
fn guarded_workflow() -> String {
    let text = WORKFLOW
        .replacen("adapters = [", "adapters = [\"approvals\", ", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        );
    let approve = "operation = \"approve\", authority = [{ kind = \"task_role\", role = \"reviewer\" }], inputs = [\"head\"], dependencies = \"none\", guards = []";
    assert_eq!(text.matches(approve).count(), 1, "the approve edge");
    let text = text.replacen(
        approve,
        &approve.replace("guards = []", "guards = [\"approved\"]"),
        1,
    );
    format!(
        "{text}\n[project]\nid = \"lamp\"\n\n[[workflow_authorities]]\nadapter = \"approvals\"\n\
         issuer = \"ledger\"\ncontract = 1\nkey = \"ed25519:{}\"\n",
        hex(authority_key().verifying_key().as_bytes())
    )
}

/// The authority reports generation `generation` of e1, following `previous`.
fn reported(r: &Repo, generation: u64, previous: Option<u64>) {
    use symphony_api::workflow::{ATTESTATION_CONTRACT, AuthorityState, Observation};
    let state = |generation: u64| format!("s{generation}");
    let observation = Observation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation,
        state: state(generation),
        previous: previous.map(|generation| AuthorityState {
            epoch: "e1".into(),
            generation,
            state: state(generation),
        }),
        at: 1.0,
    };
    let signed = signed_by_authority(true, serde_json::to_string(&observation).unwrap());
    r.board.store.observe_authority(&signed).unwrap();
}

/// `slug` approved by its reviewer at `tip` with the authority's pass, met at generation 3.
fn approved_under_guard(r: &Repo, slug: &str, people: &People, tip: &str) {
    use symphony_api::workflow::{ATTESTATION_CONTRACT, Attestation, Attested};
    use symphony_board::workflow::guard::{MAX_VALIDITY, attestee};
    let store = &r.board.store;
    let rev = agent_session(&people.rev);
    let next = store.workflow_actions(slug, &rev, &Host::local()).unwrap();
    let active = store.workflow_active().unwrap().unwrap();
    let issued = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        - 1.0;
    let attestation = Attestation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation: 3,
        project: "lamp".into(),
        task: slug.into(),
        guard: "approved".into(),
        edge: "approve".into(),
        workflow: active.digest,
        activation_generation: active.generation,
        task_revision: next.snapshot.task_revision,
        at: next.snapshot.at.clone(),
        input_revisions: BTreeMap::new(),
        principal: attestee(&rev),
        result: Attested::Pass,
        facts: vec!["ledger:decision/7".into()],
        reasons: Vec::new(),
        issued_at: issued,
        expires_at: issued + MAX_VALIDITY,
    };
    let request = OperationRequest {
        project: store.database().database().into(),
        task: slug.into(),
        edge: "approve".into(),
        expected: next.snapshot,
        input_revisions: BTreeMap::new(),
        idempotency_key: format!("{slug}:approve:{tip}"),
        inputs: BTreeMap::from([("head".to_string(), InputValue::Revision(tip.into()))]),
    };
    let given = [signed_by_authority(
        false,
        serde_json::to_string(&attestation).unwrap(),
    )];
    store
        .apply_attested(&rev, &Host::local(), &request, &given)
        .unwrap();
    assert_eq!(at(r, slug), (pair("deliver", "approved"), 4));
}

#[test]
fn a_decision_revoked_between_the_guarded_approve_and_the_land_merges_nothing() {
    let r = Repo::new();
    manifest(&r, &format!("\n{}", guarded_workflow()));
    let followed = follow(&r.board.store, &r.root).unwrap();
    assert!(matches!(followed, Followed::Activated(_)), "{followed:?}");
    let (people, _wt, tip) = submitted(&r, SLUG);
    reported(&r, 3, None);
    approved_under_guard(&r, SLUG, &people, &tip);
    // The approval withdrawn in the ledger: its authority reports the decision that follows.
    reported(&r, 4, Some(3));
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    assert!(
        line.contains("approvals decided again since approved was met at generation 3 of e1: a later decision stands"),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before, "nothing was merged");
    assert_eq!(intent(&r, SLUG).stage, Stage::Withdrawn);
}
