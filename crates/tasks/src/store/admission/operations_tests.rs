//! The board's operations on held tasks, on both databases: lost holders and unsuccessful
//! prerequisites settled by each position's configuration, once, and a claim named by its role.

use super::super::tests::{BACKENDS, events, pair, refused, roles, take};
use super::*;
use crate::health::{LIVE_SECONDS, STALE_SECONDS};
use crate::store_fixture::{Backend, Fixture, Seed};
use crate::workflow::activate::tests::{MAPPED, custom};
use crate::workflow::operate::tests::REVIEWED;
use crate::workflow::{Custom, TaskPosition, agent_session};

pub(crate) fn reviewed() -> Custom {
    custom(&format!("{REVIEWED}{MAPPED}"))
}

/// Sessions amber, birch, cedar and orch; sim/a ready, sim/c ready on sim/gone (cancelled),
/// sim/d in progress on sim/gone, each orchestrated by orch; [`REVIEWED`] activated.
pub(crate) fn board(backend: Backend) -> Fixture {
    let seed = Seed::default()
        .session("amber")
        .session("birch")
        .session("cedar")
        .session("orch")
        .task("sim/a", "ready")
        .task("sim/c", "ready")
        .task("sim/d", "in_progress")
        .task("sim/gone", "cancelled")
        .role("sim/a", "orch", "orchestrator")
        .role("sim/c", "orch", "orchestrator")
        .role("sim/d", "orch", "orchestrator")
        .link("sim/c", "dep", "sim/gone")
        .link("sim/d", "dep", "sim/gone");
    let f = Fixture::on(backend, seed);
    f.store
        .activate_workflow(&reviewed(), &Host::local())
        .unwrap();
    f
}

/// Past the lost line for everyone but `staying`, who beat just now.
fn lose_all_but(f: &Fixture, staying: &[&str]) {
    f.advance(LIVE_SECONDS + STALE_SECONDS + 1.0);
    for handle in staying {
        f.store.touch(handle).unwrap();
    }
}

fn position(f: &Fixture, slug: &str) -> TaskPosition {
    crate::store::workflow::task_position(&f.store.get(slug).unwrap().fields).unwrap()
}

fn settle(f: &Fixture) -> Vec<String> {
    f.store
        .settle_held(&f.store.board().unwrap())
        .unwrap()
        .lines
}

#[test]
fn a_board_with_no_workflow_settles_nothing_and_reads_no_workflow() {
    for backend in BACKENDS {
        let f = Fixture::on(
            backend,
            Seed::default()
                .session("amber")
                .task("sim/a", "in_progress")
                .role("sim/a", "amber", "executor"),
        );
        lose_all_but(&f, &[]);
        let board = f.store.board().unwrap();
        assert_eq!(f.store.operating_for(&board.tasks).unwrap(), None);
        assert!(f.store.settle_held(&board).unwrap().lines.is_empty());
        assert!(!f.store.settle_held_if_due(&board));
        assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
        assert_eq!(roles(&f, "sim/a"), [("amber".into(), "executor".into())]);
    }
}

#[test]
fn a_lost_executor_is_recovered_by_the_board_once() {
    for backend in BACKENDS {
        let f = board(backend);
        take(&f.store, "sim/a", &agent_session("amber"), "claim").unwrap();
        assert!(
            settle(&f).iter().all(|l| !l.contains("sim/a")),
            "{backend:?}: live"
        );
        lose_all_but(&f, &["orch"]);
        let lines = settle(&f);
        assert!(
            lines.contains(&"recovered amber on sim/a".to_string()),
            "{lines:?}"
        );
        assert_eq!(position(&f, "sim/a").at(), pair("build", "available"));
        assert_eq!(roles(&f, "sim/a"), [("orch".into(), "orchestrator".into())]);
        assert!(
            (f.said("sim/a").iter())
                .any(|l| l.contains("the board's recovery (sim/a@3): amber's executor lost")),
            "{:?}",
            f.said("sim/a")
        );
        assert!(settle(&f).iter().all(|l| !l.contains("sim/a")), "once");
    }
}

