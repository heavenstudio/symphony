//! Project media and the vendored viewers, served by the native server. Raw documents (including
//! SVG/XML and mockups opened full size) keep the frame's opaque-origin sandbox in their response.
//! A Markdown file no entrypoint serves (a guide such as `AGENTS.md`) opened as a page is drawn
//! read-only in the project's look instead, its headings' ids a document's, so `#anchor` lands, in
//! the same sandbox. A fetch of it gets the file.

use std::path::{Component, Path};

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(crate) fn handles(path: &str) -> bool {
    path == "/favicon.ico" || path.starts_with("/static/") || mime(path).is_some()
}

/// What a text file is served as: Markdown, source and settings alike.
const TEXT: &str = "text/plain; charset=utf-8";

/// Whether `request` opens a page: a link followed or an address typed (`Sec-Fetch-Dest:
/// document`), or, from a browser that sends no fetch metadata, a request asking for HTML. A fetch,
/// a picture or a frame is none.
fn navigation(request: &axum::http::Request<()>) -> bool {
    let headers = request.headers();
    match headers.get("sec-fetch-dest") {
        Some(destination) => destination == "document",
        None => headers
            .get(header::ACCEPT)
            .and_then(|accept| accept.to_str().ok())
            .is_some_and(|accept| accept.contains("text/html")),
    }
}

fn mime(path: &str) -> Option<&'static str> {
    Some(
        match Path::new(path)
            .extension()?
            .to_str()?
            .to_ascii_lowercase()
            .as_str()
        {
            "html" | "htm" => "text/html; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "js" | "mjs" => "text/javascript; charset=utf-8",
            "svg" => "image/svg+xml",
            "xml" => "application/xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "ico" => "image/x-icon",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "gltf" | "json" => "application/json",
            "md" | "txt" | "toml" | "yaml" | "yml" | "rs" | "gd" | "sh" => TEXT,
            "csv" => "text/csv; charset=utf-8",
            "bin" => "application/octet-stream",
            "wav" => "audio/wav",
            "mp3" => "audio/mpeg",
            "ogg" => "audio/ogg",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            _ => return None,
        },
    )
}

/// The type the file `path` of the project at `root` is served as ([`served_type`]).
fn content_type(root: &Path, path: &str) -> &'static str {
    let plugins = symphony::plugins::Plugins::of(root);
    served_type(symphony::renderers::content_type(&plugins, path), path)
}

