//! An approval authority's seam: declared by its adapter, asked on the wire as
//! `authorities.attest` since 0.4 with the operation, the project's root and its time, and
//! answering what it signed, its error, or a refusal naming its panic or an adapter it lacks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::Plugin;
use crate::execution::PrincipalKind;
use crate::protocol::{Ask, Attest, Declaration, Request};
use crate::testing::Host;

/// What the ledger authority was last asked: the root, the operation and its time.
static ASKED: Mutex<Option<(PathBuf, Operation, Duration)>> = Mutex::new(None);

/// Signs a statement naming the guard it was asked of.
struct Ledger;
/// Refuses to say, naming why.
struct Closed;
/// Panics when asked.
struct Broken;

impl Authority for Ledger {
    fn adapter(&self) -> &str {
        "approvals"
    }
    fn attest(
        &self,
        root: &Path,
        operation: &Operation,
        within: Duration,
    ) -> Result<Signed, String> {
        *ASKED.lock().unwrap() = Some((root.to_path_buf(), operation.clone(), within));
        Ok(Signed::new(
            format!("{{\"guard\":\"{}\"}}", operation.guard),
            "ab".repeat(64),
        ))
    }
}

impl Authority for Closed {
    fn adapter(&self) -> &str {
        "audits"
    }
    fn attest(&self, _: &Path, _: &Operation, _: Duration) -> Result<Signed, String> {
        Err("the ledger is closed for the night".into())
    }
}

impl Authority for Broken {
    fn adapter(&self) -> &str {
        "broken"
    }
    fn attest(&self, _: &Path, _: &Operation, _: Duration) -> Result<Signed, String> {
        panic!("the ledger's file is gone")
    }
}

struct Authorities;

impl Plugin for Authorities {
    fn name(&self) -> &str {
        "authorities"
    }
    fn authorities(&self) -> Vec<&dyn Authority> {
        vec![&Ledger, &Closed, &Broken]
    }
}

struct Nothing;

impl Plugin for Nothing {
    fn name(&self) -> &str {
        "nothing"
    }
}

fn operation(guard: &str) -> Operation {
    Operation::new(
        "sim".into(),
        "sim/a".into(),
        "accept".into(),
        guard.into(),
        Snapshot::new("d1".into(), 2, 3, "verify".into(), "awaiting".into()),
        BTreeMap::from([("design".to_string(), "r1".to_string())]),
        Principal::new(PrincipalKind::Human, "local".into(), "owner".into()),
    )
}

fn attest(adapter: &str, within_ms: u64) -> Attest {
    Attest::new(
        adapter.into(),
        PathBuf::from("/project"),
        operation("approved"),
        within_ms,
    )
}

#[test]
fn a_plugin_declares_each_authority_by_its_adapter_in_its_order_and_none_by_default() {
    let declared = Declaration::of(&Authorities);
    assert_eq!(declared.authorities, ["approvals", "audits", "broken"]);
    assert!(Declaration::of(&Nothing).authorities.is_empty());
    assert!(Nothing.authorities().is_empty());
}

#[test]
fn a_declaration_of_0_3_names_no_authorities_and_reads_as_none() {
    let mut written = serde_json::to_value(Declaration::of(&Authorities)).unwrap();
    written.as_object_mut().unwrap().remove("authorities");
    let read: Declaration = serde_json::from_value(written).unwrap();
    assert!(read.authorities.is_empty());
}

#[test]
fn attest_is_0_4s_call_and_goes_on_the_wire_as_its_seam_and_call() {
    assert_eq!((Attest::TAG, Attest::SINCE), ("authorities.attest", 4));
    let request: Request = attest("approvals", 2500).into();
    let written = serde_json::to_value(&request).unwrap();
    let fields = &written["authorities.attest"];
    assert_eq!(fields["authority"], "approvals");
    assert_eq!(fields["within_ms"], 2500);
    assert_eq!(fields["operation"]["snapshot"]["task_revision"], 3);
    assert_eq!(fields["operation"]["principal"]["kind"], "Human");
    let read: Request = serde_json::from_value(written).unwrap();
    assert_eq!(read, request);
}

#[test]
fn the_authority_of_the_adapter_named_is_asked_the_operation_root_and_time_and_answers_its_word() {
    let signed = Host::new(&Authorities).ask(attest("approvals", 2500));
    assert_eq!(
        signed,
        Ok(Ok(Signed::new(
            "{\"guard\":\"approved\"}".into(),
            "ab".repeat(64)
        )))
    );
    let asked = ASKED.lock().unwrap().clone();
    assert_eq!(
        asked,
        Some((
            PathBuf::from("/project"),
            operation("approved"),
            Duration::from_millis(2500)
        ))
    );
}

#[test]
fn an_authority_that_cannot_say_answers_why_as_its_error() {
    assert_eq!(
        Host::new(&Authorities).ask(attest("audits", 1000)),
        Ok(Err("the ledger is closed for the night".into()))
    );
}

#[test]
fn a_panic_or_an_adapter_the_plugin_lacks_is_refused_naming_it() {
    let host = Host::new(&Authorities);
    let panicked = host.ask(attest("broken", 1000)).unwrap_err();
    assert!(
        panicked.contains("authorities panicked: the ledger's file is gone"),
        "{panicked}"
    );
    let missing = host.ask(attest("ledgers", 1000)).unwrap_err();
    assert!(
        missing.contains("the plugin has no authority \"ledgers\""),
        "{missing}"
    );
    let none = Host::new(&Nothing)
        .ask(attest("approvals", 1000))
        .unwrap_err();
    assert!(
        none.contains("the plugin has no authority \"approvals\""),
        "{none}"
    );
}
