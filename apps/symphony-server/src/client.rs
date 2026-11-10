//! Client selection and the release assets compiled into this server's package.

use std::path::{Path, PathBuf};

mod shipped {
    include!(concat!(env!("OUT_DIR"), "/client.rs"));
}

pub(crate) fn asset(name: &str) -> Option<&'static [u8]> {
    shipped::FILES
        .binary_search_by_key(&name, |(path, _)| *path)
        .ok()
        .map(|index| shipped::FILES[index].1)
}

/// An explicit installed bundle or preview override always owns its failures. A local build
/// wins over the shipped bundle; only the ordinary default can select the embedded release.
pub(crate) fn select(
    root: &Path,
    install: Option<&Path>,
    override_dir: Option<PathBuf>,
) -> (PathBuf, bool) {
    if let Some(install) = install {
        return (install.join("client/public"), false);
    }
    if let Some(directory) = override_dir {
        return (directory, false);
    }
    let directory = root.join("build/symphony-ui/public");
    let absent = matches!(std::fs::symlink_metadata(&directory), Err(error) if error.kind() == std::io::ErrorKind::NotFound);
    let embedded = absent && asset("index.html").is_some();
    (directory, embedded)
}

pub(crate) fn build(dist: &Path, embedded: bool) -> String {
    let shell = if embedded {
        String::from_utf8_lossy(crate::client::asset("index.html").unwrap_or_default()).into_owned()
    } else {
        std::fs::read_to_string(dist.join("index.html")).unwrap_or_default()
    };
    // dx writes the module script with a hashed filename. This is exactly the path the browser
    // compares with Board.build.
    shell
        .split("src=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .find(|src| src.ends_with(".js"))
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests;

/// Read the foreground override separately from an installed server's explicit bundle.
pub(crate) fn selection(root: &Path, install: Option<&Path>) -> (PathBuf, bool) {
    select(
        root,
        install,
        std::env::var_os("SF_SYMPHONY_UI_DIST").map(PathBuf::from),
    )
}
