//! The external-effect boundary on a board, on both databases, with an adapter the test stands in
//! for: an intent durable with its prepare, a reservation stale or cancelled refused before
//! dispatch, finalize only on what the adapter read back, and an effect that happened recorded
//! whatever moved in the gap.

use super::super::tests::{BACKENDS, apply, board, pair, refused, request, take};
use super::*;
use crate::store_fixture::Fixture;
use crate::workflow::activate::tests::{MAPPED, custom};
use crate::workflow::agent_session;
use crate::workflow::effect::tests::{LANDING, landing};
use crate::workflow::effect::{DEFAULT_LAND, Outcome, PROJECT_LAND};
use std::cell::RefCell;
use symphony_api::workflow::OperationResult;

const SLUG: &str = "sim/a";
const HEAD: &str = "1111111111111111111111111111111111111111";

/// An adapter whose system is the test's: what it reads now, and what it reads back.
struct Fake {
    name: &'static str,
    now: RefCell<Bindings>,
    outcome: RefCell<Option<Outcome>>,
    unreadable: RefCell<bool>,
}

impl Fake {
    fn new(name: &'static str, bound: &Bindings) -> Fake {
        Fake {
            name,
            now: RefCell::new(bound.clone()),
            outcome: RefCell::new(None),
            unreadable: RefCell::new(false),
        }
    }

    /// The effect happened, under the policy and the evidence bound.
    fn happened(&self, receipt: &str) {
        let now = self.now.borrow();
        self.outcome.replace(Some(Outcome {
            receipt: receipt.into(),
            policy: now.policy.clone(),
            evidence: now.evidence.clone(),
        }));
    }
}

impl Adapter for Fake {
    fn name(&self) -> &str {
        self.name
    }

    fn current(&self, _: &Intent) -> Result<Bindings, String> {
        if *self.unreadable.borrow() {
            return Err("the repository is gone".into());
        }
        Ok(self.now.borrow().clone())
    }

    fn outcome(&self, _: &Intent) -> Result<Option<Outcome>, String> {
        if *self.unreadable.borrow() {
            return Err("the repository is gone".into());
        }
        Ok(self.outcome.borrow().clone())
    }
}

fn bindings(f: &Fixture, head: &str) -> Bindings {
    Bindings {
        project: f.store.database().database().into(),
        task: SLUG.into(),
        repository: "/repo/.git".into(),
        pull: None,
        head: head.into(),
        policy: "policy-1".into(),
        evidence: "evidence-1".into(),
        destination: "refs/heads/main".into(),
    }
}

fn orch() -> Principal {
    agent_session("orch")
}

/// `edge` taken on sim/a by `principal`, its `head` input given.
fn take_with_head(f: &Fixture, principal: &Principal, edge: &str) -> OperationResult {
    let mut request = request(&f.store, SLUG, principal, edge, &format!("{SLUG}:{edge}"));
    request
        .inputs
        .insert("head".into(), InputValue::Revision(HEAD.into()));
    apply(&f.store, principal, &request).unwrap()
}

/// A board on `adapter`'s landing workflow, sim/a built by amber and approved by birch at HEAD:
/// at deliver/approved, workflow revision 4.
fn approved(backend: crate::store_fixture::Backend, adapter: &str) -> Fixture {
    let f = board(&landing(adapter));
    let amber = agent_session("amber");
    take(&f.store, SLUG, &amber, "claim").unwrap();
    take(&f.store, SLUG, &amber, "submit").unwrap();
    take_with_head(&f, &agent_session("birch"), "approve");
    assert_eq!(position(&f), (pair("deliver", "approved"), 4));
    f
}

fn position(f: &Fixture) -> (symphony_api::workflow::Pair, u64) {
    let position = f.store.workflow_position(SLUG).unwrap().unwrap();
    (position.at(), position.revision)
}

/// The prepare edge's request from where sim/a stands, under `key`.
fn preparing(f: &Fixture, key: &str) -> OperationRequest {
    let mut request = request(&f.store, SLUG, &orch(), "prepare_land", key);
    request
        .inputs
        .insert("head".into(), InputValue::Revision(HEAD.into()));
    request
}

/// sim/a prepared by orch for `fake`: its intent, at workflow revision 5.
fn prepared(f: &Fixture, fake: &Fake) -> Intent {
    let bound = fake.now.borrow().clone();
    (f.store)
        .prepare_effect(
            &orch(),
            &Host::local(),
            fake,
            &preparing(f, "land:1"),
            &bound,
        )
        .unwrap()
}

fn intent(f: &Fixture) -> Intent {
    f.store.effect_intent(SLUG, 5).unwrap().unwrap()
}

