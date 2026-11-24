//! Recovery on a board: a lost holder, an agent's release and a session's
//! leave answered by each staffed position's configured action, on the record, and a recovery
//! racing the holder's own transition.

use super::super::tests::{board, events, pair, refused, request, roles, take};
use super::*;
use crate::health::{LIVE_SECONDS, STALE_SECONDS};
use crate::store_fixture::Fixture;
use crate::workflow::activate::tests::{MAPPED, custom};
use crate::workflow::recover::tests::staffed;
use crate::workflow::{Custom, TaskPosition, agent_session, board_service};
use symphony_plugin::execution::Principal;

fn workflow() -> Custom {
    custom(&format!("{}{MAPPED}", staffed()))
}

/// Past the lost line for everyone but `staying`, who beat just now.
fn lose_all_but(f: &Fixture, staying: &[&str]) {
    f.advance(LIVE_SECONDS + STALE_SECONDS + 1.0);
    for handle in staying {
        f.store.touch(handle).unwrap();
    }
}

fn position(f: &Fixture, slug: &str) -> TaskPosition {
    let task = f.store.get(slug).unwrap();
    serde_json::from_value(task.fields["_board"]["workflow"].clone()).unwrap()
}

fn owned(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    (rows.iter())
        .map(|(h, r)| (h.to_string(), r.to_string()))
        .collect()
}

/// sim/a claimed by amber, submitted, and taken to review by birch: at verify/reviewing.
fn reviewing(f: &Fixture) {
    let (amber, birch) = (agent_session("amber"), agent_session("birch"));
    take(&f.store, "sim/a", &amber, "claim").unwrap();
    take(&f.store, "sim/a", &amber, "submit").unwrap();
    take(&f.store, "sim/a", &birch, "review").unwrap();
}

#[test]
fn a_lost_executor_is_recovered_to_its_positions_target_on_the_record() {
    let f = board(&workflow());
    take(&f.store, "sim/a", &agent_session("amber"), "claim").unwrap();
    assert_eq!(
        refused(
            f.store
                .recover_holder("sim/a", "amber", "birch", &Host::local())
        ),
        "explicit_action_required: amber is live on the board: only a lost holder's work is \
         recovered, and amber lets go of its own (`just tasks release sim/a`)"
    );
    lose_all_but(&f, &["birch", "orch"]);
    let recovered = (f.store)
        .recover_holder("sim/a", "amber", "birch", &Host::local())
        .unwrap()
        .unwrap();
    assert_eq!(
        (&recovered.id, &recovered.to, &recovered.status),
        (
            &"sim/a@3".to_string(),
            &pair("build", "available"),
            &"ready".to_string()
        )
    );
    assert_eq!(recovered.principal, board_service("recovery"));
    let task = f.store.get("sim/a").unwrap();
    assert_eq!(task.status, "ready");
    assert_eq!(roles(&f, "sim/a"), owned(&[("orch", "orchestrator")]));
    assert_eq!(position(&f, "sim/a").manual, None);
    assert_eq!(
        events(&f, "sim/a").last().unwrap(),
        &(3, None, pair("build", "available"))
    );
    assert_eq!(
        f.said("sim/a").last().unwrap(),
        "the board's recovery (sim/a@3): amber's executor lost; build/working → \
         build/available, workflow revision 3; let go: amber executor"
    );
    // Nothing left of amber's to recover, and its task is taken again by its edge.
    assert_eq!(
        refused(
            f.store
                .recover_holder("sim/a", "amber", "birch", &Host::local())
        ),
        "unknown_id: amber holds no role on sim/a"
    );
    take(&f.store, "sim/a", &agent_session("birch"), "claim").unwrap();
}

#[test]
fn a_lost_reviewer_stays_for_a_person_keeping_its_role() {
    let f = board(&workflow());
    reviewing(&f);
    lose_all_but(&f, &["amber", "orch"]);
    let stayed = (f.store)
        .recover_holder("sim/a", "birch", "orch", &Host::local())
        .unwrap()
        .unwrap();
    assert_eq!(
        (stayed.from.as_ref(), &stayed.to),
        (
            Some(&pair("verify", "reviewing")),
            &pair("verify", "reviewing")
        )
    );
    assert_eq!(f.store.get("sim/a").unwrap().status, "review");
    assert!(roles(&f, "sim/a").contains(&("birch".into(), "reviewer".into())));
    let manual = position(&f, "sim/a").manual.unwrap();
    assert_eq!(
        (manual.trigger, manual.holder.as_str(), manual.manual),
        (Trigger::LostHolder, "birch", true)
    );
    assert_eq!(
        f.said("sim/a").last().unwrap(),
        "the board's recovery (sim/a@5): birch's reviewer lost; stays at verify/reviewing, \
         workflow revision 5; it waits on a person's recovery (`just tasks workflow actions \
         sim/a`)"
    );
}

