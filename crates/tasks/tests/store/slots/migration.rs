//! The index-only rollout from the committed schema 13, with its existing board data intact.

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

#[test]
fn schema_13_upgrade_preserves_board_data_and_indexes_recorded_branch_lookup() {
    assert_eq!(
        Schema::this_build().version(),
        14,
        "this is the index-only 13 to 14 rollout"
    );
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

    // Reproduce the committed 13 shape: these are the only additive definitions in 14.
    f.store
        .client()
        .write(
            "BEGIN TRANSACTION;
         REMOVE INDEX task_session_slug ON TABLE task_session;
         REMOVE INDEX task_session_branch ON TABLE task_session;
         UPDATE meta:schema SET version = 13;
         COMMIT TRANSACTION;",
            json!({}),
        )
        .unwrap();
    let old = Store::opening(
        f.other_client(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::write().at(Schema::this_build().before()),
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
    assert_eq!(current["version"], 14);
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
        refused.contains("schema 14") && refused.contains("schema, 13"),
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