fn finalize(f: &Fixture, fake: &Fake) -> Result<Finalized, Error> {
    (f.store).finalize_effect(&orch(), &Host::local(), fake, SLUG, 5)
}

#[test]
fn an_effect_is_prepared_with_its_intent_dispatched_and_finalized_on_what_was_read_back() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let bound = bindings(&f, HEAD);
        let fake = Fake::new(DEFAULT_LAND, &bound);
        assert_eq!(f.store.effect_intent(SLUG, 5).unwrap(), None);
        let made = prepared(&f, &fake);
        // Durable beside its transition, before anything is sent.
        assert_eq!(position(&f), (pair("deliver", "landing"), 5));
        assert_eq!(made, intent(&f));
        assert_eq!(
            (made.id.as_str(), made.revision, made.stage),
            ("sim/a@5", 5, Stage::Prepared)
        );
        assert_eq!(
            (made.adapter.as_str(), made.key.as_str()),
            (DEFAULT_LAND, "land:1")
        );
        assert_eq!(made.bindings, bound);
        let active = f.store.workflow_active().unwrap().unwrap();
        assert_eq!(
            (made.generation, &made.digest),
            (active.generation, &active.digest)
        );
        assert_eq!(made.principal, orch());
        let transition = f.store.workflow_transition(SLUG, 5).unwrap().unwrap();
        assert_eq!(transition.edge.as_deref(), Some("prepare_land"));

        let dispatched = f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        assert_eq!(dispatched.stage, Stage::Dispatched);
        assert_eq!(intent(&f).stage, Stage::Dispatched);
        // Dispatched again while nothing happened: its bindings read again, and it stands.
        assert_eq!(f.store.dispatch_effect(&fake, SLUG, 5).unwrap(), dispatched);

        fake.happened("merge1");
        let done = finalize(&f, &fake).unwrap();
        assert_eq!(done.transition.as_deref(), Some("sim/a@6"));
        assert_eq!(done.intent, intent(&f));
        assert_eq!(done.intent.stage, Stage::Finalized);
        assert_eq!(done.intent.receipt.as_deref(), Some("merge1"));
        assert_eq!(done.intent.recovery, None);
        assert_eq!(position(&f), (pair("deliver", "landed"), 6));
        assert_eq!(f.store.get(SLUG).unwrap().status, "done");
        let confirmed = f.store.workflow_transition(SLUG, 6).unwrap().unwrap();
        assert_eq!(confirmed.edge.as_deref(), Some("confirm_land"));
        // The receipt read back is the finalize request's input, on its receipt.
        let receipt = (f
            .store
            .workflow_receipt(&orch(), "land:1:finalize")
            .unwrap())
        .unwrap();
        assert_eq!(
            receipt.sent.inputs.get("receipt"),
            Some(&InputValue::Revision("merge1".into()))
        );
        let said = f.said(SLUG);
        let has = |text: &str| said.iter().any(|line| line.contains(text));
        assert!(
            has("prepared prepare_land for default_land (sim/a@5)"),
            "{said:?}"
        );
        assert!(has(&format!("bound to head {HEAD}")), "{said:?}");
        assert!(
            has("finalized sim/a@5 by default_land (sim/a@6): merge1"),
            "{said:?}"
        );
        assert!(!has("RECOVERY"), "{said:?}");
    }
}

#[test]
fn a_callers_word_never_prepares_or_finalizes() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        // The list of its actions says so before anyone asks: the edge is its adapter's.
        let listed = |edge: &str| {
            let next = (f.store)
                .workflow_actions(SLUG, &orch(), &Host::local())
                .unwrap();
            let action = next.actions.into_iter().find(|a| a.edge == edge).unwrap();
            action.unavailable
        };
        assert_eq!(
            listed("prepare_land"),
            vec![RefusalCode::ExplicitActionRequired]
        );
        let why = refused(apply(&f.store, &orch(), &preparing(&f, "plain")));
        assert_eq!(
            why,
            "explicit_action_required: edge prepare_land is the adapter default_land's to take, \
             with what it reads of the effect itself: no request prepares or finalizes one; `just \
             land sim/a` takes it"
        );
        assert_eq!(position(&f), (pair("deliver", "approved"), 4));
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        let mut confirm = request(&f.store, SLUG, &orch(), "confirm_land", "plain:confirm");
        (confirm.inputs).insert("receipt".into(), InputValue::Revision("made-up".into()));
        let why = refused(apply(&f.store, &orch(), &confirm));
        assert!(
            why.starts_with("explicit_action_required: edge confirm_land is the adapter")
                && why.ends_with("; `just land sim/a` takes it"),
            "{why}"
        );
        assert_eq!(
            listed("confirm_land"),
            vec![RefusalCode::ExplicitActionRequired]
        );
        assert_eq!(listed("send_back"), vec![]);
        assert_eq!(position(&f), (pair("deliver", "landing"), 5));
        assert_eq!(intent(&f).stage, Stage::Dispatched);
    }
}

