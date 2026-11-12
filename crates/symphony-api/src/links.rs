//! Which addresses a page may make a link of. The addresses a page links come from text it did not
//! write (a plugin's gate, an agent's session), and the projects of the machine's one server share
//! its origin: a `javascript:` link run there could act on any of them. So a page links only a
//! path of the app or a web address, and draws anything else as text; the server checks the same
//! where it can, so no page is handed one.

/// Whether a page may link to `url` as a web address: an `http://` or `https://` URL (in any case)
/// naming a host, with no space or control character in it.
pub fn web_address(url: &str) -> bool {
    let after = |scheme: &str| {
        url.get(..scheme.len())
            .filter(|start| start.eq_ignore_ascii_case(scheme))
            .and_then(|_| url.get(scheme.len()..))
    };
    let Some(rest) = ["https://", "http://"].into_iter().find_map(after) else {
        return false;
    };
    !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && rest
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|host| !host.is_empty())
}

/// Whether `path` is a path of this app (`/task/SLUG#message-ID`, `/docs/X.md#card`): one `/`
/// first, with no backslash or control character anywhere, since a browser reads `//host`, `/\host`
/// and a `/` a tab or newline split from the next as another host.
pub fn app_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.chars().any(|c| c.is_control() || c == '\\')
}

/// Whether a page may link to `link`: an [`app_path`] or a [`web_address`].
pub fn linkable(link: &str) -> bool {
    app_path(link) || web_address(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Addresses that would run script, read data in place or leave for another host while looking
    /// like a path of the app.
    const REFUSED: [&str; 17] = [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        " javascript:alert(1)",
        "\tjavascript:alert(1)",
        "\u{1}javascript:alert(1)",
        "java\nscript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "DATA:text/html;base64,PHNjcmlwdD4=",
        "vbscript:msgbox(1)",
        "//evil.example/x",
        "/\\evil.example/x",
        "/\t/evil.example",
        " /task/x",
        "\u{0}/task/x",
        "task/x",
        "",
        "mailto:owner@example.com",
    ];

    #[test]
    fn a_page_links_a_path_of_the_app_or_a_web_address_and_nothing_else() {
        for linked in [
            "/task/tools/slot-pool-by-tree",
            "/task/tools/x#message-124",
            "/docs/analysis/x.md#card",
            "/docs/a file.md",
            "/",
            "https://claude.ai/code/session_01abc",
            "http://mac.tail.ts.net:8080/x?y=1#z",
            "HTTPS://Example.com",
        ] {
            assert!(linkable(linked), "{linked:?}");
        }
        for refused in REFUSED {
            assert!(!linkable(refused), "{refused:?}");
        }
    }

    #[test]
    fn a_web_address_is_http_or_https_naming_a_host_with_no_space_or_control_character() {
        for linked in ["https://h", "Http://h", "https://h?q", "https://h#f"] {
            assert!(web_address(linked), "{linked:?}");
            assert!(!app_path(linked), "{linked:?}");
        }
        for refused in [
            "https",
            "https:/h",
            "https://",
            "https:///x",
            "https://h x",
            "https://h\u{a0}x",
            " https://h",
            "ftp://h/x",
            "/task/x",
        ] {
            assert!(!web_address(refused), "{refused:?}");
        }
    }

    #[test]
    fn a_path_of_the_app_has_one_slash_first_and_no_backslash_or_control_character() {
        assert!(app_path("/task/x"));
        for refused in ["//h", "/\\h", "/x\\y", "/x\ny", "/x\ty", "x/y", "https://h"] {
            assert!(!app_path(refused), "{refused:?}");
        }
    }
}
