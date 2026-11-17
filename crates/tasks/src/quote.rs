//! Quoted names in command-line and store refusals.

use std::fmt::Write as _;

/// Quote a name in a refusal, escaping controls and choosing the less ambiguous quote.
pub(crate) fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c < ' ' || c == '\x7f' => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::repr;

    #[test]
    fn refusals_quote_names_without_changing_their_spelling() {
        for (text, expected) in [
            ("", "''"),
            ("plain", "'plain'"),
            ("it's", "\"it's\""),
            ("a\"b", "'a\"b'"),
            ("'\"", "'\\'\"'"),
            ("\\\n\r\t\0\x1f\x7f", "'\\\\\\n\\r\\t\\x00\\x1f\\x7f'"),
            ("Ünï — 😀", "'Ünï — 😀'"),
        ] {
            assert_eq!(repr(text), expected, "{text:?}");
        }
    }
}
