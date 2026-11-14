//! Python's text rules, kept exact, so a file read or written by them is byte for byte the file
//! Python would read or write: its whitespace and lines, a file's text read with universal newlines,
//! a path printed as `pathlib` prints it, and a file replaced whole. What a plugin reads its own
//! files by, and the Symphony's library with it. A string's `repr` and an OS error as Python prints
//! one are the helper `symphony-schema`'s (`symphony_schema::text`).

use std::collections::BTreeMap;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

pub use crate::syntax::lines as splitlines;
pub use crate::syntax::{is_space, is_word};

/// Identity and change times detect in-place edits, atomic replacements and timestamp restores.
#[derive(Clone, PartialEq, Eq)]
pub struct FileStamp {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl FileStamp {
    pub fn read(path: &Path) -> io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        Ok(Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        })
    }
}

/// Python's `\s` inside a pattern: Rust's `\s` plus the four separators Python also counts.
pub const S: &str = r"[\s\x1C-\x1F]";
/// Python's `\S`.
pub const NS: &str = r"[^\s\x1C-\x1F]";

/// Python's `str.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// Python's `str.lstrip()`.
pub fn lstrip(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// Python's `str.rstrip()`.
pub fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// The first `n` characters of `s`, as Python's `s[:n]`.
pub fn prefix_chars(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(at, _)| &s[..at])
}

/// `pathlib.Path.read_text(encoding="utf-8")`: universal newlines, so `\r\n` and `\r` read as `\n`.
/// A file read again while its [`FileStamp`] holds is answered from memory.
pub fn read_text(path: &Path) -> io::Result<String> {
    static TEXT: LazyLock<Mutex<BTreeMap<PathBuf, (FileStamp, String)>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let stamp = FileStamp::read(&path)?;
    {
        let text = TEXT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((before, cached)) = text.get(&path)
            && *before == stamp
        {
            return Ok(cached.clone());
        }
    }
    let text = String::from_utf8(std::fs::read(&path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let text = universal_newlines(text);
    if FileStamp::read(&path).ok().as_ref() == Some(&stamp) {
        let mut cache = TEXT.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= 2048 && !cache.contains_key(&path) {
            cache.clear();
        }
        cache.insert(path, (stamp, text.clone()));
    }
    Ok(text)
}

/// `text` as Python reads a file of it: `\r\n` and `\r` read as `\n`.
pub fn universal_newlines(text: String) -> String {
    if text.contains('\r') {
        return text.replace("\r\n", "\n").replace('\r', "\n");
    }
    text
}

/// Python's `str(pathlib.Path(s))`: repeated and trailing slashes and `.` parts dropped.
pub fn path_str(s: &str) -> String {
    let (root, rest) = if s.starts_with("//") && !s.starts_with("///") {
        ("//", &s[2..])
    } else if s.starts_with('/') {
        ("/", s.trim_start_matches('/'))
    } else {
        ("", s)
    };
    let parts: Vec<&str> = rest
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if root.is_empty() && parts.is_empty() {
        return ".".to_string();
    }
    format!("{root}{}", parts.join("/"))
}

/// Replace the file at `path` with `text` whole: written to a file beside it, then renamed over it, so a
/// reader sees the old text or the new, never one cut short, and a failed write leaves the old text. The
/// file keeps its permissions; a link is written through to the file it names. The bytes are `text`'s,
/// as a plain write would leave them.
pub fn write_whole(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = target
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().to_string());
    let beside = target.with_file_name(format!(".{name}.{}.review-tmp", std::process::id()));
    let written = std::fs::File::create(&beside).and_then(|mut file| {
        file.write_all(text.as_bytes())?;
        if let Ok(old) = std::fs::metadata(&target) {
            file.set_permissions(old.permissions())?;
        }
        file.sync_all()
    });
    let renamed = written.and_then(|()| std::fs::rename(&beside, &target));
    if renamed.is_err() {
        let _ = std::fs::remove_file(&beside);
    }
    renamed
}
