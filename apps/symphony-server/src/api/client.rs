//! The selected client bundle, served with the same containment, theme and cache rules.
//!
//! A content-hashed build file is cached for good; the page and anything else revalidate by ETag.
//! A build file is sent gzipped once per content ([`gzipped`]); the page, which takes its project
//! on every request, is left to the router's per-request compression.

use super::{Request, client_kind, error, matches_etag, project_view};
use crate::front::Shared;
use axum::body::Bytes;
use axum::http::{Extensions, HeaderMap, HeaderValue, StatusCode, Version, header};
use axum::response::{IntoResponse, Response};
use sha2::Digest;
use std::path::{Component, Path};

mod gzipped;
mod preload;

use gzipped::{gzipped, takes_gzip};

/// What the cache keeps a content-hashed file for: a year, never revalidated.
const FOR_GOOD: &str = "public, max-age=31536000, immutable";

/// Marks a response whose encoding its handler chose: [`compressed_per_request`] passes it by.
#[derive(Clone, Copy)]
struct Encoded;

/// Whether the router's compression layer compresses a response on its way out (its default
/// predicate comes first): not a client build file, which carries its own encoding.
pub(crate) fn compressed_per_request(
    _: StatusCode,
    _: Version,
    _: &HeaderMap,
    extensions: &Extensions,
) -> bool {
    extensions.get::<Encoded>().is_none()
}

pub(super) async fn static_response(shared: &Shared, path: &str, request: &Request) -> Response {
    let kind = client_kind(&project_view(&shared.root), path);
    let relative = if kind.is_some() {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if Path::new(relative)
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return error(StatusCode::NOT_FOUND, "no such app file");
    }
    let (mut bytes, extension) = if shared.embedded_ui {
        let Some(bytes) = crate::client::asset(relative) else {
            return error(StatusCode::NOT_FOUND, "no such app file");
        };
        (
            Bytes::from_static(bytes),
            Path::new(relative)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_owned(),
        )
    } else {
        match directory_asset(shared, relative).await {
            Ok((bytes, extension)) => (Bytes::from(bytes), extension),
            Err(response) => return *response,
        }
    };
    let page = relative == "index.html";
    if page {
        let mut shell = String::from_utf8_lossy(&bytes).into_owned();
        // The development proxy replaces this shell's manifest with source entries.
        if !request.headers().contains_key("x-symphony-dev")
            && let Some(kind) = kind
        {
            preload::page(shared, kind, &mut shell).await;
        }
        // A hosted visitor, whom no board names, and a request no gate marked are served the page
        // for nobody in particular, the person at the machine's lines still drawn by their name.
        let machine = shared.gate.machine_person();
        let reader = match crate::front::Admitted::actor(request.extensions()) {
            Some(actor) => crate::shell::Reader::of(&actor, machine),
            None => crate::shell::Reader {
                machine,
                ..crate::shell::Reader::default()
            },
        };
        bytes = Bytes::from(crate::shell::page_for(
            &shell,
            &shared.root,
            &shared.base,
            "",
            &reader,
        ));
    }
    let (content_type, compresses) = file_type(&extension);
    let hash: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let etag = format!("\"{hash}\"");
    // A build file's representation turns on Accept-Encoding; the page's is the layer's to say.
    let negotiated = compresses && !page;
    let mut response = if matches_etag(request, &etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let zipped = if negotiated && takes_gzip(request.headers()) {
            gzipped(&hash, &bytes).await
        } else {
            None
        };
        let encoded = zipped.is_some();
        let body = zipped.unwrap_or(bytes);
        let length = body.len();
        let mut response = ([(header::CONTENT_TYPE, content_type)], body).into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, length.into());
        if encoded {
            response
                .headers_mut()
                .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        }
        response
    };
    let headers = response.headers_mut();
    headers.insert(header::ETAG, etag.parse().expect("hex etag is a header"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if content_hashed(relative) {
            FOR_GOOD
        } else {
            "no-cache"
        }),
    );
    if negotiated {
        headers.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    if !page {
        response.extensions_mut().insert(Encoded);
    }
    response
}

/// A client file's type by its extension, and whether gzip can shrink it: pictures and fonts in
/// these formats are compressed already.
fn file_type(extension: &str) -> (&'static str, bool) {
    match extension {
        "html" => ("text/html; charset=utf-8", true),
        "js" => ("text/javascript; charset=utf-8", true),
        "css" => ("text/css; charset=utf-8", true),
        "wasm" => ("application/wasm", true),
        "json" => ("application/json", true),
        "svg" => ("image/svg+xml", true),
        "png" => ("image/png", false),
        "woff2" => ("font/woff2", false),
        _ => ("application/octet-stream", true),
    }
}

/// Whether the build file at `relative` is named by its content, so its bytes never change under
/// that name: under `assets/`, the last `-` part of its name before the extension is the build's
/// hash (sixteen hex digits).
fn content_hashed(relative: &str) -> bool {
    let Some(file) = relative.strip_prefix("assets/") else {
        return false;
    };
    let name = file.rsplit('/').next().unwrap_or(file);
    let stem = name.split('.').next().unwrap_or(name);
    let Some((_, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    hash.len() == 16
        && hash
            .bytes()
            .all(|digit| digit.is_ascii_digit() || (b'a'..=b'f').contains(&digit))
}

fn client_unbuilt(shared: &Shared) -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        if shared.installed_ui {
            "the Symphony client has not been built: run just symphony --stop, then just symphony from the main checkout"
        } else {
            "the Symphony client has not been built: run just symphony-ui-dist"
        },
    )
}

async fn directory_asset(
    shared: &Shared,
    relative: &str,
) -> Result<(Vec<u8>, String), Box<Response>> {
    let root = match tokio::fs::canonicalize(&shared.ui_dist).await {
        Ok(root) => root,
        Err(_) => return Err(Box::new(client_unbuilt(shared))),
    };
    let file = match tokio::fs::canonicalize(root.join(relative)).await {
        // The shipped client lives inside the common directory, deliberately. Only this static
        // handler may serve it, and canonical containment still rejects links outside its bundle.
        Ok(file)
            if file.starts_with(&root)
                && file.is_file()
                && (shared.installed_ui || !shared.is_metadata(&file)) =>
        {
            file
        }
        Err(e) if relative == "index.html" && e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Box::new(client_unbuilt(shared)));
        }
        _ => return Err(Box::new(error(StatusCode::NOT_FOUND, "no such app file"))),
    };
    let bytes = match tokio::fs::read(&file).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return Err(Box::new(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )));
        }
    };
    let extension = file
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned();
    Ok((bytes, extension))
}

#[cfg(test)]
mod tests;