#[test]
fn a_prepare_is_refused_for_an_adapter_or_bindings_that_are_not_the_edges() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let bound = bindings(&f, HEAD);
        let prepare = |fake: &Fake, bound: &Bindings| {
            refused((f.store).prepare_effect(
                &orch(),
                &Host::local(),
                fake,
                &preparing(&f, "land:1"),
                bound,
            ))
        };
        let unknown = prepare(&Fake::new("someone_else", &bound), &bound);
        assert_eq!(
            unknown,
            "unsupported_adapter: this host registers no adapter someone_else"
        );
        let other = prepare(&Fake::new(PROJECT_LAND, &bound), &bound);
        assert_eq!(
            other,
            "unsupported_adapter: edge prepare_land prepares nothing for the adapter project_land"
        );
        let fake = Fake::new(DEFAULT_LAND, &bound);
        for change in [
            (|b: &mut Bindings| b.task = "sim/b".into()) as fn(&mut Bindings),
            |b| b.project = "elsewhere".into(),
        ] {
            let mut elsewhere = bound.clone();
            change(&mut elsewhere);
            let why = prepare(&fake, &elsewhere);
            assert!(
                why.starts_with("unknown_id: the effect is bound to"),
                "{why}"
            );
        }
        // Admission's own refusals are the prepare's: amber is not the orchestrator.
        let amber = agent_session("amber");
        let mut by_amber = request(&f.store, SLUG, &amber, "prepare_land", "land:amber");
        (by_amber.inputs).insert("head".into(), InputValue::Revision(HEAD.into()));
        let why =
            refused((f.store).prepare_effect(&amber, &Host::local(), &fake, &by_amber, &bound));
        assert!(why.starts_with("unauthorized:"), "{why}");
        // Nothing was written by any of them.
        assert_eq!(position(&f), (pair("deliver", "approved"), 4));
        assert_eq!(f.store.effect_intent(SLUG, 5).unwrap(), None);
    }
}

#[test]
fn a_prepare_repeated_under_its_key_answers_its_intent_and_refuses_other_bindings() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let bound = bindings(&f, HEAD);
        let fake = Fake::new(DEFAULT_LAND, &bound);
        let request = preparing(&f, "land:1");
        let prepare = |bound: &Bindings| {
            (f.store).prepare_effect(&orch(), &Host::local(), &fake, &request, bound)
        };
        let first = prepare(&bound).unwrap();
        assert_eq!(prepare(&bound).unwrap(), first);
        assert_eq!(position(&f), (pair("deliver", "landing"), 5));
        let mut other = bound.clone();
        other.head = "2222222222222222222222222222222222222222".into();
        let why = refused(prepare(&other));
        assert!(
            why.contains("the key land:1 was used for another request (sim/a@5)"),
            "{why}"
        );
        assert_eq!(intent(&f), first);
    }
}

#[test]
fn a_reservation_whose_binding_moved_is_withdrawn_before_dispatch_and_nothing_is_sent() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        fake.now.borrow_mut().head = "2222222222222222222222222222222222222222".into();
        let why = refused(f.store.dispatch_effect(&fake, SLUG, 5));
        assert!(
            why.starts_with(
                "stale_input: sim/a@5 is withdrawn, and nothing is sent under it: its head was 1111"
            ),
            "{why}"
        );
        let held = intent(&f);
        assert_eq!(held.stage, Stage::Withdrawn);
        assert!(held.withdrawn.as_deref().unwrap().contains("and is 2222"));
        // Withdrawn for good: neither a dispatch with the binding back, nor a finalize.
        fake.now.borrow_mut().head = HEAD.into();
        let again = refused(f.store.dispatch_effect(&fake, SLUG, 5));
        assert!(
            again.starts_with("stale_input: sim/a@5 is withdrawn"),
            "{again}"
        );
        fake.happened("merge1");
        let why = refused(finalize(&f, &fake));
        assert!(
            why.starts_with("stale_input: sim/a@5 is withdrawn"),
            "{why}"
        );
        assert_eq!(position(&f), (pair("deliver", "landing"), 5));
        assert_eq!(intent(&f).receipt, None);
    }
}

