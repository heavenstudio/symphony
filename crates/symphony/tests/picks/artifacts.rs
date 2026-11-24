//! A full core/board adapter round trip on a scratch board, with a local private byte store.

use super::board::{Fixture, Seed};
use super::common::Tree;
use std::collections::BTreeSet;
use symphony::artifacts::{
    Binding, BoardMetadata, Core, Error, Local, Metadata, Producer, Publication, Revision, Scope,
    Status,
};

#[test]
fn durable_artifact_metadata_round_trip() {
    let fixture = Fixture::new(Seed::default());
    let tree = Tree::new();
    let provider = Local::for_project(&tree.root);
    let scope = Scope::new("tenant", "project").unwrap();
    let metadata = BoardMetadata(&fixture.store);
    let core = Core {
        metadata: &metadata,
        provider: &provider,
    };
    let binding = Binding {
        source: Revision::new("repository", &"a".repeat(40)).unwrap(),
        producer: Producer::Human {
            actor: "owner".into(),
        },
        operation: "upload".into(),
    };
    let permit = core.fence(&scope, "owner", 0, Some(binding)).unwrap();
    let id = core
        .begin(&scope, "retry", &permit, "evidence.txt", b"evidence")
        .unwrap();
    assert_eq!(core.status(&scope, &id), Ok(Status::Pending));
    let reopened = fixture.other();
    let metadata = BoardMetadata(&reopened);
    let core = Core {
        metadata: &metadata,
        provider: &provider,
    };
    assert_eq!(
        core.begin(&scope, "retry", &permit, "evidence.txt", b"evidence")
            .unwrap(),
        id
    );
    let sealed = core.upload(&scope, &id, b"evidence").unwrap();
    let publication = Publication {
        owner: "task:one".into(),
        permit: permit.clone(),
        artifacts: BTreeSet::from([id.clone()]),
        delivered: Some(Revision::new("repository", &"b".repeat(40)).unwrap()),
    };
    core.publish(&scope, "result", &publication).unwrap();
    core.retain_export(&scope, "backup", &publication.artifacts)
        .unwrap();
    core.reference_inventory(&scope, true).unwrap();
    let reopened = fixture.other();
    let provider = Local::for_project(&tree.root);
    let metadata = BoardMetadata(&reopened);
    let core = Core {
        metadata: &metadata,
        provider: &provider,
    };
    assert_eq!(core.reconcile(&scope, &id).unwrap(), sealed);
    assert_eq!(core.read(&scope, &id, None), Ok(b"evidence".to_vec()));
    core.publish(&scope, "result", &publication).unwrap();
    assert_eq!(core.reserve_delete(&scope, &id), Err(Error::Protected));
    core.release_owner(&scope, "task:one").unwrap();
    assert_eq!(core.reserve_delete(&scope, &id), Err(Error::Protected));
    core.release_export(&scope, "backup").unwrap();
    core.fence(&scope, &permit.owner, permit.revision, None)
        .unwrap();
    assert_eq!(
        core.publish(&scope, "stale-result", &publication),
        Err(Error::Stale)
    );
    core.reserve_delete(&scope, &id).unwrap();
    core.delete_reserved(&scope, &id).unwrap();
    assert_eq!(core.status(&scope, &id), Ok(Status::Deleted));
    let raw = fixture.store.raw();
    let row = raw.row("artifact_ledger", serde_json::json!(["tenant", "project"]));
    row.set(serde_json::json!({"revision": -1})).unwrap();
    assert!(matches!(metadata.load(&scope), Err(Error::Metadata)));
    row.set(serde_json::json!({"body": {"encoded": "not JSON"}}))
        .unwrap();
    assert!(matches!(metadata.load(&scope), Err(Error::Metadata)));
}
