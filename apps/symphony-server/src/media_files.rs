//! Project media and the vendored viewers, served by the native server. Raw documents (including
//! SVG/XML and mockups opened full size) keep the frame's opaque-origin sandbox in their response.
//! A Markdown file no entrypoint serves (a guide such as `AGENTS.md`) opened as a page is drawn
//! read-only in the project's look instead, its headings' ids a document's, so `#anchor` lands, in
//! that sandbox with no script run ([`PAGE_POLICY`]). A fetch of it gets the file.

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

/// The sandbox a Markdown file drawn as a page is served in: the raw HTML its text may hold (a
/// `<script>`, an `onerror`, a `javascript:` link) runs nothing, as the file served as text ran
/// nothing. The page needs no script of its own; its data block is read, never run.
const PAGE_POLICY: &str = "sandbox allow-scripts; script-src 'none'";

/// The policy an HTML document's file is served under ([`html_document`]): the frame's sandbox,
/// its scripts run in an opaque origin, never the app's; its own inline scripts and styles, `data:`
/// and `blob:` URLs, and the files of its entrypoint (`within`, `HOST/BASE/URL/`), from nowhere
/// else, so a picture or a stylesheet beside it loads and nothing outside its entrypoint or on
/// another site does; and no connection at all: no fetch, XHR, EventSource, WebSocket or beacon.
/// Without `within` (a host the policy cannot name) it loads nothing but itself.
fn html_policy(within: Option<&str>) -> String {
    let within = within.map_or_else(String::new, |within| format!(" {within}"));
    format!(
        "sandbox allow-scripts; default-src 'unsafe-inline' 'unsafe-eval' data: blob:{within}; \
         connect-src 'none'"
    )
}

/// The source an HTML document's policy names its entrypoint's files by: `host` (the request's,
/// which the server checked names this machine) and the entrypoint's path, `prefix`, its every
/// byte but letters, digits and `/-._~@` escaped. None for a host a source cannot spell (an IPv6
/// address, a stray character), which then loads nothing beside the document.
fn entrypoint_source(host: &str, prefix: &str) -> Option<String> {
    let (name, port) = host.rsplit_once(':').unwrap_or((host, ""));
    let named = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        && port.bytes().all(|byte| byte.is_ascii_digit());
    named.then(|| format!("{host}{}", crate::api::encode_location(prefix)))
}

