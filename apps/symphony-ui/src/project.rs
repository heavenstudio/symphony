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

/// A link of the header: where it goes, its name, and whether it is the page shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderLink {
    pub href: String,
    pub label: String,
    pub active: bool,
}

/// The header's links on the page at `path`, a route without its leading slash (empty for the
/// home, `tasks`, `docs/A.md`, a plugin page's document), which `document` says is a document or
/// folder page: the Symphony's own pages ([`ProjectView::own_pages`]), then the plugins'. A task's
/// page is under the board, and a plugin's page is its own, not the documents'.
pub fn header_links(view: &ProjectView, path: &str, document: bool) -> Vec<HeaderLink> {
    let plugin = nav_for_document(&view.nav, path);
    let active = [
        path.is_empty() && !document,
        path == "queue",
        document && plugin.is_none(),
        path == "tasks" || path.starts_with("task/"),
        path == "agents",
    ];
    view.own_pages()
        .into_iter()
        .zip(active)
        .map(|((href, label), active)| HeaderLink {
            href,
            label: label.into(),
            active,
        })
        .chain(view.nav.iter().map(|item| HeaderLink {
            href: item.href.clone(),
            label: item.label.clone(),
            active: item.document == path,
        }))
        .collect()
}

/// The crumbs after the header's links on the page at `path` (as [`header_links`] reads it):
/// what the lit link does not already say. A document or folder shows each folder of its route,
/// the last its page ([`crate::docs::crumbs`]); a task's page, under the lit Board, its slug alone.
/// None where the route is a header link's own page (the home, the queue, the board, the agents,
/// the documents' folder page, a plugin's page).
pub fn crumbs<'a>(view: &ProjectView, path: &'a str) -> Vec<&'a str> {
    let (mut trail, under_tasks) = crate::docs::crumbs(path);
    if under_tasks {
        trail.remove(0);
        return trail;
    }
    let linked = |page: &str| {
        view.own_pages()
            .iter()
            .any(|(href, _)| href.trim_matches('/') == page)
    };
    match trail.as_slice() {
        _ if path.trim_matches('/').is_empty() => vec![],
        _ if nav_for_document(&view.nav, path).is_some() => vec![],
        [page] if linked(page) => vec![],
        _ => trail,
    }
}

