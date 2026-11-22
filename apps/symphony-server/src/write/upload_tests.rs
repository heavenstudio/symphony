//! The upload route and the stored files' route through the real router: what is stored, what
//! is refused before anything is written, and the one way each stored kind is served back.

use crate::front::{Shared, router};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use futures_util::StreamExt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03";
const HOST: &str = "127.0.0.1:8123";

struct Fixture {
    root: PathBuf,
    app: axum::Router,
}

impl Fixture {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "symphony-upload-route-{}-{test}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let root = root.canonicalize().unwrap();
        let gate = crate::testing::open_gate();
        let shared = Shared::new(1, 8123, &root, None, gate);
        let app = router(shared).layer(MockConnectInfo(
            "127.0.0.1:5000".parse::<SocketAddr>().unwrap(),
        ));
        Self { root, app }
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, headers, body)
    }

    fn upload(name: &str, bytes: &[u8]) -> Request<Body> {
        Request::post(format!("/api/upload?name={name}"))
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("http://{HOST}"))
            .body(Body::from(bytes.to_vec()))
            .unwrap()
    }

    async fn stored(&self, name: &str, bytes: &[u8]) -> serde_json::Value {
        let (status, headers, body) = self.send(Self::upload(name, bytes)).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        serde_json::from_slice(&body).unwrap()
    }

    async fn get(&self, path: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        self.send(
            Request::get(path)
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    fn files(&self) -> Vec<String> {
        let mut files: Vec<_> = std::fs::read_dir(symphony::uploads::dir(&self.root))
            .map(|dir| {
                dir.map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn a_pasted_picture_is_stored_once_and_read_back_in_a_script_free_sandbox() {
    let f = Fixture::new("picture");
    let upload = f.stored("Screen%20Shot%201.png", PNG).await;
    let url = upload["url"].as_str().unwrap().to_string();
    let sha = url
        .strip_prefix("/artifacts/uploads/")
        .and_then(|file| file.strip_suffix(".png"))
        .unwrap()
        .to_string();
    assert_eq!(upload["name"], "Screen Shot 1.png");
    assert_eq!(upload["picture"], true);
    assert_eq!(upload["bytes"], PNG.len());
    assert_eq!(upload["markup"], format!("![Screen Shot 1.png]({url})"));
    assert_eq!(f.files(), [format!("{sha}.png")]);
    let (status, headers, body) = f.get(&url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers[header::CONTENT_DISPOSITION], "inline");
    assert_eq!(
        headers[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let policy = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(policy.starts_with("sandbox;"), "{policy}");
    assert!(!policy.contains("allow-scripts"), "{policy}");
    assert!(policy.contains("default-src 'none'"), "{policy}");
    assert_eq!(headers[header::ETAG], format!("\"{sha}\""));
    let (status, headers, body) = f
        .send(
            Request::get(&url)
                .header(header::HOST, HOST)
                .header(header::IF_NONE_MATCH, format!("\"{sha}\""))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    // The same bytes again share the file; another name is only the new reference's.
    let again = f.stored("again.png", PNG).await;
    assert_eq!(again["url"], url.as_str());
    assert_eq!(again["name"], "again.png");
    assert_eq!(f.files().len(), 1);
}

#[tokio::test]
async fn documents_are_served_as_their_one_type_in_the_sandbox_and_pdf_and_office_files_download() {
    let f = Fixture::new("documents");
    for (name, bytes, mime, disposition) in [
        (
            "notes.txt",
            b"plain words, a < b".as_slice(),
            "text/plain; charset=utf-8",
            "inline",
        ),
        (
            "brief.pdf",
            b"%PDF-1.7\n".as_slice(),
            "application/pdf",
            "attachment",
        ),
        (
            "plan.docx",
            b"PK\x03\x04word".as_slice(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "attachment",
        ),
    ] {
        let upload = f.stored(name, bytes).await;
        assert_eq!(upload["picture"], false, "{name}");
        assert_eq!(
            upload["markup"],
            format!("[{name}]({})", upload["url"].as_str().unwrap())
        );
        let (status, headers, body) = f.get(upload["url"].as_str().unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        assert_eq!(body, bytes, "{name}");
        assert_eq!(headers[header::CONTENT_TYPE], mime, "{name}");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{name}");
        assert_eq!(headers[header::CONTENT_DISPOSITION], disposition, "{name}");
        assert_eq!(
            headers
                .get(header::CONTENT_SECURITY_POLICY)
                .and_then(|policy| policy.to_str().ok()),
            Some("sandbox; default-src 'none'; img-src 'self'; style-src 'unsafe-inline'"),
            "{name}"
        );
    }
}

#[tokio::test]
async fn markup_scripts_empty_files_and_files_past_the_cap_are_refused_before_storing() {
    let f = Fixture::new("refusals");
    for (name, bytes, status) in [
        (
            "vector.svg",
            b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice(),
            415,
        ),
        (
            "page.html",
            b"<!doctype html><script></script>".as_slice(),
            415,
        ),
        ("app.js", b"alert(1)".as_slice(), 415),
        (
            "notes.txt",
            b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice(),
            415,
        ),
        (
            "README.md",
            b"<!doctype html><script></script>".as_slice(),
            415,
        ),
        ("run.log", b"#!/bin/sh\necho hi".as_slice(), 415),
        ("empty.png", b"".as_slice(), 400),
    ] {
        let (got, _, body) = f.send(Fixture::upload(name, bytes)).await;
        assert_eq!(
            got.as_u16(),
            status,
            "{name}: {}",
            String::from_utf8_lossy(&body)
        );
        let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(error["error"].is_string(), "{error}");
    }
    let max = symphony::uploads::MAX_BYTES;
    // A declared length past the cap is refused before the body is read.
    let (status, _, body) = f
        .send(
            Request::post("/api/upload?name=big.txt")
                .header(header::HOST, HOST)
                .header(header::CONTENT_LENGTH, (max + 1).to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        String::from_utf8_lossy(&body).contains("uploads are at most 10 MB"),
        "{}",
        String::from_utf8_lossy(&body)
    );
    // A body with no length stops being read at the chunk that passes the cap: the next one is
    // never asked for, so a body past the cap is never held.
    let asked_past = Arc::new(AtomicBool::new(false));
    let past = asked_past.clone();
    let chunks = futures_util::stream::iter(
        [vec![b'a'; max], vec![b'a'; 1]]
            .into_iter()
            .map(Ok::<_, std::io::Error>),
    )
    .chain(futures_util::stream::once(async move {
        past.store(true, Ordering::SeqCst);
        Err(std::io::Error::other("read past the cap"))
    }));
    let (status, _, body) = f
        .send(
            Request::post("/api/upload?name=big.txt")
                .header(header::HOST, HOST)
                .body(Body::from_stream(chunks))
                .unwrap(),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        !asked_past.load(Ordering::SeqCst),
        "the chunk after the cap was read"
    );
    // A body that breaks off stores nothing.
    let broken = futures_util::stream::iter([
        Ok(PNG.to_vec()),
        Err(std::io::Error::other("the phone lost its signal")),
    ]);
    let (status, _, body) = f
        .send(
            Request::post("/api/upload?name=cut.png")
                .header(header::HOST, HOST)
                .body(Body::from_stream(broken))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("the upload broke off"),
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(f.files().is_empty());
    // Exactly the cap is stored.
    let at_cap = f.stored("big.txt", &vec![b'a'; max]).await;
    assert_eq!(at_cap["bytes"], max);
    assert_eq!(f.files().len(), 1, "only the file at the cap was stored");
}

#[tokio::test]
async fn an_upload_keeps_the_write_host_and_origin_checks() {
    let f = Fixture::new("origin");
    for (host, origin) in [
        ("foreign.example", None),
        ("", None),
        (HOST, Some("http://foreign.example")),
        (HOST, Some("null")),
        (HOST, Some("https://127.0.0.1:8123")),
    ] {
        let mut request = Request::post("/api/upload?name=a.png").header(header::HOST, host);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let (status, _, _) = f
            .send(request.body(Body::from(PNG.to_vec())).unwrap())
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host} {origin:?}");
    }
    assert!(f.files().is_empty(), "a refused write stored nothing");
    let (status, _, _) = f
        .send(
            Request::get("/api/upload")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_ne!(status, StatusCode::OK, "only POST stores");
    let (status, _, _) = f
        .send(
            Request::post("/api/upload?name=a.png")
                .header(header::HOST, HOST)
                .body(Body::from(PNG.to_vec()))
                .unwrap(),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a write with no Origin is a local tool's"
    );
}

#[tokio::test]
async fn only_a_stored_name_under_the_uploads_folder_is_ever_served() {
    let f = Fixture::new("names");
    let none_yet = format!("/artifacts/uploads/{}.png", "e".repeat(64));
    let (status, _, _) = f.get(&none_yet).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no upload folder yet");
    let upload = f.stored("a.png", PNG).await;
    let url = upload["url"].as_str().unwrap();
    let folder = symphony::uploads::dir(&f.root);
    let sha = "b".repeat(64);
    std::fs::write(folder.join(format!("{sha}.svg")), "<svg/>").unwrap();
    std::fs::write(folder.join(format!("{sha}.html")), "<html>").unwrap();
    std::fs::create_dir(folder.join(format!("{sha}.png"))).unwrap();
    std::fs::write(f.root.join("docs/secret.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(
        f.root.join("docs/secret.txt"),
        folder.join(format!("{}.txt", "c".repeat(64))),
    )
    .unwrap();
    let unreadable = folder.join(format!("{}.gif", "f".repeat(64)));
    std::fs::write(&unreadable, "GIF89a").unwrap();
    std::fs::set_permissions(
        &unreadable,
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    for path in [
        format!("/artifacts/uploads/{sha}.svg"),
        format!("/artifacts/uploads/{sha}.html"),
        format!("/artifacts/uploads/{sha}.png"),
        format!("/artifacts/uploads/{}.txt", "c".repeat(64)),
        format!("/artifacts/uploads/{}.png", "d".repeat(64)),
        format!("/artifacts/uploads/{}.gif", "f".repeat(64)),
        url.replace(".png", ".PNG"),
        "/artifacts/uploads/".to_string(),
        "/artifacts/uploads/a.png".to_string(),
        "/artifacts/uploads/%2E%2E/%2E%2E/docs/secret.txt".to_string(),
    ] {
        let (status, _, _) = f.get(&path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    let (status, _, body) = f.get(url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
}
