//! A guarded edge on a board, on both databases: admitted only with an attestation the board
//! verifies at its authority's accepted state, recorded, fenced against an observation that moves
//! that state, and checked again before a dispatch.

use super::tests::{BACKENDS, board, refused, request, take};
use super::*;
use crate::store_fixture::{Backend, Fixture, Seed};
use crate::workflow::activate::tests::{MAPPED, custom};
use crate::workflow::guard::tests::{guarded_text, passing, sign_attestation, sign_observation};
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
fn awaiting(backend: Backend) -> Fixture {
    let f = board(backend, &guarded_two(7));
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
    Attestation {
        task: slug.into(),
        workflow: active.digest,
        activation_generation: active.generation,
        task_revision: position.revision,
        at: position.at(),
        generation,
        issued_at: f.now() - 1.0,
        expires_at: f.now() + 300.0,
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
        let f = awaiting(backend);
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
        assert_eq!(applied(&f.store, &request, &[signed]).unwrap(), result);
        assert_eq!(
            accepted_state(&f).admitted,
            1,
            "a retry admits nothing again"
        );
        let mut other = request.clone();
        other.edge = "submit".into();
        assert!(refused(applied(&f.store, &other, &[])).contains("accept"));

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
        let f = awaiting(backend);
        assert_eq!(f.store.revalidate_dispatch("sim/a", 3, &[]), Ok(Vec::new()));
        assert!(refused(f.store.revalidate_dispatch("sim/a", 2, &[])).starts_with("stale_task:"));
    }
}

#[test]
fn a_decision_revoked_since_the_attestation_refuses_it_and_stops_its_dispatch() {
    for backend in BACKENDS {
        let f = awaiting(backend);
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
        // Expired by the time of the dispatch.
        f.advance(400.0);
        assert!(
            refused(
                f.store
                    .revalidate_dispatch("sim/a", 4, &[sign_attestation(7, &fresh)])
            )
            .contains("expired")
        );
    }
}

#[test]
fn a_revocation_between_the_read_and_the_write_makes_the_admission_decide_again() {
    for backend in BACKENDS {
        let f = awaiting(backend);
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
        let f = awaiting(backend);
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
        let f = awaiting(backend);
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
fn an_observation_moves_the_authority_only_forward_and_only_by_its_committed_key() {
    for backend in BACKENDS {
        let f = board(backend, &guarded_two(7));
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

        let none = Fixture::on(backend, Seed::default());
        assert!(refused(observe(&none.store, 3, None)).contains("no workflow is active"));
    }
}

#[test]
fn two_decisions_at_one_generation_hold_every_admission_until_a_later_state() {
    for backend in BACKENDS {
        let f = awaiting(backend);
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
        let f = awaiting(backend);
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
        let f = awaiting(backend);
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
fn a_copy_carries_admissions_and_leaves_the_authorities_state_behind() {
    for backend in BACKENDS {
        let f = awaiting(backend);
        observe(&f.store, 3, None).unwrap();
        applied(&f.store, &accept(&f, "accept"), &[attested(&f, "sim/a", 3)]).unwrap();
        let dump = f.store.dump().unwrap();
        assert!(dump.workflow.iter().any(|row| row.kind == "authority"));
        let copy = Fixture::on(backend, Seed::default());
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
