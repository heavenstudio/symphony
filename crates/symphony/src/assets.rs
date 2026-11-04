//! Cache busting for the Symphony.
//!
//! Every local file a page loads is named by a validator of the file as it is now:
//! `/artifacts/x.png?v=<validator>`. A browser never shows a stale copy, since a changed file is another
//! URL; and a page's live token covers the files it shows, so an open page draws a changed picture,
//! mockup or model again.
//!
//! [`bust`] rewrites the markup the app sends, pages and fragments alike: the `src`, `data-full`,
//! `data-module` and `poster` of any element (a picture, the original behind its thumbnail, a mockup's
//! frame, a model viewer, a script and the module it imports, a video) and the `href` of a `<link>` (the
//! icons). A URL relative to the page keeps its form (a branch's page loads its own worktree's file) and is
//! only resolved against the page to find the file. A link to open (`<a href>`), an outside URL, a URL
//! with a query of its own and a file that does not exist are left as they are, and a thumbnail is
//! already named by its token. [`cache_headers`] is the answer's side: a request naming the current
//! validator may be kept for good, any other is revalidated (`ETag`, answered 304 when it matches).

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};
use sha2::{Digest, Sha256};

use crate::markup::{dirname, is_external, join, normpath, unescape, unquote};

pub const KEEP: &str = "public, max-age=31536000, immutable";
pub const REVALIDATE: &str = "no-cache";
pub const STATIC_ROUTE: &str = "/static/";
pub const THUMB_ROUTE: &str = "/thumb/";

static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([a-zA-Z][\w-]*)\b[^>]*>").expect("the tag pattern"));
static LOADS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(\s(?:src|data-full|data-module|poster)=")([^"]*)(")"#)
        .expect("the loads pattern")
});
static LINK_HREF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(\shref=")([^"]*)(")"#).expect("the href pattern"));

/// Names the file as it is now: its change time and size. Nothing for a file that is not there.
pub fn validator(path: &Path) -> String {
    let Ok(meta) = std::fs::metadata(path) else {
        return String::new();
    };
    let key = format!(
        "{}:{}",
        meta.mtime() * 1_000_000_000 + meta.mtime_nsec(),
        meta.len()
    );
    Sha256::digest(key.as_bytes())
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `reference` as the page at `page` resolves it: the path of the URL, or `None` for one with a scheme,
/// a host or a query of its own.
fn resolve(page: &str, reference: &str) -> Option<String> {
    let reference = unescape(reference);
    let reference = reference.split('#').next().unwrap_or("");
    if is_external(reference) || reference.contains('?') {
        return None;
    }
    let page = page.split(['?', '#']).next().unwrap_or("");
    let path = if reference.is_empty() {
        page.to_string()
    } else if reference.starts_with('/') {
        reference.to_string()
    } else {
        join(dirname(page), reference)
    };
    let path = normpath(&path);
    (path != ".").then_some(path)
}

/// The file of the project (or of the app's own scripts, under `/static/`) that `url`, on the page at
/// `page`, loads, or `None`. A route (`/docs/…`) names a file in the folder of the entrypoint serving it.
pub fn local_file(url: &str, root: &Path, static_dir: &Path, page: &str) -> Option<PathBuf> {
    let path = unquote(&resolve(page, url)?);
    if path.starts_with(THUMB_ROUTE) {
        return None;
    }
    let (base, rel) = match path.strip_prefix(STATIC_ROUTE) {
        Some(rest) => (static_dir.canonicalize().ok()?, rest.to_string()),
        None => (
            root.canonicalize().ok()?,
            crate::project::Project::of(root).file(path.trim_start_matches('/')),
        ),
    };
    let found = base.join(rel).canonicalize().ok()?;
    (found != base && found.starts_with(&base) && found.is_file()).then_some(found)
}

/// `markup`, shown on the page at `page`, with every local file it loads named by its validator, and
/// those files with the validators used.
pub fn bust(
    markup: &str,
    root: &Path,
    static_dir: &Path,
    page: &str,
) -> (String, BTreeMap<PathBuf, String>) {
    let mut files: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut named = |found: &Captures<'_>| -> String {
        let Some(file) = local_file(&found[2], root, static_dir, page) else {
            return found[0].to_string();
        };
        let validator = files
            .entry(file.clone())
            .or_insert_with(|| validator(&file))
            .clone();
        let (url, fragment) = match found[2].split_once('#') {
            Some((url, fragment)) => (url, format!("#{fragment}")),
            None => (&found[2], String::new()),
        };
        format!("{}{url}?v={validator}{fragment}{}", &found[1], &found[3])
    };
    let out = TAG
        .replace_all(markup, |tag: &Captures<'_>| {
            let loaded = LOADS.replace_all(&tag[0], &mut named).into_owned();
            if tag[1].eq_ignore_ascii_case("link") {
                LINK_HREF.replace_all(&loaded, &mut named).into_owned()
            } else {
                loaded
            }
        })
        .into_owned();
    (out, files)
}

/// The `v` of a query string.
fn asked(query: &str) -> &str {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("v="))
        .unwrap_or("")
}

/// The caching of an answer with the file at `path`, as (`ETag`, `Cache-Control`): kept for good when the
/// request names its current validator.
pub fn cache_headers(path: &Path, query: &str) -> (String, &'static str) {
    let current = validator(path);
    let keep = !asked(query).is_empty() && asked(query) == current;
    (
        format!("\"{current}\""),
        if keep { KEEP } else { REVALIDATE },
    )
}
