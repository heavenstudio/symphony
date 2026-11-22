//! Every JSON answer and the feed say `X-Content-Type-Options: nosniff`, whichever route made
//! them ([`super::nosniff`]): each read, save, miss and refusal of a project's router, and its
//! event stream.

use super::*;
use crate::live::Hub;
use crate::testing::{Tree, repository};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use tower::ServiceExt;

/// An answer's type without its parameters, and each value its `X-Content-Type-Options` says.
fn said(response: &Response) -> (String, Vec<String>) {
    let headers = response.headers();
    let kind = headers
        .get(header::CONTENT_TYPE)
        .map(|kind| String::from_utf8_lossy(kind.as_bytes()).into_owned())
        .unwrap_or_default();
    let kind = kind
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    let options = headers
        .get_all(header::X_CONTENT_TYPE_OPTIONS)
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .collect();
    (kind, options)
}

#[tokio::test]
async fn nosniff_marks_json_and_the_feed_and_leaves_every_other_type_alone() {
    let marked: [&[u8]; 6] = [
        b"application/json",
        b"application/json; charset=utf-8",
        b"Application/JSON",
        b"application/json ; charset=utf-8",
        b"text/event-stream",
        b"TEXT/Event-Stream; charset=utf-8",
    ];
    let unmarked: [&[u8]; 11] = [
        b"text/html; charset=utf-8",
        b"text/javascript; charset=utf-8",
        b"text/css; charset=utf-8",
        b"text/plain; charset=utf-8",
        b"application/octet-stream",
        b"image/svg+xml",
        b"application/json-seq",
        b"application/jsonp",
        b"xapplication/json",
        b"",
        b"application/json\xff",
    ];
    let cases = marked
        .iter()
        .map(|kind| (Some(*kind), true))
        .chain(unmarked.iter().map(|kind| (Some(*kind), false)))
        .chain([(None, false)]);
    for (kind, expected) in cases {
        let mut response = (StatusCode::NOT_FOUND, "the body").into_response();
        response.headers_mut().remove(header::CONTENT_TYPE);
        if let Some(kind) = kind {
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_bytes(kind).unwrap(),
            );
        }
        let response = nosniff(response).await;
        let shown = kind.map(String::from_utf8_lossy);
        let (_, options) = said(&response);
        let expected: &[&str] = if expected { &["nosniff"] } else { &[] };
        assert_eq!(options, expected, "{shown:?}");
        // The answer is otherwise as it came: its status, its type and its body.
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{shown:?}");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .map(header::HeaderValue::as_bytes),
            kind,
            "{shown:?}"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"the body", "{shown:?}");
    }
}

#[tokio::test]
async fn an_answer_that_says_nosniff_already_says_it_once_and_another_type_keeps_its_own() {
    for kind in ["application/json", "image/png"] {
        let response = (
            [
                (header::CONTENT_TYPE, kind),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            "bytes",
        )
            .into_response();
        let (said_kind, options) = said(&nosniff(response).await);
        assert_eq!(
            (said_kind.as_str(), options),
            (kind, vec!["nosniff".to_string()])
        );
    }
}

/// A project's router over a repository with no board, its hub published with its page read and
/// its board reader failing at once, so no read reaches any board; the tree it lives in.
fn project() -> (Tree, Router) {
    let tree = Tree::new();
    let root = repository(&tree);
    let shared = Shared::new(1, 1, &root, None, crate::testing::open_gate());
    let mut hub = Hub::new(&root, &root.join("build/client"));
    hub.connect_reader = Box::new(|_| Err("this fixture has no board".into()));
    hub.refresh_page("", &root).unwrap();
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let app = router(shared).layer(MockConnectInfo(
        "127.0.0.1:2".parse::<SocketAddr>().unwrap(),
    ));
    (tree, app)
}

/// What `app` answers `method` at `path` with `headers`, from this machine by loopback's name.
async fn ask(app: &Router, method: &Method, path: &str, headers: &[(&str, &str)]) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:1");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let backstop = test_time::backstop(Duration::from_secs(60));
    let request = request.body(Body::empty()).unwrap();
    tokio::time::timeout(backstop, app.clone().oneshot(request))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{method} {path} to answer: stalled: the wait's wall-clock backstop of \
                 {backstop:?} ran out; {}",
                test_time::Load::now()
            )
        })
        .unwrap()
}

/// A request of the sweep: its method, its path and its headers.
type Asked = (
    Method,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// Every read route as a page's script asks it, the same with HEAD, every save route read and
/// sent from another site, the server's own route, and the misses the gate, the API and the
/// plugin routes answer: each answers JSON, so each says `nosniff`, and a route added to those
/// lists later is asked too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_read_save_miss_and_refusal_of_a_projects_router_says_nosniff() {
    const DATA: &[(&str, &str)] = &[("accept", "application/json")];
    const FOREIGN: &[(&str, &str)] = &[
        ("accept", "application/json"),
        ("origin", "http://elsewhere.example"),
    ];
    let (_tree, app) = project();
    let mut asked: Vec<Asked> = Vec::new();
    for read in crate::api::READS.iter().copied() {
        asked.push((Method::GET, read, DATA));
        asked.push((Method::HEAD, read, DATA));
    }
    for save in symphony::save::ROUTES
        .into_iter()
        .chain([symphony_api::UPLOAD_PATH, symphony_api::AGENT_FOCUS_PATH])
    {
        asked.push((Method::GET, save, DATA));
        asked.push((Method::POST, save, FOREIGN));
    }
    for other in [
        symphony_api::FRONT_PATH,
        "/api/nope",
        "/api/.git/config",
        "/api/%FF",
        "/@nope/api/board",
        "/x/nope/file",
    ] {
        asked.push((Method::GET, other, DATA));
    }
    let mut statuses = std::collections::BTreeSet::new();
    for (method, path, headers) in &asked {
        let response = ask(&app, method, path, headers).await;
        let status = response.status();
        statuses.insert(status.as_u16());
        assert_eq!(
            said(&response),
            ("application/json".to_string(), vec!["nosniff".to_string()]),
            "{method} {path}: {status}"
        );
    }
    // The sweep reached answers of every kind: reads, refusals, misses, the wrong method and a
    // board that cannot be read.
    for status in [200, 400, 403, 404, 405, 503] {
        assert!(statuses.contains(&status), "no {status} among {statuses:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_feeds_event_stream_says_nosniff() {
    let (_tree, app) = project();
    let board = ask(&app, &Method::GET, "/api/board", &[]).await;
    assert_eq!(board.status(), StatusCode::OK);
    let board = to_bytes(board.into_body(), usize::MAX).await.unwrap();
    let version = serde_json::from_slice::<serde_json::Value>(&board).unwrap()["version"]
        .as_u64()
        .unwrap();
    let feed = format!("/api/feed?since={version}");
    for method in [Method::GET, Method::HEAD] {
        let response = ask(&app, &method, &feed, &[]).await;
        assert_eq!(response.status(), StatusCode::OK, "{method}");
        assert_eq!(
            said(&response),
            ("text/event-stream".to_string(), vec!["nosniff".to_string()]),
            "{method}"
        );
    }
}
