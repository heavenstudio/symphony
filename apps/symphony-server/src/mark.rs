//! The Symphony's own mark, the fermata, and its wordmark: what a page shows where its project's
//! theme has no icon or name of its own, and what the machine's own pages show.
//!
//! `mark/icon.svg` is the tab's icon, a tile with the mark on it, its light and dark colours
//! chosen inside the file by the system's scheme (a browser passes a tab's icon nothing of the
//! page's Appearance). A page carries it inline as a data address, so it asks for nothing.
//! `mark/touch.png` is the same tile at 180 px for a phone's home screen, square and opaque as a
//! phone rounds it, asked for only when a page is put there. `mark/mark.svg` is the bare mark,
//! which a header draws inline in the page's `--primary`; the React client draws the same
//! (`apps/symphony-web/src/frame.tsx`).

use std::sync::LazyLock;

use axum::http::header;
use axum::response::{IntoResponse, Response};
use symphony::markup::escape;

const ICON: &str = include_str!("mark/icon.svg");
const MARK: &str = include_str!("mark/mark.svg");
const TOUCH: &[u8] = include_bytes!("mark/touch.png");

/// The Symphony's wordmark.
pub(crate) const WORDMARK: &str = "Symphony";

/// The query of the address's own icon that asks for the home screen's.
const TOUCH_QUERY: &str = "size=180";

/// The icon as a data address, as an attribute in double quotes holds it.
static ADDRESS: LazyLock<String> = LazyLock::new(|| address(ICON));

/// `svg` as a data address for an attribute in double quotes: its line ends gone, its double
/// quotes single, and what a URL reads as its own (`%`, `#`) or HTML as a tag written as bytes.
fn address(svg: &str) -> String {
    let mut address = String::from("data:image/svg+xml,");
    for letter in svg.chars() {
        match letter {
            '\n' | '\r' => {}
            '"' => address.push('\''),
            '%' => address.push_str("%25"),
            '#' => address.push_str("%23"),
            '<' => address.push_str("%3C"),
            '>' => address.push_str("%3E"),
            other => address.push(other),
        }
    }
    address
}

/// The icon links of a page served at `base` whose theme has no icon: the tab's, inline, and the
/// home screen's, by its address.
pub(crate) fn links(base: &str) -> String {
    format!(
        "<link rel=\"icon\" href=\"{}\"><link rel=\"apple-touch-icon\" \
         href=\"{}/favicon.ico?{TOUCH_QUERY}\">",
        *ADDRESS,
        escape(base)
    )
}

/// What a header opens with: `icon`, the address of a theme's own icon, else the mark; and
/// `name`, a theme's own name, else the wordmark. The React frame draws the same.
pub(crate) fn brand(name: &str, icon: &str) -> String {
    let mark = if icon.is_empty() {
        MARK.trim().to_string()
    } else {
        format!("<img class=\"app-mark\" src=\"{}\" alt=\"\">", escape(icon))
    };
    let word = if name.is_empty() {
        format!("<span class=\"app-wordmark\">{WORDMARK}</span>")
    } else {
        format!("<span class=\"app-name\">{}</span>", escape(name))
    };
    format!("<span class=\"app-brand\">{mark}{word}</span>")
}

/// Whether `query`, the address's own icon's, asks for the home screen's.
pub(crate) fn touch(query: Option<&str>) -> bool {
    query == Some(TOUCH_QUERY)
}

/// The Symphony's icon as the address's own (`/favicon.ico`) answers it: the home screen's PNG
/// when `query` asks for it, else the tab's SVG. Neither changes within a build, so a browser
/// keeps it for a day.
pub(crate) fn answer(query: Option<&str>) -> Response {
    let (kind, body): (&str, &'static [u8]) = if touch(query) {
        ("image/png", TOUCH)
    } else {
        ("image/svg+xml", ICON.as_bytes())
    };
    (
        [
            (header::CONTENT_TYPE, kind),
            (header::CACHE_CONTROL, "public, max-age=86400"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
#[path = "mark_tests.rs"]
mod tests;
