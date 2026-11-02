//! A faction page's pictures come from its register, never by hand.

use symphony::art::{self, APPROVED, ArtError};

use super::{card, cards};

#[test]
fn a_card_names_its_slot_in_its_text() {
    let c = card(&[
        "- [ ] **a** — A picture. *Shows:* Great Houses · people",
        "  [2026-09-25 10:00] agent — ![x](/x.png)",
    ]);
    assert_eq!(
        art::shows(&c),
        Some(("Great Houses".into(), "people".into()))
    );
}

#[test]
fn an_older_card_is_tagged_by_an_agent_line() {
    let c = card(&[
        "- [r] **a** — A picture.",
        "  [2026-09-25 10:00] agent — ![x](/x.png)",
        "  [2026-09-25 12:00] agent — shows: Obsidian Kin · Ship",
    ]);
    assert_eq!(art::shows(&c), Some(("Obsidian Kin".into(), "ship".into())));
}

#[test]
fn an_untagged_card_shows_nothing() {
    let c = card(&[
        "- [ ] **a** — words",
        "  [2026-09-25 10:00] agent — ![x](/x.png)",
    ]);
    assert_eq!(art::shows(&c), None);
}

#[test]
fn the_picture_is_the_first_image_an_agent_posted() {
    let c = card(&[
        "- [ ] **a** — words",
        "  [2026-09-25 10:00] owner · question — ![not](/owner.png)",
        "  [2026-09-25 10:01] agent — ![one](/one.png) and ![two](/two.png)",
    ]);
    assert_eq!(art::picture(&c).unwrap().as_deref(), Some("/one.png"));
}

const THREE: [&str; 2] = [
    "- [x] **a** — Three shapes. *Shows:* X · ship",
    "  [2026-09-25 10:00] agent — A ![X ship A](/a.png) B ![X ship B](/b.png) C ![X ship C](/c.png)",
];

fn with(first: &str, extra: &[&str]) -> symphony::Decision {
    let mut lines = vec![first, THREE[1]];
    lines.extend_from_slice(extra);
    card(&lines)
}

fn pic(c: &symphony::Decision) -> String {
    art::picture(c).unwrap().unwrap()
}

#[test]
fn the_approval_note_picks_a_candidate_by_letter() {
    assert_eq!(
        pic(&with(
            THREE[0],
            &["  [2026-09-25 10:05] owner · approved — B"]
        )),
        "/b.png"
    );
}

#[test]
fn the_letter_can_sit_inside_a_sentence() {
    let c = with(
        THREE[0],
        &["  [2026-09-25 10:05] owner · approved — go with c, the pincer"],
    );
    assert_eq!(pic(&c), "/c.png");
}

#[test]
fn a_card_still_waiting_shows_its_first_candidate() {
    assert_eq!(pic(&with(&THREE[0].replace("[x]", "[ ]"), &[])), "/a.png");
}

#[test]
fn an_approval_naming_no_candidate_shows_the_first() {
    assert_eq!(
        pic(&with(
            THREE[0],
            &["  [2026-09-25 10:05] owner · approved — nice"]
        )),
        "/a.png"
    );
}

#[test]
fn a_question_naming_a_letter_picks_that_candidate() {
    let c = with(
        &THREE[0].replace("[x]", "[?]"),
        &["  [2026-09-25 10:05] owner · question — B, albeit where do the weapons go?"],
    );
    assert_eq!(pic(&c), "/b.png");
}

#[test]
fn a_question_reads_only_capital_letters_so_an_article_is_not_a_pick() {
    let c = with(
        &THREE[0].replace("[x]", "[?]"),
        &["  [2026-09-25 10:05] owner · question — is a C better here?"],
    );
    assert_eq!(pic(&c), "/c.png");
}

#[test]
fn a_question_naming_no_letter_shows_the_first_candidate() {
    let c = with(
        &THREE[0].replace("[x]", "[?]"),
        &["  [2026-09-25 10:05] owner · question — is this balanced at all?"],
    );
    assert_eq!(pic(&c), "/a.png");
}

