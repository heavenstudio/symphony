//! A release installed on the machine (docs/RELEASES.md, "The install"): a folder of its own in the
//! machine folder, `releases/NAME/`, holding the four binaries every project runs (`bin/`), one
//! build of the client (`client/HASH/`, which a server built the release's way serves from beside
//! it) and `manifest.json` ([`Manifest`]): the release and its commit, the board schema, the plugin
//! protocol and the `tasks api` version its binaries said they serve, and each artifact's path and
//! size.
//!
//! An install ([`install`]) is written under a temporary name beside it and renamed into place
//! whole, so a release folder is either all there or not there: a failed install leaves none. A
//! folder once in place is never written into, as a binary in it may be running. The machine's
//! `current` link names the release the machine runs ([`point`]): a new link renamed over the old,
//! never the link rewritten, so a reader finds one release or the other. The recipes and the hooks
//! run `current/bin/NAME` where a project has no build of its own ([`crate::builds`]). Earlier
//! releases stay, and pointing `current` at one is the way back.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use symphony_board::builds::CURRENT;

/// The folder of the installed releases, in the machine folder.
pub const RELEASES: &str = "releases";
/// A release's manifest, in its folder.
pub const MANIFEST: &str = "manifest.json";
/// The binaries a release holds, in `bin/`: the server, the board's commands, the messages between
/// agents and the document command line.
pub const BINARIES: [&str; 4] = ["symphony-server", "tasks", "agent-send", "symphony"];
/// What a client build must hold to be one.
const PAGE: &str = "index.html";

/// A `MAJOR.MINOR` a release serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

/// What a release's own binaries say they serve: the board schema they write, the plugin protocol
/// its server speaks and the `tasks api` its board answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Serves {
    pub schema: i64,
    pub protocol: Version,
    pub tasks_api: Version,
}

/// One artifact of a release: its path in the release's folder and its size in bytes; for the
/// client, a folder, the bytes of every file in it and how many they are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
}

/// A release's `manifest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub release: String,
    pub commit: String,
    #[serde(flatten)]
    pub serves: Serves,
    /// The client's folder in the release's, `client/HASH`.
    pub client: String,
    pub artifacts: Vec<Artifact>,
}

impl Manifest {
    /// Each artifact with its size, in one line: `tasks 31,675,888 bytes, …, client/HASH 6,001,234
    /// bytes in 322 files`.
    pub fn sizes(&self) -> String {
        let said: Vec<String> = (self.artifacts.iter())
            .map(|artifact| {
                let name = artifact.path.strip_prefix("bin/").unwrap_or(&artifact.path);
                let files = (artifact.files).map_or_else(String::new, |files| {
                    format!(" in {files} file{}", if files == 1 { "" } else { "s" })
                });
                format!("{name} {} bytes{files}", grouped(artifact.size))
            })
            .collect();
        said.join(", ")
    }
}

/// `n` with its thousands separated by commas.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// What an install takes: the release's name and commit, what its binaries serve, the folder its
/// built binaries are in, each by its name, and the folder of its client's build.
pub struct Built<'a> {
    pub release: &'a str,
    pub commit: &'a str,
    pub serves: Serves,
    pub binaries: &'a Path,
    pub client: &'a Path,
}

/// The folder of the release `name` in the machine folder `machine`.
pub fn folder(machine: &Path, name: &str) -> PathBuf {
    machine.join(RELEASES).join(name)
}

