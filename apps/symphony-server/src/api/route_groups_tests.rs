//! The reads' shared parts, each written once: the error answer, a blocking read's answer, the
//! guard for a hub not started yet, what a request asks, the arguments its route requires, and a
//! compatibility page's redirect.

use super::*;
use serde_json::Value;

fn shared(tree: &crate::testing::Tree) -> Arc<Shared> {
    let root = crate::testing::repository(tree);
    Shared::new(1, 1234, &root, None, crate::testing::open_gate())
}

fn get(uri: &str) -> Request {
    Request::builder()
        .uri(uri)
        .header("Host", "localhost:1234")
        .body(())
        .unwrap()
}

/// A response's status, its `Cache-Control` and its body as sent.
async fn sent(response: Response) -> (StatusCode, String, String) {
    let cache = response
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|value| value.to_str().unwrap().to_string())
        .unwrap_or_default();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, cache, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn the_error_answer_is_one_whoever_sends_it() {
    let response = error(StatusCode::CONFLICT, "no \"such\" thing");
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let said = sent(response).await;
    assert_eq!(
        said,
        (
            StatusCode::CONFLICT,
            "no-store".into(),
            r#"{"error":"no \"such\" thing"}"#.into()
        )
    );
    // Its words owned or borrowed, and the pages' name for it, are the same answer.
    let owned = error(StatusCode::CONFLICT, String::from("no \"such\" thing"));
    assert_eq!(sent(owned).await, said);
    let pages = crate::pages::json(StatusCode::CONFLICT, "no \"such\" thing");
    assert_eq!(sent(pages).await, said);
    assert_eq!(
        sent(error(StatusCode::NOT_FOUND, "")).await.2,
        r#"{"error":""}"#
    );
}

