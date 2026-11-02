//! A model card names its hull and version in its text.

use symphony::model::{is_model, model};

use super::card;

#[test]
fn the_tag_gives_hull_and_version() {
    let c = card(&["- [ ] **x** — Kite. *Model:* REP-F01 v3"]);
    assert_eq!(model(&c), Some(("REP-F01".to_string(), 3)));
    assert!(is_model(&c));
}

#[test]
fn an_art_card_is_not_a_model_card() {
    let c = card(&["- [x] **rep-f01-art** — Kite. *Shows:* REP-F01 Kite · ship"]);
    assert_eq!(model(&c), None);
    assert!(!is_model(&c));
}

#[test]
fn a_tag_without_a_version_is_not_a_model_card() {
    assert_eq!(model(&card(&["- [ ] **x** — *Model:* REP-F01"])), None);
}

#[test]
fn the_tag_counts_only_at_the_end_of_the_card_line_before_its_replaces() {
    assert!(is_model(&card(&[
        "- [ ] **x** — Kite. *Model:* REP-F01 v3 *Replaces:* x-old"
    ])));
    assert!(!is_model(&card(&[
        "- [ ] **x** — *Model:* REP-F01 v3 and then some words"
    ])));
}