#[test]
fn an_agents_release_runs_its_positions_release_action() {
    let f = board(&workflow());
    let amber = agent_session("amber");
    take(&f.store, "sim/a", &amber, "claim").unwrap();
    let task = f.store.release("sim/a", "amber", Some("executor")).unwrap();
    assert_eq!(task.status, "ready");
    assert_eq!(roles(&f, "sim/a"), owned(&[("orch", "orchestrator")]));
    let last = events(&f, "sim/a").last().cloned().unwrap();
    assert_eq!(last, (3, None, pair("build", "available")));
    // A reviewer's own release goes back to awaiting one.
    let again = |principal: &Principal, edge: &str| {
        let request = request(&f.store, "sim/a", principal, edge, &format!("again:{edge}"));
        super::super::tests::apply(&f.store, principal, &request).unwrap();
    };
    again(&amber, "claim");
    again(&amber, "submit");
    again(&agent_session("birch"), "review");
    let task = f.store.release("sim/a", "birch", None).unwrap();
    assert_eq!(task.status, "review");
    assert_eq!(position(&f, "sim/a").at(), pair("verify", "awaiting"));
    // amber's executor role, which verify/awaiting does not staff, goes alone.
    let revision = position(&f, "sim/a").revision;
    f.store.release("sim/a", "amber", None).unwrap();
    assert_eq!(position(&f, "sim/a").revision, revision);
    assert_eq!(roles(&f, "sim/a"), owned(&[("orch", "orchestrator")]));
    assert_eq!(
        f.said("sim/a").last().unwrap(),
        "amber let go of amber executor, which its workflow position does not staff"
    );
    // The orchestrator's role stays the board's to keep, as its own release says.
    assert!(refused(f.store.release("sim/a", "orch", None)).starts_with(
        "orch cannot let go of orchestrator on sim/a: sim/a would have no orchestrator"
    ));
}

#[test]
fn a_session_that_leaves_is_recovered_by_each_positions_lost_holder_action() {
    let f = board(&workflow());
    take(&f.store, "sim/b", &agent_session("amber"), "claim").unwrap();
    reviewing(&f);
    f.store.leave("birch").unwrap();
    // birch's reviewer stays for a person; the task keeps its status.
    assert_eq!(f.store.get("sim/a").unwrap().status, "review");
    assert!(roles(&f, "sim/a").contains(&("birch".into(), "reviewer".into())));
    assert!(position(&f, "sim/a").manual.is_some());
    // Left, it leaves no more: refused as any write by a session that left, before any
    // recovery, and at once, with no try again.
    let (before, tried) = (f.store.dump().unwrap(), Store::tried_again_here());
    let again = f.store.leave("birch");
    assert!(f.store.refuses_actor(again.as_ref().unwrap_err(), "birch"));
    assert_eq!(refused(again), "session birch has left; attach again");
    assert_eq!(Store::tried_again_here(), tried);
    assert_eq!(f.store.dump().unwrap(), before);
    f.store.leave("amber").unwrap();
    // amber's executor on sim/b moves back to available, and its leftover executor role on
    // sim/a, which verify/reviewing does not staff, goes alone.
    let b = f.store.get("sim/b").unwrap();
    assert_eq!(b.status, "ready");
    assert_eq!(position(&f, "sim/b").at(), pair("build", "available"));
    assert_eq!(roles(&f, "sim/b"), owned(&[("orch", "orchestrator")]));
    assert!(!roles(&f, "sim/a").contains(&("amber".into(), "executor".into())));
    let sessions = f.store.sessions().unwrap();
    assert!(
        (sessions.iter())
            .filter(|s| ["amber", "birch"].contains(&s.row.handle.as_str()))
            .all(|s| s.row.left_at.is_some())
    );
}

