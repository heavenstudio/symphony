//! The client's build as the router serves it: a content-hashed file cached for good and the rest
//! revalidated, each build file gzipped once however often it is asked for, and Accept-Encoding
//! read as the router's compression layer reads it.

use super::gzipped::{runs, takes_gzip};
use super::*;
use crate::front::router;
use crate::gate::Gate;
use crate::testing::Tree;
use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::Method;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use symphony_board::sys::{Command, spawn_piped, wait_output};
use tower::ServiceExt;

/// What one request got back.
struct Got {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl Got {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|value| value.to_str().unwrap())
    }
}

/// The router of a server of its own whose installed client is `files`, under `public/`.
fn serving(files: &[(&str, &[u8])]) -> (Tree, Router) {
    let tree = Tree::new();
    for (name, bytes) in files {
        let path = tree.path(&format!("install/client/public/{name}"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    std::fs::create_dir_all(tree.path("repo")).unwrap();
    // An install folder fixes the client to these files, whatever SF_SYMPHONY_UI_DIST names.
    let shared = Shared::new(
        1,
        1,
        &tree.path("repo"),
        Some(&tree.path("install")),
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let loopback: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    (tree, router(shared).layer(MockConnectInfo(loopback)))
}

async fn send(app: &Router, method: Method, path: &str, headers: &[(&str, &str)]) -> Got {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:1");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    Got {
        status: parts.status,
        headers: parts.headers,
        body: axum::body::to_bytes(body, usize::MAX).await.unwrap(),
    }
}

/// `bytes` decoded by the system's gzip, as a browser would.
fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut decoder = spawn_piped(Command::new("gzip").arg("-dc")).unwrap();
    let mut input = decoder.stdin.take().unwrap();
    // Written beside the read: gzip answers while it reads, and a pipe holds only so much.
    let decoded = std::thread::scope(|scope| {
        let written = scope.spawn(move || input.write_all(bytes));
        let decoded = wait_output(decoder).unwrap();
        written.join().unwrap().unwrap();
        decoded
    });
    assert!(decoded.status.success());
    decoded.stdout
}

fn sha256(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A compressible file of `size` bytes no other test serves, so its count is its own.
fn module(test: &str, size: usize) -> Vec<u8> {
    format!("export const fixture = {test:?}; // one module of the build\n")
        .bytes()
        .cycle()
        .take(size)
        .collect()
}

#[test]
fn a_content_hash_names_dx_and_package_assets_and_nothing_else() {
    for hashed in [
        "assets/board-f129bf6e05941e78.js",
        "assets/symphony-shell-4375f62f92e3d4a2.js",
        "assets/symphony-ui_bg-dxh8d1bca92fd62af8.wasm",
        "assets/dx-components-theme-dxh3f7454274acd8235.css",
        "assets/app-dxh123.js",
        "assets/chunks/old-dxh789.js",
        "assets/board-f129bf6e05941e78.min.js",
    ] {
        assert!(content_hashed(hashed), "{hashed}");
    }
    for unhashed in [
        "index.html",
        "assets/board.js",
        "assets/board-latest.js",
        "assets/board-f129bf6e05941e7.js",
        "assets/board-f129bf6e05941e78a.js",
        "assets/board-F129BF6E05941E78.js",
        "assets/board-g129bf6e05941e78.js",
        "assets/board-dxh.js",
        "assets/board-dxhzz.js",
        "assets/dxh123-board.js",
        "assets/f129bf6e05941e78.js",
        "assets/f129bf6e05941e78-board.js",
        "static/board-f129bf6e05941e78.js",
        "board-f129bf6e05941e78.js",
    ] {
        assert!(!content_hashed(unhashed), "{unhashed}");
    }
}

/// The committed release: every file but the page is named by its content, so none revalidates.
#[test]
fn every_file_the_server_ships_under_assets_is_cached_for_good() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("client/public/assets");
    let mut names = 0;
    for entry in std::fs::read_dir(assets).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(content_hashed(&format!("assets/{name}")), "{name}");
        names += 1;
    }
    assert!(names > 0, "the server ships no client assets");
}

/// Each header against the router's own layer: the file goes gzipped exactly when the layer
/// would have gzipped it.
#[tokio::test]
async fn gzip_is_taken_exactly_when_the_routers_compression_layer_takes_it() {
    let cases: &[&[&str]] = &[
        &[],
        &["gzip"],
        &["x-gzip"],
        &["GZip"],
        &["identity"],
        &["br"],
        &["*"],
        &["gzip;q=0"],
        &["gzip;q=0.000"],
        &["gzip;q=0.001"],
        &["gzip; q=0.5"],
        &["gzip;q=1.000"],
        &["gzip;q=1.5"],
        &["gzip;q=0.0001"],
        &["gzip;q=.5"],
        &["gzip;level=1"],
        &["br;q=1, gzip;q=0.8"],
        &["gzip;q=0, identity;q=1"],
        &["gzip;q=0.5, identity;q=1"],
        &["gzip;q=1, identity;q=0.5"],
        &["gzip, identity"],
        &["identity;q=0.5", "gzip"],
        &["gzip;q=0.5", "gzip;q=0"],
        &["deflate, br"],
    ];
    let layer = axum::Router::new()
        .fallback(|| async { "a body the default predicate compresses".repeat(10) })
        .layer(tower_http::compression::CompressionLayer::new());
    for values in cases {
        let mut request = Request::builder();
        let mut headers = HeaderMap::new();
        for value in *values {
            request = request.header(header::ACCEPT_ENCODING, *value);
            headers.append(header::ACCEPT_ENCODING, value.parse().unwrap());
        }
        let response = layer
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let layered = response.headers().get(header::CONTENT_ENCODING).is_some();
        assert_eq!(takes_gzip(&headers), layered, "{values:?}");
    }
}

#[tokio::test]
async fn hashed_files_are_cached_for_good_and_the_page_and_unhashed_files_revalidate() {
    let (_tree, app) = serving(&[
        (
            "index.html",
            b"<html><head></head><body>shell</body></html>",
        ),
        (
            "assets/board-f129bf6e05941e78.js",
            b"export const board = 1;",
        ),
        (
            "assets/frame-dxh2b1fd8a782b46c58.css",
            b"main { margin: 0 }",
        ),
        ("assets/plain.js", b"export const plain = 1;"),
    ]);
    for (path, cache) in [
        ("/assets/board-f129bf6e05941e78.js", FOR_GOOD),
        ("/assets/frame-dxh2b1fd8a782b46c58.css", FOR_GOOD),
        ("/assets/plain.js", "no-cache"),
        ("/", "no-cache"),
        ("/tasks", "no-cache"),
    ] {
        let got = send(&app, Method::GET, path, &[]).await;
        assert_eq!(got.status, StatusCode::OK, "{path}");
        assert_eq!(got.header("cache-control"), Some(cache), "{path}");
        let etag = got.header("etag").unwrap().to_owned();
        let again = send(&app, Method::GET, path, &[("If-None-Match", &etag)]).await;
        assert_eq!(again.status, StatusCode::NOT_MODIFIED, "{path}");
        assert!(again.body.is_empty(), "{path}");
        assert_eq!(again.header("cache-control"), Some(cache), "{path}");
    }
}

#[tokio::test]
async fn a_build_file_is_gzipped_on_its_first_request_and_sent_stored_after() {
    let source = module("gzipped_once", 300_000);
    let hash = sha256(&source);
    let (_tree, app) = serving(&[
        ("index.html", b"<html><body>shell</body></html>"),
        ("assets/symphony-ui_bg-dxhfeed01.wasm", &source),
    ]);
    let path = "/assets/symphony-ui_bg-dxhfeed01.wasm";
    let gzip = [("Accept-Encoding", "br;q=1, gzip;q=0.8")];

    let plain = send(&app, Method::GET, path, &[("Accept-Encoding", "identity")]).await;
    assert_eq!(plain.status, StatusCode::OK);
    assert_eq!(plain.body, source);
    assert_eq!(plain.header("content-encoding"), None);
    assert_eq!(plain.header("vary"), Some("accept-encoding"));
    assert_eq!(plain.header("content-type"), Some("application/wasm"));
    assert_eq!(
        runs(&hash),
        0,
        "a request that takes no gzip compresses nothing"
    );

    let first = send(&app, Method::GET, path, &gzip).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.header("content-encoding"), Some("gzip"));
    assert_eq!(first.header("vary"), Some("accept-encoding"));
    assert_eq!(first.header("cache-control"), Some(FOR_GOOD));
    assert_eq!(first.header("etag"), plain.header("etag"));
    assert_eq!(
        first.header("content-length"),
        Some(first.body.len().to_string().as_str())
    );
    assert!(first.body.len() < source.len() / 10, "{}", first.body.len());
    assert_eq!(gunzip(&first.body), source);
    assert_eq!(runs(&hash), 1);

    let second = send(&app, Method::GET, path, &gzip).await;
    assert_eq!(second.body, first.body);
    assert_eq!(second.headers, first.headers);
    let head = send(&app, Method::HEAD, path, &gzip).await;
    assert!(head.body.is_empty());
    assert_eq!(head.headers, first.headers);
    let cached = send(
        &app,
        Method::GET,
        path,
        &[gzip[0], ("If-None-Match", first.header("etag").unwrap())],
    )
    .await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert_eq!(cached.header("vary"), Some("accept-encoding"));
    assert_eq!(runs(&hash), 1, "a later request does no compression work");
}

