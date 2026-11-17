//! A project's look: its theme package. A plugin provides one through its `Theme` seam, chosen by
//! `[theme] package`, and the project file sets any part over it (`[theme]`: a name, an icon, a
//! stylesheet, `[theme.tokens]` and `[theme.layouts]`). A project with neither has the Symphony's
//! defaults: its own name in the tab, the Symphony's mark (the fermata) as its icon and the
//! Symphony's wordmark in its header, no tokens, no stylesheet, every region's default layout.
//!
//! The server serves the tokens as `/<id>/theme.css` with an ETag and inlines the same text into
//! the head of every page it serves, so the first paint is right; the stylesheets follow it, and
//! each layout region carries its variant as an attribute of the page (`data-layout-header`),
//! which the client's own stylesheets draw: the client holds every variant and stays data-only.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::plugins::Plugins;
use crate::project::Project;

/// The regions the client draws in more than one layout, each with the variants it holds; the
/// first is the default.
pub const LAYOUTS: &[(&str, &[&str])] = &[
    // `compact`: the header in one row at every width, its links scrolling across.
    ("header", &["default", "compact"]),
    // `focus`: the home shows the next card and the queue, without the tiles and the progress.
    ("home", &["default", "focus"]),
];

/// A project's theme package, as the server serves it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Package {
    /// The plugin package it starts from, when a selected plugin provides the one the file names.
    pub package: Option<String>,
    /// The name of the tab: the theme's own, else the project's. The header shows it too once
    /// the theme has a name or an icon of its own ([`Package::wordmark`]).
    pub name: String,
    /// Whether the theme gave the name: the plugin's package or the file's `[theme] name`.
    pub named: bool,
    /// The icon of the header and the tab, project-relative. None is the Symphony's mark.
    pub icon: Option<String>,
    /// Token names (`primary`, `background`, `font-sans`, …) and their CSS values.
    pub tokens: BTreeMap<String, String>,
    /// Stylesheets after the tokens, project-relative, in order.
    pub stylesheets: Vec<String>,
    /// The variant of each region that is not its default.
    pub layouts: BTreeMap<String, String>,
}

impl Package {
    /// The package of `project`: the one a selected plugin provides under `[theme] package`, then
    /// the file's parts over it. A plugin's token or layout the client cannot read is left out
    /// (`symphony-status --lint` names it, [`problems`]).
    pub fn of(project: &Project) -> Package {
        let mut package = Package {
            name: project.name.clone(),
            ..Package::default()
        };
        let file = &project.theme;
        if let Some(wanted) = &file.package
            && let Some(theme) = Plugins::for_project(project).theme(wanted)
        {
            package.package = Some(wanted.clone());
            if !theme.name.trim().is_empty() {
                package.name = theme.name;
                package.named = true;
            }
            package.icon = theme.icon;
            for (name, value) in theme.tokens {
                if token(&name, &value).is_ok() {
                    package.tokens.insert(name, value);
                }
            }
            package.stylesheets = theme.stylesheets;
            for (region, variant) in theme.layouts {
                if layout(&region, &variant).is_ok() {
                    package.layouts.insert(region, variant);
                }
            }
        }
        if let Some(name) = &file.name {
            package.name.clone_from(name);
            package.named = true;
        }
        if file.icon.is_some() {
            package.icon.clone_from(&file.icon);
        }
        package.stylesheets.extend(file.stylesheet.iter().cloned());
        package.tokens.extend(file.tokens.clone());
        package.layouts.extend(file.layouts.clone());
        package
            .layouts
            .retain(|region, variant| default_of(region) != Some(variant.as_str()));
        package
    }

    /// The name the header opens with in place of the Symphony's wordmark: the package's, once
    /// the theme has a name or an icon of its own, so an icon never stands beside the Symphony's
    /// name. None is the Symphony's wordmark, the look of a project whose theme has neither.
    pub fn wordmark(&self) -> Option<&str> {
        (self.named || self.icon.is_some()).then_some(self.name.as_str())
    }

