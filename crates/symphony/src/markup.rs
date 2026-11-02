//! The small string rules the renderer shares with Python's `html`, `urllib.parse` and `posixpath`,
//! kept exact so a page's links and escapes read as they did.

use std::fmt::Write;

/// Python's `html.escape(text)`: `&`, `<`, `>`, and both quotes.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// `html.unescape` for the references an escaped attribute holds: the five named ones and numbers.
pub(crate) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Python's `urllib.parse.quote(text, safe)`: every byte but letters, digits, `_.-~` and `safe`, as `%XX`.
pub(crate) fn quote(text: &str, safe: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || b"_.-~".contains(&byte)
            || safe.as_bytes().contains(&byte)
        {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Python's `urllib.parse.unquote`: `%XX` runs decoded as UTF-8, bad bytes as U+FFFD.
pub(crate) fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |i: usize| bytes.get(i).and_then(|b| char::from(*b).to_digit(16));
        if bytes[at] == b'%'
            && let (Some(high), Some(low)) = (hex(at + 1), hex(at + 2))
        {
            out.push(u8::try_from(high * 16 + low).unwrap_or(0));
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `posixpath.normpath`: `.` and empty parts dropped, `..` folded where it can be, `.` for nothing.
pub(crate) fn normpath(path: &str) -> String {
    let rooted = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    match (rooted, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}

/// `posixpath.dirname`.
pub(crate) fn dirname(path: &str) -> &str {
    let head = &path[..path.rfind('/').map_or(0, |at| at + 1)];
    if head.chars().all(|c| c == '/') {
        head
    } else {
        head.trim_end_matches('/')
    }
}

/// `posixpath.join(base, rel)`: `rel` alone when it is absolute.
pub(crate) fn join(base: &str, rel: &str) -> String {
    if rel.starts_with('/') || base.is_empty() {
        rel.to_string()
    } else if base.ends_with('/') {
        format!("{base}{rel}")
    } else {
        format!("{base}/{rel}")
    }
}

/// A markdown target as an absolute URL path: relative ones resolve against the page, as an image's do.
pub(crate) fn target(src: &str, doc_path: &str) -> String {
    let src = unescape(src);
    if is_external(&src) || src.starts_with('/') {
        return src;
    }
    format!("/{}", normpath(&join(dirname(doc_path), &src)))
}

/// A URL with a scheme (`https:`, `data:`) or a network location (`//host`).
pub(crate) fn is_external(src: &str) -> bool {
    if src.starts_with("//") {
        return true;
    }
    let Some(colon) = src.find(':') else {
        return false;
    };
    let scheme = &src[..colon];
    scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
}
