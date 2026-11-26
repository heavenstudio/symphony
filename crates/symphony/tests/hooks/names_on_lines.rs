//! A person's short name on a line where `owner` stood (docs/product/analysis/users-and-identity.md,
//! "How a person's name is written") read over this repository's own documents: the register's
//! reader takes a dated line whose word is any short name for a person's, where it took `owner`
//! and `agent` alone, and a comment's title that starts `NAME · ` for that person's. Nothing
//! written before names may read differently, so every tracked Markdown file is read for a dated
//! line under a card, or a comment, that the new reading gives a name `owner` and `agent` never
//! were: there must be none. The count of entries read says how much was compared.

use crate::support::repo as workspace;
use std::process::Command;
use symphony::callouts::parse_comments;
use symphony_docs::syntax::{cards, lines, said};

/// Every Markdown file git tracks in the repository, by its path from the root.
fn markdown(root: &std::path::Path) -> Vec<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--", "*.md"])
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("git lists paths in UTF-8")
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn no_line_or_comment_written_before_names_reads_as_a_named_persons() {
    let root = workspace();
    let (mut entries, mut comments) = (0, 0);
    let mut named = Vec::new();
    for path in markdown(&root) {
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else {
            continue; // gone from the checkout, or no text: no card there
        };
        let read = lines(&text);
        for card in cards(&read) {
            for line in said(&card.raw) {
                entries += 1;
                if !["owner", "agent"].contains(&line.name.as_str()) {
                    named.push(format!("{path}, card {}: {}", card.slug, line.name));
                }
            }
        }
        for comment in parse_comments(&read) {
            comments += 1;
            if comment.by != "owner" {
                named.push(format!(
                    "{path}:{}: a comment by {}",
                    comment.line, comment.by
                ));
            }
        }
    }
    eprintln!("read {entries} entries under cards and {comments} comments");
    assert!(
        named.is_empty(),
        "these lines read as a named person's, where they were the owner's: {named:#?}"
    );
    // The repository holds the record it is compared on: hundreds of lines under its cards.
    assert!(entries > 100, "{entries} entries, {comments} comments");
}
