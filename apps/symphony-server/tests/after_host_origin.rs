//! Exercise Host/Origin at the real HTTP boundary, including successful handler side effects.
//! Losing either guard would admit these same valid payloads and change private state.

#[path = "after_host_origin/after_host_origin.rs"]
mod fixture;

#[path = "common/bash_env.rs"]
mod bash_env;

// How a test makes a channel ([`test_channel::channel`]).
#[path = "common/test_channel.rs"]
mod test_channel;

#[expect(
    dead_code,
    reason = "Only the private database is needed by this HTTP fixture"
)]
#[path = "../../../crates/tasks/tests/store/common.rs"]
mod board;

#[path = "../../../tools/test-support/script_board.rs"]
mod fixture_script;

use fixture::{Fixture, Server};
use serde_json::json;
use symphony_board::{NewSession, NewTask};

const DOCUMENT: &str = "/api/document?path=docs/TOPIC.md";
// Why the server's log says a write was refused.
const HOST_REFUSED: &str = "its Host is not this machine's";
const ORIGIN_REFUSED: &str = "its Origin is not http:// and its Host";

#[test]
fn bash_env_does_not_corrupt_host_lookup() {
    bash_env::run(
        "bash_env_does_not_corrupt_host_lookup",
        front_requires_a_complete_loopback_host_independently_of_document_hosts,
    );
}

#[test]
fn host_and_origin_guard_native_handlers_without_side_effects() {
    run(
        "host_and_origin_guard_native_handlers_without_side_effects",
        exercise,
    );
}

#[test]
fn front_requires_a_complete_loopback_host_independently_of_document_hosts() {
    run(
        "front_requires_a_complete_loopback_host_independently_of_document_hosts",
        exercise_front,
    );
}

#[test]
fn owner_changes_priority_and_cancels_only_open_tasks() {
    run(
        "owner_changes_priority_and_cancels_only_open_tasks",
        exercise_task_controls,
    );
}

#[test]
fn owner_approval_returns_the_work_its_stamps_held_blocked_to_review() {
    run(
        "owner_approval_returns_the_work_its_stamps_held_blocked_to_review",
        exercise_land_approve,
    );
}

