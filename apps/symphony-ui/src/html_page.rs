//! An HTML document: the file drawn whole, as its author made it, in a sandboxed frame inside the
//! app's page, under the app's header. None of the Markdown view's features: no outline, cards,
//! notes, asks, editing, versions, device cache or feed (docs/PROJECT_FILE.md, "HTML documents").
//! The document's route is always the app's page; the frame loads the file's own address, the
//! route with the file's query (`symphony_api::file_address`), which the server answers with the
//! file itself, under a policy that keeps it to its entrypoint's files and opens no connection, and
//! a route with no such document with its own Not found (`apps/symphony-server/src/media_files.rs`,
//! `html_document`): the page holds nothing but the frame.

use dioxus::prelude::*;

/// The sandbox an HTML document's frame draws it in: its scripts run, in an opaque origin, never
/// the app's, so the app's storage, cookies, feed and API are out of its reach; no same-origin, top
/// navigation, forms, popups or dialogs. Never add `allow-same-origin`.
pub const SANDBOX: &str = "allow-scripts";

/// The page of the HTML document at the route `path` of the checkout `scope` (empty for the main
/// one): its path, as a document's head shows it, over its frame, as wide as the page and as tall
/// as the window below the header, scrolling inside itself. The frame loads the document's file's
/// address under the project's base.
#[component]
pub fn HtmlPage(path: String, scope: String) -> Element {
    let src = symphony_api::file_address(&crate::project::href(&crate::docs::scoped(
        &scope,
        &format!("/{path}"),
    )));
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    rsx! {
        div { class: "document-page html-document", "data-ready": "html",
            p { class: "eyebrow", "{path}" }
            iframe { class: "html-frame", src, "sandbox": SANDBOX, title: name, referrerpolicy: "no-referrer" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame runs the document's scripts in an opaque origin and allows nothing else: no
    /// `allow-same-origin`, top navigation, forms or popups; it sends no referrer, and its address
    /// is the document's file's, a worktree's under its scope.
    #[test]
    fn an_html_document_is_drawn_in_a_frame_that_allows_its_scripts_alone() {
        for (scope, src) in [
            ("", "/docs/analysis/readout.html?raw"),
            ("feature-x", "/@feature-x/docs/analysis/readout.html?raw"),
        ] {
            let html = dioxus_ssr::render_element(rsx! {
                HtmlPage { path: "docs/analysis/readout.html", scope }
            });
            assert!(
                html.contains(r#"<p class="eyebrow">docs/analysis/readout.html</p>"#),
                "{html}"
            );
            assert!(html.contains(&format!(r#"src="{src}""#)), "{html}");
            assert!(html.contains(r#"sandbox="allow-scripts""#), "{html}");
            assert!(html.contains(r#"title="readout.html""#), "{html}");
            assert!(html.contains(r#"referrerpolicy="no-referrer""#), "{html}");
            assert_eq!(html.matches("sandbox=").count(), 1, "{html}");
            for allowed in ["same-origin", "top-navigation", "forms", "popups", "modals"] {
                assert!(!html.contains(allowed), "{allowed} in {html}");
            }
        }
    }
}
