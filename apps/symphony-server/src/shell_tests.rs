//! The shell a project's pages are served in, with each part of its theme package present or
//! absent.

use super::*;
use crate::testing::Tree;

const SHELL: &str = "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n\
<title>Symphony</title><link rel=\"icon\" href=\"data:,\"></head><body><div id=\"main\"></div>\n\
<script type=\"module\" src=\"/assets/shell.js\"></script></body></html>\n";

fn project(theme: &str) -> Tree {
    let tree = Tree::new();
    tree.write("docs/README.md", "# Docs\n");
    tree.write(
        "symphony.toml",
        &format!("[project]\nname = \"Lantern\"\nid = \"lantern\"\n{theme}"),
    );
    tree
}

/// The project view the page carries, read back from its element.
fn view(page: &str) -> symphony_api::ProjectView {
    let open = format!(
        "<script type=\"application/json\" id=\"{}\">",
        symphony_api::PROJECT_ELEMENT
    );
    let start = page.find(&open).expect("the page carries the project") + open.len();
    let end = start + page[start..].find("</script>").unwrap();
    serde_json::from_str(&page[start..end].replace("\\u003c", "<")).unwrap()
}

#[test]
fn with_no_package_the_symphony_defaults_stand_and_only_the_project_rides_in() {
    let tree = project("");
    let page = page(SHELL, &tree.root, "", "");
    assert!(page.contains("<title>Lantern</title>"), "{page}");
    assert!(!page.contains(NO_ICON), "{page}");
    assert!(page.contains(&crate::mark::links("")), "{page}");
    assert!(
        !page.contains(THEME_MARK),
        "no theme element without a package: {page}"
    );
    assert!(
        page.contains("<html lang=\"en\"><head>"),
        "no layout without a package"
    );
    let view = view(&page);
    assert_eq!(view.base, "");
    assert_eq!(view.name, "Lantern");
    assert_eq!(
        (view.wordmark.as_str(), view.mark.as_str()),
        ("", ""),
        "the header opens with the Symphony's mark and wordmark"
    );
    assert_eq!(view.guide, "", "no review guide, no link to one");
    tree.write("docs/REVIEW.md", "# Review\n");
    assert_eq!(
        self::view(&super::page(SHELL, &tree.root, "", "")).guide,
        "docs/REVIEW.md",
        "the page names the project's review guide by its route"
    );
    let shell = page
        .replace(&Project::element(&view), "")
        .replace(&crate::mark::links(""), NO_ICON)
        .replace("Lantern", "Symphony");
    assert_eq!(shell, SHELL, "nothing else of the shell changes");
}

#[test]
fn a_package_names_the_tab_and_sets_icon_tokens_stylesheet_and_layouts_under_the_base() {
    let tree = project(
        "[theme]\nname = \"Lantern <Works>\"\nicon = \"look/icon.svg\"\nstylesheet = \
         \"look/look.css\"\n[theme.tokens]\nprimary = \"#a33\"\n[theme.layouts]\nheader = \
         \"compact\"\nhome = \"focus\"\n",
    );
    tree.write("look/icon.svg", "<svg/>");
    tree.write("look/look.css", "main{}");
    let page = page(SHELL, &tree.root, "/lantern", "<p>banner</p>");
    assert!(
        page.contains("<title>Lantern &lt;Works&gt;</title>"),
        "{page}"
    );
    assert!(
        page.contains("<link rel=\"icon\" href=\"/lantern/look/icon.svg?v="),
        "{page}"
    );
    assert!(
        !page.contains("data:image/svg+xml") && !page.contains("apple-touch-icon"),
        "none of the Symphony's icons beside the theme's: {page}"
    );
    let named = view(&page);
    assert_eq!(
        (named.wordmark.as_str(), named.mark.as_str()),
        (
            "Lantern <Works>",
            format!(
                "look/icon.svg?v={}",
                symphony::assets::validator(&tree.path("look/icon.svg"))
            )
            .as_str()
        ),
        "the header opens with the theme's own, versioned as the tab's icon is"
    );
    assert!(
        page.contains(&format!("href=\"/lantern/{}\"", named.mark)),
        "{page}"
    );
    let tokens = format!("<style {THEME_MARK}>:root{{--primary:#a33;}}\n</style>");
    let sheet = "<link rel=\"stylesheet\" href=\"/lantern/look/look.css?v=";
    let (tokens_at, sheet_at) = (page.find(&tokens).unwrap(), page.find(sheet).unwrap());
    assert!(tokens_at < sheet_at && sheet_at < page.find("</head>").unwrap());
    assert!(page.contains("<html data-layout-header=\"compact\" data-layout-home=\"focus\" lang"));
    assert_eq!(view(&page).base, "/lantern");
    assert!(
        page.contains("<div id=\"main\"><section id=\"shell-notices\" aria-label=\"Server notices\"><p>banner</p></section>"),
        "{page}"
    );
    assert!(
        matches!((page.find("shell-notices-style"), page.find("</head>")),
            (Some(style), Some(head)) if style < head),
        "{page}"
    );
}

