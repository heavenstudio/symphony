//! A fully assembled client put in place of the one served, as a reload publishes its build: one
//! rename, or one exchange of the folders over a client there ([`publish_client`]). What
//! `symphony-build publish-client STAGE PUBLIC` publishes ([`publish_keeping`]) keeps the stage's
//! build and the builds published just before it over PUBLIC, [`KEEP`] in all, as the folder's
//! `builds.json` lists them ([`retain`]). The command is `symphony-build`'s, this repository's own
//! (crates/symphony-build); the folder's format stays here, beside the reload that copies the
//! installed client's builds into what a build publishes over.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path};

/// How many client builds a published client folder keeps: the build it publishes and the two
/// published before it, in the order they were published (`builds.json`, newest first). A page
/// open on an older build asks for that build's modules later, when it first shows another page
/// kind or opens the chat, so the folder keeps them a while; but the server embeds every file
/// kept, in its binary and in every consumer's build, about 6 MB a build. A page whose build is
/// gone loads again onto the one served when its import fails (`apps/symphony-web/shell/shell.ts`).
/// Two previous builds is the orchestrator's choice, not the owner's, who can change it here.
pub(crate) const KEEP: usize = 3;

/// The file of a client folder that names the builds it holds, newest first.
pub(crate) const BUILDS: &str = "builds.json";

/// One build of the client: its shell (`symphony-shell-HASH.js`, the script its page names) and
/// every file under `assets/` the build made, the shell among them.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Build {
    shell: String,
    files: Vec<String>,
}

/// A client folder's `builds.json`: the bound it was published under ([`KEEP`], which
/// `tools/client-check.sh` holds the committed client to) and its builds, newest first. A stage's
/// names its own build alone and no bound.
#[derive(Debug, Deserialize, Serialize)]
struct Builds {
    #[serde(default)]
    keep: usize,
    builds: Vec<Build>,
}

/// Publish the fully assembled client `stage` over `public` with one rename, including over a
/// nonempty directory, keeping what [`retain`] keeps: what `symphony-build publish-client` does.
/// A stage it cannot keep whole is refused, and `public` is left as it was.
pub fn publish_keeping(stage: &Path, public: &Path) -> io::Result<()> {
    retain(stage, public)?;
    publish_client(stage, public)
}

/// Make `stage`, an assembled client whose `builds.json` names its own build first, the folder to
/// publish over `public`: that build, then `public`'s builds in their order, [`KEEP`] in all.
/// A kept build's file the stage lacks is copied in from `public`; one both hold must be the same
/// bytes, as a name is a content's. Every other file under the stage's `assets/` is removed, and
/// its `builds.json` names the builds kept. A `public` without `builds.json`, or none at all, has
/// no builds to keep.
pub(crate) fn retain(stage: &Path, public: &Path) -> io::Result<()> {
    let newest = read(stage)?
        .and_then(|builds| builds.builds.into_iter().next())
        .ok_or_else(|| io::Error::other(format!("{BUILDS} names no build")))?;
    let page = fs::read_to_string(stage.join("index.html"))
        .map_err(|error| io::Error::other(format!("index.html: {error}")))?;
    if !page.contains(&format!("assets/{}\"", newest.shell)) {
        return Err(io::Error::other(format!(
            "index.html does not load {}, the build {BUILDS} names",
            newest.shell
        )));
    }
    let mut kept = vec![newest];
    if let Some(published) = read(public)? {
        for build in published.builds {
            if kept.len() == KEEP {
                break;
            }
            if kept.iter().all(|other| other.shell != build.shell) {
                kept.push(build);
            }
        }
    }
    let (assets, published) = (stage.join("assets"), public.join("assets"));
    for (age, build) in kept.iter().enumerate() {
        if !build.files.contains(&build.shell) {
            return Err(io::Error::other(format!(
                "the build {} does not list its shell",
                build.shell
            )));
        }
        for name in &build.files {
            if !plain(name) {
                return Err(io::Error::other(format!(
                    "the build {} names {name:?}, not a file of assets/",
                    build.shell
                )));
            }
            let (here, there) = (assets.join(name), published.join(name));
            if regular(&here)? {
                if regular(&there)? && fs::read(&here)? != fs::read(&there)? {
                    return Err(io::Error::other(format!(
                        "immutable asset collision: assets/{name} differs from the one published"
                    )));
                }
            } else if age > 0 && regular(&there)? {
                fs::copy(&there, &here)?;
            } else {
                return Err(io::Error::other(format!(
                    "the build {} lacks assets/{name}",
                    build.shell
                )));
            }
        }
    }
    let mut wanted: BTreeSet<&str> = BTreeSet::new();
    wanted.extend(
        kept.iter()
            .flat_map(|build| build.files.iter().map(String::as_str)),
    );
    for entry in fs::read_dir(&assets)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err(io::Error::other(format!(
                "{}: client assets must be regular files",
                entry.path().display()
            )));
        }
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| wanted.contains(name))
        {
            fs::remove_file(entry.path())?;
        }
    }
    let mut text = serde_json::to_string_pretty(&Builds {
        keep: KEEP,
        builds: kept,
    })?;
    text.push('\n');
    fs::write(stage.join(BUILDS), text)
}

/// The builds `folder`'s `builds.json` names, or none when it has no such file.
fn read(folder: &Path) -> io::Result<Option<Builds>> {
    let path = folder.join(BUILDS);
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| io::Error::other(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io::Error::other(format!("{}: {error}", path.display()))),
    }
}

/// Whether `name` is one file's name in a folder, with no folder of its own and no way out.
fn plain(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) && !name.contains('/')
}

/// Whether `path` is a regular file, not a link, a folder or nothing.
fn regular(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(crate) fn publish_client(stage: &Path, public: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    if !public.try_exists()? {
        return std::fs::rename(stage, public);
    }
    let stage = CString::new(stage.as_os_str().as_bytes())?;
    let public = CString::new(public.as_os_str().as_bytes())?;
    // SAFETY: both paths are live, NUL-terminated strings; the kernel atomically exchanges their
    // directory entries. No file descriptors or pointers outlive this call.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(stage.as_ptr(), public.as_ptr(), libc::RENAME_SWAP) };
    // SAFETY: both paths are live, NUL-terminated strings, relative to the current directory for
    // AT_FDCWD. RENAME_EXCHANGE atomically swaps the entries without exposing a missing directory.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            stage.as_ptr(),
            libc::AT_FDCWD,
            public.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic client directory exchange requires macOS or Linux",
    ));
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