/// The type a file is served as: the one the renderer that draws it names (`drawn`), whatever its
/// extension's own kind, else that kind's, else bytes. The Symphony's pictures and mockups are
/// never a renderer's, so they keep their own.
fn served_type(drawn: Option<&'static str>, path: &str) -> &'static str {
    drawn
        .or_else(|| mime(path))
        .unwrap_or("application/octet-stream")
}

/// The sandbox every project file is served in, the frame's: its scripts run in an opaque origin,
/// with no reach into the app's.
const FILE_POLICY: &str = "sandbox allow-scripts";

/// The script-free sandbox every stored upload is served in, whether it opens or downloads: no
/// script, plugin, form or navigation, nothing loaded but itself.
const UPLOAD_POLICY: &str =
    "sandbox; default-src 'none'; img-src 'self'; style-src 'unsafe-inline'";

/// An owner upload (`symphony::uploads`), read from the server's own checkout whatever the page's
/// branch. Only `<sha256>.<ext>` of a stored kind is served, with that kind's one type, `nosniff`
/// and the script-free sandbox: never as markup or script. Pictures and text open in the browser;
/// a PDF or an Office file downloads. Content-addressed, so cached for good.
pub(crate) async fn upload(
    shared: &crate::front::Shared,
    file: &str,
    request: &axum::http::Request<()>,
) -> Response {
    let Some((path, kind)) = symphony::uploads::path(&shared.root, file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(root) = tokio::fs::canonicalize(&shared.root).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(folder) = tokio::fs::canonicalize(symphony::uploads::dir(&root)).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(path) = tokio::fs::canonicalize(path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !folder.starts_with(&root) || !path.starts_with(&folder) || shared.is_metadata(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(opened) = tokio::fs::File::open(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(stat) = opened.metadata().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !stat.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let etag = format!("\"{}\"", file.split('.').next().unwrap_or(file));
    let mut response = if crate::api::matches_etag(request, &etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let body = crate::file_body::stream(opened, stat.len());
        let mut response = body.into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, stat.len().into());
        response
    };
    let headers = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_TYPE, kind.mime),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        (header::ETAG, etag.as_str()),
        (
            header::CONTENT_DISPOSITION,
            if kind.inline { "inline" } else { "attachment" },
        ),
        (header::CONTENT_SECURITY_POLICY, UPLOAD_POLICY),
    ] {
        headers.insert(name, value.parse().expect("upload response header"));
    }
    response
}

pub(crate) async fn answer(
    shared: &crate::front::Shared,
    root: &Path,
    path: &str,
    request: &axum::http::Request<()>,
) -> Response {
    let file;
    let (base, relative) = if path == "/favicon.ico" {
        (root.join("tools/static"), "icon.svg")
    } else if let Some(relative) = path.strip_prefix("/static/") {
        (root.join("tools/static"), relative)
    } else {
        // A file beside a document is named by its route, as the document is (`docs/…`).
        file = symphony::project::Project::of(root).file(path.trim_start_matches('/'));
        (root.to_path_buf(), file.as_str())
    };
    if relative.is_empty()
        || Path::new(relative)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    // Resolve both roots: a source tree may itself be a symlink, but the file cannot leave it,
    // nor may the static folder point outside the served checkout.
    let Ok(root) = tokio::fs::canonicalize(root).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(base) = tokio::fs::canonicalize(base).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !base.starts_with(&root) {
        return StatusCode::NOT_FOUND.into_response();
    }
    contained(shared, &root, &base, &base.join(relative), request).await
}

/// The file `wanted` as served media when it resolves inside `base` (canonical) and outside the
/// repository's metadata, its type read from its own name and the plugins of the project at
/// `root`; not found otherwise.
pub(crate) async fn contained(
    shared: &crate::front::Shared,
    root: &Path,
    base: &Path,
    wanted: &Path,
    request: &axum::http::Request<()>,
) -> Response {
    let Ok(path) = tokio::fs::canonicalize(wanted).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !path.starts_with(base) || shared.is_metadata(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(file) = tokio::fs::File::open(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(stat) = file.metadata().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !stat.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let kind = content_type(root, &wanted.to_string_lossy());
    if kind == TEXT
        && path.extension().is_some_and(|extension| extension == "md")
        && navigation(request)
        && let Ok(text) = tokio::fs::read_to_string(&path).await
    {
        let name = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        let html = symphony::render::plain_html(&text);
        let mut page = crate::pages::html(
            StatusCode::OK,
            crate::pages::file(root, &shared.base, &name, &html),
        );
        for (name, value) in [
            (header::CONTENT_SECURITY_POLICY, FILE_POLICY),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ] {
            page.headers_mut()
                .insert(name, value.parse().expect("file page header"));
        }
        return page;
    }
    let (etag, cache) =
        symphony::assets::cache_headers(&path, request.uri().query().unwrap_or_default());
    let cached = crate::api::matches_etag(request, &etag);
    let mut response = if cached {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let body = crate::file_body::stream(file, stat.len());
        let mut response = body.into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, stat.len().into());
        response
    };
    for (name, value) in [
        (header::CONTENT_TYPE, kind),
        (header::CONTENT_SECURITY_POLICY, FILE_POLICY),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::CACHE_CONTROL, cache),
        (header::ETAG, etag.as_str()),
    ] {
        response
            .headers_mut()
            .insert(name, value.parse().expect("media response header"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{answer, content_type, handles, served_type};
    use crate::testing::Tree;
    use axum::http::{Request, header};

    /// A Markdown file no entrypoint serves, opened as a page, is drawn under the project's header
    /// with its headings' anchors, in the sandbox every project file has; fetched, it is the file,
    /// as any other file is however it is asked for.
    #[tokio::test]
    async fn a_markdown_file_opened_as_a_page_is_drawn_and_fetched_is_the_file() {
        let tree = Tree::new();
        let root = crate::testing::repository(&tree);
        let guide = "# Guide\n\n## Command ownership\n\nSee [docs](docs/TOPIC.md).\n\n<script>x()</script>\n";
        std::fs::write(root.join("GUIDE.md"), guide).unwrap();
        std::fs::write(root.join("notes.txt"), "plain notes").unwrap();
        let gate = crate::gate::Gate::new(
            std::sync::Arc::new(|_| Ok(None)),
            std::sync::Arc::new(std::time::Instant::now),
        );
        let shared = crate::front::Shared::project(1, 1234, &root, None, gate, "/alpha");
        let ask = |path: &str, headers: &[(&str, &str)]| {
            let mut request = Request::builder().uri(path);
            for (name, value) in headers {
                request = request.header(*name, *value);
            }
            let request = request.body(()).unwrap();
            let (shared, root, path) = (shared.clone(), root.clone(), path.to_string());
            async move {
                let response = answer(&shared, &root, &path, &request).await;
                let head = response.headers().clone();
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let said = |name| {
                    head.get(name)
                        .map_or("", |v| v.to_str().unwrap())
                        .to_string()
                };
                (
                    said(header::CONTENT_TYPE),
                    said(header::CONTENT_SECURITY_POLICY),
                    String::from_utf8(body.to_vec()).unwrap(),
                )
            }
        };
        let opened = [
            &[("Sec-Fetch-Dest", "document"), ("Accept", "text/html")][..],
            &[("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")],
        ];
        for headers in opened {
            let (kind, policy, page) = ask("/GUIDE.md", headers).await;
            assert_eq!(
                (kind.as_str(), policy.as_str()),
                ("text/html; charset=utf-8", super::FILE_POLICY)
            );
            for drawn in [
                "<title>GUIDE.md · ",
                "<a href=\"/alpha/\">Home</a>",
                "<article class=\"file-page\"><h1 id=\"guide\">Guide</h1>",
                "<h2 id=\"command-ownership\">Command ownership</h2>",
                "<a href=\"docs/TOPIC.md\">docs</a>",
            ] {
                assert!(page.contains(drawn), "{drawn} in {page}");
            }
        }
        let fetched = [
            (
                "/GUIDE.md",
                &[("Sec-Fetch-Dest", "empty"), ("Accept", "text/html")][..],
            ),
            ("/GUIDE.md", &[]),
            ("/notes.txt", &[("Sec-Fetch-Dest", "document")]),
        ];
        for (path, headers) in fetched {
            let (kind, policy, body) = ask(path, headers).await;
            let file = std::fs::read_to_string(root.join(&path[1..])).unwrap();
            assert_eq!(
                (kind.as_str(), policy.as_str()),
                (super::TEXT, super::FILE_POLICY)
            );
            assert_eq!(body, file, "{path} {headers:?}");
        }
    }

    #[test]
    fn a_file_a_renderer_draws_takes_its_type_over_its_extensions_own() {
        const PROBE: &str = "application/vnd.probe+json";
        for path in ["/data/a.json", "/notes/a.txt", "/a.GLTF"] {
            let served = (served_type(Some(PROBE), path), served_type(None, path));
            assert_ne!(served.1, PROBE, "{path}");
            assert_eq!(served.0, PROBE, "{path}");
        }
        let undrawn = (
            served_type(None, "/a.json"),
            served_type(None, "/a.lantern"),
        );
        assert_eq!(undrawn, ("application/json", "application/octet-stream"));
    }

    #[test]
    fn a_plugin_drawn_file_is_served_as_its_projects_renderer_names_and_others_as_their_own() {
        crate::shelf::install().unwrap();
        let shelf = Tree::new();
        shelf.write(symphony::project::FILE, crate::shelf::MANIFEST);
        let plain = Tree::new();
        for path in ["/room/shelf/items/A.item", "/A.ITEM"] {
            let types = (
                content_type(&shelf.root, path),
                content_type(&plain.root, path),
            );
            assert_eq!(
                types,
                (crate::shelf::ITEM_TYPE, "application/octet-stream"),
                "{path}"
            );
        }
        let ledger = (
            content_type(&shelf.root, "/room/ledger.csv"),
            content_type(&plain.root, "/room/ledger.csv"),
        );
        assert_eq!(
            ledger,
            (crate::shelf::LEDGER_TYPE, "text/csv; charset=utf-8")
        );
        let png = content_type(&shelf.root, "/a.png");
        assert_eq!(png, "image/png");
        assert!(
            handles("/a.png") && !handles("/a.item"),
            "the Symphony's own kinds"
        );
    }
}
