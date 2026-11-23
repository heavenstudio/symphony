//! A guarded edge on a board, on both databases: admitted only with an attestation the board
//! verifies at its authority's accepted state, recorded, fenced against an observation that moves
//! that state, and checked again before a dispatch.

use super::tests::{BACKENDS, board, refused, request, take};
use super::*;
use crate::store_fixture::{Backend, Fixture, Seed};
use crate::workflow::activate::tests::{MAPPED, custom};
use crate::workflow::guard::tests::{
    guarded_text, passing, sign_attestation, sign_observation, two_authorities_text,
};
use crate::workflow::{Custom, Followed, Observing, agent_session, local_owner};
use std::sync::{Arc, Barrier};
use symphony_api::workflow::{
    ATTESTATION_CONTRACT, Attestation, Attested, AuthorityState, Observation,
};

/// TWO with its acceptance guarded, the authority's key that of `seed`, every status mapped.
fn guarded_two(seed: u8) -> Custom {
    custom(&format!("{}{MAPPED}", guarded_text(seed)))
}

/// The guarded board with sim/a claimed and submitted by amber: at verify/awaiting, its workflow
/// revision 3, waiting on the owner's guarded acceptance.
fn awaiting() -> Fixture {
    let f = board(&guarded_two(7));
    let amber = agent_session("amber");
    take(&f.store, "sim/a", &amber, "claim").unwrap();
    take(&f.store, "sim/a", &amber, "submit").unwrap();
    f
}

fn observation(epoch: &str, generation: u64, previous: Option<(&str, u64)>) -> Observation {
    Observation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: epoch.into(),
        generation,
        state: format!("s{generation}"),
        previous: previous.map(|(epoch, generation)| AuthorityState {
            epoch: epoch.into(),
            generation,
            state: format!("s{generation}"),
        }),
        at: 1.0,
    }
}

/// The authority reports generation `generation` of e1, following `previous` of it.
fn observe(store: &Store, generation: u64, previous: Option<u64>) -> Result<Observing, Error> {
    let observed = observation("e1", generation, previous.map(|p| ("e1", p)));
    store.observe_authority(&sign_observation(7, &observed))
}

/// A pass of `approved` for the owner accepting `slug` from where it stands now, at the
/// authority's `generation` of e1, issued now for five minutes.
fn attestation(f: &Fixture, slug: &str, generation: u64) -> Attestation {
    let active = f.store.workflow_active().unwrap().unwrap();
    let position = f.store.workflow_position(slug).unwrap().unwrap();
    let issued = crate::store::now() - 1.0;
    Attestation {
        task: slug.into(),
        workflow: active.digest,
        activation_generation: active.generation,
        task_revision: position.revision,
        at: position.at(),
        generation,
        issued_at: issued,
        // As long as the board takes, so a loaded machine has all of it.
        expires_at: issued + crate::workflow::guard::MAX_VALIDITY,
        ..passing(&guarded_two(7))
    }
}

fn attested(f: &Fixture, slug: &str, generation: u64) -> Signed {
    sign_attestation(7, &attestation(f, slug, generation))
}

fn accept(f: &Fixture, key: &str) -> OperationRequest {
    request(&f.store, "sim/a", &local_owner(), "accept", key)
}

fn applied(
    store: &Store,
    request: &OperationRequest,
    given: &[Signed],
) -> Result<OperationResult, Error> {
    store.apply_attested(&local_owner(), &Host::local(), request, given)
}

fn accepted_state(f: &Fixture) -> crate::workflow::guard::Accepted {
    let authorities = f.store.workflow_authorities().unwrap();
    assert_eq!(authorities.attesters, guarded_two(7).authorities);
    authorities
        .accepted
        .into_iter()
        .next()
        .expect("an accepted state")
}

