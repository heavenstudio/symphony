//! What a task's page reads of its guards, on a scratch board with a stand-in authority (the
//! fixture of `attest_tests.rs`): the guard that refused the owner's operation, by its word, with
//! what the authority said only where its attestation verified, and the board's records of the
//! authorities, the guarded edges and the admission that let the task in
//! (`crate::guards`), each guard it met standing or not as the authority reports on.

use std::path::PathBuf;

use symphony_api::workflow::{
    ATTESTATION_CONTRACT, Attested, AuthorityState, GuardRefusal, GuardStanding, GuardWhy,
    Observation, OperationRequest, Signed, TaskGuards,
};
use symphony_board::workflow::{Host, local_owner};

use super::tests::{
    Approvals, CLOSED, KEY, OTHER_KEY, OTHER_TASK, PASSING, at, awaiting, pass, plugins, request,
    signed,
};
use super::*;
use crate::taskview::page_tests::store_fixture::Fixture;

/// What the owner's take of `request` answers with `plugin` the project's authority, as
/// `Saves::workflow_apply` composes it: the fetch, the board's admission, then the guard that
/// refused, read only for a refusal a guard gives.
fn taken(
    f: &Fixture,
    plugin: Option<&'static Approvals>,
    request: &OperationRequest,
) -> Result<(), (String, Option<Box<GuardRefusal>>)> {
    let owner = local_owner();
    let root = PathBuf::from("/project");
    let plugins = plugin.map_or_else(|| Plugins::new(Vec::new()), plugins);
    let fetched = fetched_within(&f.store, &plugins, &root, request, &owner, WITHIN)
        .map_err(|refused| (refused.failure.error, refused.guard))?;
    pushed(f, request, &fetched)
}

/// [`taken`] with the attestations `given`, pushed as a payload's are.
fn pushed(
    f: &Fixture,
    request: &OperationRequest,
    given: &[Signed],
) -> Result<(), (String, Option<Box<GuardRefusal>>)> {
    let owner = local_owner();
    (f.store
        .apply_attested(&owner, &Host::local(), request, given))
    .map(drop)
    .map_err(|refused| {
        let guard = guards(&refused)
            .then(|| f.store.guard_refusal(&owner, request, given).unwrap())
            .flatten()
            .map(Box::new);
        (refused.to_string(), guard)
    })
}

/// The ledger's report of `generation`, following generation `previous` of e1.
fn report(f: &Fixture, generation: u64, previous: u64) {
    let observation = Observation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation,
        state: format!("s{generation}"),
        previous: Some(AuthorityState {
            epoch: "e1".into(),
            generation: previous,
            state: format!("s{previous}"),
        }),
        at: 2.0,
    };
    let observed = signed(KEY, true, &serde_json::to_string(&observation).unwrap());
    f.store.observe_authority(&observed).unwrap();
}

fn guards_of(f: &Fixture) -> TaskGuards {
    f.store.workflow_guards("sim/a").unwrap()
}

#[test]
fn a_refused_take_names_the_guard_and_why_and_quotes_the_authority_only_once_verified() {
    let f = awaiting();
    let request = request(&f, "accept");
    let refused = |plugin| {
        let (said, guard) = taken(&f, plugin, &request).unwrap_err();
        assert!(said.starts_with("guard_unavailable: "), "{said}");
        let guard = guard.expect("the guard that refused");
        assert_eq!(
            (guard.guard.as_str(), guard.adapter.as_str()),
            ("approved", "approvals"),
            "{said}"
        );
        (guard.why, guard.record)
    };
    // This host asks no authority: the edge went with no attestation.
    assert_eq!(refused(None), (GuardWhy::Missing, None));
    assert_eq!(refused(Some(&CLOSED)), (GuardWhy::Unanswered, None));
    assert_eq!(refused(Some(&OTHER_KEY)), (GuardWhy::Unverified, None));
    let (why, record) = refused(Some(&OTHER_TASK));
    assert_eq!(why, GuardWhy::Stale);
    assert_eq!(record.unwrap().facts, vec!["decision-1".to_string()]);

    // The ledger's own refusal, its reasons as it gave them.
    let operation = super::operation("sim", "approved", &request, &local_owner());
    let mut refusing = pass(&operation);
    refusing.result = Attested::Refuse;
    refusing.facts.clear();
    refusing.reasons = vec![
        "/task/sim/b is still open".into(),
        "the release check failed https://ci.example/runs/7".into(),
    ];
    let given = signed(KEY, false, &serde_json::to_string(&refusing).unwrap());
    let (said, guard) = pushed(&f, &request, &[given]).unwrap_err();
    assert_eq!(
        said,
        "guard_unavailable: approvals refuses approved for sim/a: /task/sim/b is still open; \
         the release check failed https://ci.example/runs/7"
    );
    let guard = guard.unwrap();
    assert_eq!(guard.why, GuardWhy::Refused);
    let record = guard.record.unwrap();
    assert_eq!(record.reasons, refusing.reasons);
    assert_eq!(
        (record.epoch.as_str(), record.generation, record.result),
        ("e1", 1, Attested::Refuse)
    );
    assert_eq!(at(&f).state, "awaiting", "a refusal moves nothing");

    // The ledger decided again since its pass was issued: the pass is at an older generation.
    let stale = signed(
        KEY,
        false,
        &serde_json::to_string(&pass(&operation)).unwrap(),
    );
    report(&f, 2, 1);
    let (_, guard) = pushed(&f, &request, &[stale]).unwrap_err();
    assert_eq!(guard.unwrap().why, GuardWhy::DecidedAgain);
}

