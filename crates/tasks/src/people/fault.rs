//! Where in `people.json` serde_json stopped at a fault ([`steps`]): the members and items it was
//! reading, outermost first, which [`super::field`] says the fault by, never by a value found there.
//!
//! The text is read again only when it does not parse, up to the line and column serde_json names,
//! by JSON's own grammar: a reader that tracks its path as it reads (serde_path_to_error) links a
//! second reader of every profile into each binary that reads them.
//!
//! A member the file's object, a profile or a sign-in does not take is read into like any other.
//! serde_json skips its value without visiting it, its strings' surrogates unchecked, so that reader
//! stopped at its key; where the key is one of a profile's field names, the field said now goes on
//! into it: a fault in a top-level `"signins": [{"service" 1}]` is at `signins[0]`, where that
//! reader said `signins`.

use serde_json::Value;
use serde_json::error::Category;

/// A place in the text: an object's member, by its key, or a list's item, by its index from 0.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Step {
    Key(String),
    Item(usize),
}

/// What an open container is: the text's one value, a list or an object.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holds {
    Text,
    List,
    Object,
}

/// Where in an open container the read is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spot {
    /// Just opened: its first item or key, or its close.
    Open,
    /// After an object's comma: its next key.
    Key,
    /// After an object's key: its colon.
    Colon,
    /// A value due: the text's, a list's after its comma, an object's after its colon.
    Due,
    /// In a value: a container open in it, or a token that stops short.
    In,
    /// After a whole value: a comma or the close, or for the text nothing more.
    Read,
}

/// An open container and the member or item it is at.
struct Frame {
    holds: Holds,
    spot: Spot,
    index: usize,
    key: String,
}

impl Frame {
    fn new(holds: Holds, spot: Spot) -> Frame {
        Frame {
            holds,
            spot,
            index: 0,
            key: String::new(),
        }
    }

    /// Its member or item, where the fault is in it: one whose read had begun (a value due after an
    /// object's colon, or one open or cut short), and for a fault of data the value just read, or
    /// a list's item due, as serde_json faults a container where another value goes before it
    /// reads its opening. A fault of syntax between values (a missing comma or colon, a trailing
    /// comma, a list or object cut short) or at a key is the container's own.
    fn step(self, data: bool) -> Option<Step> {
        let begun = match (self.holds, self.spot) {
            (Holds::Text, _) | (_, Spot::Key | Spot::Colon) | (Holds::Object, Spot::Open) => false,
            (_, Spot::In) | (Holds::Object, Spot::Due) => true,
            (Holds::List, Spot::Open | Spot::Due) | (_, Spot::Read) => data,
        };
        match self.holds {
            Holds::List if begun => Some(Step::Item(self.index)),
            Holds::Object if begun => Some(Step::Key(self.key)),
            _ => None,
        }
    }
}

/// The members and items of `text` serde_json was reading where it stopped at `error`, outermost
/// first: none for a fault in the text's one value itself, or after it.
pub(super) fn steps(text: &str, error: &serde_json::Error) -> Vec<Step> {
    let data = error.classify() == Category::Data;
    // serde_json's column counts the bytes it read on its line, and the byte it looked at where
    // it found more than a value: after the text's one value, or a list read as a struct with
    // more items than its fields, whose next item it never read.
    let lines: usize = (text.split_inclusive('\n'))
        .take(error.line().saturating_sub(1))
        .map(str::len)
        .sum();
    let unread = usize::from(error.to_string().starts_with("trailing characters"));
    let bytes = text.as_bytes();
    let bytes = (bytes.get(..(lines + error.column()).saturating_sub(unread))).unwrap_or(bytes);
    let mut outer = Vec::new();
    let mut top = Frame::new(Holds::Text, Spot::Due);
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let due = top.spot == Spot::Due || (top.holds == Holds::List && top.spot == Spot::Open);
        match (top.holds, top.spot, byte) {
            (_, _, b' ' | b'\t' | b'\n' | b'\r') => at += 1,
            (Holds::List, Spot::Open | Spot::Read, b']')
            | (Holds::Object, Spot::Open | Spot::Read, b'}') => {
                let Some(container) = outer.pop() else { break };
                top = container;
                top.spot = Spot::Read;
                at += 1;
            }
            (Holds::List, Spot::Due, b']') => break,
            (_, _, b'[' | b'{') if due => {
                top.spot = Spot::In;
                let holds = if byte == b'[' {
                    Holds::List
                } else {
                    Holds::Object
                };
                outer.push(std::mem::replace(&mut top, Frame::new(holds, Spot::Open)));
                at += 1;
            }
            _ if due => {
                top.spot = Spot::In;
                let Some(next) = scalar(bytes, at) else { break };
                top.spot = Spot::Read;
                at = next;
            }
            (Holds::Object, Spot::Open | Spot::Key, b'"') => {
                let Some(next) = string(bytes, at) else { break };
                top.key = key(text.get(at..next).unwrap_or_default());
                top.spot = Spot::Colon;
                at = next;
            }
            (Holds::Object, Spot::Colon, b':') => {
                top.spot = Spot::Due;
                at += 1;
            }
            (Holds::List, Spot::Read, b',') => {
                top.index += 1;
                top.spot = Spot::Due;
                at += 1;
            }
            (Holds::Object, Spot::Read, b',') => {
                top.spot = Spot::Key;
                at += 1;
            }
            _ => break,
        }
    }
    outer.push(top);
    (outer.into_iter())
        .filter_map(|frame| frame.step(data))
        .collect()
}

