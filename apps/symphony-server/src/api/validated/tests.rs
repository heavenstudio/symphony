use super::*;
use crate::api::no_store;
use axum::response::{IntoResponse, Json};
use serde_json::json;
use std::time::Duration;

fn asked(values: &[&str]) -> Vec<HeaderValue> {
    values
        .iter()
        .map(|value| HeaderValue::from_str(value).unwrap())
        .collect()
}

async fn body(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

fn read() -> Response {
    no_store(Json(json!({"version": 3, "title": "A read"})))
}

fn without(mut response: Response, name: header::HeaderName) -> Response {
    response.headers_mut().remove(name);
    response
}

fn header(response: &Response, name: header::HeaderName) -> Option<&str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap())
}

#[test]
fn the_checksum_is_the_first_sixteen_hex_digits_of_the_bodys_sha256_quoted() {
    // SHA-256 of nothing and of `{}`, as `shasum -a 256` prints them.
    assert_eq!(checksum(b""), "\"e3b0c44298fc1c14\"");
    assert_eq!(checksum(b"{}"), "\"44136fa355b3678a\"");
    assert_ne!(checksum(b"{\"a\":1}"), checksum(b"{\"a\":2}"));
    assert_eq!(checksum(b"{\"a\":1}"), checksum(b"{\"a\":1}"));
}

#[tokio::test]
async fn a_json_read_carries_a_weak_tag_of_its_body_and_keeps_its_body_and_headers() {
    let sent = body(read()).await;
    let response = validated(&[], read()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, header::ETAG),
        Some(format!("W/{}", checksum(&sent)).as_str())
    );
    assert_eq!(header(&response, header::CACHE_CONTROL), Some("no-store"));
    assert_eq!(
        header(&response, header::CONTENT_TYPE),
        Some("application/json")
    );
    assert_eq!(body(response).await, sent);
}

#[tokio::test]
async fn a_request_naming_the_tag_is_answered_304_with_no_body() {
    let tag = checksum(&body(read()).await);
    for values in [
        vec![tag.clone()],
        vec![format!("W/{tag}")],
        vec![format!(" \"other\" ,\tW/{tag} ")],
        vec!["*".to_string()],
        vec!["\"other\"".to_string(), format!("W/{tag}")],
    ] {
        let named: Vec<&str> = values.iter().map(String::as_str).collect();
        let response = validated(&asked(&named), read()).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{values:?}");
        assert_eq!(
            header(&response, header::ETAG),
            Some(format!("W/{tag}").as_str()),
            "{values:?}"
        );
        assert_eq!(header(&response, header::CACHE_CONTROL), Some("no-store"));
        assert_eq!(header(&response, header::CONTENT_TYPE), None, "{values:?}");
        assert_eq!(
            header(&response, header::CONTENT_LENGTH),
            None,
            "{values:?}"
        );
        assert!(body(response).await.is_empty(), "{values:?}");
    }
}

#[tokio::test]
async fn a_request_naming_another_tag_is_sent_the_body() {
    let sent = body(read()).await;
    let changed = no_store(Json(json!({"version": 4, "title": "A read"})));
    let old = checksum(&sent);
    let response = validated(&asked(&[&old]), changed).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tag = header(&response, header::ETAG).unwrap().to_string();
    assert_ne!(tag, format!("W/{old}"), "a changed body has a new tag");
    let sent_now = body(response).await;
    assert_eq!(tag, format!("W/{}", checksum(&sent_now)));
    for values in [
        &["\"stale\""][..],
        &["W/*"],
        &["\"*\""],
        &[""],
        &[" , "],
        &[&old.to_uppercase()],
    ] {
        let response = validated(&asked(values), read()).await;
        assert_eq!(response.status(), StatusCode::OK, "{values:?}");
        assert_eq!(body(response).await, sent, "{values:?}");
    }
}

#[tokio::test]
async fn a_json_type_with_parameters_or_another_case_is_a_json_read() {
    for kind in [
        "application/json; charset=utf-8",
        "Application/JSON",
        " application/json ;q=1",
    ] {
        let response =
            validated(&[], no_store(([(header::CONTENT_TYPE, kind)], "{\"a\":1}"))).await;
        assert_eq!(
            header(&response, header::ETAG),
            Some(format!("W/{}", checksum(b"{\"a\":1}")).as_str()),
            "{kind}"
        );
    }
}

