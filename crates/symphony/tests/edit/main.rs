//! Owner comments become callouts; decision clicks update marks and dated lines.
//! Writes must preserve every neighbouring note and card.

#[path = "../common/mod.rs"]
mod common;

mod comment;
mod message;
mod person;
mod section;
mod thread;
mod verdict;

/// `text` split into lines, as Python's `str.splitlines()`.
pub fn lines(text: &str) -> Vec<String> {
    symphony::text::splitlines(text)
        .into_iter()
        .map(str::to_string)
        .collect()
}
