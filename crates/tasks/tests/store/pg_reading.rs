//! Normal reads preserve JSON nulls inside task fields on either database, while absent columns
//! remain absent. These reads share PostgreSQL's row projection with other board views.

use crate::common::{Fixture, Seed};
use serde_json::json;

#[test]
fn nested_nulls_survive_normal_task_reads_and_the_snapshot_after_a_write() {
    let f = Fixture::new(Seed::default().session("amber-heron"));
    f.task("tools/nulls");
    let fields = json!({"plugin": {
        "empty": null,
        "nested": {"missing": null, "present": 1, "empty": {}},
        "array": [null, {"missing": null}, [], {}]
    }});
    f.store
        .raw()
        .row("task", "tools/nulls")
        .set(json!({"fields": fields}))
        .unwrap();

    let check = |task: symphony_board::Task| {
        assert_eq!(serde_json::to_value(&task.fields).unwrap(), fields);
        assert_eq!(task.parent, None);
    };
    check(f.store.get("tools/nulls").unwrap());
    check(
        f.store
            .board()
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.slug == "tools/nulls")
            .unwrap(),
    );
    check(
        f.store
            .clock_board()
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.slug == "tools/nulls")
            .unwrap(),
    );
    check(
        f.store
            .set_priority("tools/nulls", 1, "amber-heron")
            .unwrap(),
    );
    check(f.store.get("tools/nulls").unwrap());
}