/// Whether `name` can name a release's folder: one path segment, not hidden, so never the
/// temporary name an install is written under.
fn plain(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

/// The manifest of the release `name` installed in `machine`: none when no such release is there.
///
/// # Errors
///
/// When its manifest is there and cannot be read: the folder is not a whole release.
pub fn installed(machine: &Path, name: &str) -> Result<Option<Manifest>, String> {
    let path = folder(machine, name).join(MANIFEST);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// A folder removed when dropped, unless it was kept: an install's temporary one.
struct Staging {
    path: PathBuf,
    kept: bool,
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.kept {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Install `built` in the machine folder `machine` as `releases/NAME/`, whole: written under a
/// temporary name beside it, then renamed into place. Its manifest, as written.
///
/// # Errors
///
/// When the name is no folder's, a release of that name is already there (a folder in place is
/// never written into), a binary is missing or does not run, the client's build has no page, or a
/// file cannot be written. Nothing is left of an install that fails.
pub fn install(machine: &Path, built: &Built<'_>) -> Result<Manifest, String> {
    let name = built.release;
    if !plain(name) {
        return Err(format!("{name:?} names no release folder"));
    }
    let place = folder(machine, name);
    if fs::symlink_metadata(&place).is_ok() {
        return Err(format!(
            "{} is already there, and an installed release is never written into",
            place.display()
        ));
    }
    let releases = machine.join(RELEASES);
    fs::create_dir_all(&releases).map_err(|error| at(&releases, &error))?;
    // What an install of this release killed outright left: one run installs at a time.
    let prefix = format!(".{name}.installing.");
    for entry in fs::read_dir(&releases)
        .map_err(|error| at(&releases, &error))?
        .flatten()
    {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    let mut staging = Staging {
        path: releases.join(format!("{prefix}{}", std::process::id())),
        kept: false,
    };
    let bin = staging.path.join("bin");
    fs::create_dir_all(&bin).map_err(|error| at(&bin, &error))?;
    let mut artifacts = Vec::new();
    for binary in BINARIES {
        let from = built.binaries.join(binary);
        if !crate::builds::runnable(&from) {
            return Err(format!(
                "the build has no {binary} that runs at {}",
                from.display()
            ));
        }
        let size = fs::copy(&from, bin.join(binary)).map_err(|error| at(&from, &error))?;
        artifacts.push(Artifact {
            path: format!("bin/{binary}"),
            size,
            files: None,
        });
    }
    let files = client_files(built.client)?;
    let hash = client_hash(built.client, &files)?;
    let client = format!("client/{hash}");
    let mut size = 0;
    for file in &files {
        let to = staging.path.join(&client).join(file);
        let parent = to.parent().unwrap_or(&staging.path);
        fs::create_dir_all(parent).map_err(|error| at(parent, &error))?;
        let from = built.client.join(file);
        size += fs::copy(&from, &to).map_err(|error| at(&from, &error))?;
    }
    artifacts.push(Artifact {
        path: client.clone(),
        size,
        files: Some(files.len() as u64),
    });
    let manifest = Manifest {
        release: name.to_string(),
        commit: built.commit.to_string(),
        serves: built.serves,
        client,
        artifacts,
    };
    let text = serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())? + "\n";
    let path = staging.path.join(MANIFEST);
    fs::write(&path, text).map_err(|error| at(&path, &error))?;
    fs::rename(&staging.path, &place).map_err(|error| at(&place, &error))?;
    staging.kept = true;
    Ok(manifest)
}

/// `path: error`.
fn at(path: &Path, error: &std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

/// Every file of the client's build in `root`, by its path there, in order.
fn client_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(folder).map_err(|error| at(folder, &error))? {
            let entry = entry.map_err(|error| at(folder, &error))?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|error| at(&path, &error))?;
            if kind.is_dir() {
                walk(root, &path, found)?;
            } else if kind.is_file() {
                found.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
            } else {
                return Err(format!(
                    "{}: a client's build holds regular files alone",
                    path.display()
                ));
            }
        }
        Ok(())
    }
    if !root.join(PAGE).is_file() {
        return Err(format!(
            "the client's build at {} has no {PAGE}",
            root.display()
        ));
    }
    let mut found = Vec::new();
    walk(root, root, &mut found)?;
    found.sort();
    Ok(found)
}

/// What names a client's build: the first 16 hex digits of the SHA-256 of its files, each by its
/// path and its bytes, in order.
fn client_hash(root: &Path, files: &[PathBuf]) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    for file in files {
        let path = root.join(file);
        let name = file.to_string_lossy();
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        let mut open = fs::File::open(&path).map_err(|error| at(&path, &error))?;
        let mut length = 0u64;
        let mut body = Sha256::new();
        loop {
            let read = open.read(&mut buffer).map_err(|error| at(&path, &error))?;
            if read == 0 {
                break;
            }
            length += read as u64;
            body.update(&buffer[..read]);
        }
        hash.update(length.to_le_bytes());
        hash.update(body.finalize());
    }
    Ok(hash
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// The release `machine`'s `current` link names: none while there is no link, or it names no
/// folder of [`RELEASES`].
pub fn current(machine: &Path) -> Option<String> {
    let target = fs::read_link(machine.join(CURRENT)).ok()?;
    let name = target.file_name()?.to_str()?.to_string();
    (target.parent() == Some(Path::new(RELEASES)) && plain(&name)).then_some(name)
}

/// Point `machine`'s `current` at the installed release `name`: a new link beside it, renamed over
/// the old. The release it named before, when it named one.
///
/// # Errors
///
/// When `name` is not installed whole in `machine`, or the link cannot be made; `current` is then
/// as it was.
pub fn point(machine: &Path, name: &str) -> Result<Option<String>, String> {
    if !plain(name) {
        return Err(format!("{name:?} names no release folder"));
    }
    if installed(machine, name)?.is_none() {
        return Err(format!(
            "{name} is not installed in {}",
            machine.join(RELEASES).display()
        ));
    }
    let before = current(machine);
    let link = machine.join(CURRENT);
    // A folder named `current` would take the new link inside it: never renamed onto.
    if fs::symlink_metadata(&link).is_ok_and(|meta| !meta.file_type().is_symlink()) {
        return Err(format!("{} is not a link", link.display()));
    }
    let new = machine.join(format!(".{CURRENT}.{}", std::process::id()));
    let _ = fs::remove_file(&new);
    std::os::unix::fs::symlink(Path::new(RELEASES).join(name), &new)
        .map_err(|error| at(&new, &error))?;
    fs::rename(&new, &link).map_err(|error| {
        let _ = fs::remove_file(&new);
        at(&link, &error)
    })?;
    Ok(before)
}

/// The installed release the binary at `binary` is of, through every link: its folder and its
/// manifest. None for a binary anywhere else, a checkout's build among them.
pub fn of_binary(binary: &Path) -> Option<(PathBuf, Manifest)> {
    let real = binary.canonicalize().ok()?;
    let bin = real.parent()?;
    let release = bin.parent()?;
    let named = |path: &Path, name: &str| path.file_name().is_some_and(|found| found == name);
    if !named(bin, "bin") || !named(release.parent()?, RELEASES) {
        return None;
    }
    let text = fs::read_to_string(release.join(MANIFEST)).ok()?;
    Some((release.to_path_buf(), serde_json::from_str(&text).ok()?))
}

/// The client the server at `binary` serves from beside it: its release's `client/HASH/`, when
/// `binary` is an installed release's and that folder holds a page.
pub fn client_beside(binary: &Path) -> Option<PathBuf> {
    let (release, manifest) = of_binary(binary)?;
    let relative = Path::new(&manifest.client);
    let inside = relative
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)));
    let client = release.join(relative);
    (inside && client.join(PAGE).is_file()).then_some(client)
}

#[cfg(test)]
#[path = "release_tests.rs"]
mod tests;
