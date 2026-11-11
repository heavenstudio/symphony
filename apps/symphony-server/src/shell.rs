//! The page the server serves for every client route: the client's shell with its project in it.
//! The head takes the project's theme package: its name as the title, its icon, its tokens inline,
//! its stylesheets after them (the shell puts each page kind's styles before these, so the package
//! wins), and its layout variants on the root element, which the client's styles select. The body
//! takes the project's view ([`symphony_api::PROJECT_ELEMENT`]), under the base the one server
//! serves it at. With no package the Symphony's defaults stand.

use std::path::Path;

use sha2::Digest;
use symphony::markup::escape;
use symphony::project::Project;
use symphony::theme::Package;

/// What marks the theme's elements in the head, where the shell puts page styles before them.
pub(crate) const THEME_MARK: &str = "data-symphony-theme";

/// `shell` with the project at `root` in it, served at `base` (empty at the server's root).
pub(crate) fn page(shell: &str, root: &Path, base: &str) -> String {
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
    if let Some(icon) = &package.icon
        && let Some(at) = page.find("<link rel=\"icon\" href=\"data:,\">")
    {
        let link = format!(
            "<link rel=\"icon\" href=\"{}\">",
            escape(&versioned(root, base, icon))
        );
        page.replace_range(at..at + "<link rel=\"icon\" href=\"data:,\">".len(), &link);
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
        let mut view = project.view();
        view.base = base.into();
        Project::element(&view)
    });
    if !element.is_empty() {
        page.insert_str(body.unwrap_or(0), &element);
    }
    page
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

/// The project file `rel`'s URL under `base`, with its content's hash as its version, so a
/// changed file is fetched again; a file that cannot be read goes without one.
fn versioned(root: &Path, base: &str, rel: &str) -> String {
    let path = format!("{base}/{}", rel.trim_start_matches('/'));
    match std::fs::read(root.join(rel)) {
        Ok(bytes) => {
            let hash: String = sha2::Sha256::digest(&bytes)
                .iter()
                .take(8)
                .map(|byte| format!("{byte:02x}"))
                .collect();
            format!("{path}?v={hash}")
        }
        Err(_) => path,
    }
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
