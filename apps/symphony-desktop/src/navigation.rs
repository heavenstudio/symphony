//! Where a server's window may go. Its web view stays on that server's origin; a link that leaves
//! it, and every new window a page asks for, opens in the system browser; any other scheme is
//! refused, so a page can never have the app open a local file or start an application.

use url::Url;

use crate::address::Server;

/// What the app does with an address a server's page asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Go {
    /// The web view loads it: the server's own origin, or a frame's document made in the page.
    Stay,
    /// The system browser opens it, an `http` or `https` address; the web view stays where it is.
    Browser(String),
    /// Nothing opens it.
    Refuse,
}

/// What a navigation to `to` in `server`'s window does. Every frame's navigation comes here, as the
/// web view tells none apart from the page's own: a frame's own document (`about:blank`,
/// `about:srcdoc`, a `blob:` of the server's origin) stays.
pub fn navigation(server: &Server, to: &str) -> Go {
    let Ok(url) = Url::parse(to) else {
        return Go::Refuse;
    };
    match url.scheme() {
        // A blob's origin is the page's that made it.
        "http" | "https" | "blob" if url.origin() == server.origin() => Go::Stay,
        "http" | "https" => Go::Browser(url.into()),
        "about" if matches!(url.path(), "blank" | "srcdoc") => Go::Stay,
        _ => Go::Refuse,
    }
}

/// What a page's new window (`window.open`, a link to `_blank`) at `to` does: the system browser
/// opens an `http` or `https` address, the server's own included, and nothing opens any other. The
/// same rule answers a download, which the app never writes to disk itself.
pub fn new_window(to: &str) -> Go {
    match Url::parse(to) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => Go::Browser(url.into()),
        _ => Go::Refuse,
    }
}

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