#[test]
fn a_recovery_racing_the_holders_own_transition_has_one_winner() {
    let f = board(&workflow());
    let amber = agent_session("amber");
    take(&f.store, "sim/a", &amber, "claim").unwrap();
    lose_all_but(&f, &["birch", "orch"]);
    // The holder moves first, after the recovery decided: the recovery decides again, and
    // only the executor's role left at verify/awaiting goes.
    let submit = request(&f.store, "sim/a", &amber, "submit", "submit");
    let store = f.other();
    let late = f.late(move || {
        store
            .apply_operation(&agent_session("amber"), &Host::local(), &submit)
            .unwrap();
    });
    let before = Store::tried_again_here();
    let recovered = late
        .recover_holder("sim/a", "amber", "birch", &Host::local())
        .unwrap();
    assert!(Store::tried_again_here() > before);
    assert_eq!(recovered, None);
    assert_eq!(
        events(&f, "sim/a").last().unwrap(),
        &(3, Some("submit".into()), pair("verify", "awaiting"))
    );
    assert_eq!(roles(&f, "sim/a"), owned(&[("orch", "orchestrator")]));
    // The recovery moves first: the holder's request from before is stale.
    take(&f.store, "sim/b", &amber, "claim").unwrap();
    let late_submit = request(&f.store, "sim/b", &amber, "submit", "late");
    lose_all_but(&f, &["birch", "orch"]);
    (f.store)
        .recover_holder("sim/b", "amber", "birch", &Host::local())
        .unwrap()
        .unwrap();
    assert!(
        refused(super::super::tests::apply(&f.store, &amber, &late_submit)).starts_with(
            "stale_task: sim/b is at build/available (workflow revision 3), not build/working"
        )
    );
}

#[test]
fn next_offers_a_held_task_by_its_open_claim_edge_and_none_whose_claim_is_taken() {
    let f = board(&workflow());
    let next = |handle: &str| f.store.next_for(handle).unwrap().map(|t| t.slug);
    assert_eq!(next("amber").as_deref(), Some("sim/a"));
    // Another's live reservation stands against it, as admission's own claim refuses it.
    f.store.reserve("sim/a", "birch", 60.0).unwrap();
    assert_eq!(next("amber").as_deref(), Some("sim/b"));
    assert_eq!(next("birch").as_deref(), Some("sim/a"));
    take(&f.store, "sim/a", &agent_session("birch"), "claim").unwrap();
    take(&f.store, "sim/b", &agent_session("amber"), "claim").unwrap();
    // Each claimed, and sim/busy at a position no claim leaves: nothing to take.
    assert_eq!(next("amber"), None);
    assert_eq!(next("birch"), None);
}

#[test]
fn a_release_it_cannot_finish_writes_nothing_and_one_the_workflow_has_no_say_in_is_the_boards() {
    let f = board(&workflow());
    // orch holds sim/a's executor and its orchestrator: letting both go is refused whole,
    // as the orchestrator's would leave the task with none.
    take(&f.store, "sim/a", &agent_session("orch"), "claim").unwrap();
    let before = f.store.dump().unwrap();
    assert!(refused(f.store.release("sim/a", "orch", None)).starts_with(
        "orch cannot let go of orchestrator on sim/a: sim/a would have no orchestrator"
    ));
    assert_eq!(f.store.dump().unwrap(), before);
    // A task not on the board, and a session holding nothing there, meet the board's refusals.
    assert_eq!(
        refused(f.store.release("sim/none", "amber", None)),
        "no task 'sim/none'"
    );
    assert!(
        refused(f.store.release("sim/a", "amber", None))
            .starts_with("amber holds no role on sim/a")
    );
}

#[test]
fn a_host_that_grants_its_recovery_service_nothing_recovers_no_holder() {
    let f = board(&workflow());
    take(&f.store, "sim/a", &agent_session("amber"), "claim").unwrap();
    lose_all_but(&f, &["birch", "orch"]);
    let host = Host {
        grants: crate::workflow::Grants::none().grant("owner", crate::workflow::local_owner()),
        adapters: Vec::new(),
    };
    assert_eq!(
        refused(f.store.recover_holder("sim/a", "amber", "birch", &host)),
        "unauthorized: this host grants the board's recovery service no `recovery`: nobody \
         recovers a holder here"
    );
    assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
}
