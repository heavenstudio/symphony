//! The file types a project shows: the Symphony's own two ([`MARKDOWN`], [`MEDIA`]) and the
//! renderers of the plugins it uses. An entrypoint's `types` names them; a type none of them
//! handles is refused when the project loads, naming the renderers it has.

use symphony_plugin::Renderer as Drawer;

use crate::entrypoint::{MARKDOWN, MEDIA, RENDERERS, Renderer};
use crate::plugins::{Plugins, compiled};

/// A plugin's renderer as an entrypoint reads it: its name and its extensions.
fn entry(drawer: &'static dyn Drawer) -> Renderer {
    Renderer {
        name: drawer.name(),
        extensions: drawer.extensions(),
    }
}

/// Every renderer of the plugins this process holds that `plugins` names, in their order.
fn drawers(plugins: &[String]) -> impl Iterator<Item = &'static dyn Drawer> + '_ {
    plugins
        .iter()
        .filter_map(|name| compiled().iter().copied().find(|p| p.name() == name))
        .flat_map(|plugin| plugin.renderers())
}

/// The renderer named `name`: the Symphony's, else the first of a plugin this process holds.
pub fn named(name: &str) -> Option<Renderer> {
    RENDERERS
        .iter()
        .copied()
        .find(|r| r.name == name)
        .or_else(|| {
            compiled()
                .iter()
                .copied()
                .flat_map(|plugin| plugin.renderers())
                .find(|drawer| drawer.name() == name)
                .map(entry)
        })
}

/// The renderers' names a project using `plugins` has: the Symphony's, then its plugins'.
pub fn names(plugins: &[String]) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = RENDERERS.iter().map(|r| r.name).collect();
    for drawer in drawers(plugins) {
        if !names.contains(&drawer.name()) {
            names.push(drawer.name());
        }
    }
    names
}

/// `types`, the entrypoint `label`'s, when a renderer of the project using `plugins` handles each;
/// else the first that none handles, named with the renderers the project has.
pub fn check_types(label: &str, types: &[String], plugins: &[String]) -> Result<(), String> {
    let names = names(plugins);
    match types.iter().find(|name| !names.contains(&name.as_str())) {
        None => Ok(()),
        Some(unknown) => Err(format!(
            "[[entrypoints]] {label} type {unknown:?}: no renderer handles it (the renderers are \
             {})",
            names.join(", ")
        )),
    }
}

/// The lower-case extension of the file a URL or path names, its query and fragment left out.
pub fn extension(src: &str) -> Option<String> {
    let path = src.split(['?', '#']).next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("");
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then(|| extension.to_ascii_lowercase())
}

/// The renderer of `plugins` that draws the file `src` names, the first to list its extension.
/// A file the Symphony's own renderers show is never a plugin's.
pub fn drawing(plugins: &Plugins, src: &str) -> Option<&'static dyn Drawer> {
    let extension = extension(src)?;
    if [MARKDOWN, MEDIA]
        .iter()
        .any(|own| own.extensions.contains(&extension.as_str()))
    {
        return None;
    }
    plugins
        .list()
        .iter()
        .copied()
        .flat_map(|plugin| plugin.renderers())
        .find(|drawer| drawer.extensions().contains(&extension.as_str()))
}

/// The content type of the file `path` names when a renderer of a plugin this process holds draws
/// its extension.
pub fn content_type(path: &str) -> Option<&'static str> {
    let extension = extension(path)?;
    compiled()
        .iter()
        .copied()
        .flat_map(|plugin| plugin.renderers())
        .find(|drawer| drawer.extensions().contains(&extension.as_str()))
        .map(|drawer| drawer.content_type())
}
