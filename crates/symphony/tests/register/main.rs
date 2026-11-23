//! Read owner callouts and cards into the work register without dropping comments.

#[path = "../common/mod.rs"]
mod common;

mod auto;
mod blank;
mod board;
mod comments;
mod decisions;
mod delivery;
mod gap;
mod group;
mod history;
mod inline;
mod lint;
mod person;
mod resolve;
mod sort;

use symphony::register::{Decision, Entry, Who, parse_decisions};

/// The cards in `lines`.
pub fn cards(lines: &[&str]) -> Vec<Decision> {
    parse_decisions(lines)
}

/// The first card in `lines`.
pub fn card(lines: &[&str]) -> Decision {
    cards(lines).into_iter().next().expect("a card")
}

/// An entry from its four parts: (ts, who, verdict, text), by the name `who` writes (`owner` or
/// `agent`).
pub fn entry(ts: Option<&str>, who: Who, verdict: Option<&str>, text: &str) -> Entry {
    named(ts, who.as_str(), verdict, text)
}

/// An entry of the line that carries `name`: an agent's for `agent`, a person's for any other.
pub fn named(ts: Option<&str>, name: &str, verdict: Option<&str>, text: &str) -> Entry {
    Entry {
        ts: ts.map(str::to_string),
        who: if name == "agent" {
            Who::Agent
        } else {
            Who::Owner
        },
        verdict: verdict.map(str::to_string),
        text: text.to_string(),
        name: name.to_string(),
    }
}

/// `text` split into lines, as Python's `str.splitlines()`.
pub fn lines(text: &str) -> Vec<String> {
    symphony::text::splitlines(text)
        .into_iter()
        .map(str::to_string)
        .collect()
}