#[test]
fn a_manual_stay_is_recorded_once_and_the_holder_keeps_its_role() {
    for backend in BACKENDS {
        let f = board(backend);
        let amber = agent_session("amber");
        take(&f.store, "sim/a", &amber, "claim").unwrap();
        take(&f.store, "sim/a", &amber, "block").unwrap();
        lose_all_but(&f, &["orch"]);
        let lines = settle(&f);
        assert!(
            lines.contains(&"recovered amber on sim/a".to_string()),
            "{lines:?}"
        );
        let at = position(&f, "sim/a");
        assert_eq!(at.at(), pair("build", "blocked"));
        assert_eq!(
            at.resume,
            Some(pair("build", "working")),
            "a stay keeps its resume"
        );
        assert_eq!(at.manual.as_ref().map(|m| m.holder.as_str()), Some("amber"));
        let revision = at.revision;
        assert!(settle(&f).iter().all(|l| !l.contains("sim/a")));
        assert_eq!(position(&f, "sim/a").revision, revision, "no second stay");
        assert!(roles(&f, "sim/a").contains(&("amber".into(), "executor".into())));
    }
}

#[test]
fn a_dependant_whose_policy_is_cancel_is_cancelled_and_block_moves_nothing() {
    for backend in BACKENDS {
        let f = board(backend);
        let d = position(&f, "sim/d");
        let lines = settle(&f);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("cancelled sim/c by cancel (sim/c@2)")),
            "{backend:?}: {lines:?}"
        );
        assert_eq!(position(&f, "sim/c").at(), pair("build", "cancelled"));
        assert_eq!(f.store.get("sim/c").unwrap().status, "cancelled");
        assert_eq!(
            events(&f, "sim/c").last().map(|e| e.1.clone()),
            Some(Some("cancel".into()))
        );
        // sim/d is at build/working, whose policy is block: it stays, for PULL to list.
        assert_eq!(position(&f, "sim/d"), d);
        assert!(lines.iter().all(|l| !l.contains("sim/d")), "{lines:?}");
        // The next pass finds sim/c finished, and nothing to cancel again.
        assert!(settle(&f).is_empty());
    }
}

#[test]
fn a_cancel_policy_with_no_edge_the_board_may_take_moves_nothing() {
    for backend in BACKENDS {
        let text = format!("{REVIEWED}{MAPPED}").replace(
            "authority = [{ kind = \"project_capability\", capability = \"owner\" }, { kind = \
             \"system_capability\", capability = \"recovery\" }]",
            "authority = [{ kind = \"project_capability\", capability = \"owner\" }]",
        );
        let f = Fixture::on(
            backend,
            Seed::default()
                .session("orch")
                .task("sim/c", "ready")
                .task("sim/gone", "cancelled")
                .link("sim/c", "dep", "sim/gone"),
        );
        f.store
            .activate_workflow(&custom(&text), &Host::local())
            .unwrap();
        assert!(settle(&f).is_empty());
        assert_eq!(position(&f, "sim/c").at(), pair("build", "available"));
    }
}

#[test]
fn a_claim_by_its_role_takes_the_one_edge_that_gives_it() {
    for backend in BACKENDS {
        let f = board(backend);
        let (amber, birch) = (agent_session("amber"), agent_session("birch"));
        let host = Host::local();
        let claimed = (f.store)
            .claim_by_workflow("sim/a", &amber, &host, Role::Executor)
            .unwrap();
        assert_eq!(claimed.transition, "sim/a@2");
        assert_eq!(position(&f, "sim/a").at(), pair("build", "working"));
        assert_eq!(
            refused(
                f.store
                    .claim_by_workflow("sim/a", &birch, &host, Role::Reviewer)
            ),
            "explicit_action_required: no edge leaving sim/a's build/working gives the reviewer \
             role: read its edges with `just tasks workflow actions sim/a`"
        );
        take(&f.store, "sim/a", &amber, "submit").unwrap();
        // Its executor never reviews it, though submitting let go of the role.
        assert!(
            refused(
                f.store
                    .claim_by_workflow("sim/a", &amber, &host, Role::Reviewer)
            )
            .starts_with("unauthorized: amber executed sim/a"),
            "{backend:?}"
        );
        (f.store)
            .claim_by_workflow("sim/a", &birch, &host, Role::Reviewer)
            .unwrap();
        assert_eq!(position(&f, "sim/a").at(), pair("verify", "reviewing"));
        assert!(roles(&f, "sim/a").contains(&("birch".into(), "reviewer".into())));
    }
}

