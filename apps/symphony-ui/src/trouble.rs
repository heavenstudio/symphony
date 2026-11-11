//! The one block a page shows in place of what it could not show: a title, a plain sentence, the
//! way back and a retry. A page tells the two cases apart: the server has no such thing (a 404),
//! or the read did not finish (offline, or the server's own error).

use dioxus::prelude::*;

/// What a page shows when its read did not bring what it names.
#[component]
pub fn Trouble(
    title: String,
    text: String,
    /// Where to go instead, as `(href, label)`.
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
                    a { class: "pill", href: "{href}", "{label}" }
                }
            }
        }
    }
}

/// The sentence for a read that did not finish, from the reason the transport gave: the page is
/// not missing, so the reader is sent to the connection, not away.
pub fn failed(why: &str) -> String {
    let why = why.trim().trim_end_matches('.');
    if why.is_empty() {
        "The read did not finish. Check the connection and try again.".into()
    } else {
        format!("The read did not finish: {why}. Check the connection and try again.")
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
    fn a_failed_read_says_why_and_sends_the_reader_to_the_connection() {
        assert_eq!(
            failed("Board snapshot unavailable."),
            "The read did not finish: Board snapshot unavailable. Check the connection and try again."
        );
        assert_eq!(
            failed("  "),
            "The read did not finish. Check the connection and try again."
        );
    }
}
