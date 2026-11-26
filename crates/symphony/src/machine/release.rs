//! A release installed on the machine (docs/workflow/RELEASES.md, "The install"): a folder of its own in the
//! machine folder, `releases/NAME/`, holding the four binaries every project runs and the one that
//! moves a board's store (`bin/`), one build of the client (`client/HASH/`, which a server built
//! the release's way serves from beside it) and `manifest.json` ([`Manifest`]): the release and its
//! commit, the board schema, the plugin protocol and the `tasks api` version its binaries said they
//! serve, and each artifact's path and size.
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
/// What a project changes to take each release up to this one, oldest first, in its folder: one
/// `## release-NAME` heading a release ([`noted`]).
pub const NOTES: &str = "notes.md";
/// The binaries a release holds, in `bin/`: the server, the board's commands, the messages between
/// agents and the document command line, which every project runs by its recipes, and the move of
/// a board's store to the other engine (`tasks-move`), a program apart from `tasks` that a project
/// on the release runs by its path there, as no recipe reaches it.
pub const BINARIES: [&str; 5] = [SERVER, "tasks", "agent-send", "symphony", MOVE];
/// The move of a board's store, the one of its [`BINARIES`] no recipe runs.
pub const MOVE: &str = "tasks-move";
/// A release's server, the first of its [`BINARIES`].
pub const SERVER: &str = "symphony-server";
/// What a client build must hold to be one.
const PAGE: &str = "index.html";

/// A `MAJOR.MINOR` a release serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

/// What a release's own binaries say they serve: the board schema they write, its additive steps
/// on that contract, the plugin protocol its server speaks, the `tasks api` its board answers and
/// the Symphony pin its server is at ([`super::PIN`]), which the host rule reads before a start
/// runs the release's server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Serves {
    pub schema: i64,
    /// Its additive steps on `schema` (`symphony_board::schema::ADDITIONS`): 0 from a release whose
    /// server did not say, as every release before the contract had none.
    #[serde(default)]
    pub additions: i64,
    pub protocol: Version,
    pub tasks_api: Version,
    /// 0 from a release whose server did not say: older than every project's pin, so no start
    /// runs it in place of its own build.
    #[serde(default)]
    pub pin: u32,
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