#[test]
fn a_guarded_acceptance_is_admitted_only_with_a_current_attestation_and_recorded() {
    for backend in BACKENDS {
        let f = awaiting();
        let request = accept(&f, "accept");
        let unobserved = refused(applied(&f.store, &request, &[]));
        assert!(
            unobserved.starts_with("guard_unavailable:") && unobserved.contains("no observation"),
            "{backend:?}: {unobserved}"
        );
        assert!(matches!(
            observe(&f.store, 3, None),
            Ok(Observing::Accepted(_))
        ));
        let bare = refused(applied(&f.store, &request, &[]));
        assert!(
            bare.contains("taken with an attestation from approvals"),
            "{bare}"
        );
        let signed = attested(&f, "sim/a", 3);
        let result = applied(&f.store, &request, std::slice::from_ref(&signed)).unwrap();
        assert_eq!(result.transition, "sim/a@4");
        assert_eq!(f.store.get("sim/a").unwrap().status, "done");

        let admission = f.store.workflow_admission("sim/a", 4).unwrap().unwrap();
        assert_eq!(
            (admission.edge.as_str(), admission.transition.as_str()),
            ("accept", "sim/a@4")
        );
        assert_eq!(admission.principal, local_owner());
        assert_eq!(admission.guards.len(), 1);
        assert_eq!(admission.guards[0].signed, signed);
        assert_eq!(admission.guards[0].generation, 3);
        assert_eq!(f.store.workflow_admission("sim/a", 3).unwrap(), None);
        assert_eq!(accepted_state(&f).admitted, 1, "the admission is counted");

        // A retry under the same key answers its first result, with or without the attestation.
        assert_eq!(applied(&f.store, &request, &[]).unwrap(), result);
        assert_eq!(
            applied(&f.store, &request, std::slice::from_ref(&signed)).unwrap(),
            result
        );
        assert_eq!(
            accepted_state(&f).admitted,
            1,
            "a retry admits nothing again"
        );
        let mut other = request.clone();
        other.edge = "submit".into();
        assert!(refused(applied(&f.store, &other, &[])).contains("accept"));
        // The same attestation under another key takes no second transition from that revision.
        let mut twice = request.clone();
        twice.idempotency_key = "accept-again".into();
        assert!(
            refused(applied(&f.store, &twice, std::slice::from_ref(&signed)))
                .starts_with("stale_task:")
        );

        // The dispatch checks the same guard again, by the attestation the admission recorded.
        assert_eq!(
            f.store.revalidate_dispatch("sim/a", 4, &[]).unwrap().len(),
            1
        );
        assert!(refused(f.store.revalidate_dispatch("sim/a", 3, &[])).starts_with("stale_task:"));
        assert!(refused(f.store.revalidate_dispatch("sim/a", 9, &[])).starts_with("unknown_id:"));
        assert!(
            refused(f.store.revalidate_dispatch("sim/none", 1, &[])).starts_with("unknown_id:")
        );
    }
}

#[test]
fn an_unguarded_transition_dispatches_on_its_position_alone() {
    for backend in BACKENDS {
        let f = awaiting();
        assert_eq!(f.store.revalidate_dispatch("sim/a", 3, &[]), Ok(Vec::new()));
        assert!(refused(f.store.revalidate_dispatch("sim/a", 2, &[])).starts_with("stale_task:"));
    }
}