/// What is not a JSON read leaves as it came: no tag, no `304`, the same body.
#[tokio::test]
async fn anything_but_a_json_read_is_untouched() {
    let everything = asked(&["*"]);
    let mut own = no_store(Json(json!({"a": 1})));
    own.headers_mut()
        .insert(header::ETAG, HeaderValue::from_static("\"own\""));
    let cached = (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        "{}",
    )
        .into_response();
    for (case, response, etag) in [
        (
            "an error",
            super::super::error(StatusCode::NOT_FOUND, "no such document"),
            None,
        ),
        (
            "a refusal",
            super::super::error(StatusCode::BAD_REQUEST, "path is required"),
            None,
        ),
        ("its own validator", own, Some("\"own\"")),
        ("a cacheable file", cached, None),
        ("no cache rule", Json(json!({"a": 1})).into_response(), None),
        (
            "a page",
            no_store(([(header::CONTENT_TYPE, "text/html")], "<p>page</p>")),
            None,
        ),
        (
            "JSON lines",
            no_store(([(header::CONTENT_TYPE, "application/json-seq")], "{}")),
            None,
        ),
        (
            "no type",
            without(no_store("{}"), header::CONTENT_TYPE),
            None,
        ),
    ] {
        let status = response.status();
        let response = validated(&everything, response).await;
        assert_eq!(response.status(), status, "{case}");
        assert_eq!(header(&response, header::ETAG), etag, "{case}");
        assert!(!body(response).await.is_empty(), "{case}");
    }
}

/// The feed is answered `no-store` too, and its stream never ends: it is never collected.
#[tokio::test]
async fn the_feeds_stream_is_never_collected() {
    let stream = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
    let feed = no_store((
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(stream),
    ));
    let response = tokio::time::timeout(Duration::from_secs(5), validated(&asked(&["*"]), feed))
        .await
        .expect("the feed passes straight through");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, header::ETAG), None);
}

#[tokio::test]
async fn a_body_that_fails_to_read_is_a_server_error() {
    let stream = futures_util::stream::once(async {
        Err::<Vec<u8>, _>(std::io::Error::other("the body broke"))
    });
    let broken = no_store((
        [(header::CONTENT_TYPE, "application/json")],
        Body::from_stream(stream),
    ));
    let response = validated(&[], broken).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(header(&response, header::ETAG), None);
    let said: serde_json::Value = serde_json::from_slice(&body(response).await).unwrap();
    assert!(
        said["error"].as_str().unwrap().contains("the body broke"),
        "{said}"
    );
}

/// A document, its outline and its sections' bodies, as `/api/document` and `/api/section`
/// send them, at the feed's cursor `version`.
fn documents(version: u64, title: &str) -> [serde_json::Value; 3] {
    let document = json!({"version": version, "path": "docs/A.md", "title": title, "sections": []});
    [
        document.clone(),
        json!({"document": document, "anchors": {"a": ["a"]}}),
        json!({"version": version, "change": {"path": "docs/A.md", "title": title}}),
    ]
}

#[test]
fn a_documents_tag_leaves_out_the_feeds_cursor_and_its_value_as_it_came() {
    let [document, outline, sections] = documents(7, "A");
    for (shape, value) in [
        ("document", document),
        ("outline", outline),
        ("sections", sections),
    ] {
        let mut read = value.clone();
        let tag = without_cursor(&mut read);
        assert_eq!(read, value, "{shape}: the value is left as it came");
        // The same content at another cursor has the same tag; other content has another.
        let mut moved = serde_json::from_str::<serde_json::Value>(
            &value.to_string().replace("\"version\":7", "\"version\":8"),
        )
        .unwrap();
        assert_ne!(moved, value, "{shape}: the cursor moved");
        assert_eq!(without_cursor(&mut moved), tag, "{shape}");
        let mut edited =
            serde_json::from_str(&value.to_string().replace("\"A\"", "\"B\"")).unwrap();
        assert_ne!(without_cursor(&mut edited), tag, "{shape}");
        // The tag is a checksum like any other, of the value with no cursor.
        let mut bare = value.clone();
        if let Some(document) = bare.get_mut("document") {
            document["version"] = serde_json::Value::Null;
        } else {
            bare["version"] = serde_json::Value::Null;
        }
        assert_eq!(
            tag,
            checksum(&serde_json::to_vec(&bare).unwrap()),
            "{shape}"
        );
    }
    // A read with no cursor (a section by its anchor) is tagged by all of it.
    let mut section = json!({"anchor": "a", "blocks": []});
    assert_eq!(
        without_cursor(&mut section),
        checksum(&serde_json::to_vec(&section).unwrap())
    );
}

/// A route's own validator stands in for the body's checksum, and the body is sent as built.
#[tokio::test]
async fn a_routes_content_tag_stands_in_for_the_bodys_checksum() {
    let tagged = || {
        let mut response = read();
        response
            .extensions_mut()
            .insert(Content("\"0123456789abcdef\"".into()));
        response
    };
    let sent = body(read()).await;
    let response = validated(&[], tagged()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, header::ETAG),
        Some("W/\"0123456789abcdef\"")
    );
    assert!(response.extensions().get::<Content>().is_none());
    assert_eq!(body(response).await, sent);
    let cached = validated(&asked(&["W/\"0123456789abcdef\""]), tagged()).await;
    assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
    assert!(body(cached).await.is_empty());
    let checksum_named = validated(&asked(&[&checksum(&sent)]), tagged()).await;
    assert_eq!(
        checksum_named.status(),
        StatusCode::OK,
        "the body's checksum is not the tag"
    );
}
