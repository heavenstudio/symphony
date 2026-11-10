//! `tasks api`'s red marks: only a red's filer marks a task red, and only a red series' own task.

use super::answer;
use crate::Case;
use serde_json::json;

/// `create`'s `red` (api 7) marks the task it files red when a red's filer files it, as land files
/// a red of main, and `mark_red` marks one it filed before; a filer is a session `register` takes
/// of kind `filer`, which the command line's `register` does not. Any other session's mark is
/// refused, as is a filer's on a task of no red series, and a call without one marks nothing.
#[test]
fn only_a_reds_filer_marks_a_task_red_through_the_api() {
    let c = Case::new();
    let worker = c.register("worker");
    let filer = answer(
        &c.env,
        json!({"method": "register", "args": {"kind": "filer"}}),
    )["ok"]["handle"]
        .as_str()
        .unwrap()
        .to_string();
    let unfiled = format!(
        "only a red's filer marks a task red, a session of kind filer as the tools that file reds register; '{worker}' is not one"
    );
    let made = answer(
        &c.env,
        json!([
            {"method": "create", "args": {"slug": "tests/main-red-abc", "title": "Main red", "by": filer, "priority": 0, "red": "main"}},
            {"method": "create", "args": {"slug": "work/escape", "title": "Escape", "by": filer, "red": "main"}},
            {"method": "create", "args": {"slug": "sim/a", "title": "A", "by": worker, "red": null}},
            {"method": "create", "args": {"slug": "tests/main-red-b", "title": "B", "by": filer, "red": ""}},
            {"method": "create", "args": {"slug": "tests/nightly-red-old", "title": "Old", "by": filer, "priority": 1}},
            {"method": "mark_red", "args": {"slug": "tests/nightly-red-old", "red": "nightly", "by": worker}},
            {"method": "mark_red", "args": {"slug": "tests/nightly-red-old", "red": "nightly", "by": filer}},
            {"method": "mark_red", "args": {"slug": "sim/a", "red": "main", "by": filer}},
        ]),
    );
    let unseries = |slug: &str| json!({"refused": format!("{slug} is no red series' own task: a red is marked only on a task of the series reds are filed in, tests/main-red-* and tests/nightly-red-*, never on one named under it")});
    assert_eq!(made[0]["ok"]["priority"], json!(0));
    assert_eq!(made[1], unseries("work/escape"));
    assert_eq!(made[2]["ok"]["slug"], json!("sim/a"));
    assert_eq!(
        made[3],
        json!({"refused": "a red task names what is red, such as main"})
    );
    assert_eq!(made[5], json!({"refused": unfiled}));
    assert_eq!(made[6]["ok"]["priority"], json!(0));
    assert_eq!(made[7], unseries("sim/a"));
    let store = c.store();
    let red = |slug: &str| store.get(slug).map(|t| t.red()).ok();
    assert_eq!(
        [
            "tests/main-red-abc",
            "work/escape",
            "sim/a",
            "tests/nightly-red-old"
        ]
        .map(red),
        [Some(true), None, Some(false), Some(true)]
    );
    let ran = c.run_as("", &["register", "--kind", "filer"], "");
    assert_ne!(ran.code, 0, "{}", ran.out);
    assert!(ran.err.contains("invalid choice: 'filer'"), "{}", ran.err);
}