#[test]
fn a_decision_revoked_since_the_attestation_refuses_it_and_stops_its_dispatch() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        let before = attested(&f, "sim/a", 3);
        // The ledger revokes: its authority reports the next generation.
        assert!(matches!(
            observe(&f.store, 4, Some(3)),
            Ok(Observing::Accepted(_))
        ));
        let request = accept(&f, "accept");
        let stale = refused(applied(&f.store, &request, &[before]));
        assert!(
            stale.contains("a later decision stands"),
            "{backend:?}: {stale}"
        );
        let current = attested(&f, "sim/a", 4);
        applied(&f.store, &request, &[current]).unwrap();

        // Revoked after the admission: the recorded attestation no longer carries the dispatch.
        observe(&f.store, 5, Some(4)).unwrap();
        let revoked = refused(f.store.revalidate_dispatch("sim/a", 4, &[]));
        assert!(revoked.contains("a later decision stands"), "{revoked}");
        // A fresh attestation of the same operation, at the authority's new state, carries it.
        let mut fresh = attestation(&f, "sim/a", 5);
        fresh.task_revision = 3;
        fresh.at = super::tests::pair("verify", "awaiting");
        assert_eq!(
            f.store
                .revalidate_dispatch("sim/a", 4, &[sign_attestation(7, &fresh)])
                .unwrap()
                .len(),
            1
        );
        let mut refusing = fresh.clone();
        refusing.result = Attested::Refuse;
        refusing.reasons = vec!["withdrawn".into()];
        assert!(
            refused(
                f.store
                    .revalidate_dispatch("sim/a", 4, &[sign_attestation(7, &refusing)])
            )
            .contains("refuses approved")
        );
        let mut later = fresh.clone();
        later.task_revision = 4;
        assert!(
            refused(
                f.store
                    .revalidate_dispatch("sim/a", 4, &[sign_attestation(7, &later)])
            )
            .contains("task revision")
        );
        // Held to this machine's clock, never the store's, which its caller sets: an attestation
        // expired by the machine stays expired with the store's clock inside its time, and one
        // issued at the store's clock an hour ahead is ahead of the machine.
        let now = crate::store::now();
        let expired = Attestation {
            issued_at: now - 400.0,
            expires_at: now - 100.0,
            ..fresh.clone()
        };
        f.advance(now - 200.0 - f.now());
        assert!(
            refused(
                f.store
                    .revalidate_dispatch("sim/a", 4, &[sign_attestation(7, &expired)])
            )
            .contains("expired")
        );
        f.advance(now + 3_600.0 - f.now());
        let ahead = Attestation {
            issued_at: f.now(),
            expires_at: f.now() + 300.0,
            ..fresh
        };
        assert!(
            refused(
                f.store
                    .revalidate_dispatch("sim/a", 4, &[sign_attestation(7, &ahead)])
            )
            .contains("ahead")
        );
    }
}

#[test]
fn a_revocation_between_the_read_and_the_write_makes_the_admission_decide_again() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        let signed = attested(&f, "sim/a", 3);
        let request = accept(&f, "accept");
        let other = f.other();
        let late = f.late(move || {
            observe(&other, 4, Some(3)).unwrap();
        });
        let before = Store::tried_again_here();
        let stale = refused(applied(&late, &request, &[signed]));
        assert!(
            stale.contains("a later decision stands"),
            "{backend:?}: {stale}"
        );
        assert!(
            Store::tried_again_here() > before,
            "the write's fence caught the move"
        );
        assert_eq!(
            f.store
                .workflow_position("sim/a")
                .unwrap()
                .unwrap()
                .revision,
            3
        );
        assert_eq!(f.store.workflow_admission("sim/a", 4).unwrap(), None);
        let state = accepted_state(&f);
        assert_eq!((state.generation, state.admitted), (4, 0));
    }
}

#[test]
fn an_admission_between_an_observations_read_and_write_makes_it_decide_again() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        let signed = attested(&f, "sim/a", 3);
        let request = accept(&f, "accept");
        let other = f.other();
        let late = f.late(move || {
            applied(&other, &request, std::slice::from_ref(&signed)).unwrap();
        });
        let before = Store::tried_again_here();
        assert!(matches!(
            observe(&late, 4, Some(3)),
            Ok(Observing::Accepted(_))
        ));
        assert!(
            Store::tried_again_here() > before,
            "the observation's fence caught the move"
        );
        let state = accepted_state(&f);
        assert_eq!((state.generation, state.admitted), (4, 1), "{backend:?}");
        assert_eq!(
            f.store
                .workflow_admission("sim/a", 4)
                .unwrap()
                .unwrap()
                .guards[0]
                .generation,
            3
        );
        assert!(
            refused(f.store.revalidate_dispatch("sim/a", 4, &[]))
                .contains("a later decision stands")
        );
    }
}

