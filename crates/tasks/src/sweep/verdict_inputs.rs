//! Verdict evidence depends on file metadata and Git's ignore policy. Reuse stats saved witnesses
//! or inventory entries and their policy files, without recursively enumerating the checkout.

use super::git;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Stamp {
    device: u64,
    inode: u64,
    seconds: i64,
    nanos: i64,
    changed_seconds: i64,
    changed_nanos: i64,
    mode: u32,
    length: u64,
}

impl Stamp {
    /// Reuse an already-read entry's full Unix metadata identity.
    pub(super) fn of(meta: fs::Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            seconds: meta.mtime(),
            nanos: meta.mtime_nsec(),
            changed_seconds: meta.ctime(),
            changed_nanos: meta.ctime_nsec(),
            mode: meta.mode(),
            length: meta.len(),
        }
    }

    pub fn predates(&self, began: SystemTime) -> bool {
        let Ok(elapsed) = began.duration_since(SystemTime::UNIX_EPOCH) else {
            return false;
        };
        i128::from(self.changed_seconds) * 1_000_000_000 + i128::from(self.changed_nanos)
            <= elapsed.as_nanos().try_into().unwrap_or(i128::MAX)
    }

    /// Directory entries witness later changes to their immediate child names.
    pub fn is_directory(&self) -> bool {
        self.mode & 0o170_000 == 0o040_000
    }
}

pub(super) fn stamp(path: &Path) -> Option<Stamp> {
    fs::metadata(path).ok().map(Stamp::of)
}

pub(super) fn entry_stamp(path: &Path) -> Option<Stamp> {
    fs::symlink_metadata(path).ok().map(Stamp::of)
}

/// Missing policy files are inputs too. A dangling symlink or unreadable file refuses reuse.
fn policy_stamp(path: &Path) -> Option<Option<Stamp>> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => Some(Some(Stamp::of(meta))),
        Err(error) if error.kind() == ErrorKind::NotFound => match fs::symlink_metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => Some(None),
            _ => None,
        },
        _ => None,
    }
}

pub(super) type PolicyInputs = BTreeMap<PathBuf, Option<Stamp>>;

fn capture(paths: impl IntoIterator<Item = PathBuf>) -> Option<PolicyInputs> {
    paths
        .into_iter()
        .map(|path| Some((path.clone(), policy_stamp(&path)?)))
        .collect()
}

/// Configuration is queried once per pass; its source files are rechecked per candidate. A
/// conditional include or per-worktree configuration needs a separate Git interpretation, so
/// those cases conservatively rescan. Attribute-driven external filters are not cache proofs.
pub(super) struct Policy {
    pub config: String,
    sources: PolicyInputs,
    external: BTreeSet<PathBuf>,
    attributes: BTreeSet<PathBuf>,
    common: PathBuf,
}

fn common(root: &Path) -> Option<PathBuf> {
    let admin = git::admin(root)?;
    let common = match fs::read_to_string(admin.join("commondir")) {
        Ok(path) => admin.join(path.trim()),
        Err(error) if error.kind() == ErrorKind::NotFound => admin,
        Err(_) => return None,
    };
    fs::canonicalize(common).ok()
}

