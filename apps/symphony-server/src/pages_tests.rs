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
    assert!(page.contains(&format!("<link rel=\"icon\" href=\"{ICON}\">")));
    assert!(page.contains("--background:#fbfaf7"), "the client's tokens");
    assert!(page.contains(".app-header {"), "the client's frame");
    assert!(page.contains(".projects li {"), "the pages' own styles");
    assert_eq!(page.matches(&format!("id=\"{NOTICE_STYLE}\"")).count(), 1);
    assert!(
        page.contains(
            "<header class=\"app-header\" id=\"top\"><a href=\"/\" class=\"active\">⌂ projects</a>"
        ),
        "{page}"
    );
    assert!(page.contains("<span>not found</span>"), "{page}");
    assert_eq!(notice_of(&page), "");
    assert!(page.contains("</section><p>Body</p></main>"), "{page}");
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
    assert!(page.contains("href=\"/lantern/look/icon.svg?v="), "{page}");
    assert!(!page.contains(ICON), "the theme's icon wins: {page}");
    assert!(page.contains("--primary:#a33;"), "{page}");
    let theme = page.find("--primary:#a33;").unwrap();
    assert!(
        page.find("--primary:#2f5d9e").unwrap() < theme,
        "the theme's tokens come after the client's: {page}"
    );
    for link in [
        "<a href=\"/\" class=\"project-switch\">Change board</a>",
        "<a href=\"/lantern/\">⌂ board</a>",
        "<a href=\"/lantern/docs/\">☰ docs</a>",
        "<a href=\"/lantern/tasks\">▦ tasks</a>",
        "<a href=\"/lantern/agents\">◉ agents</a>",
    ] {
        assert!(page.contains(link), "{link}: {page}");
    }
    assert_eq!(notice_of(&page), said);
    // A server of its own has no project list to lead to, and its icon is the Symphony's.
    let alone = lantern("");
    let page = project(&alone.root, "", "Not found", "not found", "");
    assert!(!page.contains("Change board"), "{page}");
    assert!(page.contains("<a href=\"/tasks\">▦ tasks</a>"), "{page}");
    assert!(page.contains(&format!("<link rel=\"icon\" href=\"{ICON}\">")));
    assert!(
        page.contains("<title>Not found · Lantern</title>"),
        "{page}"
    );
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
    assert!(said.contains("<a href=\"/lantern/\">board</a>"), "{said}");
    assert!(
        said.contains("<a href=\"/lantern/docs/\">docs</a>"),
        "{said}"
    );
    assert!(said.contains("<a href=\"/\">the list</a>"), "{said}");
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
    assert!(said.contains("<a href=\"/lantern/\">board</a>"), "{said}");
    let plain = body(gone_page(&tree, None)).await;
    assert!(
        plain.contains("<a href=\"/lantern/tasks\">Open this page"),
        "{plain}"
    );
}

fn gone_page(tree: &Tree, query: Option<&str>) -> Response {
    gone(&tree.root, "/lantern", "x", "/tasks", query)
}
