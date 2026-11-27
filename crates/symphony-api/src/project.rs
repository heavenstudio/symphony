//! The project as the client reads it: the view the server inlines in every page it serves, the
//! routes the Symphony answers itself, and the plugins' navigation entries ([`NavItem`], the plugin
//! kit's).

use serde::{Deserialize, Serialize};

use crate::NavItem;

/// The project as the client's header and home read it, inlined in the page the server serves
/// (the script element whose id is [`PROJECT_ELEMENT`]), so the first paint needs no request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProjectView {
    /// The name its theme package shows in the header and the tab: the project's own by default.
    pub name: String,
    /// The decision register as a link names it: its route when an entrypoint holds it
    /// (`docs/DECISIONS.md`), whatever the folder is called, else its project-relative path; empty
    /// when the project has no such file yet, and the home then offers no link to one.
    pub register: String,
    /// The review guide as a link names it, by its route (`docs/workflow/REVIEW.md`); empty when the
    /// project has none, and the home then offers no link to one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guide: String,
    /// The plugins' navigation entries, after the Symphony's own.
    #[serde(default)]
    pub nav: Vec<NavItem>,
    /// The routes the project's entrypoints are served at, in its file's order, without slashes:
    /// `docs`, or empty for one at `/`. None sent is the convention, `docs` alone.
    #[serde(default)]
    pub entrypoints: Vec<String>,
    /// The prefix of every path of the project's pages: `/<id>` on the machine's one server,
    /// which serves several projects, and empty on a server of its own. Every link the client
    /// draws and every request it sends lies under it ([`ProjectView::here`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base: String,
    /// The name the header opens with in place of the Symphony's wordmark: the theme package's,
    /// once the project's theme has a name or an icon of its own. Empty is the Symphony's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wordmark: String,
    /// The icon the header opens with in place of the Symphony's mark: the theme package's own,
    /// its file project-relative with no slash first, served under the base; in the page the
    /// server serves, with the version (`?v=`) its answer is kept for good under. Empty is the
    /// Symphony's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mark: String,
    /// Who reads the page, as the header names them and links to their profile: the person at the
    /// machine (`owner` until they give themselves a name), or a person a board named. None in a
    /// page served for nobody in particular.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub you: Option<PersonName>,
    /// Where who reads the page writes their profile, a path of the project: the person at the
    /// machine's own section of the People page (`/people#you`), else their own card there
    /// (`/people#person-NAME`). Empty with no one to name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile: String,
    /// The person at the machine, once they gave themselves a name: every line they wrote, under
    /// it and under `owner` before it, is drawn with how they are shown. None while they gave none,
    /// and their lines read `owner`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<PersonName>,
}

/// A person as a page names them: the short name their lines carry, and how pages show them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct PersonName {
    /// The short name every line of theirs carries: `owner` for the person at the machine while
    /// they gave themselves none.
    pub name: String,
    /// Their display name, else their short name.
    pub shown: String,
}

impl ProjectView {
    /// `path` under the project's [`ProjectView::base`]: a path of the site (one slash first,
    /// `/tasks`) takes the base; a URL, a protocol-relative one, a fragment and a relative path
    /// stay as they are.
    pub fn here(&self, path: &str) -> String {
        if self.base.is_empty() || !path.starts_with('/') || path.starts_with("//") {
            path.to_string()
        } else {
            format!("{}{path}", self.base)
        }
    }

