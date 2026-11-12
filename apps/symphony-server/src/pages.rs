//! The pages the server draws itself, in the client's look: the machine's project list, and what a
//! wrong address answers. Their styles are the client's own tokens and frame (`board.css`'s
//! `:root` blocks and `frame.css`, as every page kind loads them), the shell's notice styles and
//! `pages.css`; a project's page also takes the project's theme package, as each of its pages
//! does ([`crate::shell::page`]). A miss under `/api/` answers JSON, as every API error does.

use std::path::Path;
use std::sync::LazyLock;

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use symphony::markup::escape;
use symphony_api::page_names;

use crate::shell::{NOTICE_STYLE, REGION};

const BOARD: &str = include_str!("../../symphony-ui/assets/board.css");
const FRAME: &str = include_str!("../../symphony-ui/assets/frame.css");
const NOTICES: &str = include_str!("notices.css");
const PAGES: &str = include_str!("pages.css");

/// The tab's icon of a page no project's theme names one for: two beamed notes on the client's
/// primary blue.
const ICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 \
                    32 32'%3E%3Crect width='32' height='32' rx='8' fill='%232f5d9e'/%3E%3Cpath \
                    fill='%23fff' d='M12 22.5a3 3 0 1 1-2-2.83V9l12-3v12.5a3 3 0 1 1-2-2.83V10.5l-8 \
                    2z'/%3E%3C/svg%3E";

/// The icon link of a page with none yet, which [`crate::shell::page`] gives a theme's icon.
const NO_ICON: &str = "<link rel=\"icon\" href=\"data:,\">";

/// Every style the pages carry, in their head.
static STYLE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "<style>{}\n{FRAME}\n{PAGES}</style><style id=\"{NOTICE_STYLE}\">{NOTICES}</style>",
        tokens(BOARD)
    )
});

/// The client's colour, type and spacing tokens, light and dark: `board`'s `:root` blocks, which
/// run from its first `:root` to its first `html` rule.
fn tokens(board: &str) -> &str {
    let start = board.find(":root").unwrap_or(board.len());
    let end = board[start..]
        .find("\nhtml ")
        .map_or(board.len(), |end| start + end);
    &board[start..end]
}

/// A page: the header's `links`, the crumb `crumb` (none when it is empty), `notice` in the notice
/// region and `body` after it, titled `title`.
fn frame(title: &str, links: &str, crumb: &str, notice: &str, body: &str) -> String {
    let crumbs = if crumb.is_empty() {
        String::new()
    } else {
        format!(
            "<nav class=\"crumbs\" aria-label=\"Breadcrumbs\"><span>{}</span></nav>",
            escape(crumb)
        )
    };
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width, initial-scale=1\"><title>{title}</title>{NO_ICON}{style}\
         </head><body><div class=\"app-frame\"><header class=\"app-header\" id=\"top\">{links}\
         {crumbs}</header><main class=\"server-page\" id=\"main\">{REGION}{notice}</section>{body}\
         </main></div></body></html>\n",
        title = escape(title),
        style = *STYLE,
    )
}

/// The header's row of pages, each as its `href` and name, the one at `active` lit.
fn pages(links: Vec<(String, &str)>, active: Option<&str>) -> String {
    let mut nav = String::from("<nav class=\"app-pages\" aria-label=\"Pages\">");
    for (href, name) in links {
        let lit = if active == Some(href.as_str()) {
            " class=\"active\" aria-current=\"page\""
        } else {
            ""
        };
        nav.push_str(&format!(
            "<a href=\"{}\"{lit}>{}</a>",
            escape(&href),
            escape(name)
        ));
    }
    nav.push_str("</nav>");
    nav
}

/// `page` with the Symphony's icon when nothing gave it one.
fn iconed(page: String) -> String {
    page.replacen(NO_ICON, &format!("<link rel=\"icon\" href=\"{ICON}\">"), 1)
}

/// A page of the machine's, under no project: its header's one page is the project list, lit
/// when this page is it, and `crumb`, when there is one, names what this page is instead (a miss).
pub(crate) fn machine(title: &str, crumb: &str, notice: &str, body: &str) -> String {
    let list = vec![("/".to_string(), page_names::PROJECTS)];
    let active = crumb.is_empty().then_some("/");
    iconed(frame(title, &pages(list, active), crumb, notice, body))
}

/// A page of the project at `root`, served at `base`: its theme, and the header its client pages
/// draw (the switch to the project list on the one server, then the Symphony's own pages and the
/// plugins'), `notice` in the notice region. Its title is `title` before the project's name.
pub(crate) fn project(root: &Path, base: &str, title: &str, crumb: &str, notice: &str) -> String {
    let mut view = crate::api::project_view(root);
    view.base = base.to_string();
    let mut links = String::new();
    if !base.is_empty() {
        links.push_str(&format!(
            "<a href=\"/\" class=\"project-switch\">{}</a>",
            page_names::PROJECTS
        ));
    }
    let all = view
        .own_pages()
        .into_iter()
        .chain(
            view.nav
                .iter()
                .map(|item| (item.href.clone(), item.label.as_str())),
        )
        .map(|(href, name)| (view.here(&href), name))
        .collect();
    links.push_str(&pages(all, None));
    let shell = frame("", &links, crumb, "", "");
    let page = crate::shell::page(&shell, root, base, notice);
    iconed(page.replacen("<title>", &format!("<title>{} · ", escape(title)), 1))
}

