//! Resolve the single hosted scope once, before any project/body access.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, Method, header, request::Parts};
use axum::response::Response;

use super::{Action, Config, Refusal, Target};

pub(super) fn authority(origin: &str) -> Result<String, Refusal> {
    let value = origin.strip_prefix("https://").ok_or(Refusal::Invalid)?;
    let parsed: axum::http::uri::Authority = value.parse().map_err(|_| Refusal::Invalid)?;
    if value.is_empty()
        || value.contains(['@', '/', '?', '#', '\\', '%', '*'])
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
        || parsed.host().is_empty()
        || parsed.host().starts_with('.')
        || parsed.host().ends_with('.')
        || value.ends_with(':')
        || (value.contains(':') && !value.ends_with(']') && parsed.port_u16().is_none())
        || parsed.port_u16() == Some(0)
    {
        return Err(Refusal::Invalid);
    }
    Ok(value.into())
}

fn single<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, Refusal> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|v| v.to_str().map_err(|_| Refusal::Invalid))
        .transpose()?;
    if values.next().is_some() {
        return Err(Refusal::Invalid);
    }
    Ok(value)
}

pub(super) fn headers(config: &Config, head: &Parts) -> Result<(), Refusal> {
    if single(&head.headers, "host")? != Some(config.authority.as_str())
        || head
            .uri
            .authority()
            .is_some_and(|a| a.as_str() != config.authority)
        || head
            .uri
            .scheme_str()
            .is_some_and(|scheme| scheme != "https")
    {
        return Err(Refusal::Denied);
    }
    if let Some(origin) = single(&head.headers, "origin")?
        && origin != config.origin
    {
        return Err(Refusal::Denied);
    }
    Ok(())
}

pub(super) fn browser(config: &Config, head: &Parts, mutation: bool) -> Result<(), Refusal> {
    if single(&head.headers, "origin")? != Some(config.origin.as_str())
        || single(&head.headers, "sec-fetch-site")?.is_some_and(|site| site != "same-origin")
    {
        return Err(Refusal::Denied);
    }
    if mutation
        && (single(&head.headers, "content-type")? != Some("application/json")
            || single(&head.headers, "x-symphony-csrf")?.is_none_or(str::is_empty)
            || single(&head.headers, "cookie")?.is_none_or(str::is_empty))
    {
        return Err(Refusal::Denied);
    }
    Ok(())
}

fn path(value: &str) -> Result<(), Refusal> {
    if value.contains(['\\', '%', '?', '#'])
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| {
            part == "."
                || part == ".."
                || part.starts_with('@')
                || part.eq_ignore_ascii_case(".git")
        })
        || value.contains("//")
    {
        return Err(Refusal::Missing);
    }
    Ok(())
}

fn decode(value: &str) -> Result<String, Refusal> {
    crate::api::decode_path(value).ok_or(Refusal::Invalid)
}

/// Encoded separators and double encodings cannot acquire another handler's meaning. Harmless
/// encoded UTF-8/space is supported; scope selectors and metadata are refused after decoding.
pub(super) fn target(
    config: &Config,
    head: &Parts,
) -> Result<(Target, BTreeMap<String, String>), Refusal> {
    let raw = head.uri.path();
    let route = decode(raw)?;
    path(&route)?;
    let lower = raw.to_ascii_lowercase();
    if ["%2f", "%5c", "%2e", "%40"]
        .iter()
        .any(|part| lower.contains(part))
    {
        return Err(Refusal::Missing);
    }
    let mut query = BTreeMap::new();
    for pair in head
        .uri
        .query()
        .into_iter()
        .flat_map(|query| query.split('&'))
    {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode(&key.replace('+', " "))?;
        let value = decode(&value.replace('+', " "))?;
        selector(config, &key, &value)?;
        if query.insert(key, value).is_some() {
            return Err(Refusal::Invalid);
        }
    }
    // These surfaces expose machine state or invoke local capabilities, even on GET.
    if matches!(
        route.as_str(),
        "/api/front"
            | "/api/projects"
            | "/api/machine-work"
            | "/api/agent-screen"
            | "/api/agent-focus"
            | "/api/task-offer"
            | "/api/task-gates"
            | "/api/health"
            | symphony_api::SCHEMA_UPDATES_PATH
            | symphony_api::SETTINGS_PATH
            | symphony_api::SETTING_WRITE_PATH
    ) {
        return Err(Refusal::Missing);
    }
    let action = if head.method == Method::POST {
        if crate::write::methods(&route).is_none() && !route.starts_with("/x/") {
            return Err(Refusal::Missing);
        }
        Action::Mutation(route.clone())
    } else if head.method != Method::GET && head.method != Method::HEAD {
        return Err(Refusal::Missing);
    } else if route.starts_with("/x/") {
        Action::Plugin(route.clone())
    } else if route == "/api/feed" {
        Action::Feed
    } else if crate::api::READS.contains(&route.as_str())
        || route == "/"
        || route.starts_with("/assets/")
    {
        Action::Read
    } else if route.starts_with("/api/") {
        return Err(Refusal::Missing);
    } else {
        // Includes document pages and raw files. A media grant is required even for HEAD/304.
        Action::Media
    };
    if head.headers.contains_key(header::UPGRADE) && action != Action::Feed {
        return Err(Refusal::Missing);
    }
    Ok((
        Target {
            project: config.project.clone(),
            route,
            action,
        },
        query,
    ))
}

