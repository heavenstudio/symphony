//! The file types a project shows: the Symphony's own two ([`MARKDOWN`], [`MEDIA`]) and the
//! renderers of the plugins it uses, never another project's. An entrypoint's `types` names them; a
//! type none of them handles, or two of them sharing a name, is refused when the project loads.

use std::collections::BTreeMap;

use symphony_plugin::{Plugin, Renderer as PluginRenderer};

use crate::entrypoint::{MARKDOWN, MEDIA, RENDERERS, Renderer};
use crate::plugins::{Plugins, compiled, process};
use crate::project::PluginProcess;

/// One renderer a project has, with the plugin that gives it (none for the Symphony's own).
struct Owned {
    plugin: Option<&'static str>,
    renderer: Renderer,
}

/// The renderers a project using `plugins` has: the Symphony's, then each plugin's in the
/// project's order; refused when two of them share a name, naming both owners. A plugin answering
/// from its process (`processes`, its `[plugins.process]`) gives the renderers it declared; one
/// whose process cannot say them gives none, and refuses the project, naming why, only when
/// `wanted` names a type none of the others handles.
pub fn of_project(
    plugins: &[String],
    processes: &BTreeMap<String, PluginProcess>,
    wanted: &[String],
) -> Result<Vec<Renderer>, String> {
    let mut silent = None;
    let mut found_plugins = Vec::new();
    for name in plugins {
        match plugin(name, processes) {
            Ok(found) => found_plugins.extend(found),
            Err(why) => silent = silent.or(Some(why)),
        }
    }
    let has = given(found_plugins)?;
    match silent {
        Some(why) if !wanted.iter().all(|name| has.iter().any(|r| r.name == name)) => Err(why),
        _ => Ok(has),
    }
}

/// The plugin `name` as it answers: from its process when `processes` names one for it
/// ([`process::answering`]), else the compiled plugin of that name; or why its process cannot say
/// what it declared, naming its log.
fn plugin(
    name: &str,
    processes: &BTreeMap<String, PluginProcess>,
) -> Result<Option<&'static dyn Plugin>, String> {
    let compiled = || compiled().iter().copied().find(|p| p.name() == name);
    let Some(started) = processes
        .get(name)
        .and_then(|own| process::answering(name, own))
    else {
        return Ok(compiled());
    };
    let remote = process::made(name, &started);
    remote.declaration().map_err(|why| {
        format!(
            "[plugins.process] {name} cannot say its renderers: {why} (its log: {})",
            started.log.display()
        )
    })?;
    Ok(Some(remote))
}

/// The renderers of the Symphony and of `plugins`, refused when two share a name.
fn given(plugins: Vec<&'static dyn Plugin>) -> Result<Vec<Renderer>, String> {
    let own = RENDERERS.iter().map(|renderer| Owned {
        plugin: None,
        renderer: *renderer,
    });
    let given = plugins.into_iter().flat_map(|plugin| {
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
        .find(|renderer| renderer.extensions().contains(&extension.as_str()))
}

/// The content type of the file `path` names when a renderer of `plugins` draws it: the one
/// [`renderer_of`] picks, so a file is served as the type of the renderer that draws it.
pub fn content_type(plugins: &Plugins, path: &str) -> Option<&'static str> {
    renderer_of(plugins, path).map(|renderer| renderer.content_type())
}
