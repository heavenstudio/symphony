use super::*;

#[test]
fn the_client_draws_the_symphonys_pages_and_each_entrypoints_documents() {
    let convention = symphony_api::ProjectView {
        nav: vec![symphony_api::NavItem {
            label: "▤ shelf".into(),
            title: "Shelf items".into(),
            href: "/shelf".into(),
            document: "docs/SHELF.md".into(),
        }],
        ..symphony_api::ProjectView::default()
    };
    let root = symphony_api::ProjectView {
        entrypoints: vec![String::new()],
        ..convention.clone()
    };
    for (path, by_convention, at_root) in [
        ("/", true, true),
        ("/tasks", true, true),
        ("/task", true, true),
        ("/task/", true, true),
        ("/task/tools/x", true, true),
        ("/shelf/", true, true),
        ("/docs/", true, true),
        ("/docs/game/SPEC.md", true, true),
        ("/index.md", false, true),
        ("/guide/", false, true),
        ("/docs/v1.0/", true, true),
        ("/docs/v1.0", false, false),
        ("/v1.0/", false, true),
        ("/tasks/", true, true),
        ("/api/", false, false),
        ("/media/a.png", false, false),
        ("/api/document", false, false),
        ("/assets/app.js", false, false),
    ] {
        assert_eq!(client_page(&convention, path), by_convention, "{path}");
        assert_eq!(client_page(&root, path), at_root, "{path} at /");
    }
}

/// A listing's status no listing selects is a bad request said before anything reads the board,
/// so a board that is slow or down never answers for it first: no hub is started for it.
#[tokio::test]
async fn a_listing_status_no_listing_selects_is_refused_before_the_board_is_read() {
    let tree = crate::testing::Tree::new();
    let root = crate::testing::repository(&tree);
    let shared = crate::front::Shared::new(
        1,
        1234,
        &root,
        None,
        crate::gate::Gate::new(Arc::new(|_| Ok(None)), Arc::new(std::time::Instant::now)),
    );
    for (status, refused) in [
        ("nope", true),
        ("Review", true),
        ("", true),
        ("review", false),
        ("signoff", false),
        ("build", false),
    ] {
        let request = Request::builder()
            .uri(format!("/api/task-board?status={status}"))
            .header("Host", "localhost:1234")
            .body(())
            .unwrap();
        let said = status_argument_of(&request);
        assert_eq!(said.is_err(), refused, "{status}: {said:?}");
        if refused {
            let response = answer(&shared, request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{status}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body, serde_json::json!({ "error": said.unwrap_err() }));
        }
    }
    assert!(shared.api.get().is_none(), "no hub was started");
    let said = status_argument_of(
        &Request::builder()
            .uri("/api/task-board?status=nope")
            .body(())
            .unwrap(),
    )
    .unwrap_err();
    assert!(
        said.starts_with("no status \"nope\": a listing selects one of ")
            && said.ends_with(" or signoff, build, test"),
        "{said}"
    );
    assert!(
        status_argument_of(&Request::builder().uri("/api/task-board").body(()).unwrap()).is_ok()
    );
}

fn status_argument_of(request: &Request) -> Result<(), String> {
    task_board::status_argument(request)
}