#[test]
fn a_changed_file_gets_a_new_version_and_a_missing_one_none() {
    let tree = project("[theme]\nstylesheet = \"look/look.css\"\n");
    tree.write("look/look.css", "main{}");
    let first = page(SHELL, &tree.root, "", "");
    tree.write("look/look.css", "main{color:red}");
    let second = page(SHELL, &tree.root, "", "");
    let href = |page: &str| {
        let at = page.find("href=\"/look/look.css").unwrap() + "href=\"".len();
        page[at..at + page[at..].find('"').unwrap()].to_string()
    };
    assert_ne!(href(&first), href(&second));
    assert!(href(&first).starts_with("/look/look.css?v="));
    assert_eq!(
        versioned(&tree.root, "/x", "look/none.css"),
        "/x/look/none.css"
    );
}

/// What `url`, a path under the base `/lantern`, is answered with, asked with `headers`: its
/// status, its `Cache-Control` and `ETag`, and its body.
async fn served(root: &Path, url: &str, headers: &[(&str, &str)]) -> (u16, String, String, String) {
    let path = url.strip_prefix("/lantern").expect("a path under the base");
    let gate = crate::testing::open_gate();
    let shared = crate::front::Shared::project(1, 1234, root, None, gate, "/lantern");
    let mut request = axum::http::Request::builder().uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request.body(()).unwrap();
    let file = path.split('?').next().unwrap();
    let response = crate::media_files::answer(&shared, root, file, &request).await;
    let said = |name| {
        response
            .headers()
            .get(name)
            .map_or("", |value| value.to_str().unwrap())
            .to_string()
    };
    let (status, control, etag) = (
        response.status().as_u16(),
        said(axum::http::header::CACHE_CONTROL),
        said(axum::http::header::ETAG),
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        control,
        etag,
        String::from_utf8(body.to_vec()).unwrap(),
    )
}

/// The theme files the page asks for, under the base: its stylesheet, its tab's icon and its
/// header's icon, each with the version the shell gave it.
fn theme_files(page: &str) -> [String; 3] {
    let href = |open: &str| {
        let at = page
            .find(open)
            .unwrap_or_else(|| panic!("{open} in {page}"))
            + open.len();
        page[at..at + page[at..].find('"').unwrap()].to_string()
    };
    [
        href("<link rel=\"stylesheet\" href=\""),
        href("<link rel=\"icon\" href=\""),
        format!("/lantern/{}", view(page).mark),
    ]
}

/// A theme file asked with the shell's own version is answered as one to keep for good; once it
/// changes, the shell names it by another version, and a request with the old one is answered
/// with the new bytes only to revalidate, never to keep. Asked with no version, it is revalidated.
#[tokio::test]
async fn a_theme_file_asked_with_the_shells_version_is_kept_until_it_changes() {
    let tree = Tree::new();
    let root = crate::testing::repository_holding(
        &tree,
        "lantern",
        "symphony.toml",
        "[project]\nname = \"Lantern\"\nid = \"lantern\"\n[theme]\nicon = \"look/icon.svg\"\n\
         stylesheet = \"look/look.css\"\n",
    );
    tree.write("lantern/look/icon.svg", "<svg/>");
    tree.write("lantern/look/look.css", "main{}");
    let first = theme_files(&page(SHELL, &root, "/lantern", ""));
    assert_eq!(
        first[1], first[2],
        "the header's icon is the tab's, under the one version"
    );
    for url in &first {
        let (status, control, etag, _) = served(&root, url, &[]).await;
        assert_eq!(
            (status, control.as_str()),
            (200, symphony::assets::KEEP),
            "{url}"
        );
        assert_eq!(
            format!("\"{}\"", &url[url.find("?v=").unwrap() + 3..]),
            etag,
            "{url}: the version is the file's validator"
        );
    }
    tree.write("lantern/look/look.css", "main{color:red}");
    let second = theme_files(&page(SHELL, &root, "/lantern", ""));
    assert_ne!(first[0], second[0], "a changed file is named anew");
    assert_eq!(first[1], second[1], "an unchanged one keeps its version");
    let (status, control, etag, body) = served(&root, &first[0], &[]).await;
    assert_eq!(
        (status, control.as_str(), body.as_str()),
        (200, symphony::assets::REVALIDATE, "main{color:red}"),
        "the old version's request is not kept as the new bytes"
    );
    let (_, kept, new_etag, _) = served(&root, &second[0], &[]).await;
    assert_eq!(
        (kept.as_str(), new_etag.as_str()),
        (symphony::assets::KEEP, etag.as_str())
    );
    let (status, control, plain_etag, _) = served(&root, "/lantern/look/look.css", &[]).await;
    assert_eq!(
        (status, control.as_str(), plain_etag.as_str()),
        (200, symphony::assets::REVALIDATE, etag.as_str()),
        "with no version it is revalidated by its tag"
    );
    let (status, _, _, body) = served(
        &root,
        "/lantern/look/look.css",
        &[("If-None-Match", etag.as_str())],
    )
    .await;
    assert_eq!((status, body.as_str()), (304, ""));
}