/// A notice for the notice region: `role` `status` draws it as a warning, `alert` as a refusal;
/// `html` is its paragraphs, already escaped.
pub(crate) fn notice(role: &str, html: &str) -> String {
    format!("<div class=\"shell-notice\" role=\"{role}\">{html}</div>")
}

/// `page` as an answer with `status`.
pub(crate) fn html(status: StatusCode, page: String) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        page,
    )
        .into_response()
}

/// `why` as an API error with `status`: `{"error": why}`.
pub(crate) fn json(status: StatusCode, why: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "error": why })),
    )
        .into_response()
}

/// Whether a miss at `route` (a project's route, its branch scope taken off) answers JSON: under
/// `/api/`, and the queue asked for as data, as the client reads them; a page is HTML.
pub(crate) fn data(route: &str, headers: &HeaderMap) -> bool {
    route == "/api"
        || route.starts_with("/api/")
        || (route == "/queue"
            && headers
                .get(header::ACCEPT)
                .and_then(|accept| accept.to_str().ok())
                .is_some_and(|accept| accept.contains("application/json")))
}

/// What a project's router answers for `status`, 404 or 400, at the browser's `path` (raw, as the
/// request named it): nothing answers there, or the path does not decode. JSON under `/api/`,
/// else the project's page saying so, linking its board and its docs.
pub(crate) fn missing(
    root: &Path,
    base: &str,
    path: &str,
    headers: &HeaderMap,
    status: StatusCode,
) -> Response {
    let decoded = crate::api::decode_path(path);
    let (_, route) = crate::api::scope_path(decoded.as_deref().unwrap_or(path));
    if data(route, headers) {
        return json(
            status,
            &match decoded {
                None => "invalid URL path: its percent-encoding is not UTF-8".to_string(),
                Some(_) if route.starts_with("/api") => format!("no such API route: {route}"),
                Some(_) => format!("not found: {route}"),
            },
        );
    }
    let mut view = crate::api::project_view(root);
    view.base = base.to_string();
    let shown = format!("{base}{}", decoded.as_deref().unwrap_or(path));
    let ways = format!(
        "<p>Open this project's <a href=\"{}\">{}</a> or <a href=\"{}\">{}</a>{}.</p>",
        escape(&view.here("/")),
        page_names::HOME,
        escape(&view.here(&view.documents_href())),
        page_names::DOCS,
        if base.is_empty() {
            String::new()
        } else {
            format!(
                ", or another one from <a href=\"/\">{}</a>",
                page_names::PROJECTS
            )
        },
    );
    let (title, said) = if decoded.is_none() {
        (
            "Bad address",
            format!(
                "<p><strong>This address cannot be read</strong>: its percent-encoding does not \
                 spell UTF-8 text.</p><code>{}</code>{ways}",
                escape(&shown)
            ),
        )
    } else {
        (
            "Not found",
            format!(
                "<p><strong>Nothing is here</strong>: no page or file of {} answers at</p>\
                 <code>{}</code>{ways}",
                escape(&view.name),
                escape(&shown)
            ),
        )
    };
    let crumb = title.to_lowercase();
    html(
        status,
        project(root, base, title, &crumb, &notice("status", &said)),
    )
}

/// What a page of the branch worktree `scope` answers once no worktree holds it: the project's
/// page saying so, linking the same page in the main checkout (`route`, decoded, and `query` as
/// sent). The link is a path on this server whatever `route` starts with: `//host` would be
/// another site's.
pub(crate) fn gone(
    root: &Path,
    base: &str,
    scope: &str,
    route: &str,
    query: Option<&str>,
) -> Response {
    let mut here =
        crate::api::encode_location(&format!("{base}/{}", route.trim_start_matches('/')));
    if let Some(query) = query {
        here.push('?');
        here.push_str(query);
    }
    let said = format!(
        "<p><strong>The worktree of branch <q>{}</q> is gone</strong>: its branch merged or its \
         worktree was removed, so the Symphony serves its pages no more.</p><p><a href=\"{}\">Open \
         this page in the main checkout</a>, or its <a href=\"{}\">{}</a>.</p>",
        escape(&scope.replace('~', "/")),
        escape(&here),
        escape(&format!("{base}/")),
        page_names::HOME,
    );
    html(
        StatusCode::NOT_FOUND,
        project(
            root,
            base,
            "Branch gone",
            "branch gone",
            &notice("status", &said),
        ),
    )
}

#[cfg(test)]
#[path = "pages_tests.rs"]
mod tests;
