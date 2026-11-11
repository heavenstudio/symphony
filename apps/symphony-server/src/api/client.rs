//! The selected client bundle, served with the same containment, theme and cache rules.

use super::{Request, client_page, error, matches_etag, project_view};
use crate::front::Shared;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::path::{Component, Path};

pub(super) async fn static_response(shared: &Shared, path: &str, request: &Request) -> Response {
    let relative = if client_page(&project_view(&shared.root), path) {
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
            bytes.to_vec(),
            Path::new(relative)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_owned(),
        )
    } else {
        match directory_asset(shared, relative).await {
            Ok(asset) => asset,
            Err(response) => return *response,
        }
    };
    if relative == "index.html" {
        let banner = shared.orphan_banner().unwrap_or_default();
        let shell = String::from_utf8_lossy(&bytes).into_owned();
        bytes = crate::shell::page(&shell, &shared.root, &shared.base, &banner).into_bytes();
    }
    let content_type = match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    let hash: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let etag = format!("\"{hash}\"");
    let cached = matches_etag(request, &etag);
    let length = bytes.len();
    let mut response = if cached {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
    };
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().expect("hex etag is a header"));
    if !cached {
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, length.into());
    }
    let immutable =
        relative.starts_with("assets/") && relative.split('-').any(|part| part.starts_with("dxh"));
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );
    response
}

use sha2::Digest;

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