/// The file of the HTML document at the route `path` in the checkout at `root` (`scope` its
/// worktree's, empty for the main checkout), asked for at its file's address
/// (`symphony_api::file_address`), which the frame that draws it in the app loads, whatever headers
/// come with it. Only a page its entrypoint shows and its walk reaches
/// ([`symphony::project::Project::html_page`]) is served, under [`html_policy`]; any other route is
/// not found, a file outside its entrypoint included.
pub(crate) async fn html_document(
    shared: &crate::front::Shared,
    root: &Path,
    path: &str,
    scope: &str,
    request: &axum::http::Request<()>,
) -> Response {
    let project = symphony::project::Project::of(root);
    let found = project.html_page(root, path.trim_start_matches('/'));
    let canonical = tokio::fs::canonicalize(root).await;
    match (found, canonical) {
        (Some((entry, file)), Ok(canonical)) => {
            let scope = if scope.is_empty() {
                String::new()
            } else {
                format!("/@{scope}")
            };
            let prefix = format!(
                "{}{scope}/{}",
                shared.base,
                entry.label().trim_start_matches('/')
            );
            let host = request
                .headers()
                .get(header::HOST)
                .and_then(|host| host.to_str().ok())
                .unwrap_or_default();
            let policy = html_policy(entrypoint_source(host, &prefix).as_deref());
            let mut response = contained(shared, &canonical, &canonical, &file, request).await;
            if response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED {
                response.headers_mut().insert(
                    header::CONTENT_SECURITY_POLICY,
                    policy.parse().expect("an HTML document's policy"),
                );
            }
            response
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

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
    // The address's own icon is the project's `tools/static/icon.svg`, else the Symphony's, whose
    // home-screen size is asked for by pages that show the Symphony's alone.
    if path == "/favicon.ico"
        && (crate::mark::touch(request.uri().query())
            || !root.join("tools/static/icon.svg").is_file())
    {
        return crate::mark::answer(request.uri().query());
    }
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
            (header::CONTENT_SECURITY_POLICY, PAGE_POLICY),
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
    /// with its headings' anchors, its raw HTML as written under a policy that runs no script of it
    /// (a `<script>`, an `onerror`, a `javascript:` link); fetched, it is the file, as any other
    /// file is however it is asked for.
    #[tokio::test]
    async fn a_markdown_file_opened_as_a_page_is_drawn_and_fetched_is_the_file() {
        let tree = Tree::new();
        let root = crate::testing::repository(&tree);
        let live = [
            "<script>x()</script>",
            "<img src=\"x\" onerror=\"y()\" />",
            "<a href=\"javascript:z()\">go</a>",
        ];
        let guide = format!(
            "# Guide\n\n## Command ownership\n\nSee [docs](docs/TOPIC.md).\n\n{}\n\n\
             <img src=\"x\" onerror=\"y()\" />\n\n[go](javascript:z())\n",
            live[0]
        );
        std::fs::write(root.join("GUIDE.md"), guide).unwrap();
        std::fs::write(root.join("notes.txt"), "plain notes").unwrap();
        let gate = crate::testing::open_gate();
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
                ("text/html; charset=utf-8", super::PAGE_POLICY)
            );
            // The file's own script comes out as written; the policy is what runs none of it.
            for written in live {
                assert!(page.contains(written), "{written} in {page}");
            }
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
        let directives: Vec<&str> = super::PAGE_POLICY.split(';').map(str::trim).collect();
        assert!(
            directives.contains(&"sandbox allow-scripts")
                && directives.contains(&"script-src 'none'"),
            "an opaque origin, and no script, inline, handler or javascript: URL, runs: {directives:?}"
        );
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

    /// An HTML document's file, as its frame loads it: the file as written, under the frame's
    /// sandbox, loading only its entrypoint's files and connecting nowhere; any route that is no
    /// page its entrypoint shows and reaches is not found, a file outside the entrypoint, behind a
    /// link out of the project or in a dot folder among them. The requests carry no fetch metadata,
    /// as a browser's to a plain-http address do: none is read. The router's dispatch to it is
    /// tested through the machine's routing (`projects_tests.rs`).
    #[tokio::test]
    async fn an_html_documents_file_is_served_sandboxed_to_its_entrypoint_and_nothing_else() {
        let tree = Tree::new();
        let root = crate::testing::repository(&tree);
        let page = "<!doctype html><title>Readout</title><script>document.title = 'ran'</script>";
        for (path, text) in [
            ("docs/analysis/readout.html", page),
            ("docs/analysis/board.css", "body { margin: 0 }"),
            ("docs/.drafts/hidden.html", page),
            ("outside.html", page),
        ] {
            tree.write(&format!("main/{path}"), text);
        }
        let away = Tree::new();
        away.write("away.html", page);
        std::os::unix::fs::symlink(away.path("away.html"), root.join("docs/leak.html")).unwrap();
        let gate = crate::testing::open_gate();
        let shared = crate::front::Shared::project(1, 1234, &root, None, gate, "/alpha");
        let ask = |path: &str, scope: &str, headers: &[(&str, &str)]| {
            let mut request = Request::builder().uri(path);
            for (name, value) in headers {
                request = request.header(*name, *value);
            }
            let request = request.body(()).unwrap();
            let (shared, root, path, scope) = (
                shared.clone(),
                root.clone(),
                path.to_string(),
                scope.to_string(),
            );
            async move {
                let response = super::html_document(&shared, &root, &path, &scope, &request).await;
                let status = response.status().as_u16();
                let head = response.headers().clone();
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let said = |name: header::HeaderName| {
                    head.get_all(name)
                        .iter()
                        .map(|value| value.to_str().unwrap())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let said = [
                    said(header::CONTENT_TYPE),
                    said(header::CONTENT_SECURITY_POLICY),
                    said(header::X_CONTENT_TYPE_OPTIONS),
                    said(header::ETAG),
                ];
                (status, said, String::from_utf8(body.to_vec()).unwrap())
            }
        };
        let framed = [("Host", "127.0.0.1:1234")];
        let policy = |within: &str| {
            format!(
                "sandbox allow-scripts; default-src 'unsafe-inline' 'unsafe-eval' data: blob:{within}; \
                 connect-src 'none'"
            )
        };
        for (scope, within) in [
            ("", " 127.0.0.1:1234/alpha/docs/"),
            ("feature-x", " 127.0.0.1:1234/alpha/@feature-x/docs/"),
        ] {
            let (status, said, body) = ask("/docs/analysis/readout.html", scope, &framed).await;
            assert_eq!((status, body.as_str()), (200, page), "{scope}");
            let [kind, csp, sniff, _] = said;
            assert_eq!(
                [kind, csp, sniff],
                [
                    "text/html; charset=utf-8".to_string(),
                    policy(within),
                    "nosniff".into(),
                ],
                "{scope}"
            );
        }
        // A host a source cannot spell: the document loads nothing beside itself.
        let ipv6 = [("Host", "[::1]:1234")];
        let (status, said, _) = ask("/docs/analysis/readout.html", "", &ipv6).await;
        assert_eq!((status, said[1].clone()), (200, policy("")));
        // Asked again naming its tag: unchanged, under the same policy.
        let (_, said, _) = ask("/docs/analysis/readout.html", "", &framed).await;
        let tagged = [framed[0], ("If-None-Match", said[3].as_str())];
        let (status, again, body) = ask("/docs/analysis/readout.html", "", &tagged).await;
        assert_eq!(
            (status, again[1].clone(), body.as_str()),
            (304, policy(" 127.0.0.1:1234/alpha/docs/"), "")
        );
        for refused in [
            "/docs/analysis/missing.html",
            "/docs/.drafts/hidden.html",
            "/docs/leak.html",
            "/outside.html",
            "/docs/../outside.html",
            "/docs/analysis/board.css",
            "/docs/TOPIC.md",
            "/docs/analysis",
        ] {
            let (status, said, _) = ask(refused, "", &framed).await;
            assert_eq!((status, said[1].as_str()), (404, ""), "{refused}");
        }
    }

    #[test]
    fn an_html_documents_entrypoint_is_named_by_its_host_and_escaped_path_or_not_at_all() {
        for (host, prefix, source) in [
            (
                "127.0.0.1:8765",
                "/alpha/docs/",
                Some("127.0.0.1:8765/alpha/docs/"),
            ),
            ("localhost", "/docs/", Some("localhost/docs/")),
            (
                "my-mac.tail.ts.net:80",
                "/a/@wt/design docs/",
                Some("my-mac.tail.ts.net:80/a/@wt/design%20docs/"),
            ),
            (
                "box:8765",
                "/a/docs;script-src */",
                Some("box:8765/a/docs%3Bscript-src%20%2A/"),
            ),
            ("[::1]:8765", "/docs/", None),
            ("", "/docs/", None),
            (":8765", "/docs/", None),
            ("box:87x", "/docs/", None),
            ("a b", "/docs/", None),
            ("box;", "/docs/", None),
        ] {
            assert_eq!(
                super::entrypoint_source(host, prefix).as_deref(),
                source,
                "{host} {prefix}"
            );
        }
    }
}