#[test]
fn a_reservation_cancelled_by_an_edge_out_is_withdrawn_before_dispatch() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        take(&f.store, SLUG, &orch(), "send_back").unwrap();
        let why = refused(f.store.dispatch_effect(&fake, SLUG, 5));
        assert!(
            why.contains("sim/a moved on to verify/awaiting (workflow revision 6, prepared at 5)"),
            "{why}"
        );
        assert_eq!(intent(&f).stage, Stage::Withdrawn);
    }
}

#[test]
fn a_dispatch_is_refused_while_the_adapter_cannot_read_and_the_intent_stays() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        fake.unreadable.replace(true);
        let why = refused(f.store.dispatch_effect(&fake, SLUG, 5));
        assert!(
            why.contains("could not read what its effect is bound to: the repository is gone"),
            "{why}"
        );
        assert_eq!(intent(&f).stage, Stage::Prepared);
        let none = refused(f.store.dispatch_effect(&fake, SLUG, 9));
        assert_eq!(
            none,
            "unknown_id: no effect was prepared at workflow revision 9 of sim/a"
        );
        let theirs = refused((f.store).dispatch_effect(
            &Fake::new(PROJECT_LAND, &bindings(&f, HEAD)),
            SLUG,
            5,
        ));
        assert!(
            theirs.contains("is the adapter default_land's, not project_land's"),
            "{theirs}"
        );
    }
}

#[test]
fn nothing_is_finalized_without_a_result_the_adapter_read_back() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        // Never dispatched: nothing was sent, whatever anyone says happened.
        fake.happened("merge1");
        let why = refused(finalize(&f, &fake));
        assert!(
            why.starts_with("explicit_action_required: sim/a@5 was never dispatched"),
            "{why}"
        );
        fake.outcome.replace(None);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        // Dispatched, and the adapter reads back nothing.
        let why = refused(finalize(&f, &fake));
        assert!(
            why.starts_with(
                "explicit_action_required: the adapter default_land read back no result of sim/a@5"
            ),
            "{why}"
        );
        fake.unreadable.replace(true);
        let why = refused(finalize(&f, &fake));
        assert!(
            why.starts_with("unsupported_adapter: the adapter could not read"),
            "{why}"
        );
        assert_eq!(position(&f), (pair("deliver", "landing"), 5));
        let held = intent(&f);
        assert_eq!((held.stage, held.receipt), (Stage::Dispatched, None));
    }
}

#[test]
fn a_crash_between_the_effect_and_its_finalize_is_finished_by_the_same_intent_once() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        // The effect happened, and the process that made it is gone: another reads it back.
        fake.happened("merge1");
        let again = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        again.happened("merge1");
        let why = refused(f.store.dispatch_effect(&again, SLUG, 5));
        assert!(
            why.contains("the effect of sim/a@5 happened already: it is finalized, not sent again"),
            "{why}"
        );
        let done = finalize(&f, &again).unwrap();
        assert_eq!(done.transition.as_deref(), Some("sim/a@6"));
        // Finalized again: the same answer, and no second transition.
        assert_eq!(finalize(&f, &again).unwrap(), done);
        assert_eq!(position(&f), (pair("deliver", "landed"), 6));
        assert_eq!(f.store.workflow_transition(SLUG, 7).unwrap(), None);
        let why = refused(f.store.dispatch_effect(&again, SLUG, 5));
        assert!(
            why.contains("is finalized: nothing more is sent under it"),
            "{why}"
        );
    }
}

#[test]
fn a_policy_or_evidence_changed_in_the_gap_records_the_outcome_and_the_recovery_owed() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        fake.outcome.replace(Some(Outcome {
            receipt: "merge1".into(),
            policy: "policy-2".into(),
            evidence: "evidence-1".into(),
        }));
        let done = finalize(&f, &fake).unwrap();
        // The merge happened: it is recorded as it was authorized, and the edge taken.
        assert_eq!(done.transition.as_deref(), Some("sim/a@6"));
        assert_eq!(done.intent.receipt.as_deref(), Some("merge1"));
        assert_eq!(done.intent.bindings.policy, "policy-1");
        assert_eq!(
            done.intent.recovery.as_deref(),
            Some(
                "the landing policy was policy-1 when prepared and policy-2 when the effect happened"
            )
        );
        assert_eq!(done.intent, intent(&f));
        let line = (f.store.thread(SLUG).unwrap().into_iter())
            .find(|m| m.text.contains("RECOVERY REQUIRED"))
            .unwrap();
        assert_eq!(line.recipient.as_deref(), Some("@orchestrator"));
        assert!(
            line.text
                .contains("nothing here says the policy now in force accepts it")
        );
    }
}

