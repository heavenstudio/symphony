//! Branch lookup indexes and session revisions migrate schema 13 with its board data intact.

// On SurrealDB alone: SurrealDB's schema 13 and its migration are its subject.

use super::{K, O, sessions, with};
use crate::common::{Fixture, HOST, NOW, RawWrite, refused};
use serde_json::{Value, json};
use symphony_board::{NewLaunch, NewTask, Opening, SLOT_READS, Schema, Store};

fn definition(store: &Store) -> Value {
    store.client().query(
        "RETURN { version: (SELECT VALUE version FROM ONLY meta:schema), indexes: object::keys((INFO FOR TABLE task_session).indexes) };",
        json!({}),
    ).unwrap().pop().unwrap()
}

/// A schema 13 writer: this build's steps, as far as the rule goes, at version 13.
fn schema_13() -> Schema {
    let mut schema = Schema::this_build();
    while schema.version() > 13 {
        schema = schema.before();
    }
    schema
}

#[test]
fn schema_13_upgrade_preserves_board_data_and_indexes_recorded_branch_lookup() {
    assert_eq!(schema_13().version(), 13);
    let slug = "legacy/feature";
    let branch = "legacy-branch";
    let mirror = "review/legacy-branch";
    let seed = with(sessions(), slug, "review", None, Some(O));
    let seed = with(seed, mirror, "review", None, None).role(slug, K, "executor");
    let f = Fixture::new(seed);
    f.store
        .record_launch(&NewLaunch {
            branch: branch.into(),
            claude_session: "retained-conversation".into(),
            worktree: "/legacy/checkout".into(),
            pane: "t13".into(),
            ..NewLaunch::new(slug, K, "executor")
        })
        .unwrap();
    let original = f.store.dump().unwrap();
    assert!(!original.tasks.is_empty() && !original.roles.is_empty());
    assert_eq!(original.task_sessions.len(), 1);

    let marker = f
        .store
        .client()
        .query(
            &format!("RETURN {};", symphony_board::schema::BOARD_MARKER),
            json!({}),
        )
        .unwrap()
        .remove(0);
    // Reproduce schema 13, before the branch indexes and per-session revision event.
    f.store
        .client()
        .write(
            "BEGIN TRANSACTION;
         REMOVE INDEX task_session_slug ON TABLE task_session;
         REMOVE INDEX task_session_branch ON TABLE task_session;
         REMOVE TABLE session_revision;
         UPDATE counter:board SET last = $marker;
         DELETE counter:board_structure;
         DEFINE EVENT OVERWRITE board_changed ON session WHEN $before != $after THEN {
             IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; }
             ELSE { CREATE counter:board SET last = 1 RETURN NONE; };
         };
         UPDATE meta:schema SET version = 13;
         DELETE meta:additions;
         COMMIT TRANSACTION;",
            json!({"marker":marker}),
        )
        .unwrap();
    let old = Store::opening(
        f.other_client(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::write().at(schema_13()),
    )
    .unwrap();
    assert_eq!(
        old.client().requests(),
        1,
        "the schema 13 writer makes no migration"
    );
    assert_eq!(definition(&old), json!({"version": 13, "indexes": []}));
    assert_eq!(old.dump().unwrap(), original);

    let upgraded = f.other();
    let current = definition(&upgraded);
    assert_eq!(current["version"], Schema::this_build().version());
    for index in ["task_session_slug", "task_session_branch"] {
        assert!(
            current["indexes"]
                .as_array()
                .unwrap()
                .contains(&json!(index)),
            "{current}"
        );
    }
    assert_eq!(
        upgraded.dump().unwrap(),
        original,
        "migration preserves every copied row"
    );
    for (statement, index, wanted) in [
        (SLOT_READS[7], "task_session_slug", slug),
        (SLOT_READS[8], "task_session_branch", branch),
    ] {
        let answer = upgraded
            .client()
            .query(
                &format!("EXPLAIN {statement};"),
                json!({"slot_wanted": [wanted]}),
            )
            .unwrap();
        let plan = answer.last().unwrap().as_str().unwrap();
        assert!(
            plan.contains(index) && !plan.contains("TableScan"),
            "{plan}"
        );
    }
    let paired = upgraded.tasks_for_branch(branch).unwrap();
    assert_eq!(
        paired
            .iter()
            .map(|task| task.slug.as_str())
            .collect::<Vec<_>>(),
        [slug, mirror]
    );
    assert_eq!(paired[0].branch.as_deref(), Some(branch));
    assert_eq!(paired[0].holders("orchestrator"), [O]);
    assert_eq!(paired[0].holders("executor"), [K]);
    let refused = refused(old.create(&NewTask::new("stale/writer", "Must not write", "owner")));
    assert!(
        refused.contains(&format!("schema {}", Schema::this_build().version()))
            && refused.contains("schema, 13"),
        "{refused}"
    );
    assert_eq!(
        upgraded.dump().unwrap(),
        original,
        "the old writer changed nothing"
    );
    let warm = f.other();
    assert_eq!(warm.client().requests(), 1, "the migration is not repeated");
}
