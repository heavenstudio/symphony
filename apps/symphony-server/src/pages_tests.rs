//! The server's own pages: the client's look, a project's theme and header, and what a miss
//! answers, as a page or as an API error.

use super::*;
use crate::testing::Tree;

/// A project `lantern` with a plugin-free convention, and, given `theme`, a theme package.
fn lantern(theme: &str) -> Tree {
    let tree = Tree::new();
    tree.write("docs/README.md", "# Docs\n");
    tree.write(
        "symphony.toml",
        &format!("[project]\nname = \"Lantern\"\nid = \"lantern\"\n{theme}"),
    );
    tree
}

/// What a header in the Symphony's own look opens with: its mark and its wordmark.
const SYMPHONYS: &str = "<span class=\"app-brand\"><svg \
                         xmlns=\"http://www.w3.org/2000/svg\" class=\"app-mark\" viewBox=\"4.5 9.5 \
                         23 14.4\" aria-hidden=\"true\"><path d=\"M5 21A11 11 0 0 1 27 21H24.6A8.6 \
                         7.2 0 0 0 7.4 21Z\"/><circle cx=\"16\" cy=\"19.6\" r=\"2.9\"/></svg><span \
                         class=\"app-wordmark\">Symphony</span></span>";

async fn body(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn kind(response: &Response) -> &str {
    response.headers()[header::CONTENT_TYPE].to_str().unwrap()
}

/// The text of the page's notice region.
fn notice_of(page: &str) -> &str {
    let start = page.find(REGION).expect("a notice region") + REGION.len();
    &page[start..start + page[start..].find("</section>").unwrap()]
}

#[test]
fn the_tokens_are_the_clients_own_light_and_dark_and_none_of_its_page_rules() {
    let client = tokens(BOARD);
    for token in [
        "--background:",
        "--card:",
        "--font-sans:",
        "--r-pill:",
        "--fs-sm:",
    ] {
        assert!(client.contains(token), "{token}: {client}");
    }
    assert!(client.starts_with(":root"), "{client}");
    assert!(
        client.contains("@media (prefers-color-scheme: dark)"),
        "{client}"
    );
    assert!(!client.contains("body {"), "{client}");
    assert!(!client.contains("html {"), "{client}");
    // A sheet with no tokens gives none, and one with no page rules gives them all.
    assert_eq!(tokens(".x { color:red; }"), "");
    assert_eq!(tokens("a{}:root { --x:1; }"), ":root { --x:1; }");
}

#[test]
fn a_machine_page_wears_the_client_frame_and_the_symphony_icon() {
    let page = machine("Not found · <Symphony>", "not found", "", "<p>Body</p>");
    assert!(
        page.contains("<title>Not found · &lt;Symphony&gt;</title>"),
        "{page}"
    );
    assert!(!page.contains("href=\"data:,\""), "{page}");
    // Its icons are the Symphony's: the tab's inline, the home screen's at the address's own.
    assert!(page.contains(&crate::mark::links("")), "{page}");
    assert!(
        page.contains("<link rel=\"apple-touch-icon\" href=\"/favicon.ico?size=180\">"),
        "{page}"
    );
    assert!(page.contains("--background:#fbfaf7"), "the client's tokens");
    assert!(page.contains(".app-header {"), "the client's frame");
    assert!(page.contains(".projects li {"), "the pages' own styles");
    assert_eq!(page.matches(&format!("id=\"{NOTICE_STYLE}\"")).count(), 1);
    assert!(
        page.contains(".app-brand .app-wordmark {"),
        "the mark's rules"
    );
    // The header opens with the Symphony's mark and wordmark; a miss leads back to the list,
    // unlit, and its crumb says what it is.
    assert!(
        page.contains(&format!(
            "<header class=\"app-header\" id=\"top\">{SYMPHONYS}<nav class=\"app-pages scroll-row\" \
             aria-label=\"Pages\"><a href=\"/\">Projects</a></nav><nav class=\"crumbs\" \
             aria-label=\"Breadcrumbs\"><span>not found</span></nav></header>"
        )),
        "{page}"
    );
    assert_eq!(notice_of(&page), "");
    assert!(page.contains("</section><p>Body</p></main>"), "{page}");
}

/// The list itself is the lit page, and no crumb repeats its name.
#[test]
fn the_machine_list_lights_its_one_page_and_has_no_crumb() {
    let page = machine("Symphony", "", "", "<ul class=\"projects\"></ul>");
    assert!(
        page.contains(&format!(
            "<header class=\"app-header\" id=\"top\">{SYMPHONYS}<nav class=\"app-pages scroll-row\" \
             aria-label=\"Pages\"><a href=\"/\" class=\"active\" \
             aria-current=\"page\">Projects</a></nav></header>"
        )),
        "{page}"
    );
    assert!(!page.contains("class=\"crumbs\""), "{page}");
}

#[test]
fn a_project_page_takes_its_theme_and_the_header_its_client_pages_draw_under_its_base() {
    let tree = lantern(
        "[theme]\nname = \"Lantern <Works>\"\nicon = \"look/icon.svg\"\n[theme.tokens]\nprimary = \
         \"#a33\"\n",
    );
    tree.write("look/icon.svg", "<svg/>");
    let said = notice("status", "<p>Gone</p>");
    let page = project(&tree.root, "/lantern", "Not found", "not found", &said);
    assert!(
        page.contains("<title>Not found · Lantern &lt;Works&gt;</title>"),
        "{page}"
    );
    let version = symphony::assets::validator(&tree.path("look/icon.svg"));
    assert!(
        page.contains(&format!("href=\"/lantern/look/icon.svg?v={version}\"")),
        "{page}"
    );
    assert!(
        !page.contains("data:image/svg+xml") && !page.contains("apple-touch-icon"),
        "the theme's icon wins, and the Symphony's is on no home screen for it: {page}"
    );
    assert!(page.contains("--primary:#a33;"), "{page}");
    let theme = page.find("--primary:#a33;").unwrap();
    assert!(
        page.find("--primary:#2f5d9e").unwrap() < theme,
        "the theme's tokens come after the client's: {page}"
    );
    // The theme's own icon, versioned as the tab's is, and name, then the switch, then the
    // client's own pages in its order and with its names, under the base.
    assert!(
        page.contains(&format!(
            "<header class=\"app-header\" id=\"top\"><span class=\"app-brand\"><img \
             class=\"app-mark\" src=\"/lantern/look/icon.svg?v={version}\" alt=\"\"><span \
             class=\"app-name\">Lantern &lt;Works&gt;</span></span><a href=\"/\" \
             class=\"project-switch\">Projects</a><nav class=\"app-pages scroll-row\" \
             aria-label=\"Pages\"><a href=\"/lantern/\">Home</a><a \
             href=\"/lantern/queue\">Queue</a><a href=\"/lantern/docs/\">Docs</a><a \
             href=\"/lantern/tasks\">Board</a><a href=\"/lantern/reports\">Reports</a><a \
             href=\"/lantern/settings\">Settings</a><a href=\"/lantern/runs\">Runs</a><a \
             href=\"/lantern/agents\">Agents</a></nav><nav \
             class=\"crumbs\" aria-label=\"Breadcrumbs\"><span>not found</span></nav>"
        )),
        "{page}"
    );
    assert_eq!(notice_of(&page), said);
    // A server of its own has no project list to lead to, and its icon is the Symphony's.
    let alone = lantern("");
    let page = project(&alone.root, "", "Not found", "not found", "");
    // (The inlined frame stylesheet names the switch's class; the header holds no such link.)
    assert!(!page.contains("class=\"project-switch\""), "{page}");
    assert!(!page.contains(">Projects<"), "{page}");
    assert!(
        page.contains(&format!(
            "<header class=\"app-header\" id=\"top\">{SYMPHONYS}<nav class=\"app-pages scroll-row\""
        )),
        "{page}"
    );
    assert!(page.contains("<a href=\"/tasks\">Board</a>"), "{page}");
    assert!(page.contains(&crate::mark::links("")), "{page}");
    assert!(
        page.contains("<title>Not found · Lantern</title>"),
        "{page}"
    );
    // A theme that names itself and has no icon keeps the Symphony's mark beside its name, and
    // the Symphony's icons, under its base.
    let named = lantern("[theme]\nname = \"Works\"\n");
    let page = project(&named.root, "/lantern", "Not found", "not found", "");
    assert!(
        page.contains("</svg><span class=\"app-name\">Works</span></span>"),
        "{page}"
    );
    assert!(page.contains(&crate::mark::links("/lantern")), "{page}");
}

#[tokio::test]
async fn a_miss_off_the_api_is_the_projects_page_naming_what_was_asked() {
    let tree = lantern("");
    let page = missing(
        &tree.root,
        "/lantern",
        "/no%20such/<b>",
        &HeaderMap::new(),
        StatusCode::NOT_FOUND,
    );
    assert_eq!(page.status(), StatusCode::NOT_FOUND);
    assert_eq!(kind(&page), "text/html; charset=utf-8");
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
    let page = body(page).await;
    let said = notice_of(&page);
    assert!(
        said.contains("no page or file of Lantern answers"),
        "{said}"
    );
    assert!(
        said.contains("<code>/lantern/no such/&lt;b&gt;</code>"),
        "{said}"
    );
    // Its ways out name the pages as the header does.
    assert!(
        said.contains(
            "<p>Open this project's <a href=\"/lantern/\">Home</a> or <a href=\"/lantern/docs/\">Docs</a>, or another one from <a href=\"/\">Projects</a>.</p>"
        ),
        "{said}"
    );
    assert!(
        page.contains("<title>Not found · Lantern</title>"),
        "{page}"
    );

    let bad = missing(
        &tree.root,
        "/lantern",
        "/docs/%ZZ",
        &HeaderMap::new(),
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let bad = body(bad).await;
    let said = notice_of(&bad);
    assert!(said.contains("This address cannot be read"), "{said}");
    assert!(said.contains("<code>/lantern/docs/%ZZ</code>"), "{said}");
    assert!(
        bad.contains("<title>Bad address · Lantern</title>"),
        "{bad}"
    );
}

#[tokio::test]
async fn a_miss_under_the_api_or_a_data_read_of_the_queue_is_an_api_error() {
    let tree = lantern("");
    let json_accept: HeaderMap = [(header::ACCEPT, "application/json".parse().unwrap())]
        .into_iter()
        .collect();
    for (path, headers, status, said) in [
        (
            "/api/nope",
            HeaderMap::new(),
            StatusCode::NOT_FOUND,
            "no such API route: /api/nope",
        ),
        (
            "/@work~x/api/nope",
            HeaderMap::new(),
            StatusCode::NOT_FOUND,
            "no such API route: /api/nope",
        ),
        (
            "/api",
            HeaderMap::new(),
            StatusCode::NOT_FOUND,
            "no such API route: /api",
        ),
        (
            "/queue",
            json_accept.clone(),
            StatusCode::NOT_FOUND,
            "not found: /queue",
        ),
        (
            "/api/%FF",
            HeaderMap::new(),
            StatusCode::BAD_REQUEST,
            "invalid URL path: its percent-encoding is not UTF-8",
        ),
    ] {
        let response = missing(&tree.root, "/lantern", path, &headers, status);
        assert_eq!(response.status(), status, "{path}");
        assert_eq!(kind(&response), "application/json", "{path}");
        let answer: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(answer, serde_json::json!({ "error": said }), "{path}");
    }
    // The queue's page itself, and anything off the API, is a page.
    for (path, headers) in [
        ("/queue", HeaderMap::new()),
        ("/apis", json_accept.clone()),
        ("/docs/api/x.md", json_accept),
    ] {
        let response = missing(&tree.root, "", path, &headers, StatusCode::NOT_FOUND);
        assert_eq!(kind(&response), "text/html; charset=utf-8", "{path}");
    }
}

#[tokio::test]
async fn a_gone_branch_page_links_the_same_page_in_the_main_checkout() {
    let tree = lantern("");
    let gone = gone(
        &tree.root,
        "/lantern",
        "tools~old-work",
        "/docs/A b.md",
        Some("x=1&y=%20"),
    );
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    assert_eq!(kind(&gone), "text/html; charset=utf-8");
    let page = body(gone).await;
    let said = notice_of(&page);
    assert!(
        said.contains("The worktree of branch <q>tools/old-work</q> is gone"),
        "{said}"
    );
    assert!(
        said.contains("<a href=\"/lantern/docs/A%20b.md?x=1&amp;y=%20\">Open this page in the main checkout</a>"),
        "{said}"
    );
    assert!(
        said.contains("main checkout</a>, or its <a href=\"/lantern/\">Home</a>.</p>"),
        "{said}"
    );
    let plain = body(gone_page(&tree, None)).await;
    assert!(
        plain.contains("<a href=\"/lantern/tasks\">Open this page"),
        "{plain}"
    );
}

fn gone_page(tree: &Tree, query: Option<&str>) -> Response {
    gone(&tree.root, "/lantern", "x", "/tasks", query)
}

/// A route that starts with more than one slash (`/@gone//evil.example`) still links a path on
/// this server: `//evil.example` would be another site, on a server of its own above all.
#[tokio::test]
async fn a_gone_branch_page_never_links_off_this_server() {
    let tree = lantern("");
    for (base, route, href) in [
        ("", "//evil.example", "/evil.example"),
        ("", "///evil.example/x", "/evil.example/x"),
        ("", "/\\evil.example", "/%5Cevil.example"),
        ("", "", "/"),
        ("/lantern", "//evil.example", "/lantern/evil.example"),
    ] {
        let page = body(gone(&tree.root, base, "x", route, None)).await;
        let said = notice_of(&page);
        assert!(
            said.contains(&format!(
                "<a href=\"{href}\">Open this page in the main checkout</a>"
            )),
            "{base} {route}: {said}"
        );
        assert!(!said.contains("href=\"//"), "{base} {route}: {said}");
    }
}

/// On a phone the pages' header is the client's: its pages in a row of their own, which nothing in
/// the pages' own styles lays out again.
#[test]
fn a_phones_header_is_the_clients() {
    let style = &*STYLE;
    let frame = style
        .find("@media (max-width:760px) {\n  .app-header")
        .expect("the frame's phone header");
    assert!(
        style[frame..].contains(".app-pages { order:10; flex-basis:100%; gap:0;"),
        "the pages' row of its own"
    );
    assert!(!PAGES.contains(".app-header"), "{PAGES}");
    assert!(!PAGES.contains(".app-pages"), "{PAGES}");
}