fn exercise_land_approve(address: std::net::SocketAddr) {
    let f = Fixture::child(address);
    let host = f.host();
    let origin = format!("http://{host}");
    let slug = "review/change/held";
    f.board
        .create(&NewTask::new(slug, "Review change/held", "owner"))
        .unwrap();
    // Seed it blocked through the board's copy boundary, as land holds it while it waits on the owner.
    let mut dump = f.board.dump().unwrap();
    dump.tasks
        .iter_mut()
        .find(|task| task.slug == slug)
        .unwrap()
        .status = "blocked".into();
    f.board.copy(&dump).unwrap();
    f.approving("", &format!("[\"{slug}\"]"));
    let offer = || {
        let response = f.request(
            "GET",
            &format!("/api/task-offer?slug={slug}"),
            Some(&host),
            None,
            &serde_json::Value::Null,
        );
        assert_eq!(response.status, 200, "{response:?}");
        response.json()
    };
    // The action offers nothing on work that is not in review, and calls nothing: the offer names
    // the task's branch alone.
    assert_eq!(
        offer(),
        json!({"action":"", "label":"", "branch":"change/held", "payload":null})
    );
    assert!(f.land_calls().is_empty());
    let approve = || {
        f.request(
            "POST",
            symphony_api::TASK_ACTION_ROUTE,
            Some(&host),
            Some(&origin),
            &json!({"slug":slug, "action":"approve to land", "payload":"fixture-patch", "note":""}),
        )
    };
    let response = approve();
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(response.json()["ok"], true);
    assert_eq!(response.json()["said"], "approved change/held to land");
    assert_eq!(f.board.get(slug).unwrap().status, "review");
    let said: Vec<String> = (f.board.thread(slug).unwrap().into_iter())
        .map(|message| format!("{}: {}", message.sender, message.text))
        .collect();
    assert_eq!(
        said[said.len() - 2..],
        [
            "owner: approved change/held to land",
            "owner: owner moved it blocked → review"
        ]
    );
    let calls = f.land_calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(
        calls,
        ["approve change/held fixture-patch", "release change/held"]
    );
    // In review, land's patch is offered under the action's name and label.
    assert_eq!(
        offer(),
        json!({"action":"approve to land", "label":"Approve to land",
               "branch":"change/held", "payload":"fixture-patch"})
    );
    assert_eq!(f.land_calls()[2], "offer change/held");
    // A move the board refuses (the work is in review already) leaves the approval standing.
    let response = approve();
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(f.board.get(slug).unwrap().status, "review");
    // A release land cannot answer leaves the work blocked, and the approval standing.
    let mut dump = f.board.dump().unwrap();
    dump.tasks
        .iter_mut()
        .find(|task| task.slug == slug)
        .unwrap()
        .status = "blocked".into();
    f.board.copy(&dump).unwrap();
    f.approving("", "not json");
    let response = approve();
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(f.board.get(slug).unwrap().status, "blocked");
    // An approval land refuses, or an action no plugin of the project offers, says and moves nothing.
    let heard = f.board.thread(slug).unwrap().len();
    f.approving("the patch moved", &format!("[\"{slug}\"]"));
    let response = approve();
    assert_eq!(response.status, 400, "{response:?}");
    assert_eq!(response.json()["error"], "the patch moved");
    let response = f.request(
        "POST",
        symphony_api::TASK_ACTION_ROUTE,
        Some(&host),
        Some(&origin),
        &json!({"slug":slug, "action":"approve to release", "payload":"fixture-patch", "note":""}),
    );
    assert_eq!(response.status, 404, "{response:?}");
    assert_eq!(response.json()["error"], "no such board action");
    assert_eq!(f.board.thread(slug).unwrap().len(), heard);
    assert_eq!(f.board.get(slug).unwrap().status, "blocked");
    // The Symphony's own action, beside the plugin's: the owner's approve of the tip its offer
    // showed, posted as the owner's line the `owner` stamp reads. A payload that is no commit's id
    // says nothing; neither moves the work.
    let own = |payload: &str| {
        f.request(
            "POST",
            symphony_api::TASK_ACTION_ROUTE,
            Some(&host),
            Some(&origin),
            &json!({"slug":slug, "action":"owner-approve", "payload":payload, "note":""}),
        )
    };
    let response = own("the tip");
    assert_eq!(response.status, 400, "{response:?}");
    assert_eq!(
        response.json()["error"],
        "the owner's approve names the commit it approves, 7 to 40 hex digits: not \"the tip\""
    );
    assert_eq!(f.board.thread(slug).unwrap().len(), heard);
    let response = own("0123abc4");
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(response.json()["said"], "approve 0123abc4");
    let last = f.board.thread(slug).unwrap().pop().unwrap();
    assert_eq!(
        (last.sender.as_str(), last.text.as_str()),
        ("owner", "approve 0123abc4")
    );
    assert_eq!(f.board.get(slug).unwrap().status, "blocked");
}

#[test]
fn owner_cancel_finishes_only_from_the_shared_installation() {
    let test = "owner_cancel_finishes_only_from_the_shared_installation";
    if std::env::var_os("SYMPHONY_AFTER_HOST_ORIGIN_CHILD").is_none() {
        Fixture::run_finishing(test);
        return;
    }
    run(test, exercise_finish);
}

#[test]
fn owner_cancel_answers_before_cleanup_finishes_and_reports_failure_on_the_task() {
    let test = "owner_cancel_answers_before_cleanup_finishes_and_reports_failure_on_the_task";
    if std::env::var_os("SYMPHONY_AFTER_HOST_ORIGIN_CHILD").is_none() {
        Fixture::run_finishing(test);
        return;
    }
    run(test, exercise_blocked_finish);
}