#[test]
fn an_effect_that_happened_after_its_task_moved_is_recorded_and_takes_no_edge() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        // In flight, it cannot be recalled: the edge out is taken, and the merge still happens.
        take(&f.store, SLUG, &orch(), "send_back").unwrap();
        fake.happened("merge1");
        let done = finalize(&f, &fake).unwrap();
        assert_eq!(done.transition, None);
        assert_eq!(done.intent.stage, Stage::Finalized);
        assert_eq!(done.intent.receipt.as_deref(), Some("merge1"));
        let recovery = done.intent.recovery.clone().unwrap();
        assert!(
            recovery.contains("sim/a moved on to verify/awaiting"),
            "{recovery}"
        );
        assert_eq!(position(&f), (pair("verify", "awaiting"), 6));
        let line = (f.store.thread(SLUG).unwrap().into_iter())
            .find(|m| m.text.contains("its finalize edge is not taken"))
            .unwrap();
        assert_eq!(line.recipient.as_deref(), Some("@orchestrator"));
        assert!(
            line.text
                .contains("the effect of sim/a@5 happened (default_land: merge1)")
        );
        // Asked again, the same record: still no edge.
        assert_eq!(finalize(&f, &fake).unwrap().transition, None);
        assert_eq!(position(&f), (pair("verify", "awaiting"), 6));
    }
}

#[test]
fn an_effect_that_happened_under_another_generation_is_recorded_and_takes_no_edge() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        let relabelled = custom(
            &format!("{LANDING}{MAPPED}").replace("label = \"Landing\"", "label = \"Merging\""),
        );
        f.store
            .activate_workflow(&relabelled, &Host::local())
            .unwrap();
        fake.happened("merge1");
        let done = finalize(&f, &fake).unwrap();
        assert_eq!(done.transition, None);
        let recovery = done.intent.recovery.unwrap();
        assert!(
            recovery.contains("the workflow activated again since it was prepared"),
            "{recovery}"
        );
        assert_eq!(intent(&f).receipt.as_deref(), Some("merge1"));
        // A dispatch under the new generation would have been withdrawn the same way.
        assert_eq!(position(&f).0, pair("deliver", "landing"));
    }
}

#[test]
fn a_finalize_edge_refused_records_the_outcome_and_is_taken_by_who_may() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        fake.happened("merge1");
        let amber = agent_session("amber");
        let by_amber = (f.store)
            .finalize_effect(&amber, &Host::local(), &fake, SLUG, 5)
            .unwrap();
        assert_eq!(by_amber.transition, None);
        let recovery = by_amber.intent.recovery.unwrap();
        assert!(
            recovery.starts_with("its finalize edge is refused (unauthorized:"),
            "{recovery}"
        );
        assert_eq!(position(&f), (pair("deliver", "landing"), 5));
        // The orchestrator's finalize takes the edge, and owes nothing.
        let done = finalize(&f, &fake).unwrap();
        assert_eq!(done.transition.as_deref(), Some("sim/a@6"));
        assert_eq!(done.intent.recovery, None);
        assert_eq!(position(&f), (pair("deliver", "landed"), 6));
        // An adapter the host does not register finalizes nothing.
        let why = refused((f.store).finalize_effect(
            &orch(),
            &Host::local(),
            &Fake::new("someone_else", &bindings(&f, HEAD)),
            SLUG,
            5,
        ));
        assert_eq!(
            why,
            "unsupported_adapter: this host registers no adapter someone_else"
        );
    }
}

#[test]
fn the_review_standing_is_read_from_the_approves_transition_and_its_receipt() {
    for backend in BACKENDS {
        let f = board(&landing(DEFAULT_LAND));
        assert_eq!(f.store.reviewed(SLUG).unwrap(), None);
        // A task no workflow holds has none.
        assert_eq!(f.store.reviewed("sim/dep").unwrap(), None);
        let amber = agent_session("amber");
        take(&f.store, SLUG, &amber, "claim").unwrap();
        take(&f.store, SLUG, &amber, "submit").unwrap();
        assert_eq!(f.store.reviewed(SLUG).unwrap(), None);
        let birch = agent_session("birch");
        take_with_head(&f, &birch, "approve");
        let reviewed = f.store.reviewed(SLUG).unwrap().unwrap();
        assert_eq!(reviewed.transition, "sim/a@4");
        assert_eq!(reviewed.by, birch);
        assert_eq!(reviewed.head.as_deref(), Some(HEAD));
        // Later transitions that record no review leave it standing.
        prepared(&f, &Fake::new(DEFAULT_LAND, &bindings(&f, HEAD)));
        assert_eq!(
            f.store.reviewed(SLUG).unwrap().unwrap().transition,
            "sim/a@4"
        );
    }
}