#[test]
fn a_revocation_racing_an_admission_never_lets_it_land_on_the_revoked_generation() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        let signed = attested(&f, "sim/a", 3);
        let request = accept(&f, "accept");
        let gate = Arc::new(Barrier::new(2));
        let admitting = {
            let (store, gate) = (f.other(), Arc::clone(&gate));
            std::thread::spawn(move || {
                gate.wait();
                applied(&store, &request, &[signed])
            })
        };
        let revoking = {
            let (store, gate) = (f.other(), Arc::clone(&gate));
            std::thread::spawn(move || {
                gate.wait();
                observe(&store, 4, Some(3))
            })
        };
        let admitted = admitting.join().unwrap();
        assert!(matches!(
            revoking.join().unwrap(),
            Ok(Observing::Accepted(_))
        ));
        let state = accepted_state(&f);
        assert_eq!(state.generation, 4, "{backend:?}");
        match admitted {
            // The admission went first: it was counted at generation 3, before the revocation,
            // and the dispatch it waits on now refuses.
            Ok(_) => {
                assert_eq!(state.admitted, 1);
                let admission = f.store.workflow_admission("sim/a", 4).unwrap().unwrap();
                assert_eq!(admission.guards[0].generation, 3);
                assert!(f.store.revalidate_dispatch("sim/a", 4, &[]).is_err());
            }
            Err(refusal) => {
                assert!(
                    refused(Err::<(), _>(refusal)).contains("a later decision stands"),
                    "{backend:?}"
                );
                assert_eq!(state.admitted, 0);
                assert_eq!(f.store.workflow_admission("sim/a", 4).unwrap(), None);
            }
        }
    }
}

#[test]
fn two_first_observations_racing_never_let_one_overwrite_the_other() {
    for backend in BACKENDS {
        for round in 0..6 {
            let f = board(&guarded_two(7));
            let gate = Arc::new(Barrier::new(2));
            let observers: Vec<_> = ["s3", "x3"]
                .into_iter()
                .map(|state| {
                    let (store, gate) = (f.other(), Arc::clone(&gate));
                    let observed = Observation {
                        state: state.into(),
                        ..observation("e1", 3, None)
                    };
                    std::thread::spawn(move || {
                        gate.wait();
                        store.observe_authority(&sign_observation(7, &observed))
                    })
                })
                .collect();
            for observer in observers {
                observer.join().unwrap().unwrap();
            }
            // The one that wrote second read the first's state, whichever it was: two decisions
            // at one generation, which hold every admission.
            let state = accepted_state(&f);
            assert!(state.conflicted, "{backend:?}, round {round}: {state:?}");
        }
    }
}

/// On PostgreSQL a first observation finds no row to lock before it writes, so another first one
/// can commit between its lock and its write: it decides again on the state that one left, and
/// marks the conflict, never overwriting it (SurrealDB's optimistic transaction conflicts by
/// itself).
#[test]
fn a_first_observation_meeting_another_before_its_write_decides_again() {
    let f = board(Backend::PostgreSql, &guarded_two(7));
    let crate::Database::PostgreSql(client) = f.store.database() else {
        unreachable!("a PostgreSQL board");
    };
    let (holding, watching) = (client.another(), client.another());
    holding
        .simple(
            "BEGIN; INSERT INTO workflow (id, kind, body, at) VALUES \
             ('[\"authority\", \"approvals\"]', 'authority', '{\"adapter\": \"approvals\", \
             \"issuer\": \"ledger\", \"epoch\": \"e1\", \"generation\": 3, \"state\": \"x3\", \
             \"conflicted\": false, \"admitted\": 0}', 1.0)",
        )
        .unwrap();
    let observing = {
        let store = f.other();
        std::thread::spawn(move || observe(&store, 3, None))
    };
    // It waits on the uncommitted row's key: past its read and the lock that found no row.
    let waiting = || {
        watching
            .simple(
                "SELECT count(*) FROM pg_stat_activity \
                 WHERE wait_event_type = 'Lock' AND datname = current_database()",
            )
            .unwrap()[0]
            .value()
            != Some("0")
    };
    test_time::wait::until_true(
        "the observation to wait on the row",
        std::time::Duration::from_secs(30),
        waiting,
    )
    .unwrap();
    holding.simple("COMMIT").unwrap();
    let observed = observing.join().unwrap();
    assert!(
        matches!(observed, Ok(Observing::Conflicted(_))),
        "{observed:?}"
    );
    let state = accepted_state(&f);
    assert!(state.conflicted && state.state == "x3", "{state:?}");
}

