//! BlankInsideCardTest: a card's indented lines after a blank line (a table, images) belong to it; a
//! trailing blank does not.

use symphony::register::{resolve, sort_register, supersede};

use crate::common::plain;
use crate::{cards, lines};

const LINES: [&str; 8] = [
    "- [ ] **a** — x",
    "  [2026-10-05 12:17] agent — t",
    "",
    "    | h |",
    "",
    "    ![p](/p.png)",
    "",
    "- [ ] **b** — y",
];

#[test]
fn the_card_spans_the_blank_lines_inside_it() {
    let found = cards(&LINES);
    assert_eq!(found[0].raw, LINES[1..6].to_vec());
    assert!(found[1].raw.is_empty());
}

#[test]
fn sort_register_keeps_every_line() {
    let text = format!(
        "<!-- register:start -->\n## Needs your decision\n\n{}\n<!-- register:end -->\n",
        LINES.join("\n")
    );
    let out = lines(&sort_register(&text, &plain()).unwrap());
    let kept = |all: &[String]| {
        let mut kept: Vec<String> = all
            .iter()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .cloned()
            .collect();
        symphony_board::order::sort_by(&mut kept, &Ord::cmp);
        kept
    };
    assert_eq!(kept(&out), kept(&lines(&text)));
    let at = out.iter().position(|l| l == "- [ ] **a** — x").unwrap();
    assert_eq!(out[at..at + 6].to_vec(), LINES[..6].to_vec());
}

#[test]
fn supersede_dismisses_after_the_whole_thread() {
    let mut given = LINES[..7].to_vec();
    given.push("- [ ] **b** — y *Replaces:* a");
    let (out, changed) = supersede(&(given.join("\n") + "\n"), "2026-10-05 13:00");
    assert_eq!(changed, vec!["a"]);
    let written = lines(&out);
    assert_eq!(written[1..6].to_vec(), LINES[1..6].to_vec());
    assert_eq!(
        written[6],
        "  [2026-10-05 13:00] agent · dismissed — superseded by **b**"
    );
}

#[test]
fn tidy_resolve_closes_after_the_whole_thread() {
    let given = [
        "- [ ] **a** — x",
        "  [2026-10-05 12:17] agent — t",
        "",
        "    | h |",
        "",
        "    ![p](/p.png)",
        "  [2026-10-05 12:20] owner · approved",
    ];
    let (out, _) = resolve(&(given.join("\n") + "\n"), "2026-10-05 13:00", &plain());
    let written = lines(&out);
    for line in &given[1..] {
        assert!(
            written.iter().any(|w| w == line),
            "{line:?} lost from {out}"
        );
    }
}

#[test]
fn a_blank_then_unindented_text_still_ends_the_card() {
    assert!(cards(&["- [ ] **a** — x", "", "plain"])[0].raw.is_empty());
}
