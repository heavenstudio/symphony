//! A person's line read as the owner's: `ana · approved — A` is a person's line as `owner ·` is,
//! so every reader of a card (its group in the queue, whether a tidy may resolve it, whether an
//! agent owes it work, its unanswered words, when it was decided) reads a card the same whoever of
//! the two wrote its lines, and keeps the name.

use symphony::register::Who;

use crate::card;
use crate::common::plain;

/// Cards whose person's lines are the owner's, each a different place in the queue.
const CARDS: [&[&str]; 7] = [
    &["- [x] **a** — 1", "  [2026-11-23 09:00] owner · approved"],
    &[
        "- [x] **a** — 1",
        "  [2026-11-23 09:00] owner · approved — go, and say why",
    ],
    &[
        "- [-] **a** — 1",
        "  [2026-11-23 09:00] owner · rejected — no",
    ],
    &[
        "- [-] **a** — 1",
        "  [2026-11-23 09:00] owner · rejected — no",
        "  [2026-11-23 09:10] agent — Rewritten.",
    ],
    &[
        "- [?] **a** — 1",
        "  [2026-11-23 09:00] owner · question — which?",
    ],
    &["- [ ] **a** — 1", "  [2026-11-23 09:00] owner — a word"],
    &[
        "- [x] **a** — 1",
        "  [2026-11-23 08:00] owner · approved — base",
        "  [2026-11-23 08:20] agent · question — Which base?",
        "  [2026-11-23 09:00] owner — the first",
    ],
];

#[test]
fn every_reader_of_a_card_reads_a_persons_lines_as_the_owners() {
    let plugins = plain();
    for lines in CARDS {
        let theirs: Vec<String> = (lines.iter())
            .map(|line| line.replace("] owner ", "] ana "))
            .collect();
        let theirs: Vec<&str> = theirs.iter().map(String::as_str).collect();
        let (owner, ana) = (card(lines), card(&theirs));
        assert_eq!(ana.group(&plugins), owner.group(&plugins), "{lines:?}");
        assert_eq!(ana.open(&plugins), owner.open(&plugins), "{lines:?}");
        assert_eq!(
            ana.resolvable(&plugins),
            owner.resolvable(&plugins),
            "{lines:?}"
        );
        assert_eq!(ana.unanswered(), owner.unanswered(), "{lines:?}");
        assert_eq!(ana.asks_owner(), owner.asks_owner(), "{lines:?}");
        assert_eq!(ana.decided_at(), owner.decided_at(), "{lines:?}");
        // The same lines, the name kept.
        let (mine, hers) = (owner.entries(), ana.entries());
        assert_eq!(mine.len(), hers.len());
        for (mine, hers) in mine.iter().zip(&hers) {
            assert_eq!(
                (hers.who, &hers.ts, &hers.verdict, &hers.text),
                (mine.who, &mine.ts, &mine.verdict, &mine.text)
            );
            let name = if mine.who == Who::Owner {
                "ana"
            } else {
                "agent"
            };
            assert_eq!(hers.name, name);
        }
    }
}

#[test]
fn a_dated_line_under_no_name_gives_no_verdict_and_reads_as_undated_words() {
    // `board`, `unknown`, a numbered handle and capitals are no one's name: the line is no dated
    // entry, and reads as the undated words a legacy line is, as any such line always did.
    for word in ["board", "unknown", "amber-heron-12", "Ana"] {
        let line = format!("  [2026-11-23 09:00] {word} · approved — A");
        let read = card(&["- [ ] **a** — 1", &line]);
        let entries = read.entries();
        assert_eq!(entries.len(), 1, "{word}: {entries:?}");
        let entry = &entries[0];
        assert_eq!(
            (
                entry.ts.as_deref(),
                entry.verdict.as_deref(),
                entry.text.as_str()
            ),
            (None, None, line.trim()),
            "{word}"
        );
        assert_eq!(read.decided_at(), None, "{word}");
    }
}