    /// The browser's `path` without the project's base: the route the pages read. A path the
    /// base is not above stays as it is.
    pub fn strip<'a>(&self, path: &'a str) -> &'a str {
        if self.base.is_empty() {
            return path;
        }
        match path.strip_prefix(self.base.as_str()) {
            Some("") => "/",
            Some(rest) if rest.starts_with('/') => rest,
            _ => path,
        }
    }

    /// The routes documents are served at: the project's entrypoints, or the convention's.
    pub fn entrypoints(&self) -> Vec<&str> {
        if self.entrypoints.is_empty() {
            vec!["docs"]
        } else {
            self.entrypoints.iter().map(String::as_str).collect()
        }
    }

    /// Whether `path` (a route, no leading slash) is a document page or a folder page of one of
    /// the entrypoints: under one, on no route the Symphony or a plugin's page answers (which
    /// win), and a folder (a trailing slash, or an entrypoint's own route) or a name with no dot,
    /// a Markdown one or an HTML one. A file of any other type is served as itself.
    pub fn is_document_route(&self, path: &str) -> bool {
        let page = path.trim_end_matches('/');
        let pages = self.nav.iter().map(|item| item.href.trim_matches('/'));
        let entrypoints = self.entrypoints();
        let name = page.rsplit('/').next().unwrap_or(page);
        !SYMPHONY_ROUTES
            .iter()
            .copied()
            .chain(pages)
            .any(|route| within(route, page))
            && entrypoints.iter().any(|url| within(url, page))
            && (path.ends_with('/')
                || entrypoints.contains(&page)
                || !name.contains('.')
                || is_page(name))
    }

    /// Whether `path` is a document route ([`ProjectView::is_document_route`]) naming a folder page
    /// rather than a document: a trailing slash, an entrypoint's own route, or a name that is
    /// neither Markdown nor HTML. A folder or an entrypoint named `guide.md` is a folder page.
    pub fn is_folder_route(&self, path: &str) -> bool {
        let page = path.trim_end_matches('/');
        self.is_document_route(path)
            && (path.ends_with('/') || self.entrypoints().contains(&page) || !is_page(page))
    }

    /// Whether `path` is a document route naming an HTML page: the app's page, whatever the request,
    /// which draws the document whole in a sandboxed frame loading its file's own address
    /// ([`file_address`]).
    pub fn is_html_route(&self, path: &str) -> bool {
        self.is_document_route(path) && !self.is_folder_route(path) && is_html(path)
    }

    /// The URL of the first entrypoint's folder page: `/docs/`, or `/` at the root.
    pub fn documents_href(&self) -> String {
        match self.entrypoints().first() {
            Some(url) if !url.is_empty() => format!("/{url}/"),
            _ => "/".into(),
        }
    }

    /// The route of the first entrypoint's README, where a question no page names goes:
    /// `docs/README.md`, or `README.md` at the root.
    pub fn readme_route(&self) -> String {
        match self.entrypoints().first() {
            Some(url) if !url.is_empty() => format!("{url}/README.md"),
            _ => "README.md".into(),
        }
    }

    /// The Symphony's own pages in the header, in its order, each as its path and its name: the
    /// review home, the owner's queue, the documents' folder page, the task board, the reports, the
    /// settings, the people, the runs and the agents. The plugins' pages ([`ProjectView::nav`])
    /// follow them.
    pub fn own_pages(&self) -> [(String, &'static str); 9] {
        use page_names::{AGENTS, BOARD, DOCS, HOME, PEOPLE, QUEUE, REPORTS, RUNS, SETTINGS};
        [
            ("/".into(), HOME),
            ("/queue".into(), QUEUE),
            (self.documents_href(), DOCS),
            ("/tasks".into(), BOARD),
            ("/reports".into(), REPORTS),
            ("/settings".into(), SETTINGS),
            ("/people".into(), PEOPLE),
            ("/runs".into(), RUNS),
            ("/agents".into(), AGENTS),
        ]
    }
}

/// What the header and every link call the Symphony's pages: one name each, wherever it is
/// written.
pub mod page_names {
    /// The review home, a project's `/`.
    pub const HOME: &str = "Home";
    /// The owner's review queue, `/queue`.
    pub const QUEUE: &str = "Queue";
    /// The documents' folder page.
    pub const DOCS: &str = "Docs";
    /// The task board, `/tasks`, and a task's page under it.
    pub const BOARD: &str = "Board";
    /// The project's reports, `/reports`: how its work flows, its agents, decisions and code, by
    /// the hour, the day and the week.
    pub const REPORTS: &str = "Reports";
    /// The project's settings, `/settings`: everything in force and where each value comes
    /// from.
    pub const SETTINGS: &str = "Settings";
    /// Who is on the project's board, `/people`: its people, groups and powers, and the log of
    /// every change of them.
    pub const PEOPLE: &str = "People";
    /// The project's runs, `/runs`: what runs now, every run with its steps and its log.
    pub const RUNS: &str = "Runs";
    /// The agents, `/agents`.
    pub const AGENTS: &str = "Agents";
    /// The machine's project list, `/` above every project's base, which the one server's
    /// header leads to.
    pub const PROJECTS: &str = "Projects";
}

/// The routes the Symphony answers itself, each with everything under it, without slashes. An
/// entrypoint is never served on one, and a file of an entrypoint whose route lands on one is
/// refused: the Symphony's pages win.
pub const SYMPHONY_ROUTES: &[&str] = &[
    "tasks",
    "task",
    "agents",
    "queue",
    "reports",
    "runs",
    "settings",
    "people",
    "api",
    "assets",
    "static",
    "thumb",
    "x",
    "artifacts/uploads",
    "app",
    "doc",
    "history",
    "diff",
    "favicon.ico",
];

/// Whether `name`, a route or a file name, names a document: a Markdown file, `.md` with its case
/// kept (the docs tree holds `.md` alone, as docs-index lists it), or an HTML page ([`is_html`]).
fn is_page(name: &str) -> bool {
    extension(name).is_some_and(|extension| extension == "md" || html_extension(extension))
}