fn exercise_blocked_finish(_: std::net::SocketAddr) {
    let runtime = tokio::runtime::Handle::current();
    let server = runtime.block_on(Server::start_shared());
    let shared = Fixture::child(server.address);
    let mut feed = shared.feed();
    for (index, outcome) in ["ok", "closed", "fail", "reported"].into_iter().enumerate() {
        let slug = format!("fixture/blocked-finish-{index}");
        shared
            .board
            .create(&NewTask::new(&slug, "Blocked finish", "owner"))
            .unwrap();
        let mut release = shared.block_finish();
        // Neither the FIFO nor a timer releases cleanup to let the request pass. A synchronous
        // finish would stall this real HTTP response until its read timeout failed the test.
        let response = shared.request(
            "POST",
            "/api/task-cancel",
            Some(&shared.host()),
            None,
            &json!({"slug":slug}),
        );
        assert_eq!(response.status, 200, "{response:?}");
        assert_eq!(response.json()["ok"], true);
        assert_eq!(shared.board.get(&slug).unwrap().status, "cancelled");
        let calls = shared.wait_for_finish_calls(index * 2 + 1);
        assert_eq!(calls.len(), index * 2 + 1, "sweep must wait for finish");
        assert_eq!(
            calls.last().unwrap(),
            &format!("finish {slug}|session=unset|terminal=unset|socket=unset")
        );
        release.release(outcome);
        let calls = shared.wait_for_finish_calls(index * 2 + 2);
        assert_eq!(
            calls.last().unwrap(),
            &format!(
                "sweep --background={slug} is cancelled --task={slug}|session=unset|terminal=unset|socket=unset"
            )
        );
        let thread = shared.board.thread(&slug).unwrap();
        let failures: Vec<_> = thread
            .iter()
            .filter(|message| message.text.contains("cleanup failed"))
            .collect();
        if matches!(outcome, "fail" | "reported") {
            assert_eq!(failures.len(), 1, "{thread:?}");
            let failure = failures[0];
            if outcome == "fail" {
                assert!(
                    failure.text.contains("fixture finish failure"),
                    "{failure:?}"
                );
                assert!(failure.text.contains("9"), "{failure:?}");
            } else {
                assert!(
                    failure.text.contains(&format!(
                        "could not clean up the sessions of {slug}: fixture hc unavailable"
                    )),
                    "{failure:?}"
                );
                assert!(failure.text.contains("\nfixture hc details"), "{failure:?}");
            }
            assert_eq!(failure.recipient.as_deref(), Some("owner"));
            assert_ne!(failure.sender, "owner");
            assert_ne!(failure.sender, "fixture-session");
            assert_eq!(
                shared.board.session(&failure.sender).unwrap().row.kind,
                "app"
            );
            assert_eq!(
                shared
                    .board
                    .sessions()
                    .unwrap()
                    .iter()
                    .filter(|session| session.row.kind == "app")
                    .count(),
                1,
                "repeated failures reuse the app identity"
            );
            let history = shared.request(
                "GET",
                &format!("/api/task-history?slug={slug}"),
                Some(&shared.host()),
                None,
                &serde_json::Value::Null,
            );
            assert_eq!(history.status, 200, "{history:?}");
            assert!(
                history.json()["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| { entry["message"]["text"] == failure.text }),
                "owner must see the cleanup failure in task history: {history:?}"
            );
            // Durable thread history and a finished sweep do not synchronize the hub cache.
            feed.activity(failure.id);
            let activity = shared.request(
                "GET",
                "/api/activity",
                Some(&shared.host()),
                None,
                &serde_json::Value::Null,
            );
            assert_eq!(activity.status, 200, "{activity:?}");
            assert!(
                activity.json()["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| {
                        item["id"] == format!("task#{}", failure.id)
                            && item["href"] == format!("/task/{slug}")
                    }),
                "cleanup failure must reach owner activity: {activity:?}"
            );
        } else {
            assert!(failures.is_empty(), "{thread:?}");
        }
    }
    drop(feed);
    runtime.block_on(server.finish());
}

