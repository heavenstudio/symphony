//! What a rule given as the owner's words looks like, dated or quoted: "the owner, 2026-11-11", "the
//! owner's, 2026-11-11", "the owner's word of 2026-11-12", `the owner's "…"`, `(the owner: "…")`.
//! A text that matches says the rule it sets instead, without the quote, the date or the card; the
//! words are kept in `docs/history/owner-words.md` and the decision register. Two readers: what a
//! published package carries (`tests/packages.rs`), and the guides and the documents
//! (`tests/hooks/owner_words.rs`).

use regex::Regex;

/// The owner's words, dated or quoted, as a line of text gives them.
pub fn pattern() -> Regex {
    Regex::new(r#"(?i)owner('s)?( (word|rule))?,? (of )?20\d\d-\d\d|owner's "|the owner: ""#)
        .unwrap()
}
