//! The project file's `[theme]` table: a theme package a plugin brings, by name, and the files and
//! values the project sets over it (`crate::theme::Package` puts the two together).

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Theme {
    /// A theme package a plugin brings; `None` is the Symphony's own.
    pub package: Option<String>,
    /// The name shown in the header and the tab, over the package's and the project's.
    pub name: Option<String>,
    /// The icon of the header and the tab, project-relative.
    pub icon: Option<String>,
    /// A stylesheet appended after the package's, project-relative.
    pub stylesheet: Option<String>,
    /// Tokens (`[theme.tokens]`, `primary = "#a33"`) over the package's.
    pub tokens: BTreeMap<String, String>,
    /// The layout variant chosen for each region (`[theme.layouts]`, `header = "compact"`).
    pub layouts: BTreeMap<String, String>,
}

/// The table as written: every key optional, one this build does not know kept by name.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileTheme {
    package: Option<String>,
    name: Option<String>,
    icon: Option<String>,
    stylesheet: Option<String>,
    tokens: BTreeMap<String, String>,
    layouts: BTreeMap<String, String>,
    #[serde(flatten)]
    pub(super) unknown: super::file::Unknown,
}

impl Theme {
    /// The table as written: its paths inside the project, its tokens and layouts each one the
    /// client reads, a blank name or package refused.
    pub(super) fn from_file(file: FileTheme) -> Result<Theme, String> {
        let inside = |key: &str, path: Option<String>| {
            path.map(|path| super::inside(key, &path)).transpose()
        };
        let named = |key: &str, value: Option<String>| match value {
            Some(value) if value.trim().is_empty() => Err(format!("[theme] {key} is empty")),
            value => Ok(value),
        };
        for (token, value) in &file.tokens {
            crate::theme::token(token, value).map_err(|why| format!("[theme.tokens] {why}"))?;
        }
        for (region, variant) in &file.layouts {
            crate::theme::layout(region, variant)
                .map_err(|why| format!("[theme.layouts] {why}"))?;
        }
        Ok(Theme {
            package: named("package", file.package)?,
            name: named("name", file.name)?,
            icon: inside("[theme] icon", file.icon)?,
            stylesheet: inside("[theme] stylesheet", file.stylesheet)?,
            tokens: file.tokens,
            layouts: file.layouts,
        })
    }
}