#[test]
fn an_observation_moves_the_authority_only_forward_and_only_by_its_committed_key() {
    for backend in BACKENDS {
        let f = board(&guarded_two(7));
        let first = observation("e1", 3, None);
        let foreign = refused(f.store.observe_authority(&sign_observation(8, &first)));
        assert!(foreign.contains("committed key"), "{backend:?}: {foreign}");
        let anchored = refused(observe(&f.store, 3, Some(2)));
        assert!(
            anchored.contains("first observation names none"),
            "{anchored}"
        );
        let Ok(Observing::Accepted(accepted)) = observe(&f.store, 3, None) else {
            panic!("the first observation is accepted");
        };
        assert_eq!(
            observe(&f.store, 3, None),
            Ok(Observing::Unchanged(accepted))
        );
        assert!(refused(observe(&f.store, 2, None)).contains("never lowered"));
        assert!(refused(observe(&f.store, 5, Some(4))).contains("report again from the state"));
        let mut elsewhere = first.clone();
        elsewhere.adapter = "elsewhere".into();
        assert!(
            refused(f.store.observe_authority(&sign_observation(7, &elsewhere)))
                .contains("no approval authority the active workflow commits")
        );
        let unsigned = Signed {
            payload: "{".into(),
            signature: String::new(),
        };
        assert!(refused(f.store.observe_authority(&unsigned)).contains("names no adapter"));
        // A new epoch starts only from the state accepted.
        let rotated = observation("e2", 1, Some(("e1", 3)));
        let Ok(Observing::Accepted(next)) =
            f.store.observe_authority(&sign_observation(7, &rotated))
        else {
            panic!("a new epoch from the accepted state is accepted");
        };
        assert_eq!((next.epoch.as_str(), next.generation), ("e2", 1));

        let none = Fixture::new(Seed::default());
        assert!(refused(observe(&none.store, 3, None)).contains("no workflow is active"));
    }
}

#[test]
fn two_decisions_at_one_generation_hold_every_admission_until_a_later_state() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        let mut other = observation("e1", 3, Some(("e1", 2)));
        other.state = "forked".into();
        let Ok(Observing::Conflicted(conflicted)) =
            f.store.observe_authority(&sign_observation(7, &other))
        else {
            panic!("another decision at the accepted generation conflicts");
        };
        assert!(conflicted.conflicted);
        let request = accept(&f, "accept");
        let held = refused(applied(&f.store, &request, &[attested(&f, "sim/a", 3)]));
        assert!(
            held.contains("two different decisions"),
            "{backend:?}: {held}"
        );
        assert!(matches!(
            observe(&f.store, 4, Some(3)),
            Ok(Observing::Accepted(_))
        ));
        assert!(!accepted_state(&f).conflicted);
        applied(&f.store, &request, &[attested(&f, "sim/a", 4)]).unwrap();
    }
}

