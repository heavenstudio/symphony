//! Repository I/O boundaries shared by readers and their verifier. Reports contain attempts,
//! not just successful reads: a missing path and a directory enumeration are inputs too.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs::{self, Metadata, ReadDir};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

pub const HEADER: &str = "symphony-read-audit-v1\n";
/// The variables a verifier names the audit's root and report by.
pub const ROOT_ENV: &str = symphony_env::READ_AUDIT_ROOT.name;
pub const REPORT_ENV: &str = symphony_env::READ_AUDIT_REPORT.name;

/// The root is explicit even when a check runs in another working directory. The verifier creates
/// the report; readers may append, but cannot quietly replace a missing report with an empty one.
pub struct Reader {
    root: PathBuf,
    report: Option<PathBuf>,
}

impl Reader {
    pub fn new(root: PathBuf, report: Option<PathBuf>) -> io::Result<Self> {
        if !root.is_absolute() {
            return Err(io::Error::other(
                "repository reader needs an absolute source root",
            ));
        }
        Ok(Self { root, report })
    }

    /// Process-edge adapter for build scripts and native tests. Gate code supplies its Env values
    /// directly to new instead; no reader derives its root from the ambient working directory.
    pub fn process(root: PathBuf) -> io::Result<Self> {
        let report = symphony_env::READ_AUDIT_REPORT.os().map(PathBuf::from);
        let root = match (symphony_env::READ_AUDIT_ROOT.os(), report.is_some()) {
            (Some(root), true) => PathBuf::from(root),
            (None, true) => return Err(io::Error::other("read audit report has no source root")),
            (_, false) => root,
        };
        Self::new(root, report)
    }

    fn record(&self, operation: &str, path: &Path) -> io::Result<()> {
        let Some(report) = &self.report else {
            return Ok(());
        };
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let mut line = format!("{operation}\t");
        for byte in path.as_os_str().as_bytes() {
            write!(line, "{byte:02x}").expect("writing a String");
        }
        line.push('\n');
        fs::OpenOptions::new()
            .append(true)
            .open(report)?
            .write_all(line.as_bytes())
    }

    fn attempt(&self, operation: &str, path: &Path) -> io::Result<PathBuf> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        self.record(operation, &absolute)?;
        if self.report.is_none() {
            return Ok(absolute);
        }
        // Resolve components in filesystem order: link/.. means the target's parent, not the
        // link's parent. Record every link target even when a later component does not exist.
        let mut pending = parts(&absolute);
        let mut resolved = PathBuf::from("/");
        let mut links = 0;
        while let Some(part) = pending.pop_front() {
            if part == "." {
                continue;
            }
            if part == ".." {
                resolved.pop();
                continue;
            }
            resolved.push(part);
            match fs::symlink_metadata(&resolved) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    self.record("link", &resolved)?;
                    let target = fs::read_link(&resolved)?;
                    resolved.pop();
                    let target = if target.is_absolute() {
                        target
                    } else {
                        resolved.join(target)
                    };
                    self.record("target", &target)?;
                    links += 1;
                    if links > 40 {
                        return Err(io::Error::other("read audit: symlink loop"));
                    }
                    let mut next = parts(&target);
                    next.append(&mut pending);
                    pending = next;
                    resolved = PathBuf::from("/");
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        self.record("resolved", &resolved)?;
        Ok(absolute)
    }

    pub fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(self.attempt("file", path)?)
    }

    pub fn read_dir(&self, path: &Path) -> io::Result<ReadDir> {
        fs::read_dir(self.attempt("directory", path)?)
    }

    pub fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        fs::metadata(self.attempt("metadata", path)?)
    }

    /// The bounded JSON-folder reader used by the content scanner and preselection traversal.
    /// Parsing, ids, kind order and schema rules remain in content-index. Diagnostics use the
    /// caller's display path, while reported attempts retain the actual absolute source path.
    pub fn json_files(&self, folder: &Path, shown: &Path) -> io::Result<Vec<(PathBuf, String)>> {
        let named = |path: &Path, error: io::Error| {
            io::Error::new(error.kind(), format!("{}: {error}", path.display()))
        };
        let mut files = Vec::new();
        for entry in self.read_dir(folder).map_err(|error| named(shown, error))? {
            let path = entry.map_err(|error| named(shown, error))?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let text = self.read_to_string(&path).map_err(|error| {
                    named(&shown.join(path.file_name().expect("entry name")), error)
                })?;
                files.push((path, text));
            }
        }
        Ok(files)
    }
}

fn parts(path: &Path) -> VecDeque<OsString> {
    path.components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_owned()),
            Component::ParentDir => Some(OsStr::new("..").to_owned()),
            Component::CurDir => Some(OsStr::new(".").to_owned()),
            Component::RootDir | Component::Prefix(_) => None,
        })
        .collect()
}
