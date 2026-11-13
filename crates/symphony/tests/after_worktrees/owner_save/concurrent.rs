//! Separate Saves workers exercise shared identity locking and the atomic board receipt.

use serde_json::{Value, json};
use std::path::Path;

/// `payload` saved by a worker of its own, posted again while its save is uncertain.
fn post(root: &Path, payload: &Value) -> Result<Value, symphony::save::Failure> {
    let saves = symphony::save::Saves::start(root);
    let result = test_time::until("a concurrent save settled", super::LIMIT, || {
        let result = saves.post("", "/api/quick-task", payload);
        let uncertain = result.as_ref().is_ok_and(|value| value["pending"] == true)
            || result.as_ref().is_err_and(|error| error.status == 503);
        (!uncertain).then_some(result)
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
    saves.close();
    result
}

pub(super) fn saves(root: &Path) {
    struct Case {
        name: &'static str,
        second_id: &'static str,
        second_body: &'static str,
    }
    for Case {
        name: case,
        second_id,
        second_body,
    } in [
        Case {
            name: "same-id",
            second_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            second_body: "Same body",
        },
        Case {
            name: "same-text",
            second_id: "cccccccccccccccccccccccccccccccc",
            second_body: "Same body",
        },
        Case {
            name: "other-text",
            second_id: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            second_body: "Other body",
        },
    ] {
        let first_id = match case {
            "same-id" => "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "same-text" => "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            _ => "dddddddddddddddddddddddddddddddd",
        };
        let slug = format!("inbox/{case}");
        let first = json!({"request":first_id,"slug":slug,"title":"Same title","body":"Same body"});
        let second =
            json!({"request":second_id,"slug":slug,"title":"Same title","body":second_body});
        let results = std::thread::scope(|scope| {
            let first = scope.spawn(|| post(root, &first));
            let second = scope.spawn(|| post(root, &second));
            [first.join().unwrap(), second.join().unwrap()]
        });
        let confirmed = results
            .iter()
            .filter(|result| result.as_ref().is_ok_and(|value| value["slug"] == slug))
            .count();
        assert_eq!(confirmed, if case == "same-id" { 2 } else { 1 }, "{case}");
        for result in results {
            if let Err(error) = result {
                assert_eq!(error.status, 400, "{case}: {}", error.error);
            }
        }
        let repeated = post(root, &first);
        if case == "same-id" {
            assert_eq!(repeated.ok(), Some(json!({"slug":slug})));
        }
    }
}
