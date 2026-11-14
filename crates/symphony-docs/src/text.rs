//! Python's text rules, kept exact, so a file read or written by them is byte for byte the file
//! Python would read or write: its whitespace and lines, a file's text read with universal newlines,
//! and a file replaced whole. What a plugin reads its own files by, and the Symphony's library with
//! it. A string's `repr` and an OS error as Python prints one are the helper `symphony-schema`'s
//! (`symphony_schema::text`).

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use crate::syntax::is_space;
pub use crate::syntax::is_word;
pub use crate::syntax::lines as splitlines;

/// What a file is now, to tell that it changed: on Unix its device and inode, its size, and its
/// modification and change times, which see an edit in place, a file replaced by another and a
/// modification time put back; elsewhere its size and modification time, which miss a file replaced
/// by one of the same size whose modification time was put back.
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
        Ok(Self::of(&std::fs::metadata(path)?))
    }

    #[cfg(unix)]
    fn of(meta: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }

    #[cfg(not(unix))]
    fn of(meta: &Metadata) -> Self {
        // Seconds and nanoseconds since the epoch, as Unix counts them; a time the system cannot
        // give reads as none.
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or((0, 0), |since| {
                (
                    i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
                    i64::from(since.subsec_nanos()),
                )
            });
        Self {
            device: 0,
            inode: 0,
            size: meta.len(),
            modified,
            changed: (0, 0),
        }
    }
}

/// Python's `\s` inside a pattern: Rust's `\s` plus the four separators Python also counts.
pub const S: &str = r"[\s\x1C-\x1F]";

/// Python's `str.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
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
        let text = TEXT.lock().unwrap_or_else(PoisonError::into_inner);
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
        let mut cache = TEXT.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= 2048 && !cache.contains_key(&path) {
            cache.clear();
        }
        cache.insert(path, (stamp, text.clone()));
    }
    Ok(text)
}

/// `text` as Python reads a file of it: `\r\n` and `\r` read as `\n`.
fn universal_newlines(text: String) -> String {
    if text.contains('\r') {
        return text.replace("\r\n", "\n").replace('\r', "\n");
    }
    text
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
