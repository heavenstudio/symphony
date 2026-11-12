//! The one block a page shows in place of what it could not show: a title, a plain sentence, the
//! way back and a retry. A page tells the two cases apart: the server has no such thing (a 404),
//! or the read did not finish: the server could not be reached, or it answered with an error of
//! its own, which the block says with its status. The block says it once: the page's notices
//! leave it out while the block stands.

use dioxus::prelude::*;

/// What a page shows when its read did not bring what it names.
#[component]
pub fn Trouble(
    title: String,
    text: String,
    /// Where to go instead, as `(path, label)`: a path of the site, which the link writes under
    /// the project's base.
    #[props(default)]
    back: Option<(String, String)>,
    /// Reads again; a page that retries on its own still offers it.
    #[props(default)]
    retry: Option<EventHandler<()>>,
) -> Element {
    rsx! {
        section { class: "trouble card", role: "status",
            h1 { "{title}" }
            p { "{text}" }
            p { class: "trouble-actions",
                // A plain action: the entry modules that draw this block keep no mouse-event decoder.
                if let Some(retry) = retry {
                    crate::components::button::Button { r#type: "button", actions: crate::entry_events::click(move || retry.call(())), "Retry" }
                }
                if let Some((href, label)) = back {
                    a { class: "pill", href: crate::project::href(&href), "{label}" }
                }
            }
        }
    }
}

/// A region's `data-ready` while it has nothing of its own to show: `missing` when the server has
/// no such thing, `failed` when its read did not bring it, else `loading`. The first two are final:
/// whatever waits for the page to draw (`symphony shot`, a test) stops waiting there, and can tell
/// which it was.
pub fn ready(missing: bool, failed: bool) -> &'static str {
    if missing {
        "missing"
    } else if failed {
        "failed"
    } else {
        "loading"
    }
}

/// What a read fails with when no answer came at all (`request.rs`): the fetch itself failed.
pub const UNREACHABLE: &str = "The server could not be reached";

/// What a read fails with when the server answered `status` (`request.rs`): its own `error`, when
/// it gave one, then the status, so a page says what the server said rather than blaming the
/// connection.
pub fn answered(status: u16, error: Option<&str>) -> String {
    match error.map(|error| error.trim().trim_end_matches('.')) {
        Some(error) if !error.is_empty() => format!("{error} (the server answered {status})"),
        _ => format!("The server answered {status}"),
    }
}

/// The sentence for a read that did not finish, from the reason the transport gave: the server's
/// own error and status, or that it could not be reached, said once. Only a server that could not
/// be reached sends the reader to the connection.
pub fn failed(why: &str) -> String {
    let why = why.trim().trim_end_matches('.');
    if why.is_empty() {
        "The read did not finish. Try again.".into()
    } else if why == UNREACHABLE {
        format!("{UNREACHABLE}. Check the connection and try again.")
    } else {
        format!("{why}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus_ssr::render_element as render;

    /// The block with a retry, built inside the runtime its handler needs.
    #[component]
    fn Retried() -> Element {
        rsx! {
            Trouble {
                title: "No task nope",
                text: "The board has no task by this name.",
                back: Some(("/tasks".to_string(), "All tasks".to_string())),
                retry: move |_| {},
            }
        }
    }

    #[test]
    fn the_block_says_what_is_missing_and_offers_the_way_back_and_a_retry() {
        let html = render(rsx! { Retried {} });
        assert!(
            html.contains(r#"<section class="trouble card" role="status"><h1>No task nope</h1><p>The board has no task by this name.</p>"#),
            "{html}"
        );
        assert!(html.contains(">Retry</button>"), "{html}");
        assert!(
            html.contains(r#"<a class="pill" href="/tasks">All tasks</a>"#),
            "{html}"
        );
        let bare = render(rsx! { Trouble { title: "Gone", text: "Nothing here." } });
        assert!(!bare.contains("Retry") && !bare.contains("<a "), "{bare}");
    }

    #[test]
    fn a_region_without_its_content_is_missing_failed_or_still_loading() {
        assert_eq!(ready(true, false), "missing");
        assert_eq!(ready(true, true), "missing", "a 404 is said over a failed read");
        assert_eq!(ready(false, true), "failed");
        assert_eq!(ready(false, false), "loading");
    }

    #[test]
    fn a_refused_read_says_the_servers_own_error_and_its_status() {
        assert_eq!(
            answered(503, Some("Board snapshot unavailable.")),
            "Board snapshot unavailable (the server answered 503)"
        );
        assert_eq!(answered(500, Some("  ")), "The server answered 500");
        assert_eq!(answered(502, None), "The server answered 502");
    }

    #[test]
    fn a_failed_read_says_what_failed_once_and_only_an_unreachable_server_names_the_connection() {
        assert_eq!(
            failed(&answered(503, Some("Board snapshot unavailable"))),
            "Board snapshot unavailable (the server answered 503)."
        );
        assert_eq!(
            failed(UNREACHABLE),
            "The server could not be reached. Check the connection and try again."
        );
        assert_eq!(failed("  "), "The read did not finish. Try again.");
    }
}