#[test]
fn another_projects_attestation_never_passes_on_this_board() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        // The same authority's word for another project, sent to this board as its own.
        let mut theirs = attestation(&f, "sim/a", 3);
        theirs.project = "theirs".into();
        let request = accept(&f, "accept");
        let substituted = refused(applied(&f.store, &request, &[sign_attestation(7, &theirs)]));
        assert!(
            substituted.contains("the attestation of approved is for project theirs"),
            "{backend:?}: {substituted}"
        );
        // A request naming another project is no request to this board at all.
        let mut named = accept(&f, "accept-2");
        named.project = "theirs".into();
        assert!(
            refused(applied(&f.store, &named, &[sign_attestation(7, &theirs)]))
                .starts_with("unknown_id: the request is for project theirs")
        );
        // An attestation for another task of this project is refused as well.
        let mut sibling = attestation(&f, "sim/a", 3);
        sibling.task = "sim/b".into();
        assert!(
            refused(applied(
                &f.store,
                &request,
                &[sign_attestation(7, &sibling)]
            ))
            .contains("is for task sim/b")
        );
        applied(&f.store, &request, &[attested(&f, "sim/a", 3)]).unwrap();
    }
}

/// `custom` as another commit of the project file records it: another digest, the same words.
fn from_another_commit(mut custom: Custom) -> Custom {
    custom.revision = (custom.definition)
        .revision(symphony_api::workflow::Source {
            commit: "3".repeat(40),
            project_blob: "4".repeat(40),
        })
        .unwrap();
    custom
}

#[test]
fn a_changed_key_is_a_new_activation_and_the_old_keys_word_stops() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        let old = attested(&f, "sim/a", 3);
        // The same words from another commit are the workflow active already.
        assert!(matches!(
            f.store
                .activate_workflow(&from_another_commit(guarded_two(7)), &Host::local())
                .unwrap(),
            Followed::Unchanged(_)
        ));
        let rotated = from_another_commit(guarded_two(8));
        let Followed::Activated(active) =
            f.store.activate_workflow(&rotated, &Host::local()).unwrap()
        else {
            panic!("a changed key activates a new generation");
        };
        assert_eq!(active.generation, 2, "{backend:?}");
        assert!(matches!(
            f.store.activate_workflow(&rotated, &Host::local()).unwrap(),
            Followed::Unchanged(_)
        ));
        let request = accept(&f, "accept");
        let unverified = refused(applied(&f.store, &request, &[old]));
        assert!(unverified.contains("does not verify"), "{unverified}");
        // The old key reports nothing more; the new one goes on from the state accepted.
        assert!(refused(observe(&f.store, 4, Some(3))).contains("committed key"));
        let next = observation("e1", 4, Some(("e1", 3)));
        f.store
            .observe_authority(&sign_observation(8, &next))
            .unwrap();
        let current = attestation(&f, "sim/a", 4);
        assert_eq!(current.activation_generation, 2);
        let request = accept(&f, "accept-2");
        applied(&f.store, &request, &[sign_attestation(8, &current)]).unwrap();
    }
}

#[test]
fn a_workflow_committing_an_authority_and_naming_no_project_never_activates() {
    let f = Fixture::new(Backend::SurrealDb, Seed::default());
    let unbound = Custom {
        project: None,
        ..guarded_two(7)
    };
    let refusal = refused(f.store.activate_workflow(&unbound, &Host::local()));
    assert!(refusal.contains("names none"), "{refusal}");
    assert_eq!(f.store.workflow_active().unwrap(), None);
}