/// An adapter that reads back what `inner` does, once `then` has run: what another process did
/// between a finalize's first read of its intent and its write.
struct Raced<'a> {
    inner: &'a Fake,
    then: Box<dyn Fn() + 'a>,
}

impl Adapter for Raced<'_> {
    fn name(&self) -> &str {
        self.inner.name
    }

    fn current(&self, intent: &Intent) -> Result<Bindings, String> {
        self.inner.current(intent)
    }

    fn outcome(&self, intent: &Intent) -> Result<Option<Outcome>, String> {
        (self.then)();
        self.inner.outcome(intent)
    }
}

#[test]
fn a_finalize_that_meets_an_intent_withdrawn_since_it_read_it_answers_withdrawn_at_once() {
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        fake.happened("merge1");
        // A dispatch run again, which reads another head and no merge, withdraws the intent
        // after the finalize read it dispatched.
        let other = "2222222222222222222222222222222222222222";
        let mover = Fake::new(DEFAULT_LAND, &bindings(&f, other));
        let raced = Raced {
            inner: &fake,
            then: Box::new(|| {
                let why = refused(f.store.dispatch_effect(&mover, SLUG, 5));
                assert!(
                    why.starts_with("stale_input: sim/a@5 is withdrawn"),
                    "{why}"
                );
            }),
        };
        let why = refused((f.store).finalize_effect(&orch(), &Host::local(), &raced, SLUG, 5));
        assert_eq!(
            why,
            format!(
                "stale_input: sim/a@5 is withdrawn, and nothing is sent under it: its head was \
                 {HEAD} when prepared and is {other}"
            )
        );
        let held = intent(&f);
        assert_eq!((held.stage, held.receipt), (Stage::Withdrawn, None));
        assert_eq!(position(&f), (pair("deliver", "landing"), 5));
    }
}

#[test]
fn a_projects_own_landing_takes_the_same_boundary_with_its_pull_request_bound() {
    for backend in BACKENDS {
        let f = approved(PROJECT_LAND);
        let mut bound = bindings(&f, HEAD);
        bound.pull = Some("github.com/acme/widgets#42".into());
        let fake = Fake::new(PROJECT_LAND, &bound);
        let made = prepared(&f, &fake);
        assert_eq!(made.adapter, PROJECT_LAND);
        assert_eq!(
            made.bindings.pull.as_deref(),
            Some("github.com/acme/widgets#42")
        );
        // The default landing is not this workflow's adapter.
        let why = refused((f.store).dispatch_effect(&Fake::new(DEFAULT_LAND, &bound), SLUG, 5));
        assert!(
            why.contains("is the adapter project_land's, not default_land's"),
            "{why}"
        );
        // The pull request bound is the one dispatched: another withdraws it.
        fake.now.borrow_mut().pull = Some("github.com/acme/widgets#43".into());
        let why = refused(f.store.dispatch_effect(&fake, SLUG, 5));
        assert!(
            why.contains("its pull request was github.com/acme/widgets#42 when prepared"),
            "{why}"
        );
        assert_eq!(intent(&f).stage, Stage::Withdrawn);
    }
}

#[test]
fn a_host_that_registers_no_adapter_neither_activates_nor_admits_its_edges() {
    let custom = landing(DEFAULT_LAND);
    let mut bare = Host::local();
    bare.adapters.clear();
    let why = crate::workflow::activate::supported(&custom.definition, &bare).unwrap_err();
    assert_eq!(why, "unsupported adapter: default_land");
    for backend in BACKENDS {
        let f = approved(DEFAULT_LAND);
        let actions = f.store.workflow_actions(SLUG, &orch(), &bare).unwrap();
        let prepare = (actions.actions.iter())
            .find(|a| a.edge == "prepare_land")
            .unwrap();
        assert_eq!(
            prepare.unavailable,
            vec![
                RefusalCode::UnsupportedAdapter,
                RefusalCode::ExplicitActionRequired
            ]
        );
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        let why = refused((f.store).prepare_effect(
            &orch(),
            &bare,
            &fake,
            &preparing(&f, "land:1"),
            &bindings(&f, HEAD),
        ));
        assert_eq!(
            why,
            "unsupported_adapter: this host registers no adapter default_land"
        );
    }
}

