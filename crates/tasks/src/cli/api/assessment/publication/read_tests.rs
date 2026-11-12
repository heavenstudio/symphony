//! Where the landing assessments are read from (the project's `[board] assessments`), and the
//! evidence read under the protocols' own names and, during the bridge, their old ones.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const TIP: &str = "0123456789abcdef0123456789abcdef01234567";

/// A main checkout whose common dir is its `.git` folder, removed when dropped.
struct Checkout(PathBuf);

impl Checkout {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "symphony-assessments-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".git")).unwrap();
        Self(root)
    }

    fn common(&self) -> PathBuf {
        self.0.join(".git")
    }

    /// Its project file, naming `folder` for the assessments.
    fn naming(&self, folder: &str) -> &Self {
        fs::write(
            self.0.join(crate::project::FILE),
            format!("[board]\n{ASSESSMENTS} = \"{folder}\"\n"),
        )
        .unwrap();
        self
    }

    /// `value` written at `rel` under the assessments' folder.
    fn evidence(&self, rel: &str, value: &Value) {
        let path = home(&self.common()).unwrap().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_protocol_is_read_by_its_own_name_and_its_old_one_alone() {
    for name in [SUBMISSION, EVENT, BINDING] {
        let old = name.replacen("symphony-", "sf-", 1);
        assert!(protocol(name, name), "{name}");
        assert!(protocol(&old, name), "{old}");
    }
    for (written, name) in [
        ("sf-land-binding/1", SUBMISSION),
        ("sf-land-binding/2", BINDING),
        ("xsf-land-binding/1", BINDING),
        ("land-binding/1", BINDING),
        ("symphony-land-binding/1", "sf-land-binding/1"),
        ("", BINDING),
    ] {
        assert!(!protocol(written, name), "{written:?} as {name}");
    }
}

#[test]
fn the_assessments_are_where_the_main_checkouts_project_names_them_and_nowhere_unnamed() {
    let checkout = Checkout::new();
    let common = checkout.common();
    let refused = home(&common).unwrap_err();
    assert!(refused.contains("[board] assessments"), "{refused}");
    assert_eq!(
        home(&checkout.naming(".git/land/assessments").common()).unwrap(),
        common.join("land/assessments")
    );
    assert_eq!(
        home(&checkout.naming("evidence").common()).unwrap(),
        checkout.0.join("evidence")
    );
    // A common dir that is no main checkout's `.git` has no project to name them.
    assert!(home(&checkout.0.join("elsewhere")).is_err());
    // An unnamed folder refuses every read before looking for evidence.
    fs::remove_file(checkout.0.join(crate::project::FILE)).unwrap();
    assert!(
        read_source(&common, "occ-1")
            .err()
            .unwrap()
            .contains("[board] assessments")
    );
    assert!(
        read_event(&common, "occ-1")
            .err()
            .unwrap()
            .contains("[board] assessments")
    );
}

fn submission(protocol: &str, id: &str) -> Value {
    json!({
        "protocol": protocol, "id": id, "branch": "tools/x", "tip": TIP, "verification": null,
        "actor": "amber-heron", "outcome": {"kind": "assessment"}, "target": "board",
    })
}

#[test]
fn a_submission_is_read_under_either_name_of_its_protocol_and_no_other() {
    let checkout = Checkout::new();
    checkout.naming(".git/land/assessments");
    let common = checkout.common();
    for (id, written) in [
        ("occ-new", SUBMISSION),
        ("occ-old", "sf-land-submission/1"),
    ] {
        checkout.evidence(
            &format!("submissions/{id}/submission.json"),
            &submission(written, id),
        );
        let source = read_source(&common, id).unwrap();
        assert_eq!(
            (
                source.publication.id.as_str(),
                source.publication.branch.as_str(),
                source.actor.as_str(),
                source.kind.as_str(),
                source.target.as_str()
            ),
            (id, "tools/x", "amber-heron", "assessment", "board"),
            "{written}"
        );
    }
    for (id, written) in [("occ-event", EVENT), ("occ-unknown", "sf-land-submission/2")] {
        checkout.evidence(
            &format!("submissions/{id}/submission.json"),
            &submission(written, id),
        );
        assert_eq!(
            read_source(&common, id).err().as_deref(),
            Some("assessment submission is incompatible"),
            "{written}"
        );
    }
}

#[test]
fn an_event_is_read_under_either_name_of_its_protocol_and_no_other() {
    let checkout = Checkout::new();
    checkout.naming("evidence");
    let common = checkout.common();
    let decision = json!({
        "branch": "tools/x", "task": "tools/x", "tip": TIP, "actor": "amber-heron",
        "record": format!("{TIP}:"), "changes_requested": false, "verification": null,
    });
    for (id, written) in [
        ("occ-new", EVENT),
        ("occ-old", "sf-land-assessment/1"),
        ("occ-submission", SUBMISSION),
    ] {
        checkout.evidence(
            &format!("events/{id}/decision.json"),
            &json!({"protocol": written, "id": id, "publication": "occ-source", "decision": decision}),
        );
        let read = read_event(&common, id);
        if written == SUBMISSION {
            assert_eq!(
                read.err().as_deref(),
                Some("assessment event journal is incompatible")
            );
        } else {
            let event = read.unwrap();
            assert_eq!(event.publication, "occ-source", "{written}");
            assert_eq!(event.decision.tip, TIP, "{written}");
        }
    }
}

#[test]
fn new_bindings_are_written_under_the_symphonys_own_protocol() {
    let checkout = Checkout::new();
    checkout.naming(".git/land/assessments");
    checkout.evidence(
        "submissions/occ-1/submission.json",
        &submission(SUBMISSION, "occ-1"),
    );
    let source = read_source(&checkout.common(), "occ-1").unwrap();
    let bindings = Bindings::new(&source);
    assert_eq!(bindings.protocol, BINDING);
    assert_eq!(bindings.value().unwrap()["protocol"], BINDING);
}