#[test]
fn an_edge_two_authorities_guard_is_met_by_each_ones_own_word_and_fenced_on_both() {
    for backend in BACKENDS {
        let f = board(&custom(&format!("{}{MAPPED}", two_authorities_text())));
        let amber = agent_session("amber");
        take(&f.store, "sim/a", &amber, "claim").unwrap();
        take(&f.store, "sim/a", &amber, "submit").unwrap();
        let reviews = |generation: u64, previous: Option<u64>| Observation {
            adapter: "reviews".into(),
            issuer: "panel".into(),
            ..observation("f1", generation, previous.map(|p| ("f1", p)))
        };
        observe(&f.store, 3, None).unwrap();
        f.store
            .observe_authority(&sign_observation(8, &reviews(5, None)))
            .unwrap();
        let approved = attestation(&f, "sim/a", 3);
        let reviewed = Attestation {
            adapter: "reviews".into(),
            issuer: "panel".into(),
            epoch: "f1".into(),
            generation: 5,
            guard: "reviewed".into(),
            ..approved.clone()
        };
        // `approvals`'s key on `reviewed`, at the state the board accepted from `reviews`.
        let crossed = Attestation {
            guard: "reviewed".into(),
            epoch: "f1".into(),
            generation: 5,
            ..approved.clone()
        };
        let given = [
            sign_attestation(7, &approved),
            sign_attestation(8, &reviewed),
        ];
        let crossing = refused(applied(
            &f.store,
            &accept(&f, "crossed"),
            &[
                sign_attestation(7, &approved),
                sign_attestation(7, &crossed),
            ],
        ));
        assert!(
            crossing.starts_with("guard_unavailable:") && crossing.contains("only reviews"),
            "{backend:?}: {crossing}"
        );
        applied(&f.store, &accept(&f, "accept"), &given).unwrap();
        let states = f.store.workflow_authorities().unwrap().accepted;
        let counted: Vec<(&str, u64)> = (states.iter())
            .map(|a| (a.adapter.as_str(), a.admitted))
            .collect();
        assert_eq!(counted, [("approvals", 1), ("reviews", 1)], "{backend:?}");
        assert_eq!(
            f.store.revalidate_dispatch("sim/a", 4, &[]).unwrap().len(),
            2
        );
        // One authority's later decision stops the dispatch, whatever the other's says.
        f.store
            .observe_authority(&sign_observation(8, &reviews(6, Some(5))))
            .unwrap();
        let revoked = refused(f.store.revalidate_dispatch("sim/a", 4, &[]));
        assert!(revoked.contains("from reviews"), "{backend:?}: {revoked}");
    }
}

#[test]
fn a_transition_admitted_under_an_older_activation_is_not_dispatched_under_a_newer_one() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        let request = accept(&f, "accept");
        applied(&f.store, &request, &[attested(&f, "sim/a", 3)]).unwrap();
        assert_eq!(
            f.store.revalidate_dispatch("sim/a", 4, &[]).unwrap().len(),
            1
        );
        let rotated = from_another_commit(guarded_two(8));
        assert!(matches!(
            f.store.activate_workflow(&rotated, &Host::local()).unwrap(),
            Followed::Activated(_)
        ));
        let stale = refused(f.store.revalidate_dispatch("sim/a", 4, &[]));
        assert!(
            stale.starts_with("stale_activation:")
                && stale.contains("admitted under generation 1, and generation 2 is active"),
            "{backend:?}: {stale}"
        );
    }
}

#[test]
fn a_copy_carries_admissions_and_leaves_the_authorities_state_behind() {
    for backend in BACKENDS {
        let f = awaiting();
        observe(&f.store, 3, None).unwrap();
        applied(&f.store, &accept(&f, "accept"), &[attested(&f, "sim/a", 3)]).unwrap();
        let dump = f.store.dump().unwrap();
        assert!(dump.workflow.iter().any(|row| row.kind == "authority"));
        let copy = Fixture::new(Seed::default());
        copy.store.copy(&dump).unwrap();
        assert!(copy.store.workflow_admission("sim/a", 4).unwrap().is_some());
        let authorities = copy.store.workflow_authorities().unwrap();
        assert_eq!(authorities.attesters.len(), 1);
        assert_eq!(authorities.accepted, Vec::new(), "{backend:?}");
        // Its guards wait for the authority to report again: from nothing, as a first report.
        assert!(matches!(
            observe(&copy.store, 3, None),
            Ok(Observing::Accepted(_))
        ));
    }
}