#[test]
fn a_claim_by_its_role_says_what_stops_its_one_edge() {
    for backend in BACKENDS {
        let f = board(backend);
        let (amber, birch) = (agent_session("amber"), agent_session("birch"));
        let host = Host::local();
        (f.store)
            .claim_by_workflow("sim/a", &amber, &host, Role::Executor)
            .unwrap();
        take(&f.store, "sim/a", &amber, "submit").unwrap();
        take(&f.store, "sim/a", &birch, "review").unwrap();
        take(&f.store, "sim/a", &birch, "changes").unwrap();
        // Back at build/available, with nothing done before it: amber claims it again.
        let again = (f.store)
            .claim_by_workflow("sim/a", &amber, &host, Role::Executor)
            .unwrap();
        assert_eq!(again.transition, "sim/a@6");
        // No workflow active refuses as such, and the owner holds no task role.
        let plain = Fixture::on(
            backend,
            Seed::default().session("amber").task("sim/x", "ready"),
        );
        assert!(
            refused(
                plain
                    .store
                    .claim_by_workflow("sim/x", &amber, &host, Role::Executor)
            )
            .starts_with("unsupported_activation: ")
        );
        assert!(
            refused(f.store.claim_by_workflow(
                "sim/c",
                &crate::workflow::local_owner(),
                &host,
                Role::Executor
            ))
            .starts_with("unauthorized: ")
        );
    }
}

/// Sessions amber, birch, cedar and orch; sim/a and sim/b ready, each orchestrated by orch;
/// [`REVIEWED`] activated.
fn two_ready(backend: Backend) -> Fixture {
    let seed = Seed::default()
        .session("amber")
        .session("birch")
        .session("cedar")
        .session("orch")
        .task("sim/a", "ready")
        .task("sim/b", "ready")
        .role("sim/a", "orch", "orchestrator")
        .role("sim/b", "orch", "orchestrator");
    let f = Fixture::on(backend, seed);
    f.store
        .activate_workflow(&reviewed(), &Host::local())
        .unwrap();
    f
}

/// The slots orch holds, by the task whose work each is.
fn holdings(store: &Store) -> Vec<String> {
    let all = store.slot_holdings().unwrap();
    all.get("orch").into_iter().flatten().cloned().collect()
}

#[test]
fn a_held_task_holds_its_slot_by_its_positions_policy_from_its_executor_to_its_reviewer() {
    for backend in BACKENDS {
        let f = two_ready(backend);
        let capped = f.other().capping(Some(1));
        let [amber, birch, cedar] = ["amber", "birch", "cedar"].map(agent_session);
        let held = || holdings(&f.store);
        let full = |slug: &str, by: &Principal, edge: &str| {
            let why = refused(take(&capped, slug, by, edge));
            assert!(
                why.starts_with("capacity: "),
                "{backend:?} {slug} {edge}: {why}"
            );
        };
        take(&capped, "sim/a", &amber, "claim").unwrap();
        assert_eq!(held(), ["sim/a"]);
        full("sim/b", &birch, "claim");
        // A held block retains its slot, though it projects to blocked, which takes none.
        take(&capped, "sim/a", &amber, "block").unwrap();
        assert_eq!(f.store.get("sim/a").unwrap().status, "blocked");
        assert_eq!(held(), ["sim/a"]);
        full("sim/b", &birch, "claim");
        // Back from the block it held the slot all along: no start, never refused.
        take(&capped, "sim/a", &amber, "resume").unwrap();
        // Submitted, its position releases the slot: other work starts in it.
        take(&capped, "sim/a", &amber, "submit").unwrap();
        assert_eq!(f.store.get("sim/a").unwrap().status, "review");
        assert!(held().is_empty(), "{backend:?}");
        take(&capped, "sim/b", &birch, "claim").unwrap();
        // The reviewer's position acquires one, though it projects to review, which the board's
        // own ledger counts apart: a start the cap refuses while it is full.
        full("sim/a", &cedar, "review");
        take(&capped, "sim/b", &birch, "submit").unwrap();
        take(&capped, "sim/a", &cedar, "review").unwrap();
        assert_eq!(f.store.get("sim/a").unwrap().status, "review");
        assert_eq!(held(), ["sim/a"]);
        full("sim/b", &amber, "review");
        // Approved, it releases it, though it projects to done short of its terminal position.
        take(&capped, "sim/a", &cedar, "approve").unwrap();
        assert!(held().is_empty(), "{backend:?}");
        take(&capped, "sim/b", &amber, "review").unwrap();
        assert_eq!(held(), ["sim/b"]);
    }
}

