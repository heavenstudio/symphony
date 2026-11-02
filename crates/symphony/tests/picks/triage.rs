//! The owner's decision churn and the ranked review queue (the auto-approve cases go with
//! tools/python-zero/art-cards).

use symphony::board::{Doc, scan};
use symphony::triage::{self, Bucket};

use super::card;
use super::common::Tree;

fn register(lines: &[&str]) -> String {
    format!(
        "# Page\n\n<!-- register:start -->\n### T\n\n{}\n<!-- register:end -->\n",
        lines.join("\n")
    )
}

fn ships() -> String {
    register(&[
        // first post, approved first time with a note and a letter
        "- [r] **rep-a-art** — A. *Shows:* REP-A Alpha · ship",
        "  [2026-09-25 10:00] agent — A ![REP-A Alpha ship A](/a1.png) B ![REP-A Alpha ship B](/a2.png)",
        "  [2026-09-25 11:00] owner · approved — B, but brighter engines",
        "  [2026-09-25 12:00] agent — done",
        "  [2026-09-25 13:00] agent · resolved",
        // first post, rejected
        "- [-] **rep-b-art** — B. *Shows:* REP-B Beta · ship",
        "  [2026-09-25 10:00] agent — ![b](/b.png)",
        "  [2026-09-25 11:00] owner · rejected — too flat",
        // redraw, approved plainly
        "- [r] **rep-b-art-v2** — B again. *Shows:* REP-B Beta · ship *Replaces:* rep-b-art",
        "  [2026-09-26 10:00] agent — A ![REP-B Beta ship A](/b1.png) B ![REP-B Beta ship B](/b2.png) C ![REP-B Beta ship C](/b3.png)",
        "  [2026-09-26 11:00] owner · approved — A",
        "  [2026-09-26 13:00] agent · resolved",
        // pending, never answered
        "- [ ] **rep-c-art** — C. *Shows:* REP-C Gamma · ship",
        "  [2026-09-26 09:00] agent — A ![REP-C Gamma ship A](/c1.png) B ![REP-C Gamma ship B](/c2.png)",
    ])
}

fn faction() -> String {
    register(&[
        "- [?] **folk-people** — People. *Shows:* Folk · people",
        "  [2026-09-25 10:00] agent — ![p](/p.png)",
        "  [2026-09-25 11:00] owner · question — why the hats?",
        "  [2026-09-25 12:00] agent — they are helmets.",
    ])
}

fn design() -> String {
    register(&[
        "- [ ] **weapon-kinds** — Should damage differ? A: yes. B: no. Approve and say A or B in the note, or comment. [[ship:REP-A|small]] [[ships:REP-B,REP-C]]",
        "  [2026-09-26 09:00] agent — posted",
        "- [ ] **small-thing** — A minor rule.",
        "  [2026-09-26 08:00] agent — posted",
        "- [ ] **after-kinds** — Needs the kinds first. *Depends:* weapon-kinds",
        "- [x] **picked** — A or B? Approve and say A or B in the note.",
        "  [2026-09-24 10:00] owner · approved — B",
        "- [-] **refused** — no.",
        "  [2026-09-24 10:00] owner · rejected — no",
    ])
}

fn tree() -> Tree {
    let tree = Tree::new();
    tree.write("docs/ships/republic.md", &ships());
    tree.write("docs/factions/folk.md", &faction());
    tree.write("docs/WEAPONS.md", &design());
    tree
}

fn docs(tree: &Tree) -> Vec<Doc> {
    scan(&tree.root).unwrap()
}

mod category {
    use super::*;

    #[test]
    fn a_ships_page_card_is_roster_art_for_its_faction() {
        let c = card(&["- [ ] **a** — x *Shows:* R · ship"]);
        for path in [
            "docs/ships/republic.md",
            "@design~policy/docs/ships/republic.md",
        ] {
            assert_eq!(triage::category(path, &c), "roster art · republic");
        }
    }

    #[test]
    fn a_faction_page_picture_is_faction_art() {
        let c = card(&["- [ ] **a** — x *Shows:* Folk · people"]);
        assert_eq!(triage::category("docs/factions/folk.md", &c), "faction art");
    }

    #[test]
    fn any_other_card_is_design_for_its_doc() {
        let c = card(&["- [ ] **a** — a rule"]);
        for path in ["docs/WEAPONS.md", "@design~policy/docs/WEAPONS.md"] {
            assert_eq!(triage::category(path, &c), "design · docs/WEAPONS.md");
        }
    }

