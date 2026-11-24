//! The page the server serves for every client route: the client's shell with its project in it.
//! The head takes the project's theme package: its name as the title, its icon (the Symphony's
//! own, [`crate::mark`], where the package has none), its tokens inline,
//! its stylesheets after them (the shell puts each page kind's styles before these, so the package
//! wins), and its layout variants on the root element, which the client's styles select. Each file
//! of the package is named by the version its answer is kept for good under ([`versioned`]). The
//! body takes the project's view ([`symphony_api::PROJECT_ELEMENT`]), under the base the one server
//! serves it at, its header's icon versioned as the tab's is, and on a page the server draws itself
//! ([`crate::pages`]) that page's notice. With no package the Symphony's defaults stand, its mark
//! as the icon among them.

use std::path::Path;

use symphony::markup::escape;
use symphony::project::Project;
use symphony::theme::Package;

/// What marks the theme's elements in the head, where the shell puts page styles before them.
pub(crate) const THEME_MARK: &str = "data-symphony-theme";

/// The icon link of a shell, which names no icon: the page served takes the theme's in its place,
/// else the Symphony's.
pub(crate) const NO_ICON: &str = "<link rel=\"icon\" href=\"data:,\">";

/// The opening of the shell's notice region, which a banner goes at the start of.
pub(crate) const REGION: &str = "<section id=\"shell-notices\" aria-label=\"Server notices\">";

/// What carries the notice styles in the head; a page holding it gets them no second time.
pub(crate) const NOTICE_STYLE: &str = "shell-notices-style";

/// `shell` with the project at `root` in it, served at `base` (empty at the server's root), and
/// `banner` (empty for none) in the shell's notice region, styled even before the client loads.
pub(crate) fn page(shell: &str, root: &Path, base: &str, banner: &str) -> String {
    page_for(shell, root, base, banner, &Reader::default())
}

/// Who a page is served to, as its project's view says ([`symphony_api::ProjectView::you`],
/// [`symphony_api::ProjectView::machine`]): none for a page served for nobody in particular.
#[derive(Clone, Debug, Default)]
pub(crate) struct Reader {
    pub(crate) you: Option<symphony_api::PersonName>,
    /// Where they write their profile ([`symphony_api::ProjectView::profile`]).
    pub(crate) profile: String,
    pub(crate) machine: Option<symphony_api::PersonName>,
}

impl Reader {
    /// The page served to `actor`, `machine` the person at the machine as the gate names them once
    /// they gave themselves a name: the header names `actor` and links to their own section of the
    /// People page when they are the person at the machine, else to their own card there.
    pub(crate) fn of(
        actor: &symphony_plugin::Actor,
        machine: Option<symphony_api::PersonName>,
    ) -> Reader {
        let profile = if symphony_board::workflow::is_machine(actor) {
            "/people#you".to_string()
        } else {
            format!("/people#person-{}", actor.name())
        };
        Reader {
            you: Some(symphony_api::PersonName {
                name: actor.name().into(),
                shown: actor.sender.display.clone(),
            }),
            profile,
            machine,
        }
    }
}

/// [`page`], served to `reader`: the header names them, and the lines of the person at the machine
/// are drawn with how they are shown, from the page's own view, so neither asks for anything more.
pub(crate) fn page_for(
    shell: &str,
    root: &Path,
    base: &str,
    banner: &str,
    reader: &Reader,
) -> String {
    let project = Project::of(root);
    let package = Package::of(&project);
    let mut page = shell.to_string();
    if let Some((open, close)) = page
        .find("<title>")
        .map(|at| at + "<title>".len())
        .and_then(|open| page[open..].find("</title>").map(|end| (open, open + end)))
    {
        page.replace_range(open..close, &escape(&package.name));
    }
    if let Some(at) = page.find(NO_ICON) {
        let link = match &package.icon {
            Some(icon) => format!(
                "<link rel=\"icon\" href=\"{}\">",
                escape(&versioned(root, base, icon))
            ),
            None => crate::mark::links(base),
        };
        page.replace_range(at..at + NO_ICON.len(), &link);
    }
    if let Some(at) = page.find("</head>") {
        page.insert_str(at, &head(root, base, &package));
    }
    if let Some(at) = page.find("<html").map(|at| at + "<html".len()) {
        let layouts: String = package
            .layouts
            .iter()
            .map(|(region, variant)| {
                format!(" data-layout-{}=\"{}\"", escape(region), escape(variant))
            })
            .collect();
        page.insert_str(at, &layouts);
    }
    let body = page
        .find("<body")
        .and_then(|at| page[at..].find('>').map(|end| at + end + 1));
    // The project's name and navigation ride in the page's body, so the first paint asks for
    // nothing; a shell without a body is served as it is, the client then drawing no nav.
    let element = body.map_or_else(String::new, |_| {
        let mut view = project.view_at(root);
        view.base = base.into();
        view.you.clone_from(&reader.you);
        view.profile.clone_from(&reader.profile);
        view.machine.clone_from(&reader.machine);
        if !view.mark.is_empty() {
            view.mark = versioned(root, "", &view.mark)[1..].to_string();
        }
        Project::element(&view)
    });
    if !element.is_empty() {
        page.insert_str(body.unwrap_or(0), &element);
    }
    if !banner.is_empty() {
        notice(&mut page, banner);
    }
    page
}

/// Older installed client shells have no notice region or eager styles yet.
fn notice(page: &mut String, banner: &str) {
    if !page.contains(&format!("id=\"{NOTICE_STYLE}\"")) {
        let style = format!(
            "<style id=\"{NOTICE_STYLE}\">{}</style>",
            include_str!("notices.css")
        );
        let at = page.find("</head>").unwrap_or(0);
        page.insert_str(at, &style);
    }
    if let Some(at) = page.find(REGION) {
        page.insert_str(at + REGION.len(), banner);
    } else {
        let at = page
            .find("<div id=\"main\">")
            .map(|at| at + "<div id=\"main\">".len())
            .or_else(|| {
                page.find("<body")
                    .and_then(|at| page[at..].find('>').map(|end| at + end + 1))
            })
            .unwrap_or(0);
        page.insert_str(at, &format!("{REGION}{banner}</section>"));
    }
}

/// The package's tokens inline and its stylesheets after them, each marked [`THEME_MARK`].
fn head(root: &Path, base: &str, package: &Package) -> String {
    let mut head = String::new();
    let css = package.css();
    if !css.is_empty() {
        // Token values are checked (symphony::theme::token): none can close the element.
        head.push_str(&format!("<style {THEME_MARK}>{css}</style>"));
    }
    for sheet in &package.stylesheets {
        head.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"{}\" {THEME_MARK}>",
            escape(&versioned(root, base, sheet))
        ));
    }
    head
}

/// The project file `rel`'s URL under `base`, versioned by the validator its answer is kept for
/// good under ([`symphony::assets::versioned`]), so a changed file is fetched again; a path no
/// file answers goes without one.
pub(crate) fn versioned(root: &Path, base: &str, rel: &str) -> String {
    let path = format!("/{}", rel.trim_start_matches('/'));
    format!("{base}{}", symphony::assets::versioned(&path, root))
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