#[test]
fn a_refusal_no_guard_gives_reads_no_guard() {
    let f = awaiting();
    let mut moved = request(&f, "accept");
    moved.expected.task_revision += 1;
    let (said, guard) = pushed(&f, &moved, &[]).unwrap_err();
    assert!(said.starts_with("stale_task: "), "{said}");
    assert_eq!(guard, None);
    for (words, guard) in [
        ("guard_unavailable: edge accept is guarded", true),
        ("stale_input: the attestation binds", true),
        ("unauthorized: the attestation covers", true),
        ("stale_task: sim/a is at", false),
        ("guard_unavailable", false),
        ("guard_unavailable_too: x", false),
        ("", false),
    ] {
        let refused = symphony_board::Error::Refused(words.into());
        assert_eq!(guards(&refused), guard, "{words}");
    }
}

/// A refusal is a guard's by its code alone, read from the words as the board writes a refusal:
/// whatever those words say after it, another code's word included, and never from a board that
/// could not be reached, whatever its words.
#[test]
fn a_guards_refusal_is_known_by_its_code_whatever_its_words_say() {
    use symphony_api::workflow::RefusalCode::{self, *};
    // Every code, decided here: a code added to the board fails to compile until it is.
    let a_guards = |code: RefusalCode| match code {
        GuardUnavailable | StaleInput | Unauthorized => true,
        StaleTask
        | StaleWorkflow
        | StaleActivation
        | UnknownId
        | RemovedId
        | Dependency
        | Capacity
        | HumanWait
        | UnsupportedActivation
        | UnsupportedWriter
        | UnsupportedAdapter
        | ExplicitActionRequired => false,
    };
    let every = [
        StaleTask,
        StaleWorkflow,
        StaleActivation,
        StaleInput,
        UnknownId,
        RemovedId,
        Unauthorized,
        Dependency,
        Capacity,
        HumanWait,
        GuardUnavailable,
        UnsupportedActivation,
        UnsupportedWriter,
        UnsupportedAdapter,
        ExplicitActionRequired,
    ];
    for code in every {
        for words in [
            "",
            "edge accept is guarded",
            "unauthorized: words that begin with a guard's code",
            "stale_task: words that begin with another code",
        ] {
            let refused = symphony_board::Error::Refused(Refusal::new(code, words).to_string());
            assert_eq!(guards(&refused), a_guards(code), "{refused}");
        }
    }
    for words in [
        "guard_unavailable: edge accept is guarded",
        "unauthorized: x",
    ] {
        assert!(!guards(&symphony_board::Error::Db(words.into())), "{words}");
    }
}