    #[test]
    fn an_older_untagged_picture_on_a_faction_page_counts_as_faction_art() {
        let pic = card(&[
            "- [r] **a** — Ram ships, second pass",
            "  [2026-09-24 10:00] agent — ![r](/r.png)",
        ]);
        assert_eq!(
            triage::category("docs/factions/empire.md", &pic),
            "faction art"
        );
        let rule = card(&["- [r] **a** — a trait"]);
        assert_eq!(
            triage::category("docs/factions/empire.md", &rule),
            "design · docs/factions/empire.md"
        );
    }

    #[test]
    fn a_card_is_art_only_when_it_fills_a_picture_slot() {
        assert!(triage::is_art(&card(&[
            "- [ ] **a** — x *Shows:* R · ship"
        ])));
        let gun = card(&[
            "- [ ] **a** — a picture of a gun",
            "  [2026-09-26 10:00] agent — ![g](/g.png)",
        ]);
        assert!(!triage::is_art(&gun));
    }

    #[test]
    fn a_model_card_has_its_own_category() {
        let c = card(&["- [ ] **rep-f02-model-v1** — Kite. *Model:* REP-F02 v1"]);
        // under the `### Republic` topic of docs/art/HULL_MODELS.md
        let lines = [
            "### Republic",
            "",
            "- [ ] **rep-f02-model-v1** — Kite. *Model:* REP-F02 v1",
        ];
        let under = symphony::register::parse_decisions(&lines);
        assert_eq!(
            triage::category("docs/art/HULL_MODELS.md", &under[0]),
            "hull model · republic"
        );
        assert_eq!(
            triage::category("docs/art/HULL_MODELS.md", &c),
            "hull model · general"
        );
    }
}

mod first_verdict {
    use super::*;

    fn verdict(lines: &[&str]) -> Option<String> {
        triage::first_verdict(&card(lines))
    }

    #[test]
    fn the_owners_first_verdict_counts_not_the_last() {
        let v = verdict(&[
            "- [x] **a** — x",
            "  [2026-09-24 10:00] owner · rejected — no",
            "  [2026-09-24 11:00] owner · approved",
        ]);
        assert_eq!(v.as_deref(), Some("rejected"));
    }

    #[test]
    fn a_chat_line_and_a_reset_are_not_verdicts() {
        let v = verdict(&[
            "- [x] **a** — x",
            "  [2026-09-24 09:00] owner — hm",
            "  [2026-09-24 09:30] owner · pending",
            "  [2026-09-24 10:00] owner · approved",
        ]);
        assert_eq!(v.as_deref(), Some("approved"));
    }

    #[test]
    fn a_card_with_only_chat_from_the_owner_has_none() {
        assert_eq!(
            verdict(&[
                "- [ ] **a** — x",
                "  [2026-09-24 09:00] owner — what about the guns?"
            ]),
            None
        );
    }

    #[test]
    fn an_unanswered_card_has_none() {
        assert_eq!(
            verdict(&["- [ ] **a** — x", "  [2026-09-24 09:00] agent — pic"]),
            None
        );
    }

    #[test]
    fn agent_verdicts_are_not_the_owners() {
        assert_eq!(
            verdict(&[
                "- [~] **a** — x",
                "  [2026-09-24 09:00] agent · dismissed — superseded by **b**"
            ]),
            None
        );
        assert_eq!(
            verdict(&[
                "- [a] **a** — x",
                "  [2026-09-24 09:00] agent · auto-approved — A: fine"
            ]),
            None
        );
    }

    #[test]
    fn an_undated_card_falls_back_on_its_marker() {
        assert_eq!(verdict(&["- [r] **a** — x"]).as_deref(), Some("approved"));
        assert_eq!(
            verdict(&["- [-] **a** — x", "  owner words"]).as_deref(),
            Some("rejected")
        );
    }
}

mod stats {
    use super::*;

    fn bucket(decided: u32, approved_first: u32) -> Bucket {
        Bucket {
            decided,
            approved_first,
        }
    }

    #[test]
    fn first_pass_approval_rate_per_category() {
        let found = triage::stats(&docs(&tree())).unwrap();
        assert_eq!(found.categories["roster art · republic"], bucket(3, 2));
        assert_eq!(found.categories["faction art"], bucket(1, 0));
        assert_eq!(found.categories["design · docs/WEAPONS.md"], bucket(2, 1));
    }

    #[test]
    fn redraws_against_first_posts() {
        let found = triage::stats(&docs(&tree())).unwrap();
        assert_eq!(found.redraw, bucket(1, 1));
        assert_eq!(found.first_post, bucket(5, 2));
    }

    #[test]
    fn how_often_an_approval_carries_a_note() {
        // rep-a-art's note is work; rep-b-art-v2's "A" and picked's "B" only name a letter
        let found = triage::stats(&docs(&tree())).unwrap();
        assert_eq!((found.approvals, found.with_note), (3, 1));
    }