#[tokio::test]
async fn requests_arriving_together_wait_for_one_compression() {
    let source = module("compressed_together", 200_000);
    let hash = sha256(&source);
    let (_tree, app) = serving(&[
        ("index.html", b"<html><body>shell</body></html>"),
        ("assets/panel-0123456789abcdef.js", &source),
    ]);
    let path = "/assets/panel-0123456789abcdef.js";
    let gzip = [("Accept-Encoding", "gzip")];
    let (one, two, three) = tokio::join!(
        send(&app, Method::GET, path, &gzip),
        send(&app, Method::GET, path, &gzip),
        send(&app, Method::GET, path, &gzip),
    );
    for got in [&one, &two, &three] {
        assert_eq!(got.header("content-encoding"), Some("gzip"));
        assert_eq!(got.body, one.body);
    }
    assert_eq!(gunzip(&one.body), source);
    assert_eq!(runs(&hash), 1);
}

#[tokio::test]
async fn a_file_gzip_cannot_shrink_goes_as_it_is_and_is_not_tried_again() {
    let source = b"x=1;".to_vec();
    let hash = sha256(&source);
    let (_tree, app) = serving(&[
        ("index.html", b"<html><body>shell</body></html>"),
        ("assets/tiny-dxhabc.js", &source),
    ]);
    for _ in 0..2 {
        let got = send(
            &app,
            Method::GET,
            "/assets/tiny-dxhabc.js",
            &[("Accept-Encoding", "gzip")],
        )
        .await;
        assert_eq!(got.body, source);
        assert_eq!(got.header("content-encoding"), None);
        assert_eq!(got.header("vary"), Some("accept-encoding"));
    }
    assert_eq!(runs(&hash), 1);
}

