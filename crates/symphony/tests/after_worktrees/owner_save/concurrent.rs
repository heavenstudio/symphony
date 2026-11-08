//! Separate Saves workers exercise shared identity locking and the atomic board receipt.

use serde_json::{Value, json};
use std::path::Path;

fn post(root: &Path, payload: &Value) -> Result<Value, symphony::save::Failure> {
    let saves = symphony::save::Saves::start(root);
    let began = std::time::Instant::now();
    let mut result = saves.post("", "/api/quick-task", payload);
    while result.as_ref().is_ok_and(|value| value["pending"] == true)
        || result.as_ref().is_err_and(|error| error.status == 503)
    {
        assert!(
            began.elapsed() < super::LIMIT,
            "concurrent save stayed uncertain"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        result = saves.post("", "/api/quick-task", payload);
    }
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
