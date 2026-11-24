//! The client's build as the router serves it: a content-hashed file cached for good and the rest
//! revalidated, each build file gzipped once however often it is asked for, and Accept-Encoding
//! read as the router's compression layer reads it.

use super::gzipped::{Store, runs, takes_gzip};
use super::*;
use crate::front::router;
use crate::testing::Tree;
use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::Method;
use std::io::Write;
use std::net::SocketAddr;
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
    // An install folder fixes the client to these files, whatever SYMPHONY_UI_DIST names.
    let shared = Shared::new(
        1,
        1,
        &tree.path("repo"),
        Some(&tree.path("install")),
        crate::testing::open_gate(),
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
fn a_content_hash_names_a_builds_assets_and_nothing_else() {
    for hashed in [
        "assets/board-f129bf6e05941e78.js",
        "assets/symphony-shell-4375f62f92e3d4a2.js",
        "assets/theme-3f7454274acd8235.css",
        "assets/chunks/old-789789789789789a.js",
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
        "assets/board-123.js",
        "assets/board-.js",
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
fn every_file_the_server_serves_under_assets_is_cached_for_good() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("client/public/assets");
    let mut names = 0;
    for entry in std::fs::read_dir(assets).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(content_hashed(&format!("assets/{name}")), "{name}");
        names += 1;
    }
    assert!(names > 0, "the server serves no client assets");
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

/// An HTML document's route is the app's page whatever headers ask for it: a page's load, a
/// frame's, a fetch, a browser's that sends no fetch metadata (any plain-http address but
/// loopback's), and a query that is not the file's. Only the file's own address is the file
/// (`symphony_api::file_address`; its answer is tested through the machine's routing). Nothing
/// says the page varies by a header, since none decides it.
#[tokio::test]
async fn an_html_documents_route_is_the_app_whatever_headers_ask_for_it() {
    let (_tree, app) = serving(&[(
        "index.html",
        b"<html><head></head><body>shell</body></html>",
    )]);
    let page_load = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
    for (path, headers) in [
        ("/docs/analysis/readout.html", &[][..]),
        ("/docs/analysis/readout.html", &[("Accept", page_load)]),
        (
            "/docs/analysis/readout.HTM",
            &[("Sec-Fetch-Dest", "document")],
        ),
        (
            "/docs/analysis/readout.html",
            &[("Sec-Fetch-Dest", "iframe")],
        ),
        (
            "/docs/analysis/readout.html",
            &[("Sec-Fetch-Dest", "empty")],
        ),
        ("/docs/analysis/readout.html?rawness=1", &[]),
        ("/docs/analysis/readout.md", &[]),
    ] {
        let got = send(&app, Method::GET, path, headers).await;
        assert_eq!(got.status, StatusCode::OK, "{path} {headers:?}");
        let page = String::from_utf8_lossy(&got.body);
        assert!(
            page.ends_with("shell</body></html>"),
            "{path} {headers:?}: {page}"
        );
        let vary: Vec<String> = got
            .headers
            .get_all(header::VARY)
            .iter()
            .map(|value| value.to_str().unwrap().to_ascii_lowercase())
            .collect();
        assert!(
            !vary.iter().any(|value| value.contains("sec-fetch")),
            "{path}: {vary:?}"
        );
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
        ("assets/frame-2b1fd8a782b46c58.css", b"main { margin: 0 }"),
        ("assets/plain.js", b"export const plain = 1;"),
    ]);
    for (path, cache) in [
        ("/assets/board-f129bf6e05941e78.js", FOR_GOOD),
        ("/assets/frame-2b1fd8a782b46c58.css", FOR_GOOD),
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
        ("assets/program-feed01feed01feed.wasm", &source),
    ]);
    let path = "/assets/program-feed01feed01feed.wasm";
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

/// A store keyed by the file's name would send the old gzip here, under the new ETag.
#[tokio::test]
async fn a_file_rewritten_under_its_name_is_gzipped_anew() {
    let before = module("rewritten_before", 100_000);
    let after = module("rewritten_after", 100_000);
    let name = "assets/review-fedcba9876543210.js";
    let (tree, app) = serving(&[
        ("index.html", b"<html><body>shell</body></html>"),
        (name, &before),
    ]);
    let path = format!("/{name}");
    let gzip = [("Accept-Encoding", "gzip")];

    let first = send(&app, Method::GET, &path, &gzip).await;
    assert_eq!(first.header("content-encoding"), Some("gzip"));
    assert_eq!(gunzip(&first.body), before);
    std::fs::write(tree.path(&format!("install/client/public/{name}")), &after).unwrap();
    let second = send(&app, Method::GET, &path, &gzip).await;

    let etag = format!("\"{}\"", sha256(&after));
    assert_eq!(second.header("etag"), Some(etag.as_str()));
    assert_ne!(second.header("etag"), first.header("etag"));
    assert_eq!(second.header("content-encoding"), Some("gzip"));
    assert_eq!(gunzip(&second.body), after);
    assert_eq!(runs(&sha256(&before)), 1);
    assert_eq!(
        runs(&sha256(&after)),
        1,
        "the new content takes one more compression"
    );
}

/// Three contents whose gzips the store can hold two of, each with its key and the length its
/// gzip counts.
async fn three_over_a_cap_of_two() -> ([(String, Bytes); 3], usize) {
    let contents = ["dropped_a", "dropped_b", "dropped_c"].map(|name| {
        let bytes = Bytes::from(module(name, 50_000));
        (sha256(&bytes), bytes)
    });
    let measure = Store::new(usize::MAX);
    let mut total = 0;
    for (hash, bytes) in &contents {
        total += measure.gzipped(hash, bytes).await.unwrap().len() + hash.len();
    }
    (contents, total - 1)
}

#[tokio::test]
async fn past_its_cap_the_store_drops_the_gzip_asked_for_least_recently() {
    let ([(a, a_bytes), (b, b_bytes), (c, c_bytes)], cap) = three_over_a_cap_of_two().await;
    let store = Store::new(cap);
    let a_zipped = store.gzipped(&a, &a_bytes).await.unwrap();
    store.gzipped(&b, &b_bytes).await.unwrap();
    // Asked for again, `a` is now more recent than `b`, though it was made first.
    assert_eq!(store.gzipped(&a, &a_bytes).await.unwrap(), a_zipped);
    store.gzipped(&c, &c_bytes).await.unwrap();
    assert!(
        store.kept() <= cap,
        "{} kept over a cap of {cap}",
        store.kept()
    );

    assert_eq!(store.gzipped(&a, &a_bytes).await.unwrap(), a_zipped);
    assert_eq!(store.runs(&a), 1, "the most recently asked for stays");
    let b_again = store.gzipped(&b, &b_bytes).await.unwrap();
    assert_eq!(gunzip(&b_again), b_bytes);
    assert_eq!(
        store.runs(&b),
        2,
        "a content dropped past the cap is compressed again"
    );
    // Making `b` again dropped `c`, now the least recently asked for.
    store.gzipped(&c, &c_bytes).await.unwrap();
    assert_eq!(store.runs(&c), 2);
    assert!(
        store.kept() <= cap,
        "{} kept over a cap of {cap}",
        store.kept()
    );
}

#[tokio::test]
async fn a_gzip_larger_than_the_whole_cap_is_sent_and_not_kept() {
    let source = Bytes::from(module("over_the_cap", 50_000));
    let hash = sha256(&source);
    let store = Store::new(100);
    for runs in 1..=2 {
        let zipped = store.gzipped(&hash, &source).await.unwrap();
        assert_eq!(gunzip(&zipped), source);
        assert_eq!(store.kept(), 0);
        assert_eq!(store.runs(&hash), runs);
    }
}

#[tokio::test]
async fn a_file_gzip_cannot_shrink_goes_as_it_is_and_is_not_tried_again() {
    let source = b"x=1;".to_vec();
    let hash = sha256(&source);
    let (_tree, app) = serving(&[
        ("index.html", b"<html><body>shell</body></html>"),
        ("assets/tiny-abcabcabcabcabca.js", &source),
    ]);
    for _ in 0..2 {
        let got = send(
            &app,
            Method::GET,
            "/assets/tiny-abcabcabcabcabca.js",
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
        ("assets/icon-1111111111111111.png", &picture),
        ("assets/face-2222222222222222.woff2", &font),
    ]);
    let gzip = [("Accept-Encoding", "gzip")];
    for (path, bytes, kind) in [
        ("/assets/icon-1111111111111111.png", &picture, "image/png"),
        ("/assets/face-2222222222222222.woff2", &font, "font/woff2"),
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

#[tokio::test]
async fn page_preloads_follow_the_named_shell_even_with_a_newer_build_kept_beside_it() {
    let html = b"<html><head></head><body><script type=\"module\" src=\"/assets/symphony-shell-1111111111111111.js\"></script></body></html>";
    let script = br#"const modules = {"review":{"imports":["/assets/home-1111111111111111.js"]},"board":{"imports":["/assets/board-1111111111111111.js"]},"docs":{"imports":["/assets/docs-1111111111111111.js"]},"task":{"imports":["/assets/task-1111111111111111.js"]}};
"#;
    let (_tree, app) = serving(&[
        ("index.html", html),
        ("assets/symphony-shell-1111111111111111.js", script),
        (
            "assets/symphony-shell-2222222222222222.js",
            b"const modules = {};\n",
        ),
        (
            "builds.json",
            b"{\"builds\":[{\"shell\":\"symphony-shell-2222222222222222.js\"}]}",
        ),
    ]);
    for (path, kind) in [
        ("/", "home"),
        ("/tasks", "board"),
        ("/task/", "board"),
        ("/task/tools/x", "task"),
        ("/docs/page.md", "docs"),
        ("/docs/page.html", "docs"),
    ] {
        let got = send(&app, Method::GET, path, &[]).await;
        assert_eq!(got.status, StatusCode::OK);
        let page = std::str::from_utf8(&got.body).unwrap();
        let link =
            format!("<link rel=\"modulepreload\" href=\"/assets/{kind}-1111111111111111.js\">");
        assert!(page.contains(&format!("{link}</head>")), "{path}: {page}");
        assert_eq!(page.matches("rel=\"modulepreload\"").count(), 1);
        assert_eq!(got.header("cache-control"), Some("no-cache"));
        let cached = send(
            &app,
            Method::GET,
            path,
            &[("If-None-Match", got.header("etag").unwrap())],
        )
        .await;
        assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
        assert!(cached.body.is_empty());
    }
    let dev = send(&app, Method::GET, "/tasks", &[("x-symphony-dev", "1")]).await;
    assert!(
        !std::str::from_utf8(&dev.body)
            .unwrap()
            .contains("modulepreload")
    );
    let shell = send(
        &app,
        Method::GET,
        "/assets/symphony-shell-1111111111111111.js",
        &[],
    )
    .await;
    assert_eq!(shell.body, script.as_slice());
}

#[tokio::test]
async fn absent_or_older_shells_and_heads_do_not_prevent_serving_the_page() {
    for (html, script) in [
        ("<html><head></head><body>old shell</body></html>", None),
        (
            "<html><head></head><script src=\"/assets/symphony-shell-1111111111111111.js\"></script></html>",
            None,
        ),
        (
            "<html><head></head><script src=\"/assets/symphony-shell-1111111111111111.js\"></script></html>",
            Some("// old shell"),
        ),
        (
            "<html><script src=\"/assets/symphony-shell-1111111111111111.js\"></script></html>",
            Some(
                "const modules = {\"review\":{\"imports\":[\"/assets/chunk-1111111111111111.js\"]}};",
            ),
        ),
    ] {
        let mut files = vec![("index.html", html.as_bytes())];
        if let Some(script) = script {
            files.push((
                "assets/symphony-shell-1111111111111111.js",
                script.as_bytes(),
            ));
        }
        let (_tree, app) = serving(&files);
        let got = send(&app, Method::GET, "/", &[]).await;
        assert_eq!(got.status, StatusCode::OK);
        assert!(
            !std::str::from_utf8(&got.body)
                .unwrap()
                .contains("modulepreload")
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_shell_symlink_outside_the_bundle_does_not_contribute_preloads() {
    let (tree, app) = serving(&[("index.html", b"<html><head></head><script src=\"/assets/symphony-shell-1111111111111111.js\"></script></html>")]);
    tree.write(
        "outside.js",
        "const modules = {\"review\":{\"imports\":[\"/assets/private-1111111111111111.js\"]}};",
    );
    std::fs::create_dir_all(tree.path("install/client/public/assets")).unwrap();
    std::os::unix::fs::symlink(
        tree.path("outside.js"),
        tree.path("install/client/public/assets/symphony-shell-1111111111111111.js"),
    )
    .unwrap();
    let got = send(&app, Method::GET, "/", &[]).await;
    assert_eq!(got.status, StatusCode::OK);
    assert!(
        !std::str::from_utf8(&got.body)
            .unwrap()
            .contains("modulepreload")
    );
}

/// The project view a served page carries.
fn carried(page: &str) -> symphony_api::ProjectView {
    let open = format!(
        "<script type=\"application/json\" id=\"{}\">",
        symphony_api::PROJECT_ELEMENT
    );
    let start = page.find(&open).expect("the page carries the project") + open.len();
    let end = start + page[start..].find("</script>").unwrap();
    serde_json::from_str(&page[start..end].replace("\\u003c", "<")).unwrap()
}

/// The page names whom it is served to, as the gate admitted them, and the person at the machine
/// by the name their profile gives them: `owner` and nobody to draw before one is given.
#[tokio::test]
async fn the_page_names_its_reader_and_the_person_at_the_machine_as_their_profile_does() {
    let tree = Tree::new();
    let shell = b"<html><head></head><body>shell</body></html>";
    let path = tree.path("install/client/public/index.html");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, shell).unwrap();
    std::fs::create_dir_all(tree.path("repo")).unwrap();
    let machine = tree.path("machine");
    let serve = || {
        let gate = crate::testing::open_gate().with_machine(machine.clone());
        let shared = Shared::new(1, 1, &tree.path("repo"), Some(&tree.path("install")), gate);
        let loopback: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        router(shared).layer(MockConnectInfo(loopback))
    };
    let got = send(&serve(), Method::GET, "/docs/README.md", &[]).await;
    let view = carried(&String::from_utf8_lossy(&got.body));
    let owner = symphony_api::PersonName {
        name: "owner".into(),
        shown: "owner".into(),
    };
    assert_eq!((view.you, view.machine), (Some(owner), None));
    symphony_board::people::Profiles {
        people: vec![symphony_board::people::Profile {
            name: "stefano".into(),
            display: "Stefano".into(),
            ..Default::default()
        }],
        machine: Some("stefano".into()),
    }
    .write(&machine)
    .unwrap();
    let got = send(&serve(), Method::GET, "/docs/README.md", &[]).await;
    let view = carried(&String::from_utf8_lossy(&got.body));
    let named = symphony_api::PersonName {
        name: "stefano".into(),
        shown: "Stefano".into(),
    };
    assert_eq!((view.you, view.machine), (Some(named.clone()), Some(named)));
}