#[tokio::test]
async fn pictures_and_fonts_go_as_they_are_and_the_page_is_still_compressed_per_request() {
    let picture = module("a picture stands in", 4_000);
    let font = module("a font stands in", 4_000);
    let page = format!("<html><body>{}</body></html>", "Native shell. ".repeat(300));
    let (_tree, app) = serving(&[
        ("index.html", page.as_bytes()),
        ("assets/icon-dxh1.png", &picture),
        ("assets/face-dxh2.woff2", &font),
    ]);
    let gzip = [("Accept-Encoding", "gzip")];
    for (path, bytes, kind) in [
        ("/assets/icon-dxh1.png", &picture, "image/png"),
        ("/assets/face-dxh2.woff2", &font, "font/woff2"),
    ] {
        let got = send(&app, Method::GET, path, &gzip).await;
        assert_eq!(got.header("content-type"), Some(kind), "{path}");
        assert_eq!(got.header("content-encoding"), None, "{path}");
        assert_eq!(got.header("vary"), None, "{path}");
        assert_eq!(&got.body, bytes, "{path}");
        assert_eq!(runs(&sha256(bytes)), 0, "{path}");
    }
    let shell = send(&app, Method::GET, "/", &gzip).await;
    assert_eq!(shell.header("content-encoding"), Some("gzip"));
    assert_eq!(shell.header("cache-control"), Some("no-cache"));
    let shell = String::from_utf8(gunzip(&shell.body)).unwrap();
    assert!(shell.contains("Native shell."), "{shell}");
}
