//! The actual JSON API caller reaches the guarded occurrence transaction, including retries
//! after a later write, state races, argument/actor protection and simultaneous delivery.

use super::answer;
use crate::Case;
use serde_json::{Value, json};

const SLUG: &str = "tools/assessment";
const RECORD: &str = "aaa: changes";

fn setup() -> (Case, String) {
    let mut case = Case::new();
    // An orchestrator that stays, which the board gives the task: past its first live line and
    // waiting. With none, the task would wait for one, marked in its board fields.
    let orch = case.register_settled("orchestrator");
    case.ok(&orch, &["wait", "--timeout", "0"]);
    let actor = case.register("worker");
    let made = answer(
        &case.env,
        json!({"method": "create", "args": {
            "slug": SLUG, "title": "Assessment", "by": actor,
        }}),
    );
    assert_eq!(made["ok"]["slug"], SLUG, "{made}");
    assert_eq!(made["ok"]["roles"][0]["handle"], json!(orch), "{made}");
    (case, actor)
}

fn state(case: &Case) -> Value {
    let answer = answer(
        &case.env,
        json!({"method": "assessment_state", "args": {"slug": SLUG}}),
    );
    assert_eq!(answer["ok"]["protocol"], "assessment/1", "{answer}");
    let mut state = answer["ok"].clone();
    state.as_object_mut().unwrap().remove("protocol");
    state
}

fn request(actor: &str, id: &str, expected: &Value) -> Value {
    json!({"method": "record_waits", "args": {
        "protocol": "assessment/1", "slug": SLUG, "record": RECORD,
        "assessment": id, "actor": actor, "expected": expected,
    }})
}

fn waits(case: &Case) -> Vec<Value> {
    let thread = answer(
        &case.env,
        json!({"method": "thread", "args": {"slug": SLUG}}),
    );
    thread["ok"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|line| {
            line["text"]
                .as_str()
                .unwrap()
                .contains(" set _board.waits to ")
        })
        .cloned()
        .collect()
}

#[test]
fn the_api_confirms_original_receipts_and_distinguishes_fresh_identical_assessments_at_one_time() {
    let (case, actor) = setup();
    let expected = state(&case);
    let first = answer(&case.env, request(&actor, "first", &expected))["ok"].clone();
    assert_eq!(first["protocol"], "assessment/1");
    assert_eq!(first["slug"], SLUG);
    assert_eq!(first["id"], "first");
    assert_eq!(first["record"], RECORD);
    assert_eq!(first["current"], true);
    assert!(first["sid"].as_i64().unwrap() > 0);
    assert_eq!(first["at"], crate::common::NOW);
    assert_eq!(
        answer(&case.env, request(&actor, "first", &expected))["ok"],
        first
    );
    let after_first = state(&case);
    assert_eq!(
        after_first["version"].as_i64().unwrap(),
        expected["version"].as_i64().unwrap() + 1
    );
    let second = answer(&case.env, request(&actor, "second", &after_first))["ok"].clone();
    assert_ne!(first["sid"], second["sid"]);
    assert_eq!(first["at"], second["at"]);
    let after_second = state(&case);
    assert_eq!(
        answer(
            &case.env,
            json!({"method": "leave", "args": {"handle": actor}})
        ),
        json!({"ok": null})
    );
    let retry = answer(&case.env, request(&actor, "first", &expected))["ok"].clone();
    let mut original = first.clone();
    original["current"] = json!(false);
    assert_eq!(retry, original);
    assert_eq!(state(&case), after_second);
    let lines = waits(&case);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["id"], first["sid"]);
    assert_eq!(lines[1]["id"], second["sid"]);
    let task = case.store().get(SLUG).unwrap();
    assert_eq!(task.fields["_board"]["assessed"]["id"], "second");
    assert_eq!(
        task.fields["_board"]["assessments"]["first"][RECORD]["sid"],
        first["sid"]
    );
}

#[test]
fn stale_state_untrusted_actors_and_extra_fields_are_refused_through_the_actual_api() {
    let (case, actor) = setup();
    let expected = state(&case);
    let claimed = answer(
        &case.env,
        json!({"method": "claim", "args": {"slug": SLUG, "handle": actor}}),
    );
    assert_eq!(claimed["ok"]["status"], "in_progress", "{claimed}");
    let refused = answer(&case.env, request(&actor, "stale", &expected));
    assert!(
        refused["refused"]
            .as_str()
            .unwrap()
            .contains("changed since its assessment")
    );
    let current = state(&case);
    for bad_actor in ["owner", "absent"] {
        let denied = answer(&case.env, request(bad_actor, "actor", &current));
        assert!(denied["refused"].is_string(), "{bad_actor}: {denied}");
    }
    let mut extra = request(&actor, "extra", &current);
    extra["args"]["fields"] = json!({"_board": {"waits": "forged"}});
    assert!(
        answer(&case.env, extra)["error"]
            .as_str()
            .unwrap()
            .contains("fields")
    );
    let mut invalid = request(&actor, "nested", &current);
    invalid["args"]["expected"]["fields"] = json!({"_board": {"waits": "forged"}});
    assert!(
        answer(&case.env, invalid)["error"]
            .as_str()
            .unwrap()
            .contains("fields")
    );
    let mut unreachable = case.env.clone();
    unreachable
        .vars
        .insert("TASKS_SERVER".into(), crate::UNREACHED_SERVER.into());
    let mut unknown = request(&actor, "protocol", &current);
    unknown["args"]["protocol"] = json!("assessment/99");
    let error = answer(&unreachable, unknown);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("unsupported assessment protocol"),
        "{error}"
    );
    assert_eq!(state(&case), current);
    assert!(waits(&case).is_empty());
    assert!(case.store().get(SLUG).unwrap().fields.is_empty());
}

#[test]
fn simultaneous_api_deliveries_share_a_receipt_and_the_task_version_advances_only_once() {
    let (case, actor) = setup();
    let expected = state(&case);
    let start = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let results: Vec<_> = (0..4)
            .map(|_| {
                let (env, actor, expected, start) = (&case.env, &actor, &expected, &start);
                scope.spawn(move || {
                    start.wait();
                    answer(env, request(actor, "one-delivery", expected))
                })
            })
            .collect();
        let receipts: Vec<_> = results
            .into_iter()
            .map(|result| {
                let result = result.join().unwrap();
                assert!(result["ok"].is_object(), "{result}");
                result["ok"].clone()
            })
            .collect();
        assert_eq!(receipts, vec![receipts[0].clone(); 4]);
    });
    assert_eq!(waits(&case).len(), 1);
    assert_eq!(
        state(&case)["version"].as_i64().unwrap(),
        expected["version"].as_i64().unwrap() + 1
    );
}
