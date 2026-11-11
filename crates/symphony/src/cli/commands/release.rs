//! Stable, verified source inputs for the release client's path-sensitive asset macros.
#![forbid(unsafe_code)]
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sha2::{Digest, Sha256};

mod config;
use config::configure;

type Files = BTreeMap<PathBuf, (Vec<u8>, bool)>;
const REPOSITORY: &str = "https://github.com/DioxusLabs/components";
const PACKAGES: [&str; 2] = ["dioxus-attributes", "dioxus-primitives"];

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn regular_bytes(path: &Path) -> io::Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(invalid(format!(
            "release input must be a regular file: {}",
            path.display()
        )));
    }
    fs::read(path)
}

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    run(args, out, err, Path::new("/private/tmp"))
}

fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write, parent: &Path) -> i32 {
    let [root, metadata, files, component_files, mode] = args else {
        let _ = writeln!(
            err,
            "usage: symphony release-inputs ROOT METADATA FILES COMPONENT_FILES --key|LOCK"
        );
        return 2;
    };
    let key_only = mode == "--key";
    if fs::canonicalize(parent).ok().as_deref() != Some(parent) {
        let _ = writeln!(
            err,
            "symphony release-inputs: canonical release parent {} is required",
            parent.display()
        );
        return 1;
    }
    match prepare(
        Path::new(root),
        Path::new(metadata),
        Path::new(files),
        Path::new(component_files),
        parent,
        key_only,
        (!key_only).then(|| Path::new(mode)),
    ) {
        Ok(root) => match writeln!(out, "{}", root.display()) {
            Ok(()) => 0,
            Err(error) => {
                let _ = writeln!(err, "symphony release-inputs: {error}");
                1
            }
        },
        Err(error) => {
            let _ = writeln!(err, "symphony release-inputs: {error}");
            1
        }
    }
}

/// The folders `symphony init` and the agents write in a checkout, left out of every release input.
const AGENT_FOLDERS: [&str; 3] = [".agents", ".claude", ".codex"];

fn inventory(root: &Path, list: &Path, prefix: &str) -> io::Result<Files> {
    let raw = regular_bytes(list)?;
    if raw.is_empty() || raw.last() != Some(&0) {
        return Err(invalid(
            "release input inventory must be nonempty and NUL terminated",
        ));
    }
    let mut files = Files::new();
    for name in raw[..raw.len() - 1].split(|byte| *byte == 0) {
        let name = std::str::from_utf8(name).map_err(|error| invalid(error.to_string()))?;
        let path = Path::new(name);
        if name.is_empty()
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(invalid(format!("invalid release input {name:?}")));
        }
        if path.starts_with("apps/symphony-server/client/public") {
            continue;
        }
        // The agents' integration (`symphony init`: skills, their links, settings, and an agent's
        // worktrees under `.claude/`) is no input of a build, and its links are not regular files.
        if AGENT_FOLDERS.iter().any(|folder| path.starts_with(folder)) {
            continue;
        }
        if path.components().any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some(".git" | ".target" | "target" | "artifacts" | "build")
            )
        }) {
            return Err(invalid(format!(
                "derived or administrative release input {name:?}"
            )));
        }
        let source = root.join(path);
        let metadata = fs::symlink_metadata(&source)?;
        if !metadata.is_file() || !source.canonicalize()?.starts_with(root) {
            return Err(invalid(format!(
                "release input must be an owned regular file: {}",
                source.display()
            )));
        }
        if files
            .insert(
                Path::new(prefix).join(path),
                (
                    fs::read(&source)?,
                    metadata.permissions().mode() & 0o111 != 0,
                ),
            )
            .is_some()
        {
            return Err(invalid(format!("duplicate release input {name:?}")));
        }
    }
    Ok(files)
}

fn component_root(root: &Path, metadata: &Value, lock: &toml::Table) -> io::Result<PathBuf> {
    if Path::new(
        metadata["workspace_root"]
            .as_str()
            .ok_or_else(|| invalid("missing workspace root"))?,
    )
    .canonicalize()?
        != root
    {
        return Err(invalid("Cargo metadata belongs to another workspace"));
    }
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| invalid("missing Cargo packages"))?;
    let locked = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| invalid("missing locked packages"))?;
    let mut component = None;
    for name in PACKAGES {
        let candidates: Vec<_> = packages
            .iter()
            .filter(|p| p["name"].as_str() == Some(name))
            .collect();
        let [package] = candidates.as_slice() else {
            return Err(invalid(format!("ambiguous component {name}")));
        };
        let pins: Vec<_> = locked
            .iter()
            .filter(|p| p.get("name").and_then(toml::Value::as_str) == Some(name))
            .collect();
        let [pin] = pins.as_slice() else {
            return Err(invalid(format!("ambiguous locked component {name}")));
        };
        let source = pin
            .get("source")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("component has no source"))?;
        let revision = source
            .strip_prefix(&format!("git+{REPOSITORY}?rev="))
            .and_then(|rest| rest.split_once('#'))
            .filter(|(rev, pin)| {
                rev == pin && rev.len() == 40 && rev.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        if revision.is_none()
            || package["source"].as_str() != Some(source)
            || package["version"].as_str() != pin.get("version").and_then(toml::Value::as_str)
        {
            return Err(invalid(format!(
                "component {name} does not match its declared locked source"
            )));
        }
        let manifest = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or_else(|| invalid("missing component manifest"))?,
        )
        .canonicalize()?;
        let parent = manifest
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("invalid component root"))?;
        if component
            .as_deref()
            .is_some_and(|previous| previous != parent)
        {
            return Err(invalid("component packages have different roots"));
        }
        component = Some(parent.to_owned());
    }
    component.ok_or_else(|| invalid("missing component root"))
}

