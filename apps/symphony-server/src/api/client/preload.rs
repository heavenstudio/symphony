//! Start the initial kind's shared chunks with the shell. Its entry stays the shell's import:
//! preloading that too can fill all six HTTP/1.1 connections before the shell gets one.

use super::{Shared, content_hashed, directory_asset};

/// Read the manifest from the shell this page actually names, rather than the newest build.
/// An absent or older manifest leaves loading to the shell, as does a client not yet built.
pub(super) async fn page(shared: &Shared, kind: &str, html: &mut String) {
    let script = crate::client::script(html).trim_start_matches('/');
    if !content_hashed(script) {
        return;
    }
    let source = if shared.embedded_ui {
        crate::client::asset(script).map(<[u8]>::to_vec)
    } else {
        directory_asset(shared, script)
            .await
            .ok()
            .map(|(bytes, _)| bytes)
    };
    let Some(source) = source else { return };
    let Some(links) = links(&source, kind) else {
        return;
    };
    if let Some(at) = html.find("</head>") {
        html.insert_str(at, &links);
    }
}

/// A build's manifest is JSON on its first line. Load only this kind's imports, never deferred
/// modules or another build's paths; restrict URLs to immutable local JavaScript build assets.
fn links(source: &[u8], kind: &str) -> Option<String> {
    let line = std::str::from_utf8(source).ok()?.lines().next()?;
    let manifest: serde_json::Value = serde_json::from_slice(
        line.strip_prefix("const modules = ")?
            .strip_suffix(';')?
            .as_bytes(),
    )
    .ok()?;
    let imports: Vec<String> =
        serde_json::from_value(manifest.get(kind)?.get("imports")?.clone()).ok()?;
    let mut seen = std::collections::BTreeSet::new();
    let mut links = String::new();
    for href in imports {
        let relative = href.strip_prefix('/')?;
        if !content_hashed(relative)
            || !relative.ends_with(".js")
            || !relative
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/-_.".contains(&byte))
            || std::path::Path::new(relative)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return None;
        }
        if seen.insert(href.clone()) {
            links.push_str(&format!("<link rel=\"modulepreload\" href=\"{href}\">"));
        }
    }
    Some(links)
}

#[cfg(test)]
mod tests;