#[test]
fn document_feed_defers_bodies_and_reserves_only_current_anchors() {
    let event = json!({"version":4,"changes":[{"region":"document","data":{
            "path":"@design~policy/docs/POLICY.md",
            "sections":[{"key":"policy","blocks":[{"type":"heading","anchor":"policy","spans":[]}],
                "comparison":{"kind":"changed","before":[{"type":"code","language":"","text":"Old policy"}]}},
                {"key":"removed:policy","blocks":[{"type":"heading","anchor":"policy","spans":[]}],
                "comparison":{"kind":"removed"}}]
        }}]}).to_string();
    let filter = ("@design~policy/docs/POLICY.md".to_string(), Vec::new());
    let unloaded: serde_json::Value =
        serde_json::from_str(&filtered_document_event(&event, &filter)).unwrap();
    let section = &unloaded["changes"][0]["data"]["sections"][0];
    assert_eq!(section["comparison"]["kind"], "changed");
    assert_eq!(section["blocks"], json!([]));
    assert_eq!(section["comparison"]["before"], json!([]));
    assert_eq!(
        unloaded["changes"][0]["data"]["anchors"]["policy"],
        json!(["policy"])
    );
    assert_eq!(
        unloaded["changes"][0]["data"]["anchors"]["removed:policy"],
        json!([])
    );
    let loaded: serde_json::Value = serde_json::from_str(&filtered_document_event(
        &event,
        &(filter.0, vec!["policy".into(), "removed:policy".into()]),
    ))
    .unwrap();
    let section = &loaded["changes"][0]["data"]["sections"][0];
    assert_eq!(section["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(section["comparison"]["before"][0]["text"], "Old policy");
    assert_eq!(
        loaded["changes"][0]["data"]["sections"][1]["blocks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        loaded["changes"][0]["data"]["anchors"]["removed:policy"],
        json!([])
    );
}

/// A card the page has not read the body of comes on the feed as the outline gives it: its plain
/// words, cut at a word with an ellipsis; a card it reads comes whole.
#[test]
fn document_feed_gives_an_unread_card_the_outlines_line() {
    let long = "A task's page now shows what the task went through, built from the **approved** \
                pictures. It sits under the gates and subtasks and above the brief, a lane each.";
    let event = json!({"version":4,"changes":[{"region":"document","data":{
        "path":"docs/DECISIONS.md",
        "cards":[{"key":"timeline","value":{"slug":"timeline","state":"approved","text":long,
            "group":"resolved","open":false}}]
    }}]})
    .to_string();
    let text = |keys: Vec<String>| -> String {
        let filtered: serde_json::Value = serde_json::from_str(&filtered_document_event(
            &event,
            &("docs/DECISIONS.md".to_string(), keys),
        ))
        .unwrap();
        filtered["changes"][0]["data"]["cards"][0]["value"]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let line = text(Vec::new());
    assert_eq!(line, symphony::docview::card_line(long));
    assert!(line.ends_with("above the…"), "{line}");
    assert!(!line.contains("**"), "{line}");
    assert_eq!(text(vec!["card:timeline".into()]), long);
}

#[test]
fn matches_etag_handles_whitespace_empty_values_and_invalid_headers() {
    let cases: &[(&str, &[&[u8]], bool)] = &[
        ("absent", &[], false),
        ("empty", &[b""], false),
        ("whitespace", &[b" \t "], false),
        ("empty list entries", &[b", ,\t,"], false),
        ("empty quoted tag", &[b"\"\""], false),
        ("strong", &[b"\"fresh\""], true),
        ("padded strong", &[b" \t\"fresh\"\t "], true),
        (
            "padded weak in list",
            &[b" , \"stale\" ,\tW/\"fresh\" , "],
            true,
        ),
        ("padded wildcard", &[b" \t*\t "], true),
        ("weak wildcard is not a wildcard", &[b"W/*"], false),
        ("quoted wildcard is a tag", &[b"\"*\""], false),
        ("different tag", &[b"W/\"stale\""], false),
        ("case sensitive tag", &[b"\"FRESH\""], false),
        ("invalid header bytes", &[b"\xff"], false),
        ("invalid list value", &[b"\xff, \"fresh\""], false),
        ("invalid then valid header", &[b"\xff", b"\"fresh\""], true),
        ("empty then valid header", &[b"", b"W/\"fresh\""], true),
    ];
    for &(case, values, expected) in cases {
        let mut request = Request::new(());
        for value in values {
            request.headers_mut().append(
                header::IF_NONE_MATCH,
                header::HeaderValue::from_bytes(value).unwrap(),
            );
        }
        assert_eq!(matches_etag(&request, "\"fresh\""), expected, "{case}");
    }
}
