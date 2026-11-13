//! [`super::resolved`] and [`super::unescaped`]: the path a link names from the checkout's root.

use super::{resolved, unescaped};

/// `%XX` escapes are read as the browser reads them; one that is not two hex digits, or bytes that
/// do not read as UTF-8, leave the path as written.
#[test]
fn escapes_read_as_the_browser_asks_for_the_file() {
    assert_eq!(unescaped("plain.png"), "plain.png");
    assert_eq!(unescaped("my%20pic.png"), "my pic.png");
    assert_eq!(unescaped("%E2%9C%93.png"), "✓.png");
    assert_eq!(unescaped("100%zz.png"), "100%zz.png");
    assert_eq!(unescaped("ends%2"), "ends%2");
    assert_eq!(unescaped("bad%ff.png"), "bad%ff.png");
}

/// A link names its path from the root: relative to its document, or from the root when it starts
/// with `/`, its query and anchor dropped and its escapes read; an outside URL names none.
#[test]
fn a_link_names_its_path_from_the_root() {
    assert_eq!(
        resolved("docs/analysis/a.md", "../pic%20one.png?v=2#top"),
        Some("docs/pic one.png".into())
    );
    assert_eq!(
        resolved("docs/a.md", "/artifacts/x.png"),
        Some("artifacts/x.png".into())
    );
    assert_eq!(resolved("docs/a.md", "https://example.com/x.png"), None);
    assert_eq!(resolved("docs/a.md", "data:image/png;base64,AAAA"), None);
}