#[test]
fn a_plain_approval_keeps_the_letter_an_earlier_question_named() {
    let c = with(
        THREE[0],
        &[
            "  [2026-09-25 10:05] owner · question — B, albeit where do the weapons go?",
            "  [2026-09-25 10:06] agent — they go on the rail.",
            "  [2026-09-25 10:10] owner · approved",
        ],
    );
    assert_eq!(pic(&c), "/b.png");
}

#[test]
fn a_later_letter_overrides_an_earlier_one() {
    let c = with(
        THREE[0],
        &[
            "  [2026-09-25 10:05] owner · question — B, albeit where do the weapons go?",
            "  [2026-09-25 10:10] owner · approved — C after all",
        ],
    );
    assert_eq!(pic(&c), "/c.png");
}

const PETAL: [&str; 3] = [
    "- [x] **bio-f03-art-v5** — Petal. *Shows:* BIO-F03 Petal · ship",
    "  [2026-10-02 23:47] agent — A ![Petal A](/artifacts/v5/a.png) B ![Petal B](/artifacts/v5/b.png)",
    "  [2026-10-02 23:48] agent — Base C ![source](/artifacts/concepts/ships/biological/BIO-F03/shape-c.png)",
];
const SHAPE_C: &str = "/artifacts/concepts/ships/biological/BIO-F03/shape-c.png";

fn petal(extra: &[String]) -> symphony::Decision {
    let mut lines: Vec<&str> = PETAL.to_vec();
    lines.extend(extra.iter().map(String::as_str));
    card(&lines)
}

#[test]
fn explicit_approved_image_path_precedes_candidate_letters() {
    let notes = [
        "base was artifacts/concepts/ships/biological/BIO-F03/shape-c.png",
        "B was close, use `/artifacts/concepts/ships/biological/BIO-F03/shape-c.png`.",
        "Use [the base](/artifacts/concepts/ships/biological/BIO-F03/shape-c.png).",
    ];
    for note in notes {
        let c = petal(&[format!("  [2026-10-05 12:00] owner · approved — {note}")]);
        assert_eq!(pic(&c), SHAPE_C, "{note}");
        assert_eq!(
            art::choose(&[c], "BIO-F03 Petal", "ship").unwrap(),
            Some((SHAPE_C.to_string(), false)),
            "{note}"
        );
    }
    let later = petal(&[
        format!("  [2026-10-05 12:00] owner · approved — {SHAPE_C}"),
        "  [2026-10-05 12:01] owner · approved — B".to_string(),
    ]);
    assert_eq!(pic(&later), "/artifacts/v5/b.png");
    for (who, verdict) in [("owner", "question"), ("agent", "approved")] {
        let c = petal(&[format!(
            "  [2026-10-05 12:00] {who} · {verdict} — {SHAPE_C}"
        )]);
        assert_eq!(pic(&c), "/artifacts/v5/a.png", "{who} {verdict}");
    }
}

#[test]
fn a_path_glued_to_a_word_or_a_longer_path_is_not_an_explicit_pick() {
    for note in [
        "x/artifacts/concepts/ships/biological/BIO-F03/shape-c.png",
        "see xartifacts/concepts/ships/biological/BIO-F03/shape-c.png",
    ] {
        let c = petal(&[format!("  [2026-10-05 12:00] owner · approved — {note}")]);
        assert_eq!(pic(&c), "/artifacts/v5/a.png", "{note}");
    }
}

#[test]
fn an_approval_naming_an_image_never_posted_on_the_card_is_refused() {
    let c = card(&[
        PETAL[0],
        PETAL[1],
        "  [2026-10-05 12:00] owner · approved — use /artifacts/elsewhere/never-posted.png",
    ]);
    let ArtError(message) = art::picture(&c).unwrap_err();
    assert!(message.contains("bio-f03-art-v5"), "{message}");
    assert!(
        message.contains("/artifacts/elsewhere/never-posted.png"),
        "{message}"
    );
}

