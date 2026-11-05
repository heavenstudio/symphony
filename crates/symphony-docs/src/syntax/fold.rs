//! A character's ASCII fold, the first step of a heading's anchor.

/// The ASCII a character folds to under NFKD with the marks dropped, as Python's
/// `normalize("NFKD", s).encode("ascii", "ignore")` gives it, for the Latin letters, spaces and
/// punctuation the docs can hold; any other non-ASCII character folds to nothing.
pub(super) fn fold(c: char) -> &'static str {
    const LATIN1: [&str; 64] = [
        // U+00C0..U+00FF
        "A", "A", "A", "A", "A", "A", "", "C", "E", "E", "E", "E", "I", "I", "I", "I", //
        "", "N", "O", "O", "O", "O", "O", "", "", "U", "U", "U", "U", "Y", "", "", //
        "a", "a", "a", "a", "a", "a", "", "c", "e", "e", "e", "e", "i", "i", "i", "i", //
        "", "n", "o", "o", "o", "o", "o", "", "", "u", "u", "u", "u", "y", "", "y",
    ];
    const LATIN_A: [&str; 128] = [
        // U+0100..U+017F
        "A", "a", "A", "a", "A", "a", "C", "c", "C", "c", "C", "c", "C", "c", "D", "d", //
        "", "", "E", "e", "E", "e", "E", "e", "E", "e", "E", "e", "G", "g", "G", "g", //
        "G", "g", "G", "g", "H", "h", "", "", "I", "i", "I", "i", "I", "i", "I", "i", //
        "I", "", "IJ", "ij", "J", "j", "K", "k", "", "L", "l", "L", "l", "L", "l", "L", //
        "l", "", "", "N", "n", "N", "n", "N", "n", "n", "", "", "O", "o", "O", "o", //
        "O", "o", "", "", "R", "r", "R", "r", "R", "r", "S", "s", "S", "s", "S", "s", //
        "S", "s", "T", "t", "T", "t", "", "", "U", "u", "U", "u", "U", "u", "U", "u", //
        "U", "u", "U", "u", "W", "w", "Y", "y", "Y", "Z", "z", "Z", "z", "Z", "z", "s",
    ];
    match c {
        // a space, or a spacing mark (a space and the mark)
        '\u{a0}'
        | '\u{2000}'..='\u{200a}'
        | '\u{202f}'
        | '\u{205f}'
        | '\u{3000}'
        | '\u{a8}'
        | '\u{af}'
        | '\u{b4}'
        | '\u{b8}' => " ",
        '\u{aa}' => "a",
        '\u{b2}' => "2",
        '\u{b3}' => "3",
        '\u{b9}' => "1",
        '\u{ba}' => "o",
        '\u{bc}' => "14",
        '\u{bd}' => "12",
        '\u{be}' => "34",
        '\u{2024}' => ".",
        '\u{2025}' => "..",
        '\u{2026}' => "...",
        '\u{2122}' => "TM",
        '\u{fb00}' => "ff",
        '\u{fb01}' => "fi",
        '\u{fb02}' => "fl",
        '\u{fb03}' => "ffi",
        '\u{fb04}' => "ffl",
        '\u{c0}'..='\u{ff}' => LATIN1[c as usize - 0xc0],
        '\u{100}'..='\u{17f}' => LATIN_A[c as usize - 0x100],
        '\u{ff01}'..='\u{ff5e}' => {
            const ASCII: &str = "!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
            let i = c as usize - 0xff01;
            ASCII.get(i..=i).unwrap_or("")
        }
        _ => "",
    }
}