#[tokio::test]
async fn a_blocking_read_answers_what_it_made_its_refusal_or_why_it_did_not_end() {
    type Read = Result<Result<Json<Value>, Failed>, String>;
    let made: Read = Ok(Ok(Json(json!({"runs": 2}))));
    assert_eq!(
        sent(answered(made, "the runs could not be read: ")).await,
        (StatusCode::OK, "no-store".into(), r#"{"runs":2}"#.into())
    );
    let refused: Read = Ok(Err((StatusCode::NOT_FOUND, "no such run".into())));
    assert_eq!(
        sent(answered(refused, "the runs could not be read: ")).await,
        (
            StatusCode::NOT_FOUND,
            "no-store".into(),
            r#"{"error":"no such run"}"#.into()
        )
    );
    // A read that did not end is the server's failure, said after the route's own words or bare.
    for (unread, said) in [
        (
            "the runs could not be read: ",
            "the runs could not be read: task 7 panicked",
        ),
        ("", "task 7 panicked"),
    ] {
        let lost: Read = Err("task 7 panicked".into());
        assert_eq!(
            sent(answered(lost, unread)).await,
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "no-store".into(),
                json!({ "error": said }).to_string()
            )
        );
    }
    // An answer that carries headers of its own keeps them.
    let mut tagged = Json(json!([])).into_response();
    tagged.headers_mut().insert(
        symphony_api::VERSION_HEADER,
        header::HeaderValue::from(9u64),
    );
    let response = answered(Ok::<_, String>(Ok(tagged)), "");
    assert_eq!(response.headers()[symphony_api::VERSION_HEADER], "9");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

/// Before its hub, a project says the server is starting: on `Shared::hub` and on every read that
/// answers without the board. A read's own argument is refused first, as it always was.
#[tokio::test]
async fn before_the_hub_the_reads_off_the_board_say_the_server_is_starting() {
    let tree = crate::testing::Tree::new();
    let shared = shared(&tree);
    let starting = (
        StatusCode::SERVICE_UNAVAILABLE,
        "no-store".to_string(),
        r#"{"error":"the Symphony server is starting"}"#.to_string(),
    );
    assert_eq!(sent(*shared.hub().err().unwrap()).await, starting);
    for route in [
        "/api/health".to_string(),
        symphony_api::SCHEMA_UPDATES_PATH.to_string(),
        symphony_api::REPORTS_PATH.to_string(),
        format!("{}?run=r1", symphony_api::RUN_LOG_PATH),
        symphony_api::RUNS_PATH.to_string(),
        format!("{}?run=r1", symphony_api::RUN_PATH),
    ] {
        let response = answer(&shared, get(&route)).await.unwrap();
        assert_eq!(sent(response).await, starting, "{route}");
    }
    for (route, said) in [
        (
            format!("{}?grain=century", symphony_api::REPORTS_PATH),
            None,
        ),
        (symphony_api::RUN_LOG_PATH.to_string(), None),
        (format!("{}?end=never", symphony_api::RUNS_PATH), None),
        (
            symphony_api::RUN_PATH.to_string(),
            Some("run is required: a run's id"),
        ),
        (
            format!("{}?run=", symphony_api::RUN_PATH),
            Some("run is required: a run's id"),
        ),
    ] {
        let (status, cache, body) = sent(answer(&shared, get(&route)).await.unwrap()).await;
        assert_eq!(
            (status, cache.as_str()),
            (StatusCode::BAD_REQUEST, "no-store"),
            "{route}"
        );
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_ne!(body["error"], "the Symphony server is starting", "{route}");
        if let Some(said) = said {
            assert_eq!(body["error"], said, "{route}");
        }
    }
    assert!(shared.api.get().is_none(), "no hub was started");
}

#[tokio::test]
async fn what_a_request_asks_is_read_from_its_address_once() {
    let tree = crate::testing::Tree::new();
    let shared = shared(&tree);
    let asked = |uri: &str| Asked::of(&shared, &get(uri));
    // A worktree named by the address, by the `path` argument, or by both alike.
    for (uri, uri_scope, scope, path, rel) in [
        (
            "/api/document?path=docs/A.md",
            "",
            "",
            "/api/document",
            "docs/A.md",
        ),
        (
            "/@wt/api/document?path=docs/A.md",
            "wt",
            "wt",
            "/api/document",
            "docs/A.md",
        ),
        (
            "/api/document?path=@wt/docs/A.md",
            "",
            "wt",
            "/api/document",
            "docs/A.md",
        ),
        (
            "/@wt/api/document?path=@wt/docs/A.md",
            "wt",
            "wt",
            "/api/document",
            "docs/A.md",
        ),
        ("/api/document?path=@wt", "", "", "/api/document", "@wt"),
        ("/api/board", "", "", "/api/board", ""),
        ("/@wt/api/b%6Fard", "wt", "wt", "/api/board", ""),
    ] {
        let Some(Ok(asked)) = asked(uri) else {
            panic!("{uri} is a read");
        };
        assert_eq!(
            (
                asked.uri_scope.as_str(),
                asked.scope.as_str(),
                asked.path.as_str(),
                asked.rel.as_str()
            ),
            (uri_scope, scope, path, rel),
            "{uri}"
        );
        assert!(
            !asked.hosted && !asked.static_file && !asked.media_file,
            "{uri}"
        );
    }
    // A diff's file is named by its own path, whose `@` names no worktree.
    let uri = format!(
        "{}?slug=a/b&path=@scope/x.rs",
        symphony_api::TASK_DIFF_FILE_PATH
    );
    let diff = asked(&uri).unwrap().ok().unwrap();
    assert_eq!(
        (diff.scope.as_str(), diff.rel.as_str()),
        ("", "@scope/x.rs")
    );
    // Two worktrees named at once, and an address that does not decode, are refused.
    for (uri, said) in [
        (
            "/@a/api/document?path=@b/docs/A.md",
            "conflicting worktree scopes",
        ),
        ("/api/%zz", "invalid URL path"),
    ] {
        let refusal = *asked(uri).expect("a read").err().expect("a refusal");
        assert_eq!(
            sent(refusal).await,
            (
                StatusCode::BAD_REQUEST,
                "no-store".into(),
                json!({ "error": said }).to_string()
            ),
            "{uri}"
        );
    }
    // An API route no read answers, and the ask's POST, are the next handler's.
    assert!(asked("/api/nothing-here").is_none());
    let post = |uri: &str| {
        let mut request = get(uri);
        *request.method_mut() = Method::POST;
        Asked::of(&shared, &request)
    };
    assert!(post("/api/ask?id=1").is_none());
    assert!(
        post("/api/board").is_some_and(|asked| asked.is_ok()),
        "its method is refused later"
    );
    // The kinds of address: a client page, the queue as data, a thumbnail, a plugin's route, a
    // compatibility page and any other file.
    let page = asked("/tasks").unwrap().ok().unwrap();
    assert!(page.static_file && !page.media_file && page.alias.is_none());
    let queue = asked("/queue").unwrap().ok().unwrap();
    assert!(queue.static_file && !queue.json_queue);
    let mut data = get("/queue");
    data.headers_mut()
        .insert(header::ACCEPT, "application/json".parse().unwrap());
    // Asked for as data it is still the client's page by its address, answered from the hub as
    // the page's queue, never as the page's file (`read`) nor a file of the checkout.
    let queue = Asked::of(&shared, &data).unwrap().ok().unwrap();
    assert!(queue.json_queue && queue.static_file && !queue.media_file);
    let thumb = asked("/thumb/200/abc/docs/a.png.jpg")
        .unwrap()
        .ok()
        .unwrap();
    assert!(thumb.thumbnail);
    let plugin = asked("/x/shelf/items").unwrap().ok().unwrap();
    assert!(plugin.plugin_route);
    let old = asked("/@wt/doc/docs/A.md").unwrap().ok().unwrap();
    assert_eq!(old.alias.as_deref(), Some("/docs/A.md"));
    assert!(old.static_file);
    let file = asked("/src/main.rs").unwrap().ok().unwrap();
    assert!(file.media_file && !file.static_file && !file.html_file);
}

#[tokio::test]
async fn a_route_is_refused_the_argument_it_requires_with_that_arguments_words() {
    let tree = crate::testing::Tree::new();
    let shared = shared(&tree);
    let refused = |uri: &str| {
        let request = get(uri);
        let asked = Asked::of(&shared, &request).unwrap().ok().unwrap();
        arguments(&asked, &request).err().map(|refusal| *refusal)
    };
    let slugged = [
        "/api/task-page",
        "/api/task-brief",
        "/api/task-history",
        "/api/task-thread",
        "/api/task-offer",
        symphony_api::TASK_GATES_PATH,
        symphony_api::TASK_TIMELINE_PATH,
        symphony_api::TASK_TOKENS_PATH,
        symphony_api::TASK_DIFF_PATH,
        symphony_api::TASK_DIFF_FILE_PATH,
        symphony_api::workflow::WORKFLOW_ACTIONS_PATH,
        symphony_api::TASK_ATTEMPTS_PATH,
    ];
    let mut cases: Vec<(String, Option<&str>)> = Vec::new();
    for route in slugged {
        cases.push((route.to_string(), Some("slug is required")));
        cases.push((format!("{route}?slug=+"), Some("slug is required")));
        cases.push((
            format!("{route}?slug=%ff"),
            Some("slug is not valid: its percent-encoding is not UTF-8"),
        ));
        cases.push((format!("{route}?slug=tools/x"), None));
    }
    for route in [
        "/api/document",
        "/api/folder",
        "/api/section",
        "/api/history",
        "/api/versions",
        "/api/diff",
        "/api/asks",
    ] {
        cases.push((route.to_string(), Some("path is required")));
        cases.push((
            format!("{route}?path=a%01b"),
            Some("path is not valid: it holds a control character"),
        ));
        cases.push((format!("{route}?path="), None));
    }
    cases.push((
        symphony_api::AGENT_SCREEN_PATH.to_string(),
        Some("handle is required"),
    ));
    cases.push((
        format!("{}?handle=a-b-1", symphony_api::AGENT_SCREEN_PATH),
        None,
    ));
    // Routes that require none: the costs' list is judged by its own read, an ask by its own.
    for route in [
        "/api/board",
        "/api/agents",
        "/api/task-board",
        "/api/ask",
        symphony_api::TASK_COSTS_PATH,
        symphony_api::workflow::WORKFLOW_PATH,
    ] {
        cases.push((route.to_string(), None));
    }
    for (uri, said) in cases {
        match (refused(&uri), said) {
            (None, None) => {}
            (Some(refusal), Some(said)) => assert_eq!(
                sent(refusal).await,
                (
                    StatusCode::BAD_REQUEST,
                    "no-store".into(),
                    json!({ "error": said }).to_string()
                ),
                "{uri}"
            ),
            (refusal, said) => panic!("{uri}: refused {}, expected {said:?}", refusal.is_some()),
        }
    }
    let status = refused("/api/task-board?status=nope").expect("a status no listing selects");
    assert_eq!(status.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn a_compatibility_page_moves_to_its_document_in_its_worktree_with_its_query() {
    for (target, scope, uri, location) in [
        ("/docs/A.md", "", "/doc/docs/A.md", Some("/docs/A.md")),
        (
            "/docs/A.md",
            "wt",
            "/@wt/doc/docs/A.md",
            Some("/@wt/docs/A.md"),
        ),
        // The decoded name is encoded again; the query is kept as it was sent.
        (
            "/docs/a b?%.md",
            "",
            "/doc/x?at=1%202&b",
            Some("/docs/a%20b%3F%25.md?at=1%202&b"),
        ),
        ("/", "wt", "/@wt/app", Some("/@wt/")),
        // A part that would leave the project is no page.
        ("/../beta/x.js", "", "/history/x", None),
        ("/docs/./A.md", "wt", "/history/x", None),
        ("/docs/..", "", "/history/x", None),
    ] {
        let response = moved(target, scope, &get(uri));
        match location {
            Some(location) => {
                assert_eq!(
                    response.status(),
                    StatusCode::PERMANENT_REDIRECT,
                    "{target}"
                );
                assert_eq!(response.headers()[header::LOCATION], location, "{target}");
            }
            None => {
                assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
                assert!(
                    !response.headers().contains_key(header::LOCATION),
                    "{target}"
                );
            }
        }
    }
}