#[test]
fn each_part_of_a_package_is_absent_on_its_own() {
    for (theme, absent) in [
        ("[theme]\nname = \"Named\"\n", "data-layout"),
        ("[theme.tokens]\nprimary = \"#123\"\n", "rel=\"stylesheet\""),
        ("[theme.layouts]\nheader = \"compact\"\n", "<style"),
        ("[theme.layouts]\nheader = \"default\"\n", "data-layout"),
    ] {
        let tree = project(theme);
        let page = page(SHELL, &tree.root, "", "");
        assert!(!page.contains(absent), "{theme}: {page}");
        assert!(page.contains(&crate::mark::links("")), "{theme}");
    }
}

#[test]
fn a_shell_without_its_marks_is_served_as_it_is_but_for_the_project() {
    let tree = project("[theme.tokens]\nprimary = \"#123\"\n");
    assert_eq!(page("plain text", &tree.root, "", ""), "plain text");
    let warning = page("plain text", &tree.root, "", "<b>x</b>");
    assert!(
        warning.contains("aria-label=\"Server notices\"><b>x</b></section>"),
        "{warning}"
    );
    assert!(warning.ends_with("plain text"), "{warning}");
}

#[test]
fn the_eager_shell_region_receives_the_warning_once_without_repeating_styles() {
    let tree = project("");
    let shell = SHELL.replace(
        "</head>",
        "<style id=\"shell-notices-style\">/* packaged styles */</style></head>",
    ).replace(
        "<div id=\"main\">",
        "<div id=\"main\"><section id=\"shell-notices\" aria-label=\"Server notices\"></section>",
    );
    let warning = crate::pages::notice("status", "<p><strong>Nothing is here</strong></p>");
    let page = page(&shell, &tree.root, "", &warning);
    assert_eq!(
        page.matches("id=\"shell-notices-style\"").count(),
        1,
        "{page}"
    );
    assert_eq!(page.matches("id=\"shell-notices\"").count(), 1, "{page}");
    assert!(
        page.contains(&format!(
            "aria-label=\"Server notices\">{warning}</section>"
        )),
        "{page}"
    );
}

#[test]
fn an_older_shell_with_body_attributes_keeps_the_notice_in_its_body() {
    let tree = project("");
    let page = page(
        "<html><head></head><body class=\"old\">content</body></html>",
        &tree.root,
        "",
        "warning",
    );
    assert!(page.contains("<body class=\"old\"><section id=\"shell-notices\" aria-label=\"Server notices\">warning</section>"), "{page}");
}

/// A page served to a reader names them, and the person at the machine as they are shown, in the
/// view it carries, so the header and each thread draw them with nothing more asked; a page served
/// for nobody names neither, and a name that would close the element is carried as text.
#[test]
fn a_page_served_to_a_reader_carries_who_they_are_and_the_machines_person() {
    let tree = project("");
    let nobody = view(&page(SHELL, &tree.root, "", ""));
    assert_eq!((nobody.you, nobody.machine), (None, None));
    let named = |name: &str, shown: &str| symphony_api::PersonName {
        name: name.into(),
        shown: shown.into(),
    };
    let reader = Reader {
        you: Some(named("ana", "Ana </script><b>Lima")),
        machine: Some(named("stefano", "Stefano")),
    };
    let page = page_for(SHELL, &tree.root, "", "", &reader);
    assert!(!page.contains("</script><b>"), "{page}");
    let carried = view(&page);
    assert_eq!(carried.you, reader.you);
    assert_eq!(carried.machine, reader.machine);
    assert_eq!(carried.name, "Lantern", "the rest of the view as before");
}
