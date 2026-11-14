//! Quote and escape diagnostic strings as Python writes them: a string's representation, and an OS
//! error naming its path by it.

use std::fmt::Write as _;
use std::io;
use std::path::Path;

/// Not printable by Python's `str.isprintable`: controls, separators other than a space, format
/// characters and private use. (Unassigned code points, also unprintable to Python, print as is.)
fn unprintable(c: char) -> bool {
    let n = u32::from(c);
    c.is_control()
        || (c != ' ' && c.is_whitespace())
        || matches!(n, 0xad | 0x600..=0x605 | 0x61c | 0x6dd | 0x70f | 0x890..=0x891 | 0x8e2 | 0x180e)
        || matches!(n, 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x2064 | 0x2066..=0x206f | 0xfeff | 0xfff9..=0xfffb)
        || matches!(n, 0x110bd | 0x110cd | 0x13430..=0x1343f | 0x1bca0..=0x1bca3 | 0x1d173..=0x1d17a)
        || matches!(n, 0xe0001 | 0xe0020..=0xe007f | 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x0010_0000..=0x0010_fffd)
}

/// Python's `repr()` of a string, quotes and escapes as Python writes them.
pub fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if u32::from(c) < 0x20 || u32::from(c) == 0x7f => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c if c.is_ascii() || !unprintable(c) => out.push(c),
            c if u32::from(c) <= 0xff => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c if u32::from(c) <= 0xffff => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => {
                let _ = write!(out, "\\U{:08x}", u32::from(c));
            }
        }
    }
    out.push(quote);
    out
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
    use std::path::Path;

    use super::{os_error, repr};

    #[test]
    fn an_os_error_reads_as_python_prints_it() {
        let path = Path::new("/nonexistent/x.md");
        let error = std::fs::read(path).unwrap_err();
        assert_eq!(
            os_error(&error, path),
            "[Errno 2] No such file or directory: '/nonexistent/x.md'"
        );
        let quoted = Path::new("/nonexistent/it's.md");
        let error = std::fs::read(quoted).unwrap_err();
        assert_eq!(
            os_error(&error, quoted),
            "[Errno 2] No such file or directory: \"/nonexistent/it's.md\""
        );
    }

    #[test]
    fn an_error_of_no_os_code_reads_as_itself() {
        let error = std::io::Error::other("no landing input");
        assert_eq!(os_error(&error, Path::new("x.md")), "no landing input");
    }

    #[test]
    fn repr_quotes_and_escapes_like_python() {
        assert_eq!(repr("a"), "'a'");
        assert_eq!(repr("it's"), "\"it's\"");
        assert_eq!(repr("it's \"x\""), "'it\\'s \"x\"'");
        assert_eq!(repr("a\\b\n\t\r"), "'a\\\\b\\n\\t\\r'");
        assert_eq!(repr("\x07é\u{a0}\u{2028}"), "'\\x07é\\xa0\\u2028'");
        assert_eq!(repr("\u{e0001}"), "'\\U000e0001'");
        assert_eq!(repr("\u{f0000}"), "'\\U000f0000'");
    }
}
