//! The run-log read: its ask read from the query, the tail when it names no part; and the route,
//! refused before any hub for a bad ask, answering the tail and a range of a log in the hub's day
//! files' folder, and refusing an id that is no run's, a log not there and a link named as one.

use super::*;
use crate::testing::Tree;
use std::sync::Arc;

#[test]
fn an_ask_is_a_run_and_its_tail_or_a_range_never_both() {
    let run = "a".repeat(32);
    let ask = |tail, from, to| asked(Some(&run), tail, from, to);
    assert_eq!(
        ask(None, None, None).unwrap(),
        (run.clone(), Asked::Tail(RUN_LOG_TAIL))
    );
    assert_eq!(ask(Some("10"), None, None).unwrap().1, Asked::Tail(10));
    assert_eq!(
        ask(None, Some("5"), Some("9")).unwrap().1,
        Asked::Range {
            from: 5,
            to: Some(9)
        }
    );
    assert_eq!(
        ask(None, Some("5"), None).unwrap().1,
        Asked::Range { from: 5, to: None }
    );
    assert_eq!(
        ask(None, None, Some("9")).unwrap().1,
        Asked::Range {
            from: 0,
            to: Some(9)
        }
    );
    for (tail, from, to) in [(Some("1"), Some("2"), None), (Some("1"), None, Some("2"))] {
        assert_eq!(
            ask(tail, from, to).unwrap_err(),
            "tail, or from and to: one part of a log at a time"
        );
    }
    assert_eq!(
        ask(Some("-1"), None, None).unwrap_err(),
        "tail is a number of bytes, not \"-1\""
    );
    assert_eq!(
        ask(None, Some("x"), None).unwrap_err(),
        "from is a number of bytes, not \"x\""
    );
    for run in [None, Some("")] {
        assert_eq!(
            asked(run, None, None, None).unwrap_err(),
            "run is required: a run's id"
        );
    }
}

#[tokio::test]
async fn the_route_answers_a_logs_tail_and_a_range_and_refuses_what_is_no_runs_log() {
    use axum::http::StatusCode;
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    let shared = crate::front::Shared::new(1, 1234, &root, None, crate::testing::open_gate());
    let ask = |uri: String| {
        let shared = shared.clone();
        async move { crate::testing::get_json(&shared, &uri).await }
    };
    let run = "0123456789abcdef0123456789abcdef";
    let (status, said) = ask(format!("/api/run-log?run={run}&tail=1&from=2")).await;
    assert_eq!(
        (status, said),
        (
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "tail, or from and to: one part of a log at a time" })
        )
    );
    let (status, _) = ask(format!("/api/run-log?run={run}")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no hub yet");
    assert!(shared.api.get().is_none(), "no hub was started");

    let runs = tree.path("runs");
    let day = runs.join("logs/2026-11-14");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join(format!("{run}.log")), "one\ntwo\nthree\n").unwrap();
    let hub = crate::live::Hub::new(&root, &root.join("build/client"));
    let none = || Err("not this test's".to_string());
    let folders = super::super::reports::Folders {
        reports: none(),
        agents: none(),
        health: none(),
        runs: Some(runs.clone()),
    };
    hub.report_folders.set(folders).unwrap();
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let (status, said) = ask(format!("/api/run-log?run={run}&tail=6")).await;
    assert_eq!(status, StatusCode::OK);
    let tail: RunLog = serde_json::from_value(said).unwrap();
    assert_eq!(
        tail,
        RunLog {
            run: run.into(),
            log: format!("logs/2026-11-14/{run}.log"),
            size: 14,
            from: 8,
            to: 14,
            text: "three\n".into(),
        }
    );
    let (_, said) = ask(format!("/api/run-log?run={run}&from=4&to=8")).await;
    assert_eq!(said["text"], "two\n");
    let (_, said) = ask(format!("/api/run-log?run={run}")).await;
    assert_eq!(
        (said["from"].as_u64(), said["text"].as_str()),
        (Some(0), Some("one\ntwo\nthree\n"))
    );
    // An id that is no run's never names a file, a log not there is none, and a link named as one
    // is not followed out of the logs' folder.
    let (status, said) = ask("/api/run-log?run=..%2F..%2Fconfig".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    let other = "f".repeat(32);
    let (status, said) = ask(format!("/api/run-log?run={other}")).await;
    assert_eq!(
        (status, said["error"].as_str()),
        (
            StatusCode::NOT_FOUND,
            Some(format!("no log of the run {other}").as_str())
        )
    );
    let outside = tree.path("outside.log");
    std::fs::write(&outside, "not a log\n").unwrap();
    std::os::unix::fs::symlink(&outside, day.join(format!("{other}.log"))).unwrap();
    let (status, said) = ask(format!("/api/run-log?run={other}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{said}");
    assert!(
        said["error"]
            .as_str()
            .unwrap()
            .contains("not a regular file"),
        "{said}"
    );
}