/// The authority `approvals` reports generation `generation` of e1, following `previous`.
fn reported(f: &Fixture, generation: u64, previous: Option<u64>) {
    use symphony_api::workflow::{ATTESTATION_CONTRACT, AuthorityState, Observation};
    let observed = Observation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation,
        state: format!("s{generation}"),
        previous: previous.map(|generation| AuthorityState {
            epoch: "e1".into(),
            generation,
            state: format!("s{generation}"),
        }),
        at: 1.0,
    };
    let signed = crate::workflow::guard::tests::sign_observation(7, &observed);
    f.store.observe_authority(&signed).unwrap();
}

/// [`guarded_landing`] where a task stays in its approved pair
/// ([`crate::workflow::effect::tests::guarded_landing_with_stays_text`]).
fn guarded_landing_with_stays() -> crate::workflow::Custom {
    use crate::workflow::effect::tests::guarded_landing_with_stays_text;
    crate::workflow::activate::tests::custom(&guarded_landing_with_stays_text())
}

/// [`approved`] on [`guarded_landing`]: birch's approve taken with the authority's pass, met at
/// generation 3 of e1, and recorded as an admission.
fn approved_under_guard(backend: crate::store_fixture::Backend) -> Fixture {
    use crate::workflow::effect::tests::guarded_landing;
    approved_under_guard_on(&guarded_landing(DEFAULT_LAND, 7))
}

/// [`approved_under_guard`] on `custom`.
fn approved_under_guard_on(
    backend: crate::store_fixture::Backend,
    custom: &crate::workflow::Custom,
) -> Fixture {
    use crate::workflow::guard::tests::{passing, sign_attestation};
    use crate::workflow::guard::{MAX_VALIDITY, attestee};
    let f = board(custom);
    let amber = agent_session("amber");
    take(&f.store, SLUG, &amber, "claim").unwrap();
    take(&f.store, SLUG, &amber, "submit").unwrap();
    reported(&f, 3, None);
    let birch = agent_session("birch");
    let mut request = request(&f.store, SLUG, &birch, "approve", "sim/a:approve");
    request
        .inputs
        .insert("head".into(), InputValue::Revision(HEAD.into()));
    // Without the authority's word, the approve is refused.
    let bare = refused(
        f.store
            .apply_attested(&birch, &Host::local(), &request, &[]),
    );
    assert!(bare.starts_with("guard_unavailable:"), "{bare}");
    let active = f.store.workflow_active().unwrap().unwrap();
    let issued = crate::store::now() - 1.0;
    let attestation = symphony_api::workflow::Attestation {
        edge: "approve".into(),
        workflow: active.digest,
        activation_generation: active.generation,
        task_revision: 3,
        principal: attestee(&birch),
        issued_at: issued,
        expires_at: issued + MAX_VALIDITY,
        ..passing(custom)
    };
    let given = [sign_attestation(7, &attestation)];
    (f.store)
        .apply_attested(&birch, &Host::local(), &request, &given)
        .unwrap();
    assert_eq!(position(&f), (pair("deliver", "approved"), 4));
    assert!(f.store.workflow_admission(SLUG, 4).unwrap().is_some());
    f
}

#[test]
fn a_decision_revoked_between_the_guarded_approve_and_the_land_withdraws_its_dispatch() {
    for backend in BACKENDS {
        // The approve's decision standing, the land dispatches.
        let f = approved_under_guard();
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        prepared(&f, &fake);
        let dispatched = f.store.dispatch_effect(&fake, SLUG, 5).unwrap();
        assert_eq!(dispatched.stage, Stage::Dispatched, "{backend:?}");
        // Revoked before the land's prepare, or after it and before its dispatch: either way the
        // intent is withdrawn and nothing is sent, the prepare edge itself guarded by nothing.
        for revoked_first in [true, false] {
            let f = approved_under_guard();
            let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
            if revoked_first {
                reported(&f, 4, Some(3));
            }
            prepared(&f, &fake);
            if !revoked_first {
                reported(&f, 4, Some(3));
            }
            let why = refused(f.store.dispatch_effect(&fake, SLUG, 5));
            assert!(
                why.starts_with(
                    "stale_input: sim/a@5 is withdrawn, and nothing is sent under it: \
                     approvals decided again since approved was met at generation 3 of e1: a \
                     later decision stands (generation 4 of e1 now)"
                ),
                "{backend:?}: {why}"
            );
            assert_eq!(intent(&f).stage, Stage::Withdrawn, "{backend:?}");
            // Withdrawn for good: a dispatch again sends nothing under it.
            let again = refused(f.store.dispatch_effect(&fake, SLUG, 5));
            assert!(again.contains("is withdrawn"), "{backend:?}: {again}");
        }
    }
}

