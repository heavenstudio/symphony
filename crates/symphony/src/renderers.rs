//! The file types a project shows: the Symphony's own three ([`RENDERERS`]: Markdown, HTML and
//! media) and the renderers of the plugins it uses, never another project's. An entrypoint's
//! `types` names them; a type none of them handles, or two of them sharing a name, is refused when
//! the project loads.

use symphony_plugin::Renderer as PluginRenderer;

use crate::entrypoint::{RENDERERS, Renderer};
use crate::plugins::{Plugins, compiled};

/// One renderer a project has, with the plugin that gives it (none for the Symphony's own).
struct Owned {
    plugin: Option<&'static str>,
    renderer: Renderer,
}

/// The renderers a project using `plugins` has: the Symphony's, then each plugin's in the
/// project's order; refused when two of them share a name, naming both owners.
pub fn of_project(plugins: &[String]) -> Result<Vec<Renderer>, String> {
    let own = RENDERERS.iter().map(|renderer| Owned {
        plugin: None,
        renderer: *renderer,
    });
    let given = plugins
        .iter()
        .filter_map(|name| compiled().iter().copied().find(|p| p.name() == name))
        .flat_map(|plugin| {
            plugin.renderers().into_iter().map(|renderer| Owned {
                plugin: Some(plugin.name()),
                renderer: Renderer {
                    name: renderer.name(),
                    extensions: renderer.extensions(),
                },
            })
        });
    let mut found: Vec<Owned> = Vec::new();
    for owned in own.chain(given) {
        if let Some(first) = found
            .iter()
            .find(|first| first.renderer.name == owned.renderer.name)
        {
            let owner = |plugin: Option<&str>| {
                plugin.map_or("the Symphony".to_string(), |name| {
                    format!("plugin {name:?}")
                })
            };
            return Err(format!(
                "[plugins] {} and {} both give a renderer named {:?}",
                owner(first.plugin),
                owner(owned.plugin),
                owned.renderer.name
            ));
        }
        found.push(owned);
    }
    Ok(found.into_iter().map(|owned| owned.renderer).collect())
}

/// The renderers `types`, the entrypoint `label`'s, names, in its order, from those its project
/// has; else the first type none of them handles, named with the renderers there are.
pub fn resolve(label: &str, types: &[String], has: &[Renderer]) -> Result<Vec<Renderer>, String> {
    types
        .iter()
        .map(|name| {
            has.iter()
                .copied()
                .find(|renderer| renderer.name == name)
                .ok_or_else(|| {
                    let names: Vec<&str> = has.iter().map(|renderer| renderer.name).collect();
                    format!(
                        "[[entrypoints]] {label} type {name:?}: no renderer handles it (the \
                         renderers are {})",
                        names.join(", ")
                    )
                })
        })
        .collect()
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
pub fn renderer_of(plugins: &Plugins, src: &str) -> Option<&'static dyn PluginRenderer> {
    let extension = extension(src)?;
    if RENDERERS
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
        .find(|renderer| renderer.extensions().contains(&extension.as_str()))
}

/// The content type of the file `path` names when a renderer of `plugins` draws it: the one
/// [`renderer_of`] picks, so a file is served as the type of the renderer that draws it.
pub fn content_type(plugins: &Plugins, path: &str) -> Option<&'static str> {
    renderer_of(plugins, path).map(|renderer| renderer.content_type())
}