/// The binary `binary` of the release `name` in `machine`, by its own folder's path and never
/// through `current`, so a process started from it stays that release's when the link moves.
pub fn binary(machine: &Path, name: &str, binary: &str) -> PathBuf {
    folder(machine, name).join("bin").join(binary)
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
    serde_json::from_slice(text.as_bytes())
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// A release's day and its number that day, by which releases sort (`release-2026-11-12.10` after
/// `release-2026-11-12.9`), or none for a name that is no release's.
fn order(name: &str) -> Option<(&str, u32)> {
    let rest = name.strip_prefix("release-")?;
    let (day, number) = match rest.split_once('.') {
        None => (rest, 1),
        Some((day, number)) => (day, number.parse().ok().filter(|n| *n >= 2)?),
    };
    let dated = day.len() == 10
        && day.bytes().enumerate().all(|(at, byte)| match at {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    dated.then_some((day, number))
}

/// Whether `name` is a release's: `release-YYYY-MM-DD`, or `release-YYYY-MM-DD.N` with N from 2.
pub fn named(name: &str) -> bool {
    order(name).is_some()
}

/// The newest release installed in `machine` that holds [`NOTES`]: its name and their text. None
/// when no installed release holds any.
pub fn noted(machine: &Path) -> Option<(String, String)> {
    let mut found: Vec<String> = fs::read_dir(machine.join(RELEASES))
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| order(name).is_some())
        .collect();
    symphony_board::order::sort_by(&mut found, &|a, b| order(b).cmp(&order(a)));
    found.into_iter().find_map(|name| {
        let text = fs::read_to_string(folder(machine, &name).join(NOTES)).ok()?;
        Some((name, text))
    })
}

/// Each release the notes `text` ([`NOTES`]) hold, oldest first: its name, its commit (empty where
/// its section names none) and the line its section starts at.
fn sections(text: &str) -> Vec<(&str, &str, usize)> {
    let mut found: Vec<(&str, &str, usize)> = Vec::new();
    for (at, line) in text.lines().enumerate() {
        if let Some(name) = line.strip_prefix("## ").map(str::trim)
            && named(name)
        {
            found.push((name, "", at));
        } else if let (Some(commit), Some(last)) = (line.strip_prefix("Commit: "), found.last_mut())
            && last.1.is_empty()
        {
            last.1 = commit.trim();
        }
    }
    found
}

/// Where among `sections` the release `bound` names is: by its name, or by its commit, whole or
/// its first seven characters or more.
fn bound(sections: &[(&str, &str, usize)], bound: &str) -> Result<usize, String> {
    let held = match (sections.first(), sections.last()) {
        (Some(first), Some(last)) => format!("they hold {} to {}", first.0, last.0),
        _ => "they hold no release".to_string(),
    };
    if named(bound) {
        return (sections.iter())
            .position(|(name, ..)| *name == bound)
            .ok_or_else(|| format!("the notes say nothing of {bound}: {held}"));
    }
    let hex = bound.len() >= 7 && bound.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !hex {
        return Err(format!(
            "{bound:?} names no release (release-YYYY-MM-DD, release-YYYY-MM-DD.N or a \
             release's commit)"
        ));
    }
    let bound = bound.to_ascii_lowercase();
    (sections.iter())
        .position(|(_, commit, _)| !commit.is_empty() && commit.starts_with(&bound))
        .ok_or_else(|| {
            format!(
                "{bound} is the commit of no release the notes hold ({held}): name the newest \
                 release that commit holds, which `git describe --tags --match 'release-*' \
                 --abbrev=0 {bound}` says in a clone of the Symphony's repository"
            )
        })
}

/// The part of the notes `text` ([`NOTES`]) for the releases after `from` (none: from the first)
/// up to `to` (none: to the last), oldest first, each from its `## release-NAME` heading. Each
/// bound is a release's name or its commit. Empty when no release is after `from`.
///
/// # Errors
///
/// When a bound names no release, or one the notes do not hold: a project is never handed a part
/// of the steps as if it were all of them.
pub fn notes_between(text: &str, from: Option<&str>, to: Option<&str>) -> Result<String, String> {
    let sections = sections(text);
    let after = match from {
        Some(from) => bound(&sections, from)? + 1,
        None => 0,
    };
    let through = match to {
        Some(to) => bound(&sections, to)? + 1,
        None => sections.len(),
    };
    let Some(first) = sections.get(after).filter(|_| after < through) else {
        return Ok(String::new());
    };
    let end = sections.get(through).map(|section| section.2);
    let lines: Vec<&str> = text.lines().collect();
    let part = &lines[first.2..end.unwrap_or(lines.len())];
    Ok(part.join("\n").trim_end().to_string() + "\n")
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
    install_noted(machine, built, None)
}

/// [`install`], the release's folder holding `notes` as [`NOTES`] beside its manifest when there
/// are any: what a project changes to take each release, which `symphony release-notes` prints.
///
/// # Errors
///
/// As [`install`].
pub fn install_noted(
    machine: &Path,
    built: &Built<'_>,
    notes: Option<&str>,
) -> Result<Manifest, String> {
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
    if let Some(notes) = notes {
        let path = staging.path.join(NOTES);
        fs::write(&path, notes).map_err(|error| at(&path, &error))?;
    }
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
    symphony_board::order::sort_by(&mut found, &Ord::cmp);
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

/// The manifest of the release `machine`'s `current` names, when that release is installed whole
/// and its server runs: none while no link names one, or what it names is not all there.
pub fn current_installed(machine: &Path) -> Option<Manifest> {
    let manifest = installed(machine, &current(machine)?).ok().flatten()?;
    crate::builds::runnable(&binary(machine, &manifest.release, SERVER)).then_some(manifest)
}

/// The installed release this process's binary is of ([`of_binary`]): its folder and its manifest,
/// found at the first ask and the same for the process's life, so a server started through
/// `current` stays the release it started as when the link moves. None for any other build.
pub fn this() -> Option<&'static (PathBuf, Manifest)> {
    static THIS: std::sync::OnceLock<Option<(PathBuf, Manifest)>> = std::sync::OnceLock::new();
    THIS.get_or_init(|| std::env::current_exe().ok().and_then(|exe| of_binary(&exe)))
        .as_ref()
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
    Some((
        release.to_path_buf(),
        serde_json::from_slice(text.as_bytes()).ok()?,
    ))
}

/// The client the server at `binary` serves from beside it: its release's `client/HASH/`, when
/// `binary` is an installed release's and that folder holds a page.
pub fn client_beside(binary: &Path) -> Option<PathBuf> {
    let (release, manifest) = of_binary(binary)?;
    client_of(&release, &manifest)
}

/// The client `manifest` names in the folder of its release, `release`, when that folder holds a
/// page there.
pub fn client_of(release: &Path, manifest: &Manifest) -> Option<PathBuf> {
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