#[test]
fn the_guards_read_holds_what_the_board_accepted_the_edges_it_guards_and_the_admission_met() {
    let f = awaiting();
    // Before the acceptance: the edge that leaves verify/awaiting is guarded, nothing let it in.
    let before = guards_of(&f);
    let [authority] = before.authorities.as_slice() else {
        panic!("{before:?}");
    };
    assert_eq!(
        (
            authority.adapter.as_str(),
            authority.issuer.as_str(),
            authority.epoch.as_deref(),
            authority.generation,
            authority.conflicted,
            authority.admitted,
            authority.asked_here,
        ),
        ("approvals", "ledger", Some("e1"), Some(1), false, 0, false)
    );
    let named: Vec<_> = (before.guards.iter())
        .map(|g| (g.guard.as_str(), g.adapter.as_str(), g.page.as_deref()))
        .collect();
    assert_eq!(named, [("approved", "approvals", None)]);
    let edges: Vec<_> = (before.edges.iter())
        .map(|e| (e.edge.as_str(), e.guards.clone()))
        .collect();
    assert_eq!(edges, [("accept", vec!["approved".to_string()])]);
    assert_eq!(before.entry, None);

    let request = request(&f, "accept");
    // The ledger's pass, pushed by hand: the passing stand-in is another test's to ask.
    let operation = super::operation("sim", "approved", &request, &local_owner());
    let passed = signed(
        KEY,
        false,
        &serde_json::to_string(&pass(&operation)).unwrap(),
    );
    pushed(&f, &request, &[passed]).unwrap();
    let after = guards_of(&f);
    assert_eq!(after.authorities[0].admitted, 1);
    assert!(
        after.edges.is_empty(),
        "no guarded edge leaves the accepted pair"
    );
    let entry = after.entry.expect("the admission that let it in");
    assert_eq!(
        (entry.edge.as_str(), entry.revision),
        ("accept", request.expected.task_revision + 1)
    );
    assert!(!entry.label.is_empty() && entry.at > 0.0);
    assert_eq!(entry.principal.subject, local_owner().subject);
    let [met] = entry.guards.as_slice() else {
        panic!("{:?}", entry.guards);
    };
    assert_eq!(
        (
            met.guard.as_str(),
            met.adapter.as_str(),
            met.issuer.as_str(),
            met.epoch.as_str(),
            met.generation,
            met.standing,
        ),
        (
            "approved",
            "approvals",
            "ledger",
            "e1",
            1,
            GuardStanding::Met
        )
    );
    let record = met.record.as_ref().expect("what its attestation says");
    assert_eq!(record.facts, vec!["decision-1".to_string()]);
    assert_eq!(record.result, Attested::Pass);

    // The ledger decides again: the guard no longer stands, and its record is still read.
    report(&f, 2, 1);
    let again = guards_of(&f);
    assert_eq!(again.authorities[0].generation, Some(2));
    let met = &again.entry.unwrap().guards[0];
    assert_eq!(
        (met.standing, met.generation),
        (GuardStanding::DecidedAgain, 1)
    );
    assert!(met.record.is_some());
}

#[test]
fn a_task_no_workflow_holds_or_no_guard_holds_reads_no_guards() {
    use crate::taskview::page_tests::store_fixture::Seed;
    let f = Fixture::new(Seed::default().session("amber").task("sim/b", "ready"));
    assert_eq!(
        f.store.workflow_guards("sim/b").unwrap(),
        TaskGuards::default()
    );
    let f = awaiting();
    // A slug the board has no transition for: the authorities and guards, no edge and no entry.
    let unknown = f.store.workflow_guards("sim/none").unwrap();
    assert_eq!(unknown.authorities.len(), 1);
    assert!(unknown.edges.is_empty() && unknown.entry.is_none());
}

#[test]
fn the_host_fills_whether_it_asks_each_authority_and_the_page_the_project_file_names() {
    let f = awaiting();
    let file = "[[workflow_authorities]]\nadapter = \"approvals\"\nissuer = \"ledger\"\n\
                contract = 1\nkey = \"ed25519:00\"\npage = \"/docs/LEDGER.md#{task}:{guard}\"\n";
    let filled = crate::guards::filled(guards_of(&f), &plugins(&PASSING), file, "sim/a");
    assert!(filled.authorities[0].asked_here);
    assert_eq!(
        filled.guards[0].page.as_deref(),
        Some("/docs/LEDGER.md#sim/a:approved")
    );
    // No plugin answers and the file names no page: the board's records as they are.
    let bare = crate::guards::filled(guards_of(&f), &Plugins::new(Vec::new()), "", "sim/a");
    assert_eq!(bare, guards_of(&f));
    // A project folder with no file and no plugin reads as that.
    let root = PathBuf::from("/nowhere/at/all");
    assert_eq!(crate::guards::of(&root, &f.store, "sim/a").unwrap(), bare);
}