    #[test]
    fn the_letters_the_owner_picked() {
        let found = triage::stats(&docs(&tree())).unwrap();
        assert_eq!(
            found.art_letters.into_iter().collect::<Vec<_>>(),
            [("A".to_string(), 1), ("B".to_string(), 1)]
        );
        assert_eq!(
            found.design_letters.into_iter().collect::<Vec<_>>(),
            [("B".to_string(), 1)]
        );
    }

    #[test]
    fn the_share_answered_with_a_question() {
        let found = triage::stats(&docs(&tree())).unwrap();
        assert_eq!((found.decided, found.asked), (6, 1));
    }

    #[test]
    fn the_report_prints_each_part() {
        let text = triage::render_stats(&triage::stats(&docs(&tree())).unwrap());
        let row = |name: &str| {
            text.lines()
                .find(|l| l.contains(name))
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(
            row("roster art · republic").ends_with("roster art · republic 2/3 67%"),
            "{text}"
        );
        assert!(row("redraw").ends_with("redraw 1/1 100%"), "{text}");
        assert!(text.contains("approvals with a note: 1/3 33%"), "{text}");
        assert!(text.contains("art letters: A 1 · B 1"), "{text}");
        assert!(text.contains("design letters: B 1"), "{text}");
        assert!(text.contains("answered with a question: 1/6 17%"), "{text}");
    }

    #[test]
    fn the_report_is_laid_out_as_python_laid_it() {
        let text = triage::render_stats(&triage::stats(&docs(&tree())).unwrap());
        let expected = "\
FIRST-PASS APPROVAL (the owner's first verdict was an approval)
  design · docs/WEAPONS.md  1/2      50%
  faction art               0/1       0%
  roster art · republic     2/3      67%

  first post                2/5      40%
  redraw                    1/1     100%

approvals with a note: 1/3 33% (a note is still work)
art letters: A 1 · B 1
design letters: B 1
answered with a question: 1/6 17%";
        assert_eq!(text, expected);
    }

    #[test]
    fn a_rate_over_nothing_is_a_dash_and_halves_round_to_even() {
        let text = triage::render_stats(&triage::stats(&[]).unwrap());
        assert!(text.contains("first post"));
        assert!(text.contains("0/0        -"), "{text}");
        assert!(text.contains("art letters: none"), "{text}");
        // 1 of 8 is 12.5%: Python's round() goes to the even 12
        let one_in_eight = register(
            &(0..8)
                .flat_map(|i| {
                    let verdict = if i == 0 { "approved" } else { "rejected" };
                    let mark = if i == 0 { "x" } else { "-" };
                    vec![
                        format!("- [{mark}] **c{i}** — x"),
                        format!("  [2026-09-24 10:00] owner · {verdict} — fine"),
                    ]
                })
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        let tree = Tree::new();
        tree.write("docs/X.md", &one_in_eight);
        let text = triage::render_stats(&triage::stats(&scan(&tree.root).unwrap()).unwrap());
        assert!(text.contains("1/8      12%"), "{text}");
    }
}

mod queue {
    use super::*;

    const APP: &str = "http://127.0.0.1:8765";

    fn slugs(items: &[triage::Item]) -> Vec<&str> {
        items.iter().map(|i| i.slug.as_str()).collect()
    }

    #[test]
    fn design_cards_come_first_ranked_by_what_they_gate() {
        let items = triage::queue(&docs(&tree()), 10, APP);
        assert_eq!(
            slugs(&items[..3]),
            ["weapon-kinds", "after-kinds", "small-thing"]
        );
        assert!(items[0].reason.contains("3 hulls"), "{}", items[0].reason);
        assert!(items[0].reason.contains("1 card"), "{}", items[0].reason);
    }

    #[test]
    fn then_answered_questions_then_undecided_art() {
        let items = triage::queue(&docs(&tree()), 10, APP);
        assert_eq!(slugs(&items[3..]), ["folk-people", "rep-c-art"]);
    }

    #[test]
    fn auto_approved_and_decided_cards_are_not_in_it() {
        let tree = tree();
        let text = tree
            .read("docs/ships/republic.md")
            .replace("- [ ] **rep-c-art**", "- [a] **rep-c-art**");
        tree.write("docs/ships/republic.md", &text);
        let items = triage::queue(&docs(&tree), 10, APP);
        assert!(!slugs(&items).contains(&"rep-c-art"));
        assert!(!slugs(&items).contains(&"picked"));
    }

    #[test]
    fn a_question_an_agent_asked_says_so_not_that_the_triage_left_it() {
        let ask = "\n  [2026-10-03 08:02] owner · approved — C\n  [2026-10-03 08:21] agent · question — Which C?\n";
        let tree = tree();
        let art = tree.read("docs/ships/republic.md");
        tree.write(
            "docs/ships/republic.md",
            &format!("{art}\n- [x] **asked-art** — A picture. *Shows:* REP-A1 Foo · ship{ask}"),
        );
        let rule = tree.read("docs/WEAPONS.md");
        tree.write(
            "docs/WEAPONS.md",
            &format!("{rule}\n- [x] **asked-design** — A rule.{ask}"),
        );
        let items = triage::queue(&docs(&tree), 20, APP);
        let reason = |slug: &str| {
            items
                .iter()
                .find(|i| i.slug == slug)
                .unwrap()
                .reason
                .clone()
        };
        assert!(reason("asked-art").contains("an agent asks you"));
        assert!(reason("asked-design").contains("an agent asks you"));
        assert!(!reason("asked-art").contains("triage"));
        assert!(reason("rep-c-art").contains("triage left it for you"));
    }

    #[test]
    fn it_stops_at_n() {
        assert_eq!(triage::queue(&docs(&tree()), 2, APP).len(), 2);
    }

    #[test]
    fn each_item_names_its_doc_and_the_link_that_scrolls_to_the_card() {
        let item = triage::queue(&docs(&tree()), 1, APP).remove(0);
        assert_eq!(item.doc, "docs/WEAPONS.md");
        assert_eq!(
            item.link,
            "http://127.0.0.1:8765/docs/WEAPONS.md#weapon-kinds"
        );
    }

    #[test]
    fn the_report_prints_slug_doc_reason_and_link() {
        let text = triage::render_queue(&triage::queue(&docs(&tree()), 2, APP));
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[0].starts_with(" 1. weapon-kinds  docs/WEAPONS.md"),
            "{lines:?}"
        );
        assert!(lines[1].contains("gates"));
        assert_eq!(
            lines[2].trim(),
            "http://127.0.0.1:8765/docs/WEAPONS.md#weapon-kinds"
        );
        assert!(lines[3].starts_with(" 2. after-kinds"), "{lines:?}");
        assert_eq!(triage::render_queue(&[]), "nothing waiting on the owner");
    }

    #[test]
    fn a_pending_model_card_is_listed_as_a_hull_model_the_owner_alone_signs_off() {
        let tree = Tree::new();
        tree.write(
            "docs/art/HULL_MODELS.md",
            &register(&["- [ ] **rep-f02-model-v1** — Kite. *Model:* REP-F02 v1"])
                .replace("### T", "### Republic"),
        );
        let items = triage::queue(&docs(&tree), 5, "http://app");
        let found: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.slug.as_str(), i.reason.as_str()))
            .collect();
        assert_eq!(
            found,
            [(
                "rep-f02-model-v1",
                "hull model · republic · only you sign it off"
            )]
        );
    }

    #[test]
    fn gates_count_the_hulls_a_card_names_and_the_cards_waiting_on_it() {
        let docs = docs(&tree());
        let cards: Vec<(String, symphony::Decision)> = docs
            .iter()
            .flat_map(|d| d.decisions.iter().map(|c| (d.path.clone(), c.clone())))
            .collect();
        let kinds = cards
            .iter()
            .find(|(_, c)| c.slug == "weapon-kinds")
            .unwrap();
        assert_eq!(triage::gates(&kinds.1, &cards), (3, 1));
        let small = cards.iter().find(|(_, c)| c.slug == "small-thing").unwrap();
        assert_eq!(triage::gates(&small.1, &cards), (0, 0));
        let blocker = card(&["- [ ] **b** — Holds others. *Blocks:* x, y, b"]);
        assert_eq!(triage::gates(&blocker, &[]), (0, 2));
    }
}

mod app {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn the_link_points_at_the_app_of_this_tree() {
        let tree = Tree::new();
        assert_eq!(triage::app_url(&tree.root), "http://127.0.0.1:8765");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        tree.write("artifacts/docs-app.port", &format!("{port}\n"));
        assert_eq!(
            triage::app_url(&tree.root),
            format!("http://127.0.0.1:{port}")
        );
    }

    #[test]
    fn a_record_nothing_listens_on_is_stale() {
        let tree = Tree::new();
        // Binding zero assigns another port, so this record can never name a listener.
        let port = 0;
        tree.write("artifacts/docs-app.port", &format!("{port}\n"));
        assert_eq!(triage::app_url(&tree.root), "http://127.0.0.1:8765");
        tree.write("artifacts/docs-app.port", "not a port\n");
        assert_eq!(triage::app_url(&tree.root), "http://127.0.0.1:8765");
    }
}
