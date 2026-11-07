//! Quick saves retain task identity and document context through the native router.

use super::Fixture;
use serde_json::json;

pub(super) fn quick_tasks(f: &Fixture) {
    task_filing(f);
    chat_context(f);
}

fn task_filing(f: &Fixture) {
    let legacy = f
        .post(
            "/api/quick-task",
            &json!({"title":"Keep the legacy form's text"}),
        )
        .status(400)
        .json();
    assert!(
        legacy["error"]
            .as_str()
            .unwrap()
            .contains("Reload this page"),
        "legacy refusal: {legacy}"
    );
    let filed = f.post("/api/quick-task", &json!({"request":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "title":"HTTP filed task", "body":"Private board body", "page":"/docs/TOPIC.md#one", "priority":1})).status(200).json();
    assert_eq!(filed["slug"], "inbox/http-filed-task");
    let store = symphony_board::cli::connect(&symphony_board::cli::Env::of_process()).unwrap();
    let task = store.get("inbox/http-filed-task").unwrap();
    assert_eq!(task.title, "HTTP filed task");
    assert_eq!(task.status, "concept");
    assert_eq!(task.priority, 1);
    assert!(task.body.contains("Private board body"), "{}", task.body);
    assert!(
        task.body
            .contains("Sent from [/docs/TOPIC.md#one](/docs/TOPIC.md#one)"),
        "{}",
        task.body
    );
    assert!(
        task.links.iter().any(|link| link == "docs/TOPIC.md#one"),
        "{:?}",
        task.links
    );
    assert_eq!(
        f.post(
            "/api/quick-task",
            &json!({"request":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "title":"HTTP filed task", "page":"/docs/TOPIC.md"})
        )
        .status(200)
        .json()["slug"],
        "inbox/http-filed-task-2"
    );
    let symbols = f
        .post(
            "/api/quick-task",
            &json!({"request":"dddddddddddddddddddddddddddddddd", "title":"🚀", "priority":"2"}),
        )
        .status(200)
        .json();
    assert_eq!(symbols["slug"], "inbox/task");
    let task = store.get("inbox/task").unwrap();
    assert_eq!(task.title, "🚀");
    assert_eq!(task.priority, 2);
    assert_eq!(task.body, "");
    assert_eq!(task.links, Vec::<String>::new());
}

fn chat_context(f: &Fixture) {
    struct Case {
        route: &'static str,
        page: &'static str,
        text: &'static str,
        path: &'static str,
        anchor: &'static str,
        question: &'static str,
    }
    let cases = [
        Case {
            route: "/api/quick-chat",
            page: "https://example.invalid/docs/%54OPIC.md#one",
            text: "Encoded document question",
            path: "docs/TOPIC.md",
            anchor: "one",
            question: "Encoded document question",
        },
        Case {
            route: "/api/quick-chat",
            page: "/tasks%no",
            text: "Board question",
            path: "docs/README.md",
            anchor: "",
            question: "Board question\n\n(sent from /tasks%no)",
        },
        Case {
            route: "/@work~http/api/quick-chat",
            page: "/docs/TOPIC.md#one",
            text: "Branch question",
            path: "docs/README.md",
            anchor: "",
            question: "Branch question\n\n(sent from /@work~http/docs/TOPIC.md#one)",
        },
    ];
    let asks = symphony::asks::AskStore::open(&f.root.parent().unwrap().join("asks.db")).unwrap();
    for case in cases {
        let reply = f
            .post(case.route, &json!({"page":case.page,"text":case.text}))
            .status(200)
            .json();
        let stored = asks.thread(reply["id"].as_i64().unwrap()).unwrap();
        assert_eq!(stored.path, case.path, "{}", case.page);
        assert_eq!(stored.anchor, case.anchor, "{}", case.page);
        assert_eq!(stored.question, case.question, "{}", case.page);
    }
}