#[test]
fn a_dispatch_rechecks_the_approve_past_a_block_or_a_release_that_stayed_in_its_pair() {
    let custom = guarded_landing_with_stays();
    let owner = crate::workflow::local_owner();
    // What happens in the approved pair between the approve and the land's prepare.
    let held = |f: &Fixture| {
        take(&f.store, SLUG, &owner, "block").unwrap();
        take(&f.store, SLUG, &owner, "resume").unwrap();
    };
    let released = |f: &Fixture| {
        f.store.release(SLUG, "birch", Some("reviewer")).unwrap();
    };
    // Each with the transitions it records in the pair.
    type Stay<'a> = (&'a str, &'a dyn Fn(&Fixture), u64);
    let stays: [Stay; 2] = [("block and resume", &held, 2), ("release", &released, 1)];
    for backend in BACKENDS {
        for (what, stay, moves) in stays {
            for revoked in [false, true] {
                let f = approved_under_guard_on(&custom);
                stay(&f);
                assert_eq!(
                    position(&f),
                    (pair("deliver", "approved"), 4 + moves),
                    "{backend:?} {what}: stayed in the approved pair"
                );
                assert!(
                    f.store
                        .workflow_admission(SLUG, 4 + moves)
                        .unwrap()
                        .is_none()
                );
                if revoked {
                    reported(&f, 4, Some(3));
                }
                let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
                let intent = prepared(&f, &fake);
                assert_eq!(intent.revision, 5 + moves);
                let dispatched = f.store.dispatch_effect(&fake, SLUG, intent.revision);
                let stage = (f
                    .store
                    .effect_intent(SLUG, intent.revision)
                    .unwrap()
                    .unwrap())
                .stage;
                if !revoked {
                    assert_eq!(
                        dispatched.unwrap().stage,
                        Stage::Dispatched,
                        "{backend:?} {what}"
                    );
                    continue;
                }
                let why = refused(dispatched);
                assert!(
                    why.contains(
                        "approvals decided again since approved was met at generation 3 \
                         of e1: a later decision stands"
                    ),
                    "{backend:?} {what}: {why}"
                );
                assert_eq!(stage, Stage::Withdrawn, "{backend:?} {what}");
            }
        }
    }
}

#[test]
fn a_task_brought_to_the_blocked_pair_by_another_edge_is_never_resumed_past_the_guard() {
    use crate::workflow::effect::tests::guarded_landing_with_stays_text;
    // An edge from verify/awaiting to the blocked pair, which saves no resume.
    let park = "  { id = \"park\", label = \"Park\", from = { phase = \"verify\", state = \
                \"awaiting\" }, to = { phase = \"build\", state = \"blocked\" }, operation = \
                \"advance\", authority = [{ kind = \"project_capability\", capability = \"owner\" \
                }], inputs = [], dependencies = \"none\", guards = [], effects = [{ kind = \
                \"slot\", policy = \"release\" }], automatic = false, human_wait = false },\n";
    let block = "  { id = \"block\",";
    let text = guarded_landing_with_stays_text().replacen(block, &format!("{park}{block}"), 1);
    let custom = crate::workflow::activate::tests::custom(&text);
    let owner = crate::workflow::local_owner();
    for backend in BACKENDS {
        // Approved under the guard at 4, prepared at 5, sent back at 6, parked at 7.
        let f = approved_under_guard_on(&custom);
        let fake = Fake::new(DEFAULT_LAND, &bindings(&f, HEAD));
        let intent = prepared(&f, &fake);
        take(&f.store, SLUG, &orch(), "send_back").unwrap();
        take(&f.store, SLUG, &owner, "park").unwrap();
        assert_eq!(position(&f), (pair("build", "blocked"), 7), "{backend:?}");
        // The resume would stand it at approved past the guard: refused, and it stays.
        let why = refused(take(&f.store, SLUG, &owner, "resume"));
        assert!(
            why.ends_with(
                "edge resume resumes sim/a at deliver/approved, which only edges guarded by \
                 approved enter, and no block saved it there"
            ),
            "{backend:?}: {why}"
        );
        assert_eq!(position(&f), (pair("build", "blocked"), 7), "{backend:?}");
        // Nor is the land's first intent sent: the task left the pair it was prepared from.
        let stale = refused(f.store.dispatch_effect(&fake, SLUG, intent.revision));
        assert!(stale.contains("is withdrawn"), "{backend:?}: {stale}");
        assert_eq!(intent_at(&f, intent.revision).stage, Stage::Withdrawn);
    }
}

fn intent_at(f: &Fixture, revision: u64) -> Intent {
    f.store.effect_intent(SLUG, revision).unwrap().unwrap()
}
