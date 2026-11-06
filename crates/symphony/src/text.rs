//! Python's text rules, kept exact, so a file this crate writes is byte for byte the file Python would
//! write.

use std::collections::BTreeMap;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

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
pub(crate) const NS: &str = r"[^\s\x1C-\x1F]";

/// Python's `str.isspace` for one character.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Python's `\w` for one character: a letter, a digit or `_`.
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

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

/// A line boundary for `str.splitlines()`, other than `\r` (which may pair with `\n`).
fn breaks_line(c: char) -> bool {
    matches!(
        c,
        '\n' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Python's `str.splitlines()`: every line boundary Python knows, the break dropped.
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c == '\r' {
            lines.push(&s[start..at]);
            start = at + 1;
            if let Some(&(next, '\n')) = chars.peek() {
                chars.next();
                start = next + 1;
            }
        } else if breaks_line(c) {
            lines.push(&s[start..at]);
            start = at + c.len_utf8();
        }
    }
    if start < s.len() {
        lines.push(&s[start..]);
    }
    lines
}

/// The first `n` characters of `s`, as Python's `s[:n]`.
pub fn prefix_chars(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(at, _)| &s[..at])
}

pub use symphony_schema::text::repr;

/// `pathlib.Path.read_text(encoding="utf-8")`: universal newlines, so `\r\n` and `\r` read as `\n`.
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

/// An OS error as Python prints one: `[Errno 2] No such file or directory: 'x.md'`.
pub fn os_error(error: &io::Error, path: &Path) -> String {
    match error.raw_os_error() {
        Some(code) => {
            let said = error.to_string();
            let detail = said
                .strip_suffix(&format!(" (os error {code})"))
                .unwrap_or(&said);
            format!("[Errno {code}] {detail}: {}", repr(&path.to_string_lossy()))
        }
        None => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_breaks_where_python_does_and_drops_a_last_empty_line() {
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(splitlines("a\n"), vec!["a"]);
        assert_eq!(splitlines("a\n\nb"), vec!["a", "", "b"]);
        assert_eq!(
            splitlines("a\r\nb\rc\x0bd\x0ce\x1cf\x1dg\x1eh\u{85}i\u{2028}j\u{2029}k"),
            vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]
        );
        assert_eq!(splitlines("a\x1fb"), vec!["a\x1fb"]);
    }

    #[test]
    fn strip_takes_pythons_spaces_including_the_four_separators() {
        assert_eq!(strip(" \t\x1c a b\x1f\u{a0}\u{3000}"), "a b");
        assert_eq!(lstrip("  a "), "a ");
        assert_eq!(rstrip("  a \n"), "  a");
        assert!(is_space('\x1d'));
        assert!(!is_space('\u{200b}'));
        assert!(is_word('é') && is_word('_') && is_word('7') && !is_word('-'));
    }

    #[test]
    fn prefix_counts_characters_not_bytes() {
        assert_eq!(prefix_chars("éèê", 2), "éè");
        assert_eq!(prefix_chars("ab", 80), "ab");
    }

    #[test]
    fn a_path_prints_as_pathlib_prints_it() {
        assert_eq!(path_str("./tmp/x/"), "tmp/x");
        assert_eq!(path_str("/a//b/./c"), "/a/b/c");
        assert_eq!(path_str("."), ".");
        assert_eq!(path_str(""), ".");
    }

    #[test]
    fn read_text_reads_universal_newlines() {
        let path = std::env::temp_dir().join(format!("review-text-{}.md", std::process::id()));
        std::fs::write(&path, "a\r\nb\rc\n").unwrap();
        let read = read_text(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read.unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn an_os_error_reads_as_python_prints_it() {
        let path = Path::new("/nonexistent/x.md");
        let error = std::fs::read(path).unwrap_err();
        assert_eq!(
            os_error(&error, path),
            "[Errno 2] No such file or directory: '/nonexistent/x.md'"
        );
    }
}
