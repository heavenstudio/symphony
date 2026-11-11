//! Real correlated-batch plans and fixed-result scaling on owned scratch boards.

use super::*;
use crate::rows::MessageRow;
use crate::store_fixture::{NOW, Seed};
use serde_json::Value;
use std::time::Instant;

fn fixture(count: i64) -> Fixture {
    let mut seed = Seed::default().task("work", "ready");
    seed.dump.messages = (1..=count)
        .map(|id| MessageRow {
            id,
            slug: "work".into(),
            sender: if id > count - 4 { "recipient" } else { "other" }.into(),
            kind: "msg".into(),
            text: "reply".into(),
            ts: NOW,
            ..MessageRow::default()
        })
        .collect();
    Fixture::new(seed)
}

fn indexed_plans(f: &Fixture, posts: &[(&str, i64)]) {
    // EXPLAIN is the legacy suffix form inside the actual post/kind scopes. Only the SELECT's
    // result-processing expression is replaced so its plan rows are not treated as answers.
    let plan_read = format!(
        "['msg', 'verdict', 'system'].map(|$kind| ({} EXPLAIN))",
        answers::select()
    );
    let sql = answers::batch().replace(&answers::read(), &plan_read);
    assert_ne!(sql, answers::batch(), "the actual query was instrumented");
    let result = f
        .store
        .client()
        .query(&sql, json!({"posts":posts}))
        .unwrap();
    let planned = result[0].as_array().unwrap();
    assert_eq!(planned.len(), posts.len());
    for (kinds, (handle, cursor)) in planned.iter().zip(posts) {
        let kinds = kinds.as_array().unwrap();
        assert_eq!(kinds.len(), 3);
        for (plan, kind) in kinds.iter().zip(["msg", "verdict", "system"]) {
            let index = &plan[0];
            assert_eq!(index["operation"], "Iterate Index", "{plan}");
            assert_eq!(index["detail"]["plan"]["index"], "message_sender");
            assert_eq!(index["detail"]["plan"]["prefix"], json!([handle, kind]));
            assert_eq!(
                index["detail"]["plan"]["ranges"],
                json!([{"operator":">", "value":cursor}])
            );
            let operations: Vec<_> = plan
                .as_array()
                .unwrap()
                .iter()
                .map(|step| step["operation"].as_str().unwrap())
                .collect();
            assert!(!operations.contains(&"Iterate Table"), "{plan}");
            assert!(!operations.contains(&"Fallback"), "{plan}");
        }
    }
}

#[test]
fn correlated_batches_keep_sender_kind_and_cursor_bounds_as_unrelated_rows_scale() {
    for count in [64, 1024, 8192] {
        let f = fixture(count);
        let cursors = [
            ("recipient", count / 2),
            ("recipient", count - 2),
            ("absent", 0),
        ];
        indexed_plans(&f, &cursors);
        let selected = f
            .store
            .client()
            .query(&answers::batch(), json!({"posts":cursors}))
            .unwrap();
        assert_eq!(
            selected[0],
            json!([
                [
                    (count - 3, "work"),
                    (count - 2, "work"),
                    (count - 1, "work"),
                    (count, "work")
                ],
                [(count - 1, "work"), (count, "work")],
                []
            ])
        );
        for posts in [1, 8, 32] {
            let vars = json!({"posts":vec![("recipient", count / 2);posts]});
            let sql = answers::batch();
            let began = Instant::now();
            let result = f.store.client().query(&sql, vars).unwrap();
            let elapsed = began.elapsed();
            let selected = result[0].as_array().unwrap();
            assert_eq!(selected.len(), posts);
            let expected: Vec<Value> = ((count / 2 + 1)..=count)
                .filter(|id| *id > count - 4)
                .map(|id| json!([id, "work"]))
                .collect();
            for rows in selected {
                assert_eq!(rows, &json!(expected));
            }
            eprintln!(
                "unanswered batch: rows={count}, posts={posts}, fixed answers=4, elapsed={elapsed:?}"
            );
        }
    }
}