    /// The icon the header opens with in place of the Symphony's mark: the package's own, with no
    /// slash first, as it is put under the project's base. Empty is the Symphony's mark.
    pub fn mark(&self) -> &str {
        self.icon
            .as_deref()
            .map_or("", |icon| icon.trim_start_matches('/'))
    }

    /// The tokens as CSS, `/<id>/theme.css`: one `:root` rule declaring `--NAME` for each, or
    /// nothing without tokens. Every value was checked by [`token`].
    pub fn css(&self) -> String {
        if self.tokens.is_empty() {
            return String::new();
        }
        let declarations: String = self
            .tokens
            .iter()
            .map(|(name, value)| format!("--{name}:{};", value.trim()))
            .collect();
        format!(":root{{{declarations}}}\n")
    }

    /// A strong ETag of everything the package serves: its tokens, stylesheets, icon, name and
    /// layouts.
    pub fn etag(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.css());
        for part in self
            .stylesheets
            .iter()
            .chain(&self.icon)
            .chain([&self.name])
            .chain(
                self.layouts
                    .iter()
                    .flat_map(|(region, variant)| [region, variant]),
            )
        {
            digest.update([0]);
            digest.update(part);
        }
        let hex: String = digest
            .finalize()
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("\"{hex}\"")
    }
}

/// Whether `name = value` is a token the client reads: a CSS custom property's name (letters,
/// digits, `-` and `_`, without the `--`) and a declaration's value that cannot end the rule,
/// open a comment or fetch anything.
///
/// # Errors
///
/// The token and what is wrong with it.
pub fn token(name: &str, value: &str) -> Result<(), String> {
    let good_name = !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    if !good_name {
        return Err(format!(
            "{name:?} is not a token name (letters, digits, - and _)"
        ));
    }
    let lower = value.to_ascii_lowercase();
    if value.trim().is_empty()
        || value.contains([';', '{', '}', '<', '>', '\\', '\n', '\r'])
        || value.contains("/*")
        || lower.contains("url(")
        || lower.contains("@import")
    {
        return Err(format!(
            "{name} = {value:?} is not a value a token may hold (no ; {{ }} < > \\, comment, url( or @import)"
        ));
    }
    Ok(())
}

/// Whether `variant` is one the client holds for `region` ([`LAYOUTS`]).
///
/// # Errors
///
/// The region and the variants it has, or the regions there are.
pub fn layout(region: &str, variant: &str) -> Result<(), String> {
    let Some((_, variants)) = LAYOUTS.iter().find(|(name, _)| *name == region) else {
        let regions: Vec<&str> = LAYOUTS.iter().map(|(name, _)| *name).collect();
        return Err(format!(
            "{region:?} is not a region; the regions are {}",
            regions.join(", ")
        ));
    };
    if variants.contains(&variant) {
        Ok(())
    } else {
        Err(format!(
            "{region} = {variant:?} is not one of {}",
            variants.join(", ")
        ))
    }
}

fn default_of(region: &str) -> Option<&'static str> {
    LAYOUTS
        .iter()
        .find(|(name, _)| *name == region)
        .map(|(_, variants)| variants[0])
}

/// What in `project`'s theme names a package none of its plugins provides, or a plugin package's
/// token or layout the client cannot read.
pub fn problems(project: &Project) -> Vec<String> {
    let Some(wanted) = &project.theme.package else {
        return Vec::new();
    };
    let Some(theme) = Plugins::for_project(project).theme(wanted) else {
        return vec![format!(
            "{}: [theme] package {wanted} is shipped by none of the project's plugins",
            crate::project::FILE
        )];
    };
    let tokens = theme.tokens.iter().filter_map(|(n, v)| token(n, v).err());
    let layouts = theme.layouts.iter().filter_map(|(r, v)| layout(r, v).err());
    tokens
        .chain(layouts)
        .map(|why| format!("the theme package {wanted}: {why}"))
        .collect()
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
