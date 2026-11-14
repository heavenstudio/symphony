//! Python's text rules, kept exact, so a file this crate writes is byte for byte the file Python would
//! write: the plugin kit's (`symphony_docs::text`), which a plugin reads its own files by; the few
//! only this crate uses (`\S`, one-sided strips, a string's first characters, a path as `pathlib`
//! prints it), kept here so the kit promises no more than a plugin calls; and a string's `repr` and
//! an OS error as Python prints them, the helper `symphony-schema`'s.

pub use symphony_docs::syntax::is_space;
pub use symphony_docs::text::{FileStamp, S, is_word, read_text, splitlines, strip, write_whole};
pub use symphony_schema::text::{os_error, repr};

/// Python's `\S` inside a pattern: anything but [`S`].
pub(crate) const NS: &str = r"[^\s\x1C-\x1F]";

/// Python's `str.lstrip()`.
pub(crate) fn lstrip(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// Python's `str.rstrip()`.
pub(crate) fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// The first `n` characters of `s`, as Python's `s[:n]`.
pub(crate) fn prefix_chars(s: &str, n: usize) -> &str {
    s.char_indices()
        .nth(n)
        .and_then(|(at, _)| s.get(..at))
        .unwrap_or(s)
}

/// Python's `str(pathlib.Path(s))`: repeated and trailing slashes and `.` parts dropped.
pub(crate) fn path_str(s: &str) -> String {
    let (root, rest) =
        if let Some(rest) = s.strip_prefix("//").filter(|rest| !rest.starts_with('/')) {
            ("//", rest)
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

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
