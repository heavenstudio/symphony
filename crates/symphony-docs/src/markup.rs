//! The small string rules a page's markup and links follow, Python's `html.escape`, `html.unescape`
//! and `urllib.parse.quote`, kept exact so a link or an escape a plugin writes reads as the
//! Symphony's own do.

use std::fmt::Write;

/// Python's `html.escape(text)`: `&`, `<`, `>`, and both quotes.
pub fn escape(text: &str) -> String {
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
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        if let Some((c, past)) = reference(after) {
            out.push(c);
            rest = past;
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The character the reference `text` opens with names (the text past its `&`: a name of at most
/// nine bytes, then `;`), and the text past its `;`; none for anything else.
fn reference(text: &str) -> Option<(char, &str)> {
    let (name, past) = text.split_once(';').filter(|(name, _)| name.len() <= 9)?;
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
    Some((c, past))
}

/// Python's `urllib.parse.quote(text, safe)`: every byte but letters, digits, `_.-~` and `safe`, as `%XX`.
pub fn quote(text: &str, safe: &str) -> String {
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