/// A key's text, its quotes and escapes read as serde_json reads them.
fn key(quoted: &str) -> String {
    match serde_json::from_str(quoted) {
        Ok(Value::String(key)) => key,
        _ => String::new(),
    }
}

/// Where the string, number, `true`, `false` or `null` at `at` ends: none where it is cut short or
/// a byte it needs is not there.
fn scalar(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at)? {
        b'"' => string(bytes, at),
        b't' => word(bytes, at, b"true"),
        b'f' => word(bytes, at, b"false"),
        b'n' => word(bytes, at, b"null"),
        b'-' | b'0'..=b'9' => number(bytes, at),
        _ => None,
    }
}

/// Where the string whose opening quote is at `at` ends, after its closing quote: none where it is
/// cut short, or where a leading surrogate's `\u` escape (D800 to DBFF) stands directly before a
/// quote that is the last byte serde_json read. Reading a string as text, it takes that quote where
/// the trailing surrogate's `\` goes and faults at it, the string still open; a string it skips
/// closes there, its surrogates unchecked, and any fault is later. Any other escape is read whole,
/// `\u`'s four bytes with it whatever they are.
fn string(bytes: &[u8], at: usize) -> Option<usize> {
    let mut i = at + 1;
    loop {
        match bytes.get(i)? {
            b'"' => return Some(i + 1),
            b'\\' if bytes.get(i + 1) == Some(&b'u') => {
                let leading = (bytes.get(i + 2..i + 6))
                    .and_then(|hex| u16::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok())
                    .is_some_and(|n| (0xD800..=0xDBFF).contains(&n));
                if leading && bytes.get(i + 6) == Some(&b'"') && bytes.len() == i + 7 {
                    return None;
                }
                i += 6;
            }
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
}

/// Where `word` ends, written whole at `at`.
fn word(bytes: &[u8], at: usize, word: &[u8]) -> Option<usize> {
    let end = at + word.len();
    (bytes.get(at..end)? == word).then_some(end)
}

/// Where the number at `at` ends, by JSON's grammar: an optional minus, `0` or digits not led by
/// one, then a fraction and an exponent, each with a digit at least.
fn number(bytes: &[u8], at: usize) -> Option<usize> {
    let digits = |from: usize| {
        let count = (bytes.get(from..)?.iter())
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        (count > 0).then_some(from + count)
    };
    let sign = at + usize::from(bytes.get(at) == Some(&b'-'));
    let mut end = if bytes.get(sign) == Some(&b'0') {
        sign + 1
    } else {
        digits(sign)?
    };
    if bytes.get(end).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    if bytes.get(end) == Some(&b'.') {
        end = digits(end + 1)?;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        end += 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        end = digits(end)?;
    }
    Some(end)
}

#[cfg(test)]
#[path = "fault_tests.rs"]
mod tests;
