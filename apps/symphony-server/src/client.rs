//! Client selection and the release assets compiled into this server's package: none in a release's
//! build (`SYMPHONY_CLIENT_BESIDE`), which serves the client installed beside it.

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

fn consuming_host(root: &Path) -> bool {
    !symphony::reload::workspace(root)
        .join("apps/symphony-server/Cargo.toml")
        .is_file()
        && root.join("apps/symphony-host/Cargo.toml").is_file()
}

/// Source kits retain their installed bundles and local previews. A consuming host defaults
/// to its embedded release, including in the foreground; explicit preview overrides own failures.
pub(crate) fn select(
    root: &Path,
    install: Option<&Path>,
    override_dir: Option<PathBuf>,
) -> (PathBuf, bool) {
    if let Some(install) = install {
        if consuming_host(root) {
            // An installed consuming host includes the release at its immutable Cargo pin.
            // A leftover external bundle from before extraction cannot replace those bytes.
            return (install.join("client/public"), true);
        }
        return (install.join("client/public"), false);
    }
    if let Some(directory) = override_dir {
        return (directory, false);
    }
    let directory = root.join("build/symphony-ui/public");
    if consuming_host(root) {
        return (directory, true);
    }
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
    // The build names the shell's script by its content. This is exactly the path the browser
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

/// Read the foreground override separately from an installed server's explicit bundle. A server
/// run from a release installed on the machine serves that release's client, from the folder
/// beside it, whatever else is named: the release this process is, found once
/// ([`symphony::machine::release::this`]), so every project it serves later is served the same
/// client however the machine's `current` moved since.
pub(crate) fn selection(root: &Path, install: Option<&Path>) -> (PathBuf, bool) {
    let beside = symphony::machine::release::this()
        .and_then(|(release, manifest)| symphony::machine::release::client_of(release, manifest));
    if let Some(client) = beside {
        return (client, false);
    }
    select(root, install, symphony_env::UI_DIST.os().map(PathBuf::from))
}
