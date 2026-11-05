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
    assert!(page.contains("<link rel=\"icon\" href=\"data:,\">"));
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
    let shell = page
        .replace(&Project::element(&view), "")
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
    let tokens = format!("<style {THEME_MARK}>:root{{--primary:#a33;}}\n</style>");
    let sheet = "<link rel=\"stylesheet\" href=\"/lantern/look/look.css?v=";
    let (tokens_at, sheet_at) = (page.find(&tokens).unwrap(), page.find(sheet).unwrap());
    assert!(tokens_at < sheet_at && sheet_at < page.find("</head>").unwrap());
    assert!(page.contains("<html data-layout-header=\"compact\" data-layout-home=\"focus\" lang"));
    assert_eq!(view(&page).base, "/lantern");
    assert!(
        page.contains("</script><p>banner</p><div id=\"main\">"),
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
        assert!(
            page.contains("<link rel=\"icon\" href=\"data:,\">"),
            "{theme}"
        );
    }
}

#[test]
fn a_shell_without_its_marks_is_served_as_it_is_but_for_the_project() {
    let tree = project("[theme.tokens]\nprimary = \"#123\"\n");
    assert_eq!(page("plain text", &tree.root, "", ""), "plain text");
    assert_eq!(
        page("plain text", &tree.root, "", "<b>x</b>"),
        "<b>x</b>plain text"
    );
}
