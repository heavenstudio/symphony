//! The project the page belongs to: its name, its register, its plugins' navigation and its
//! entrypoints, which the server inlines in the page it serves (`symphony_api::PROJECT_ELEMENT`),
//! so the first paint asks for nothing. A page without it is a plain project with no plugin pages
//! and the convention's one entrypoint.

use symphony_api::{NavItem, ProjectView};

/// The plugin page `path` opens (`shelf`, from `/shelf`): the item whose link it is.
pub fn nav_page<'a>(nav: &'a [NavItem], path: &str) -> Option<&'a NavItem> {
    let path = path.trim_matches('/');
    nav.iter()
        .find(|item| !path.is_empty() && item.href.trim_matches('/') == path)
}

/// The item whose page shows `document`, the route's path.
pub fn nav_for_document<'a>(nav: &'a [NavItem], document: &str) -> Option<&'a NavItem> {
    nav.iter().find(|item| item.document == document)
}

/// The page's project, read from its element once.
#[cfg(target_arch = "wasm32")]
pub fn current() -> ProjectView {
    thread_local! {
        static CURRENT: ProjectView = read();
    }
    CURRENT.with(ProjectView::clone)
}

#[cfg(target_arch = "wasm32")]
fn read() -> ProjectView {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(symphony_api::PROJECT_ELEMENT))
        .and_then(|element| element.text_content())
        .and_then(|text| js_sys::JSON::parse(&text).ok())
        .and_then(|value| serde_wasm_bindgen::from_value(value).ok())
        .unwrap_or_default()
}

/// Off the browser there is no served page: the plain project.
#[cfg(not(target_arch = "wasm32"))]
pub fn current() -> ProjectView {
    ProjectView::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav() -> Vec<NavItem> {
        vec![
            NavItem {
                label: "▤ shelf".into(),
                title: "Shelf items".into(),
                href: "/shelf".into(),
                document: "docs/SHELF.md".into(),
            },
            NavItem {
                label: "◇ maps".into(),
                title: "Maps".into(),
                href: "/maps/".into(),
                document: "docs/MAPS.md".into(),
            },
        ]
    }

    #[test]
    fn a_route_finds_its_plugin_page_with_or_without_slashes() {
        let nav = nav();
        for (path, title) in [
            ("shelf", Some("Shelf items")),
            ("/shelf/", Some("Shelf items")),
            ("maps", Some("Maps")),
            ("", None),
            ("tasks", None),
            ("shelf/extra", None),
        ] {
            assert_eq!(
                nav_page(&nav, path).map(|item| item.title.as_str()),
                title,
                "{path}"
            );
        }
        assert_eq!(nav_page(&[], "shelf"), None);
    }

    #[test]
    fn a_document_finds_the_page_that_shows_it() {
        let nav = nav();
        assert_eq!(
            nav_for_document(&nav, "docs/MAPS.md").map(|item| item.href.as_str()),
            Some("/maps/")
        );
        assert_eq!(nav_for_document(&nav, "docs/OTHER.md"), None);
        assert_eq!(current(), ProjectView::default());
    }

    #[test]
    fn the_shell_yields_the_same_routes_to_the_symphony_as_the_server() {
        let shell = include_str!("../assets/shell.js");
        let list = shell
            .lines()
            .find_map(|line| line.strip_prefix("const SYMPHONY_ROUTES = "))
            .and_then(|rest| rest.strip_suffix(';'))
            .expect("shell.js names the Symphony's routes");
        let routes: Vec<String> = serde_json::from_str(list).unwrap();
        assert_eq!(routes, symphony_api::SYMPHONY_ROUTES);
    }
}
