//! Project media and the vendored viewer, served by the native server. Raw documents (including
//! SVG/XML and mockups opened full size) keep the frame's opaque-origin sandbox in their response.

use std::path::{Component, Path};

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(crate) fn handles(path: &str) -> bool {
    path == "/favicon.ico" || path.starts_with("/static/") || mime(path).is_some()
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
            "glb" => "model/gltf-binary",
            "gltf" | "json" => "application/json",
            "md" | "txt" | "toml" | "yaml" | "yml" | "rs" | "gd" | "sh" => {
                "text/plain; charset=utf-8"
            }
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
    contained(shared, &base, &base.join(relative), request).await
}

/// The file `wanted` as served media when it resolves inside `base` (canonical) and outside the
/// repository's metadata, its type read from its own name; not found otherwise.
pub(crate) async fn contained(
    shared: &crate::front::Shared,
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
        (
            header::CONTENT_TYPE,
            mime(&wanted.to_string_lossy()).unwrap_or("application/octet-stream"),
        ),
        (header::CONTENT_SECURITY_POLICY, "sandbox allow-scripts"),
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