/// Whether `name`, a route or a file name, names an HTML page: `.html` or `.htm`, in any case, as
/// the server's HTML renderer takes them.
pub fn is_html(name: &str) -> bool {
    extension(name).is_some_and(html_extension)
}

/// What follows the last dot of the last part of `name`, a route or a file name: none when that
/// part holds no dot, or only the one it starts with (`.html` names no page).
fn extension(name: &str) -> Option<&str> {
    let last = name.rsplit('/').next()?;
    let (stem, extension) = last.rsplit_once('.')?;
    (!stem.is_empty()).then_some(extension)
}

fn html_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
}

/// The query key that asks an HTML document's route for the file itself, in its sandbox: the
/// address the document's frame and a Markdown document's mockup frame load ([`file_address`]).
/// Without it the route is the app's page, whatever headers the request carries. A query names no
/// file, so the address is the route's own folder for the files beside it and no file's name.
pub const FILE_QUERY: &str = "raw";

/// Whether `query`, a URL's query without its `?`, asks for the file: it holds [`FILE_QUERY`], with
/// a value or none.
pub fn asks_for_file(query: &str) -> bool {
    query
        .split('&')
        .any(|pair| pair.split('=').next() == Some(FILE_QUERY))
}

/// `src`, an HTML document's address (its route, or a path of the site), asking for the file:
/// [`FILE_QUERY`] added to its query, before its fragment, unless it asks already.
pub fn file_address(src: &str) -> String {
    let (address, fragment) = src.split_at(src.find('#').unwrap_or(src.len()));
    match address.split_once('?') {
        Some((_, query)) if asks_for_file(query) => src.to_string(),
        Some(_) => format!("{address}&{FILE_QUERY}{fragment}"),
        None => format!("{address}?{FILE_QUERY}{fragment}"),
    }
}

/// Whether the route `path` is `route` or lies under it; every path lies under the empty route.
fn within(route: &str, path: &str) -> bool {
    route.is_empty()
        || path == route
        || path
            .strip_prefix(route)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The id of the element holding a [`ProjectView`] in the served page.
pub const PROJECT_ELEMENT: &str = "symphony-project";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_base_prefixes_site_paths_alone_and_strips_only_what_it_is_above() {
        let at_root = ProjectView::default();
        assert_eq!(at_root.here("/tasks"), "/tasks");
        assert_eq!(at_root.strip("/tasks"), "/tasks");
        let view = ProjectView {
            base: "/shelf".into(),
            ..ProjectView::default()
        };
        assert_eq!(view.here("/tasks"), "/shelf/tasks");
        assert_eq!(view.here("/"), "/shelf/");
        assert_eq!(view.here("/@a~b/docs/A.md"), "/shelf/@a~b/docs/A.md");
        for kept in ["//cdn/x.js", "https://x/y", "#top", "docs/A.md", ""] {
            assert_eq!(view.here(kept), kept, "{kept}");
        }
        assert_eq!(view.strip("/shelf/tasks"), "/tasks");
        assert_eq!(view.strip("/shelf"), "/");
        assert_eq!(view.strip("/shelf/"), "/");
        assert_eq!(view.strip("/shelves/tasks"), "/shelves/tasks");
        assert_eq!(view.strip("/tasks"), "/tasks");
    }

    #[test]
    fn the_header_names_each_own_page_once_with_the_documents_where_the_entrypoint_is() {
        let names = |view: &ProjectView| {
            view.own_pages()
                .map(|(href, name)| format!("{name} {href}"))
                .to_vec()
        };
        assert_eq!(
            names(&ProjectView::default()),
            [
                "Home /",
                "Queue /queue",
                "Docs /docs/",
                "Board /tasks",
                "Reports /reports",
                "Settings /settings",
                "People /people",
                "Runs /runs",
                "Agents /agents"
            ]
        );
        let design = ProjectView {
            entrypoints: vec!["design".into(), "docs".into()],
            ..ProjectView::default()
        };
        assert_eq!(names(&design)[2], "Docs /design/");
        // The paths stay the routes the pages read: the base is added where a link is drawn.
        let based = ProjectView {
            base: "/alpha".into(),
            ..ProjectView::default()
        };
        assert_eq!(names(&based)[1], "Queue /queue");
        assert_eq!(page_names::PROJECTS, "Projects");
    }

    #[test]
    fn a_view_without_a_base_reads_and_writes_as_before() {
        let old = r#"{"name":"P","register":"docs/DECISIONS.md"}"#;
        let view: ProjectView = serde_json::from_str(old).unwrap();
        assert_eq!(view.base, "");
        assert!(!serde_json::to_string(&view).unwrap().contains("base"));
    }
}