fn exercise_finish(address: std::net::SocketAddr) {
    let private = Fixture::child(address);
    let response = private.request(
        "POST",
        "/api/task-cancel",
        Some(&private.host()),
        None,
        &json!({"slug":"fixture/host-origin"}),
    );
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(
        private.board.get("fixture/host-origin").unwrap().status,
        "cancelled"
    );
    assert!(
        private.finish_calls().is_empty(),
        "a preview must not use copied pane ids"
    );

    let runtime = tokio::runtime::Handle::current();
    let server = runtime.block_on(Server::start_shared());
    let shared = Fixture::child(server.address);
    let slug = "fixture/shared-cancel";
    shared
        .board
        .create(&NewTask::new(slug, "Shared cancel", "owner"))
        .unwrap();
    let response = shared.request(
        "POST",
        "/api/task-priority",
        Some(&shared.host()),
        None,
        &json!({"slug":slug,"priority":0}),
    );
    assert_eq!(response.status, 200, "{response:?}");
    let response = shared.request(
        "POST",
        "/api/task-cancel",
        Some(&shared.host()),
        Some("http://foreign.example"),
        &json!({"slug":slug}),
    );
    assert_eq!(response.status, 403, "{response:?}");
    assert!(shared.finish_calls().is_empty());

    let response = shared.request(
        "POST",
        "/api/task-cancel",
        Some(&shared.host()),
        None,
        &json!({"slug":slug}),
    );
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(shared.board.get(slug).unwrap().status, "cancelled");
    let calls = shared.wait_for_finish();
    assert_eq!(
        calls,
        [
            format!("finish {slug}|session=unset|terminal=unset|socket=unset"),
            format!(
                "sweep --background={slug} is cancelled --task={slug}|session=unset|terminal=unset|socket=unset"
            ),
        ]
    );
    let response = shared.request(
        "POST",
        "/api/task-cancel",
        Some(&shared.host()),
        None,
        &json!({"slug":slug}),
    );
    assert_eq!(response.status, 400, "{response:?}");
    assert_eq!(
        shared.finish_calls(),
        calls,
        "a refused cancel must not finish again"
    );
    runtime.block_on(server.finish());
}