#[test]
fn the_transitions_of_held_tasks_are_read_by_their_ids_oldest_first() {
    for backend in BACKENDS {
        let f = two_ready(backend);
        let amber = agent_session("amber");
        take(&f.store, "sim/a", &amber, "claim").unwrap();
        take(&f.store, "sim/a", &amber, "submit").unwrap();
        // Asked past its newest, it reads what there is; a task asked for nothing reads nothing.
        let read = f.store.transitions(&[("sim/b", 1), ("sim/a", 5)]).unwrap();
        let said: Vec<(&str, u64, &str)> = (read.iter())
            .map(|t| (t.slug.as_str(), t.revision, t.status.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("sim/a", 1, "ready"),
                ("sim/a", 2, "in_progress"),
                ("sim/a", 3, "review"),
                ("sim/b", 1, "ready"),
            ],
            "{backend:?}"
        );
        assert!(f.store.transitions(&[]).unwrap().is_empty());
    }
}

/// [`REVIEWED`] with a second edge from build/available that gives the executor's role, taken on
/// `authority`.
fn two_claims(authority: &str) -> Custom {
    let pick_up = format!(
        r#"  {{ id = "pick-up", label = "Pick up", from = {{ phase = "build", state = "available" }}, to = {{ phase = "build", state = "working" }}, operation = "claim", authority = [{authority}], inputs = [], dependencies = "done", guards = [], effects = [{{ kind = "acquire_role", role = "executor" }}, {{ kind = "slot", policy = "acquire" }}], automatic = false, human_wait = false }},
  {{ id = "block","#
    );
    custom(&format!(
        "{}{MAPPED}",
        REVIEWED.replace(r#"  { id = "block","#, &pick_up)
    ))
}

#[test]
fn a_claim_by_its_role_takes_the_one_open_edge_of_several_and_refuses_a_choice_between_open_ones() {
    for backend in BACKENDS {
        let host = Host::local();
        let amber = agent_session("amber");
        let on = |definition: &Custom| {
            let f = Fixture::on(
                backend,
                Seed::default().session("amber").task("sim/a", "ready"),
            );
            f.store.activate_workflow(definition, &host).unwrap();
            f
        };
        // The owner's edge is closed to an agent: the claim takes the one left open.
        let f = on(&two_claims(
            r#"{ kind = "project_capability", capability = "owner" }"#,
        ));
        let claimed = (f.store)
            .claim_by_workflow("sim/a", &amber, &host, Role::Executor)
            .unwrap();
        assert_eq!(claimed.transition, "sim/a@2");
        assert_eq!(
            events(&f, "sim/a").last().unwrap().1.as_deref(),
            Some("claim"),
            "{backend:?}"
        );
        // Two open to it: which to take is the agent's, never the board's.
        let f = on(&two_claims(r#"{ kind = "task_role", role = "executor" }"#));
        assert_eq!(
            refused(
                f.store
                    .claim_by_workflow("sim/a", &amber, &host, Role::Executor)
            ),
            "explicit_action_required: 2 edges leaving sim/a's build/available give the executor \
             role (claim, pick-up): take one with `just tasks workflow apply sim/a EDGE --revision 1`"
        );
        assert_eq!(position(&f, "sim/a").at(), pair("build", "available"));
    }
}