/// `key` as `view`'s pages keep it in the browser: the projects of the machine's one server share
/// its origin, and so its storage, so each key takes the project's id first
/// (`alpha:review-quick-draft`); a server of its own has the origin to itself and keeps the key
/// as it is. `shell.js`'s `stored` is the same rule.
pub fn stored(view: &ProjectView, key: &str) -> String {
    match view.base.trim_start_matches('/') {
        "" => key.to_string(),
        id => format!("{id}:{key}"),
    }
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

/// The browser's page in the project: its path under the base, with its fragment.
#[cfg(target_arch = "wasm32")]
pub fn page() -> Option<String> {
    let location = web_sys::window()?.location();
    let path = location.pathname().ok()?;
    let hash = location.hash().unwrap_or_default();
    Some(format!("{}{hash}", current().strip(&path)))
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

    fn with_nav() -> ProjectView {
        ProjectView {
            nav: nav(),
            ..ProjectView::default()
        }
    }

    /// The names and the active link of each page, `*` marking it.
    fn header(view: &ProjectView, path: &str, document: bool) -> Vec<String> {
        header_links(view, path, document)
            .into_iter()
            .map(|link| {
                format!(
                    "{}{} {}",
                    if link.active { "*" } else { "" },
                    link.label,
                    link.href
                )
            })
            .collect()
    }

    #[test]
    fn the_header_names_its_pages_once_and_marks_the_one_shown() {
        let view = with_nav();
        assert_eq!(
            header(&view, "", false),
            [
                "*Home /",
                "Queue /queue",
                "Docs /docs/",
                "Board /tasks",
                "Agents /agents",
                "▤ shelf /shelf",
                "◇ maps /maps/"
            ]
        );
        let active = |path: &str, document: bool| -> Vec<String> {
            header_links(&view, path, document)
                .into_iter()
                .filter(|link| link.active)
                .map(|link| link.label)
                .collect()
        };
        for (path, document, shown) in [
            ("queue", false, "Queue"),
            ("docs/", true, "Docs"),
            ("docs/DECISIONS.md", true, "Docs"),
            ("tasks", false, "Board"),
            ("task/app/header-names", false, "Board"),
            ("agents", false, "Agents"),
            // A plugin's page shows a document, yet it is the plugin's link that is lit.
            ("docs/SHELF.md", true, "▤ shelf"),
            ("docs/MAPS.md", true, "◇ maps"),
        ] {
            assert_eq!(active(path, document), [shown], "{path}");
        }
        // A route that only begins like a link's lights none.
        assert!(active("tasksold", false).is_empty());
        assert!(active("taskforce", false).is_empty());
    }

    #[test]
    fn a_crumb_shows_only_what_the_lit_link_does_not_already_name() {
        let view = with_nav();
        for page in [
            "",
            "queue",
            "tasks",
            "agents",
            "docs",
            "docs/",
            "docs/SHELF.md",
            "docs/MAPS.md",
        ] {
            assert!(
                crumbs(&view, page).is_empty(),
                "{page}: {:?}",
                crumbs(&view, page)
            );
        }
        assert_eq!(crumbs(&view, "docs/DECISIONS.md"), ["docs", "DECISIONS.md"]);
        assert_eq!(crumbs(&view, "docs/analysis/"), ["docs", "analysis"]);
        // A task's page lights Board, so its one crumb is its slug, whole: a slug's parts are no
        // tasks of their own.
        assert_eq!(crumbs(&view, "task/app/header-names"), ["app/header-names"]);
        assert_eq!(crumbs(&view, "task/solo/"), ["solo"]);
        assert_eq!(
            crumbs(&view, "taskforce/notes.md"),
            ["taskforce", "notes.md"]
        );
        // A second entrypoint is no header link: its folder page keeps its crumb.
        let two = ProjectView {
            entrypoints: vec!["docs".into(), "design".into()],
            ..ProjectView::default()
        };
        assert_eq!(crumbs(&two, "design/"), ["design"]);
        assert!(crumbs(&two, "docs/").is_empty());
        // At the root entrypoint a document's own name is its one crumb.
        let root = ProjectView {
            entrypoints: vec![String::new()],
            ..ProjectView::default()
        };
        assert_eq!(crumbs(&root, "README.md"), ["README.md"]);
    }

    #[test]
    fn a_key_is_the_projects_own_on_the_one_server_and_as_it_is_on_a_server_of_its_own() {
        let under = |base: &str| ProjectView {
            base: base.into(),
            ..ProjectView::default()
        };
        assert_eq!(
            stored(&under("/alpha"), "review-quick-draft"),
            "alpha:review-quick-draft"
        );
        assert_eq!(
            stored(&under("/pantry"), "task-draft:a/b"),
            "pantry:task-draft:a/b"
        );
        assert_ne!(
            stored(&under("/alpha"), "studio.cssVars"),
            stored(&under("/pantry"), "studio.cssVars"),
            "two projects never share a key"
        );
        assert_eq!(
            stored(&under(""), "review-quick-draft"),
            "review-quick-draft"
        );
    }

    #[test]
    fn the_shell_keys_storage_by_the_same_rule() {
        let shell = include_str!("../assets/shell.js");
        assert!(
            shell.contains("return id ? `${id}:${key}` : key;"),
            "shell.js's stored takes the project's id first, as stored does"
        );
        assert!(
            !shell.contains("localStorage.getItem(\"")
                && !shell.contains("localStorage.setItem(\""),
            "every key the shell reads or writes goes through stored"
        );
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