fn run(test: &str, exercise: fn(std::net::SocketAddr)) {
    if std::env::var_os("SYMPHONY_AFTER_HOST_ORIGIN_CHILD").is_none() {
        Fixture::run(test);
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    symphony::plugins::install(fixture::approving::PLUGINS).unwrap();
    runtime.block_on(async {
        let server = Server::start().await;
        let result = tokio::task::spawn_blocking({
            let address = server.address;
            move || exercise(address)
        })
        .await;
        server.finish().await;
        result.unwrap();
    });
}

fn exercise(address: std::net::SocketAddr) {
    let f = Fixture::child(address);
    // This successful document read is also the explicit read-hub readiness handshake.
    f.document(Some(&f.host()), None);
    f.assert_front(Some(&f.host()), 200);
    let baseline = f.snapshot();

    let invalid_hosts = [
        None,
        Some(""),
        Some("foreign.example"),
        Some("localhost.evil.example"),
        Some("127.0.0.1.evil.example:80"),
        Some("review-fixture.tail.test.evil.example"),
        Some("review-fixture.tail.test@evil.example"),
        Some("not-review-fixture.tail.test"),
        Some("100.83.24.16"), // Only the fixture's own tailnet IP is admitted.
        Some("localhost:not-a-port"),
        Some("localhost:"),
        Some("localhost:65536"),
        Some("localhost:80:90"),
        Some("localhost/path"),
        Some("::1"),
        Some("[::1"),
        Some("[::1]suffix"),
        Some("[::1]:bad"),
        Some("[::1]:65536"),
        Some("[::2]:80"),
        Some("[fd7a:115c:a1e0::99]:80"),
    ];
    for host in invalid_hosts {
        f.refused_read(
            host,
            None,
            "Forbidden: the Host header does not name this machine",
        );
        f.refused_writes(host, None, HOST_REFUSED);
        // Nor does the feed's WebSocket read past it, under the Host's own Origin, which the
        // socket's own rule would let in, or with neither, which only the Host gate refuses.
        let own = host.map(|host| format!("http://{host}"));
        assert_eq!(f.socket(host, own.as_deref()), 403, "{host:?}");
        // Even a matching Origin cannot rescue an untrusted Host.
        if let Some(host) = host {
            f.refused_writes(Some(host), Some(&format!("http://{host}")), HOST_REFUSED);
        }
        f.assert_unchanged(&baseline, &format!("Host {host:?}"));
    }

    f.refused_read(
        Some(&f.host()),
        Some("null"),
        "Forbidden: a sandboxed page has no access to the app API",
    );
    f.assert_unchanged(&baseline, "front and sandboxed read refusals");

    for origin in [
        String::new(),
        "null".into(),
        "foreign.example".into(),
        "http://foreign.example".into(),
        format!("https://{}", f.host()),
        format!("http://localhost:{}", address.port()),
        "http://127.0.0.1".into(),
        format!("http://127.0.0.1:{}", address.port() - 1),
        format!("http://{}/", f.host()),
        format!("http://{}/path", f.host()),
        format!("http://{} http://foreign.example", f.host()),
        format!("http://{}@foreign.example", f.host()),
    ] {
        f.refused_writes(Some(&f.host()), Some(&origin), ORIGIN_REFUSED);
        f.assert_unchanged(&baseline, &format!("Origin {origin:?}"));
        // A WebSocket has no same-origin rule for its reads: the feed's is refused another page.
        assert_eq!(f.socket(Some(&f.host()), Some(&origin)), 403, "{origin:?}");
    }
    let refused = f.refusals();

    let port = address.port();
    let allowed_hosts = [
        "localhost".into(),
        "127.0.0.1".into(),
        "[::1]".into(),
        format!("localhost:{port}"),
        f.host(),
        format!("[::1]:{port}"),
        format!("LOCALHOST:{port}"),
        format!("localhost.:{port}"),
        format!("REVIEW-FIXTURE.TAIL.TEST.:{port}"),
        "review-fixture.tail.test".into(),
        format!("100.83.24.15:{port}"),
        format!("[fd7a:115c:a1e0::15]:{port}"),
    ];
    for (index, host) in allowed_hosts.iter().enumerate() {
        f.document(Some(host), None);
        // Each missing-Origin request is a command-line request permitted by the README.
        // Each present-Origin request checks the same Host's exact scheme, name and port.
        for origin in [None, Some(format!("http://{host}"))] {
            assert_eq!(
                f.socket(Some(host), origin.as_deref()),
                101,
                "{host} {origin:?}"
            );
            let marker = format!("allowed-{index}-{}", origin.is_some());
            let response = f.request(
                "POST",
                "/api/task-message",
                Some(host),
                origin.as_deref(),
                &json!({"slug":"fixture/host-origin", "text":marker}),
            );
            assert_eq!(response.status, 200, "{host} {origin:?}: {response:?}");
            assert_eq!(response.json()["ok"], true);
            let thread = f.board.thread("fixture/host-origin").unwrap();
            assert_eq!(thread.last().unwrap().text, marker);
            assert_eq!(thread.last().unwrap().sender, "owner");
        }
    }

    // These are the identical document and ask payloads used in the refusal cases. They must
    // really mutate their intended stores, otherwise the negative side-effect checks are vacuous.
    let response = f.request("POST", "/api/comment", Some(&f.host()), None, &f.comment());
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(response.json()["ok"], true);
    assert!(f.document_text().contains("Host Origin fixture comment"));
    let response = f.request(
        "POST",
        "/api/ask",
        Some(&f.host()),
        Some(&format!("http://{}", f.host())),
        &f.ask(),
    );
    assert_eq!(response.status, 200, "{response:?}");
    let ask = f
        .asks()
        .thread(response.json()["id"].as_i64().unwrap())
        .unwrap();
    assert_eq!(ask.question, "Host Origin fixture question");
    assert_eq!(ask.path, "docs/TOPIC.md");
    // Origin comparison is case-insensitive, as are DNS names.
    assert_eq!(
        f.socket(
            Some(&format!("LOCALHOST:{port}")),
            Some(&format!("HTTP://localhost:{port}"))
        ),
        101
    );
    let response = f.request(
        "POST",
        "/api/task-message",
        Some(&format!("LOCALHOST:{port}")),
        Some(&format!("HTTP://localhost:{port}")),
        &json!({"slug":"fixture/host-origin", "text":"case-insensitive origin"}),
    );
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(
        f.board
            .thread("fixture/host-origin")
            .unwrap()
            .last()
            .unwrap()
            .text,
        "case-insensitive origin"
    );
    let response = f.request(
        "POST",
        "/api/task-message",
        Some(&f.host()),
        None,
        &f.task_message(),
    );
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(response.json()["ok"], true);
    assert_eq!(
        f.board
            .thread("fixture/host-origin")
            .unwrap()
            .last()
            .unwrap()
            .text,
        "Private owner write"
    );
    assert_eq!(f.refusals(), refused, "an allowed write logs no refusal");
    f.assert_lookup_used();
    println!(
        "coverage: 21 refused Hosts; 12 allowed Hosts x absent/matching Origin; 12 refused Origins; the feed's WebSocket under each; {} refusal log lines; document, asks and board side effects checked",
        refused.len()
    );
}

fn exercise_front(address: std::net::SocketAddr) {
    let f = Fixture::child(address);
    f.document(Some(&f.host()), None);
    let baseline = f.snapshot();
    // These names must reach the document handler but never the local discovery handler.
    for host in [
        "review-fixture.tail.test",
        "100.83.24.15",
        "[fd7a:115c:a1e0::15]",
    ] {
        f.document(Some(host), None);
        f.assert_front(Some(host), 403);
    }
    for host in [
        None,
        Some(""),
        Some("foreign.example"),
        Some("localhost.evil.example"),
        Some("127.0.0.1.evil.example"),
        Some("[::1].evil.example"),
        Some("localhost:bad"),
        Some("localhost:65536"),
        Some("[::1"),
        Some("[::1]:bad"),
        Some("[::1]:65536"),
    ] {
        f.assert_front(host, 403);
    }
    f.assert_unchanged(&baseline, "front refusals");
    for host in ["localhost", "127.0.0.1", "[::1]", "LOCALHOST"] {
        f.assert_front(Some(host), 200);
        f.assert_front(Some(&format!("{host}:{}", address.port())), 200);
    }
    f.assert_lookup_used();
    println!(
        "coverage: 14 refused and 8 allowed front Hosts, including three Hosts admitted by document routes"
    );
}

fn exercise_task_controls(address: std::net::SocketAddr) {
    let f = Fixture::child(address);
    let slug = "fixture/host-origin";
    let host = f.host();
    let origin = format!("http://{host}");
    for priority in 0..=4 {
        // Accept both native JSON integers and the strings posted by existing app forms.
        for value in [json!(priority), json!(priority.to_string())] {
            let response = f.request(
                "POST",
                "/api/task-priority",
                Some(&host),
                Some(&origin),
                &json!({"slug":slug, "priority":value, "actor":"impostor"}),
            );
            assert_eq!(response.status, 200, "{response:?}");
            assert_eq!(response.json()["ok"], true);
            assert_eq!(f.board.get(slug).unwrap().priority, priority);
            let thread = f.board.thread(slug).unwrap();
            let last = thread.last().unwrap();
            assert_eq!(last.sender, "owner");
            assert!(last.text.ends_with(&format!("→ p{priority}")));
        }
    }
    let baseline = f.snapshot();
    for value in [
        json!(-1),
        json!(5),
        json!(1.5),
        json!(true),
        json!(null),
        json!("p1"),
        json!(""),
        json!("1.5"),
        json!("9223372036854775808"),
    ] {
        let response = f.request(
            "POST",
            "/api/task-priority",
            Some(&host),
            Some(&origin),
            &json!({"slug":slug, "priority":value}),
        );
        assert_eq!(response.status, 400, "{value}: {response:?}");
        assert!(
            response.json()["error"]
                .as_str()
                .unwrap()
                .contains("priority")
        );
        f.assert_unchanged(&baseline, &format!("bad priority {value}"));
    }
    for (route, body) in [
        ("/api/task-priority", json!({"slug":slug,"priority":0})),
        ("/api/task-cancel", json!({"slug":slug})),
    ] {
        for (bad_host, bad_origin) in [
            (host.as_str(), "http://foreign.example"),
            ("foreign.example", "http://foreign.example"),
        ] {
            let response = f.request("POST", route, Some(bad_host), Some(bad_origin), &body);
            assert_eq!(response.status, 403, "{route}: {response:?}");
            f.assert_unchanged(&baseline, "task control Host/Origin refusal");
        }
        // A task the board lacks is not found, as a read of it is.
        let response = f.request(
            "POST",
            route,
            Some(&host),
            Some(&origin),
            &json!({"slug":"fixture/missing", "priority":0}),
        );
        assert_eq!(response.status, 404, "{route}: {response:?}");
        assert!(
            response.json()["error"]
                .as_str()
                .unwrap()
                .contains("no task")
        );
        f.assert_unchanged(&baseline, "missing task");
    }
    // A task write naming no task is refused as such, never as a board that cannot be reached.
    for route in [
        "/api/task-message",
        "/api/task-priority",
        "/api/task-field",
        "/api/task-cancel",
        symphony_api::TASK_ACTION_ROUTE,
    ] {
        for body in [
            json!({}),
            json!({"priority": 2}),
            json!({"slug": " ", "text": "hi"}),
        ] {
            let response = f.request("POST", route, Some(&host), Some(&origin), &body);
            assert_eq!(response.status, 400, "{route} {body}: {response:?}");
            assert_eq!(
                response.json(),
                json!({"error": "slug is required"}),
                "{route} {body}"
            );
            f.assert_unchanged(&baseline, "a task write naming no task");
        }
    }
    for status in [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "blocked",
        "review",
        "done",
        "cancelled",
    ] {
        let slug = format!("fixture/cancel-{}", status.replace('_', "-"));
        let task = NewTask::new(&slug, "Owner cancellation", "owner");
        f.board.create(&task).unwrap();
        // Seed existing work through the board's copy boundary: create admits only pre-work
        // statuses, and these owner cases must have no registered agent or role.
        let mut dump = f.board.dump().unwrap();
        dump.tasks
            .iter_mut()
            .find(|task| task.slug == slug)
            .unwrap()
            .status = status.into();
        f.board.copy(&dump).unwrap();
        let before = f.snapshot();
        assert!(f.board.move_to(&slug, "ready", "owner").is_err());
        // The exception must grant no new powers to an unregistered agent.
        assert!(f.board.move_to(&slug, "cancelled", "impostor").is_err());
        let response = f.request(
            "POST",
            "/api/task-cancel",
            Some(&host),
            Some(&origin),
            &json!({"slug":slug, "actor":"impostor"}),
        );
        if matches!(status, "done" | "cancelled") {
            assert_eq!(response.status, 400, "{status}: {response:?}");
            assert!(
                response.json()["error"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("cannot go from {status} to cancelled"))
            );
            f.assert_unchanged(&before, "terminal cancellation");
        } else {
            assert_eq!(response.status, 200, "{status}: {response:?}");
            assert_eq!(response.json()["ok"], true);
            assert_eq!(f.board.get(&slug).unwrap().status, "cancelled");
            let thread = f.board.thread(&slug).unwrap();
            let last = thread.last().unwrap();
            assert_eq!(last.sender, "owner");
            assert_eq!(last.text, format!("owner moved it {status} → cancelled"));
        }
    }
    assert!(
        f.board.dump().unwrap().sessions.is_empty(),
        "owner controls must not fabricate an agent session"
    );
    let worker = f.board.register(&NewSession::new("worker")).unwrap().row;
    let outsider = f.board.register(&NewSession::new("worker")).unwrap().row;
    f.board.claim(slug, &worker.handle).unwrap();
    let before = f.snapshot();
    let refusal = f
        .board
        .move_to(slug, "cancelled", &outsider.handle)
        .unwrap_err();
    assert!(refusal.to_string().contains("holds no role"));
    f.assert_unchanged(&before, "registered outsider cannot cancel held work");
    let response = f.request(
        "POST",
        "/api/task-cancel",
        Some(&host),
        Some(&origin),
        &json!({"slug":slug}),
    );
    assert_eq!(
        response.status, 200,
        "owner can cancel held work: {response:?}"
    );
    assert_eq!(f.board.get(slug).unwrap().status, "cancelled");
    assert_eq!(
        f.board.thread(slug).unwrap().last().unwrap().sender,
        "owner"
    );
    // The shared transaction must still admit ordinary role holders after adding the owner case.
    let mut task = NewTask::new("fixture/executor-cancel", "Executor cancellation", "owner");
    task.status = "ready".into();
    f.board.create(&task).unwrap();
    f.board.claim(&task.slug, &worker.handle).unwrap();
    f.board
        .move_to(&task.slug, "cancelled", &worker.handle)
        .unwrap();
    assert_eq!(
        f.board.thread(&task.slug).unwrap().last().unwrap().sender,
        worker.handle
    );
    println!(
        "coverage: task priority 0..4 as integer/string; 9 invalid priorities; both writes refuse Host/Origin and missing task; cancellation from all 7 open statuses, both terminal refusals, owner history and unregistered-agent refusal"
    );
}
