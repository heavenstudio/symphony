//! An HTML document: the file drawn whole, as its author made it, in a sandboxed frame inside the
//! app's page, under the app's header. None of the Markdown view's features: no outline, cards,
//! notes, asks, editing, versions, device cache or feed (docs/PROJECT_FILE.md, "HTML documents").
//! The server answers the frame's load of the document's route with the file itself, under a
//! policy that keeps it to its entrypoint's files and opens no connection
//! (`apps/symphony-server/src/media_files.rs`, `html_document`).

use dioxus::prelude::*;

/// The sandbox an HTML document's frame draws it in: its scripts run, in an opaque origin, never
/// the app's, so the app's storage, cookies, feed and API are out of its reach; no same-origin, top
/// navigation, forms, popups or dialogs. Never add `allow-same-origin`.
pub const SANDBOX: &str = "allow-scripts";

/// The page of the HTML document at the route `path`: its path, as a document's head shows it,
/// over its frame, as wide as the page and as tall as the window below the header, scrolling
/// inside itself. `src` is its address under the project's base.
#[component]
pub fn HtmlFrame(path: String, src: String) -> Element {
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    rsx! {
        div { class: "html-document",
            p { class: "eyebrow", "{path}" }
            iframe { class: "html-frame", src, "sandbox": SANDBOX, title: name, referrerpolicy: "no-referrer" }
        }
    }
}

/// The HTML document at the route `path` of the checkout `scope` (empty for the main one): its
/// frame at once, and in its place what the server says when it has no such document, which a
/// `HEAD` of its address tells (the frame's own answer is out of the page's reach).
#[cfg(target_arch = "wasm32")]
#[component]
pub fn HtmlPage(path: String, scope: String) -> Element {
    let mut missing = use_signal(|| false);
    let mut retry = use_signal(|| 0_u64);
    let src = crate::project::href(&crate::docs::scoped(&scope, &format!("/{path}")));
    let head = src.clone();
    use_effect(move || {
        let _ = retry();
        let head = head.clone();
        spawn(async move {
            // A read that fails says nothing: the frame shows what it got.
            if let Some(found) = found(&head).await {
                missing.set(!found);
            }
        });
    });
    let documents = crate::docs::scoped(&scope, &crate::project::current().documents_href());
    rsx! {
        div { class: "document-page html-page", "data-ready": if missing() { "missing" } else { "html" },
            if missing() {
                crate::trouble::Trouble { title: "Document not found", text: "There is no document at {path}: it may have been moved, renamed or removed.", back: Some((documents, "All documents".to_string())), retry: move |_| retry += 1 }
            } else {
                HtmlFrame { path: path.clone(), src: src.clone() }
            }
        }
    }
}

/// Whether the server has the file at `url`, by a `HEAD` of it: `None` when no answer came.
#[cfg(target_arch = "wasm32")]
async fn found(url: &str) -> Option<bool> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window()?;
    let init = web_sys::RequestInit::new();
    init.set_method("HEAD");
    let answered = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str_and_init(url, &init))
        .await
        .ok()?;
    let response: web_sys::Response = answered.dyn_into().ok()?;
    Some(response.status() != 404)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame runs the document's scripts in an opaque origin and allows nothing else: no
    /// `allow-same-origin`, top navigation, forms or popups; it sends no referrer, and its address
    /// is the document's route under the project's base.
    #[test]
    fn an_html_document_is_drawn_in_a_frame_that_allows_its_scripts_alone() {
        let html = dioxus_ssr::render_element(rsx! {
            HtmlFrame { path: "docs/analysis/readout.html", src: "/alpha/docs/analysis/readout.html" }
        });
        assert!(
            html.contains(r#"<p class="eyebrow">docs/analysis/readout.html</p>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"src="/alpha/docs/analysis/readout.html""#),
            "{html}"
        );
        assert!(html.contains(r#"sandbox="allow-scripts""#), "{html}");
        assert!(html.contains(r#"title="readout.html""#), "{html}");
        assert!(html.contains(r#"referrerpolicy="no-referrer""#), "{html}");
        assert_eq!(html.matches("sandbox=").count(), 1, "{html}");
        for allowed in ["same-origin", "top-navigation", "forms", "popups", "modals"] {
            assert!(!html.contains(allowed), "{allowed} in {html}");
        }
    }
}