fn identity(files: &Files) -> String {
    let mut hash = Sha256::new();
    for (path, (bytes, executable)) in files {
        let path = path.to_string_lossy();
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update([u8::from(*executable)]);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify(folder: &Path, files: &Files, uid: u32) -> io::Result<()> {
    let metadata = fs::symlink_metadata(folder)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid(format!(
            "release snapshot is not a private owned directory: {}",
            folder.display()
        )));
    }
    let mut actual = Files::new();
    fn walk(root: &Path, at: &Path, actual: &mut Files) -> io::Result<()> {
        for entry in fs::read_dir(at)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if entry.file_type()?.is_dir() {
                walk(root, &entry.path(), actual)?;
            } else if entry.file_type()?.is_file() {
                actual.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|e| invalid(e.to_string()))?
                        .into(),
                    (
                        fs::read(entry.path())?,
                        metadata.permissions().mode() & 0o111 != 0,
                    ),
                );
            } else {
                return Err(invalid("nonregular release snapshot entry"));
            }
        }
        Ok(())
    }
    walk(folder, folder, &mut actual)?;
    if &actual != files {
        return Err(invalid(format!(
            "release snapshot changed: {}",
            folder.display()
        )));
    }
    Ok(())
}

struct Pending {
    path: PathBuf,
    armed: bool,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn publish(parent: &Path, destination: &Path, files: &Files, uid: u32) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return verify(destination, files, uid);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut pending = loop {
        let path = parent.join(format!(
            ".symphony-release-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => break Pending { path, armed: true },
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    };
    for (path, (bytes, executable)) in files {
        let path = pending.path.join(path);
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| invalid("missing input parent"))?,
        )?;
        fs::write(&path, bytes)?;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if *executable { 0o755 } else { 0o644 }),
        )?;
    }
    match fs::rename(&pending.path, destination) {
        Ok(()) => {
            pending.armed = false;
            verify(destination, files, uid)
        }
        Err(error) if destination.try_exists()? => {
            verify(destination, files, uid).map_err(|why| invalid(format!("{error}; {why}")))
        }
        Err(error) => Err(error),
    }
}

fn private_directory(path: &Path, uid: u32) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid(format!(
            "release input lock is not a private owned directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn prepare(
    root: &Path,
    metadata_path: &Path,
    list: &Path,
    component_list: &Path,
    parent: &Path,
    key_only: bool,
    expected: Option<&Path>,
) -> io::Result<PathBuf> {
    let root = root.canonicalize()?;
    let parent = parent.canonicalize()?;
    let metadata: Value = serde_json::from_slice(&regular_bytes(metadata_path)?)
        .map_err(|e| invalid(e.to_string()))?;
    let lock: toml::Table = std::str::from_utf8(&regular_bytes(&root.join("Cargo.lock"))?)
        .map_err(|e| invalid(e.to_string()))?
        .parse()
        .map_err(|e: toml::de::Error| invalid(e.to_string()))?;
    let components = component_root(&root, &metadata, &lock)?;
    let mut files = inventory(&root, list, "source")?;
    files.extend(inventory(&components, component_list, "components")?);
    if files
        .get(Path::new("source/Cargo.lock"))
        .map(|entry| &entry.0)
        != Some(&regular_bytes(&root.join("Cargo.lock"))?)
    {
        return Err(invalid(
            "workspace lock changed during release input capture",
        ));
    }
    let identity = identity(&files);
    let uid = fs::metadata(&root)?.uid();
    let lock_folder = parent.join(format!(".symphony-client-lock-{identity}"));
    if expected.is_some_and(|expected| expected != lock_folder) {
        return Err(invalid(
            "release inputs changed before their publication lock was taken",
        ));
    }
    private_directory(&lock_folder, uid)?;
    if key_only {
        return Ok(lock_folder);
    }
    let destination = parent.join(format!("symphony-client-{identity}"));
    configure(&mut files, &destination, lock, &identity)?;
    publish(&parent, &destination, &files, uid)?;
    Ok(destination.join("source"))
}

#[cfg(test)]
mod tests;