#[test]
fn a_single_picture_card_ignores_letters_in_the_note() {
    let c = card(&[
        "- [x] **a** — words",
        "  [2026-09-25 10:00] agent — ![one](/one.png)",
        "  [2026-09-25 10:05] owner · approved — a fine B-movie look",
    ]);
    assert_eq!(pic(&c), "/one.png");
}

#[test]
fn a_card_with_no_picture_has_none() {
    let c = card(&[
        "- [x] **a** — words",
        "  [2026-09-25 10:00] agent — no image",
    ]);
    assert_eq!(art::picture(&c).unwrap(), None);
}

fn chosen(lines: &[&str], sub: &str, kind: &str) -> Option<(String, bool)> {
    art::choose(&cards(lines), sub, kind).unwrap()
}

fn picked(path: &str, waiting: bool) -> Option<(String, bool)> {
    Some((path.to_string(), waiting))
}

#[test]
fn the_latest_approval_wins_over_a_newer_pending_picture() {
    let found = chosen(
        &[
            "- [r] **a** — *Shows:* X · ship",
            "  [2026-09-25 10:00] agent — ![a](/a.png)",
            "  [2026-09-25 10:05] owner · approved",
            "- [x] **b** — *Shows:* X · ship",
            "  [2026-09-25 09:00] agent — ![b](/b.png)",
            "  [2026-09-25 11:00] owner · approved",
            "- [ ] **c** — *Shows:* X · ship",
            "  [2026-09-25 12:00] agent — ![c](/c.png)",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, picked("/b.png", false));
}

#[test]
fn with_no_approval_the_newest_pending_picture_is_shown_as_waiting() {
    let found = chosen(
        &[
            "- [ ] **a** — *Shows:* X · ship",
            "  [2026-09-25 10:00] agent — ![a](/a.png)",
            "- [ ] **b** — *Shows:* X · ship",
            "  [2026-09-25 11:00] agent — ![b](/b.png)",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, picked("/b.png", true));
}

#[test]
fn a_questioned_card_is_shown_as_waiting_with_the_letter_named() {
    let found = chosen(
        &[
            "- [?] **a** — *Shows:* X · ship",
            "  [2026-09-25 10:00] agent — A ![X ship A](/a.png) B ![X ship B](/b.png)",
            "  [2026-09-25 10:05] owner · question — B, but why?",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, picked("/b.png", true));
}

#[test]
fn an_approval_still_wins_over_a_newer_questioned_card() {
    let found = chosen(
        &[
            "- [x] **a** — *Shows:* X · ship",
            "  [2026-09-25 09:00] agent — ![a](/a.png)",
            "  [2026-09-25 09:05] owner · approved",
            "- [?] **b** — *Shows:* X · ship",
            "  [2026-09-25 12:00] agent — ![b](/b.png)",
            "  [2026-09-25 12:05] owner · question — hm",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, picked("/a.png", false));
}

#[test]
fn the_newest_waiting_card_wins_whether_pending_or_questioned() {
    let found = chosen(
        &[
            "- [?] **a** — *Shows:* X · ship",
            "  [2026-09-25 10:00] agent — ![a](/a.png)",
            "  [2026-09-25 10:05] owner · question — hm",
            "- [ ] **b** — *Shows:* X · ship",
            "  [2026-09-25 11:00] agent — ![b](/b.png)",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, picked("/b.png", true));
}

#[test]
fn rejected_and_dismissed_pictures_are_never_shown() {
    let found = chosen(
        &[
            "- [-] **a** — *Shows:* X · ship",
            "  [2026-09-25 10:00] agent — ![a](/a.png)",
            "- [~] **b** — *Shows:* X · ship",
            "  [2026-09-25 11:00] agent — ![b](/b.png)",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, None);
}

#[test]
fn a_superseded_picture_is_never_chosen_over_the_approved_one() {
    // The plant of the brief: a rejected redraw posted later than the approved picture.
    let found = chosen(
        &[
            "- [x] **old** — *Shows:* X · ship",
            "  [2026-09-25 09:00] agent — ![old](/old.png)",
            "  [2026-09-25 09:05] owner · approved",
            "- [-] **newer** — *Shows:* X · ship *Replaces:* old",
            "  [2026-09-25 12:00] agent — ![newer](/newer.png)",
            "  [2026-09-25 12:05] owner · rejected — no",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, picked("/old.png", false));
}

#[test]
fn other_slots_do_not_count() {
    let found = chosen(
        &[
            "- [x] **a** — *Shows:* Y · ship",
            "  [2026-09-25 10:00] agent — ![a](/a.png)",
            "- [x] **b** — *Shows:* X · people",
            "  [2026-09-25 10:00] agent — ![b](/b.png)",
        ],
        "X",
        "ship",
    );
    assert_eq!(found, None);
}

const AUTO_CARD: [&str; 3] = [
    "- [a] **a** — Three shapes. *Shows:* X · ship",
    "  [2026-09-25 10:00] agent — A ![X ship A](/a.png) B ![X ship B](/b.png) C ![X ship C](/c.png)",
    "  [2026-09-25 10:05] agent · auto-approved — C: bold outline, clear mounts",
];

#[test]
fn the_auto_line_picks_the_candidate() {
    assert_eq!(pic(&card(&AUTO_CARD)), "/c.png");
}

#[test]
fn it_is_shown_as_approved_not_waiting() {
    assert!(APPROVED.contains(&"auto-approved"));
    assert_eq!(chosen(&AUTO_CARD, "X", "ship"), picked("/c.png", false));
}

#[test]
fn it_wins_over_a_newer_pending_picture() {
    let mut lines = AUTO_CARD.to_vec();
    lines.extend([
        "- [ ] **b** — *Shows:* X · ship",
        "  [2026-09-25 12:00] agent — ![n](/new.png)",
    ]);
    assert_eq!(chosen(&lines, "X", "ship"), picked("/c.png", false));
}

#[test]
fn a_later_owner_approval_is_newer_than_an_earlier_auto_approval() {
    let mut lines = AUTO_CARD.to_vec();
    lines.extend([
        "- [x] **b** — *Shows:* X · ship",
        "  [2026-09-25 09:00] agent — ![o](/owner.png)",
        "  [2026-09-25 11:00] owner · approved",
    ]);
    assert_eq!(chosen(&lines, "X", "ship"), picked("/owner.png", false));
}

#[test]
fn an_auto_approval_dates_from_its_auto_line_not_from_its_post() {
    let lines = [
        AUTO_CARD[0],
        AUTO_CARD[1],
        "  [2026-09-25 12:00] agent · auto-approved — C: bold",
        "- [x] **b** — *Shows:* X · ship",
        "  [2026-09-25 09:00] agent — ![o](/owner.png)",
        "  [2026-09-25 11:00] owner · approved",
    ];
    assert_eq!(chosen(&lines, "X", "ship"), picked("/c.png", false));
}

#[test]
fn the_owner_naming_a_letter_later_overrides_the_auto_letter() {
    let first = AUTO_CARD[0].replace("[a]", "[x]");
    let c = card(&[
        &first,
        AUTO_CARD[1],
        AUTO_CARD[2],
        "  [2026-09-25 11:00] owner · approved — A",
    ]);
    assert_eq!(pic(&c), "/a.png");
}

#[test]
fn a_plain_owner_approval_keeps_the_auto_letter() {
    let first = AUTO_CARD[0].replace("[a]", "[x]");
    let c = card(&[
        &first,
        AUTO_CARD[1],
        AUTO_CARD[2],
        "  [2026-09-25 11:00] owner · approved",
    ]);
    assert_eq!(pic(&c), "/c.png");
}

#[test]
fn a_reverted_auto_approval_no_longer_picks_a_letter() {
    let first = AUTO_CARD[0].replace("[a]", "[ ]");
    let lines = [
        first.as_str(),
        AUTO_CARD[1],
        AUTO_CARD[2],
        "  [2026-09-25 11:00] agent · reverted — back to the owner",
    ];
    assert_eq!(chosen(&lines, "X", "ship"), picked("/a.png", true));
}

#[test]
fn an_owner_rejection_drops_it() {
    let first = AUTO_CARD[0].replace("[a]", "[-]");
    let lines = [
        first.as_str(),
        AUTO_CARD[1],
        AUTO_CARD[2],
        "  [2026-09-25 11:00] owner · rejected — no",
    ];
    assert_eq!(chosen(&lines, "X", "ship"), None);
}

#[test]
fn a_slot_is_a_subfaction_and_the_kinds_after_the_dot() {
    assert_eq!(
        art::parse_slot("Cultivators · people, Ship").unwrap(),
        (
            "Cultivators".to_string(),
            vec!["people".to_string(), "ship".to_string()]
        )
    );
    assert_eq!(
        art::parse_slot(" REP-F01 Kite · ship ").unwrap(),
        ("REP-F01 Kite".to_string(), vec!["ship".to_string()])
    );
}

#[test]
fn a_slot_without_both_halves_is_refused_naming_what_was_written() {
    for bad in [
        "Cultivators",
        "Cultivators · ",
        " · ship",
        "Cultivators · , ,",
    ] {
        let ArtError(message) = art::parse_slot(bad).unwrap_err();
        assert_eq!(
            message,
            format!(
                "an art slot is written [[art:<Subfaction> · <kind>, <kind>]], not [[art:{}]]",
                bad.trim()
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn the_register_cards_are_those_between_the_markers() {
    let lines = [
        "# Page",
        "- [ ] **outside** — before",
        "<!-- register:start -->",
        "- [ ] **inside** — *Shows:* X · ship",
        "<!-- register:end -->",
        "- [ ] **after** — after",
    ];
    let slugs: Vec<String> = art::register_cards(&lines)
        .into_iter()
        .map(|c| c.slug)
        .collect();
    assert_eq!(slugs, ["inside"]);
    assert!(art::register_cards(&["- [ ] **a** — no region"]).is_empty());
}

mod pages {
    use super::super::common::Tree;
    use symphony::art;

    fn names(tree: &Tree, found: Vec<std::path::PathBuf>) -> Vec<String> {
        found
            .iter()
            .map(|p| {
                p.strip_prefix(&tree.root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn pages_are_the_faction_pages_and_one_ships_page_per_roster_faction() {
        let tree = Tree::new();
        for rel in [
            "docs/factions/README.md",
            "docs/factions/FACTIONS.md",
            "docs/factions/RELATIONSHIPS.md",
            "docs/factions/biological.md",
            "docs/factions/alien-coalition.md",
            "docs/ships/README.md",
            "docs/ships/SHIP_ROSTER.md",
            "docs/ships/matrix.md",
            "docs/ships/republic.md",
            "docs/ships/biological.md",
            "docs/ships/retired.md",
        ] {
            tree.write(rel, &format!("# {rel}\n"));
        }
        tree.write(
            art::ROSTER,
            r#"{"hulls": [{"id": "REP-F01", "faction": "republic"}, {"id": "BIO-F01", "faction": "biological"}, {"id": "REP-F02", "faction": "republic"}]}"#,
        );
        assert_eq!(
            names(&tree, art::pages(&tree.root)),
            [
                "docs/factions/alien-coalition.md",
                "docs/factions/biological.md",
                "docs/ships/biological.md",
                "docs/ships/republic.md"
            ]
        );
    }

    #[test]
    fn a_roster_faction_whose_ships_page_is_not_written_yet_has_none() {
        let tree = Tree::new();
        tree.write(
            art::ROSTER,
            r#"{"hulls": [{"id": "REP-F01", "faction": "republic"}]}"#,
        );
        assert!(art::ship_pages(&tree.root).is_empty());
    }

    #[test]
    fn no_roster_means_no_ships_pages() {
        assert!(art::ship_pages(&Tree::new().root).is_empty());
    }
}
