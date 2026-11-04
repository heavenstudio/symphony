/// Not printable by Python's `str.isprintable`: controls, separators other than a space, format
/// characters and private use. (Unassigned code points, also unprintable to Python, print as is.)
fn unprintable(c: char) -> bool {
    let n = u32::from(c);
    c.is_control()
        || (c != ' ' && c.is_whitespace())
        || matches!(n, 0xad | 0x600..=0x605 | 0x61c | 0x6dd | 0x70f | 0x890..=0x891 | 0x8e2 | 0x180e)
        || matches!(n, 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x2064 | 0x2066..=0x206f | 0xfeff | 0xfff9..=0xfffb)
        || matches!(n, 0x110bd | 0x110cd | 0x13430..=0x1343f | 0x1bca0..=0x1bca3 | 0x1d173..=0x1d17a)
        || matches!(n, 0xe0001 | 0xe0020..=0xe007f | 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
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
                out.push_str(&format!("\\x{:02x}", u32::from(c)))
            }
            c if c.is_ascii() || !unprintable(c) => out.push(c),
            c if u32::from(c) <= 0xff => out.push_str(&format!("\\x{:02x}", u32::from(c))),
            c if u32::from(c) <= 0xffff => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push_str(&format!("\\U{:08x}", u32::from(c))),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::repr;

    #[test]
    fn repr_quotes_and_escapes_like_python() {
        assert_eq!(repr("a"), "'a'");
        assert_eq!(repr("it's"), "\"it's\"");
        assert_eq!(repr("it's \"x\""), "'it\\'s \"x\"'");
        assert_eq!(repr("a\\b\n\t\r"), "'a\\\\b\\n\\t\\r'");
        assert_eq!(repr("\x07é\u{a0}\u{2028}"), "'\\x07é\\xa0\\u2028'");
    }
}
