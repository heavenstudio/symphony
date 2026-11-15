use super::*;

fn studio() -> Server {
    Server::parse("http://studio.tail1234.ts.net:8765").unwrap()
}

#[test]
fn the_servers_own_origin_and_a_frames_own_document_stay_in_its_window() {
    let server = studio();
    for to in [
        "http://studio.tail1234.ts.net:8765/",
        "http://studio.tail1234.ts.net:8765/symphony/tasks?view=1#top",
        "http://STUDIO.tail1234.ts.net:8765/other/",
        "about:blank",
        "about:srcdoc",
        "blob:http://studio.tail1234.ts.net:8765/6c1f3a1e-0000-4000-8000-000000000000",
    ] {
        assert_eq!(navigation(&server, to), Go::Stay, "{to}");
    }
}

#[test]
fn another_origin_opens_in_the_system_browser_and_the_window_stays() {
    let server = studio();
    for (to, opened) in [
        ("https://example.com/a?b=c", "https://example.com/a?b=c"),
        ("http://example.com", "http://example.com/"),
        // The same host on another port or scheme is another origin.
        (
            "http://studio.tail1234.ts.net:8766/",
            "http://studio.tail1234.ts.net:8766/",
        ),
        (
            "https://studio.tail1234.ts.net:8765/",
            "https://studio.tail1234.ts.net:8765/",
        ),
        (
            "http://127.0.0.1:8765/symphony/",
            "http://127.0.0.1:8765/symphony/",
        ),
    ] {
        assert_eq!(navigation(&server, to), Go::Browser(opened.into()), "{to}");
    }
}

#[test]
fn every_other_scheme_is_refused() {
    let server = studio();
    for to in [
        "file:///Users/someone/.ssh/id_ed25519",
        "file:///Applications/Calculator.app",
        "iterm2://open",
        "vscode://file/Users/someone/x.rs",
        "javascript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "about:preferences",
        "blob:https://example.com/6c1f3a1e-0000-4000-8000-000000000000",
        "mailto:someone@example.com",
        "not an address",
        "",
    ] {
        assert_eq!(navigation(&server, to), Go::Refuse, "{to}");
    }
}

#[test]
fn a_new_window_or_a_download_opens_http_in_the_system_browser_and_nothing_else() {
    for (to, go) in [
        (
            "http://studio.tail1234.ts.net:8765/symphony/media/a.png",
            Go::Browser("http://studio.tail1234.ts.net:8765/symphony/media/a.png".into()),
        ),
        (
            "https://example.com/",
            Go::Browser("https://example.com/".into()),
        ),
        ("file:///etc/hosts", Go::Refuse),
        ("about:blank", Go::Refuse),
        ("iterm2://open", Go::Refuse),
        ("blob:http://studio.tail1234.ts.net:8765/x", Go::Refuse),
        ("", Go::Refuse),
    ] {
        assert_eq!(new_window(to), go, "{to}");
    }
}