fn selector(config: &Config, key: &str, value: &str) -> Result<(), Refusal> {
    match key {
        "project" | "project_id" if value != config.project => Err(Refusal::Missing),
        "root" | "scope" | "tree" | "worktree" | "tenant" | "actor" | "principal" => {
            Err(Refusal::Missing)
        }
        "path" | "document" | "target" if value.starts_with('/') => Err(Refusal::Missing),
        "path" | "document" | "target" => path(value),
        "since" if value.parse::<u64>().is_err() => Err(Refusal::Invalid),
        _ => Ok(()),
    }
}

pub(super) fn payload(config: &Config, json: &serde_json::Value) -> Result<(), Refusal> {
    let object = json.as_object().ok_or(Refusal::Invalid)?;
    for (key, value) in object {
        if matches!(
            key.as_str(),
            "project"
                | "project_id"
                | "root"
                | "scope"
                | "tree"
                | "worktree"
                | "tenant"
                | "actor"
                | "principal"
                | "path"
                | "document"
                | "target"
        ) {
            selector(config, key, value.as_str().ok_or(Refusal::Invalid)?)?;
        }
        nested_payload(config, value)?;
    }
    Ok(())
}

fn nested_payload(config: &Config, value: &serde_json::Value) -> Result<(), Refusal> {
    match value {
        serde_json::Value::Object(_) => payload(config, value),
        serde_json::Value::Array(values) => {
            for value in values {
                nested_payload(config, value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Only representation/upgrade headers reach the native read handlers. Identity, session,
/// forwarding and CSRF credentials remain at the trusted boundary.
pub(super) fn strip_credentials(headers: &mut HeaderMap) {
    let keep = [
        "host",
        "origin",
        "accept",
        "accept-encoding",
        "if-none-match",
        "if-modified-since",
        "range",
        "if-range",
        "upgrade",
        "connection",
        "sec-websocket-key",
        "sec-websocket-version",
        "sec-websocket-protocol",
    ];
    let filtered = headers
        .iter()
        .filter(|(name, _)| keep.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    *headers = filtered;
}

pub(super) fn response(config: &Config, response: Response) -> Result<Response, Refusal> {
    // Native error details may contain paths or dependency failures. Hosted diagnostics are
    // finite status failures, never HTML login bodies or reflected error strings.
    if response.status().is_client_error() || response.status().is_server_error() {
        let refused = match response.status().as_u16() {
            401 => Refusal::Unauthenticated,
            403 => Refusal::Denied,
            404 => Refusal::Missing,
            400..=499 => Refusal::Invalid,
            _ => Refusal::Unavailable,
        };
        let mut sanitized = refused.response();
        // Status is protocol data: notably, a feed's 409 requests a new snapshot. Detailed
        // diagnostics are replaced without turning that recovery signal into another error.
        *sanitized.status_mut() = response.status();
        return Ok(sanitized);
    }
    if let Some(location) = single(response.headers(), "location")? {
        let relative = location.strip_prefix(&config.origin).unwrap_or(location);
        if !relative.starts_with('/') || relative.starts_with("//") || relative.contains('\\') {
            return Err(Refusal::Denied);
        }
        let route = relative.split(['?', '#']).next().ok_or(Refusal::Denied)?;
        path(&decode(route)?).map_err(|_| Refusal::Denied)?;
    }
    Ok(response)
}