impl Policy {
    pub fn read(root: &Path) -> Option<Self> {
        let began = SystemTime::now();
        let common = common(root)?;
        let vars = String::from_utf8(git::read(root, &["var", "-l"])?.stdout).ok()?;
        let mut sources = BTreeSet::from([common.join("config")]);
        let mut external = BTreeSet::new();
        let mut attributes = BTreeSet::new();
        let mut saw_config = false;
        for line in vars.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "GIT_CONFIG_SYSTEM" | "GIT_CONFIG_GLOBAL" => {
                    sources.insert(value.into());
                    saw_config = true;
                }
                "GIT_ATTR_SYSTEM" | "GIT_ATTR_GLOBAL" => {
                    external.insert(value.into());
                    attributes.insert(value.into());
                }
                _ => {}
            }
        }
        if !saw_config {
            return None;
        }
        let config = String::from_utf8(
            git::read(
                root,
                &["config", "--null", "--show-origin", "--includes", "--list"],
            )?
            .stdout,
        )
        .ok()?;
        let mut observed = BTreeSet::new();
        let mut fields = config.split_terminator('\0');
        while let Some(origin) = fields.next() {
            let field = fields.next()?;
            let (key, value) = field.split_once('\n').unwrap_or((field, ""));
            if key.starts_with("includeif.")
                || key == "extensions.worktreeconfig"
                || key == "core.fsmonitor"
                || key == "core.worktree"
            {
                return None;
            }
            if let Some(path) = origin.strip_prefix("file:") {
                sources.insert(path.into());
                observed.insert(root.join(path));
            } else if origin != "command line:" {
                return None;
            }
            match key {
                "include.path" => {
                    let path = expand(value)?;
                    let origin = Path::new(origin.strip_prefix("file:")?);
                    sources.insert(if path.is_absolute() {
                        path
                    } else {
                        origin.parent()?.join(path)
                    });
                }
                "core.attributesfile" => return None,
                "core.excludesfile" => {
                    external.insert(expand(value)?);
                }
                _ => {}
            }
        }
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        external.insert(config_home.join("git/ignore"));
        let sources = capture(sources.into_iter().map(|path| root.join(path)))?;
        // An origin existed when Git read it. If it disappeared during discovery, the parsed
        // configuration and captured files do not describe one look and cannot prove reuse.
        if observed
            .iter()
            .any(|path| sources.get(path).is_none_or(Option::is_none))
        {
            return None;
        }
        if sources
            .values()
            .flatten()
            .any(|stamp| !stamp.predates(began))
        {
            return None;
        }
        Some(Self {
            config,
            sources,
            external,
            attributes,
            common,
        })
    }

    pub fn inputs(&self, root: &Path, witnesses: &[(PathBuf, Stamp)]) -> Option<PolicyInputs> {
        let mut directories = BTreeSet::new();
        for (path, _) in witnesses {
            if path == root {
                continue;
            }
            let parents = path.strip_prefix(root).ok()?.parent()?.ancestors();
            directories.extend(parents.map(Path::to_path_buf));
        }
        self.for_directories(root, directories.iter().map(PathBuf::as_path))
    }

    /// Capture policy at all supplied relative directories, after checking this candidate uses
    /// the common configuration interpreted for the pass.
    pub fn for_directories<'directory>(
        &self,
        root: &Path,
        directories: impl Iterator<Item = &'directory Path>,
    ) -> Option<PolicyInputs> {
        if common(root)? != self.common || capture(self.sources.keys().cloned())? != self.sources {
            return None;
        }
        let mut paths = self.external.clone();
        paths.extend([
            self.common.join("info/exclude"),
            self.common.join("info/attributes"),
            root.join(".gitignore"),
            root.join(".gitattributes"),
        ]);
        for directory in directories {
            paths.insert(root.join(directory).join(".gitignore"));
            paths.insert(root.join(directory).join(".gitattributes"));
        }
        let inputs = capture(paths.into_iter().map(|path| root.join(path)))?;
        // Attributes may invoke filters that read arbitrary external state. Rescan rather than
        // claiming their output is proved by the checkout's stat identity.
        if inputs.iter().any(|(path, stamp)| {
            stamp.is_some()
                && (self
                    .attributes
                    .iter()
                    .any(|attribute| root.join(attribute) == *path)
                    || path
                        .file_name()
                        .is_some_and(|name| name == ".gitattributes" || name == "attributes")
                    || path == &self.common.join("info/attributes"))
        }) {
            return None;
        }
        Some(inputs)
    }
}

fn expand(value: &str) -> Option<PathBuf> {
    if let Some(path) = value.strip_prefix("~/") {
        Some(PathBuf::from(std::env::var_os("HOME")?).join(path))
    } else if value.starts_with('~') || value.contains("%(prefix)") || value.is_empty() {
        None
    } else {
        Some(value.into())
    }
}
