//! No line of the owner's and no posted card ever disappears: what the owner said in any worktree
//! counts until an agent answers it, a branch's answer to main's card reaches the owner whatever
//! pictures main's card shows, and a branch's page marks its drafts and draws main's card for each
//! card the branch did not post.

use std::collections::BTreeSet;

use super::posted::{ANSWER, QUESTION, RESOLVED, card, register, shown, write};
use super::{DOC, branch, commit, isolated, repository};
use symphony::{branches, docview, render::Render};

const REJECTED: &str = "  [2026-11-11 12:00] owner · rejected — Not this one.\n";
const ASKED: &str = "  [2026-11-11 12:30] owner · question — Is it fast on a phone?\n";

/// The branch page each card waiting on an agent is read on, by slug.
fn agent_pages(seen: &super::posted::Shown) -> Vec<(String, String)> {
    let mut pages: Vec<(String, String)> = seen
        .snapshot
        .queue
        .agent
        .iter()
        .map(|item| (item.slug.clone(), item.path.clone()))
        .collect();
    pages.sort();
    pages
}

pub(super) fn owner_lines_count_until_an_agent_answers() {
    isolated(
        "an_owner_line_counts_wherever_it_was_saved_until_an_agent_answers",
        |_| {
            let tree = repository();
            tree.write(DOC, &register(&card("main-card", ' ', "")));
            commit(&tree.root, "base", 1);
            let main_card = card("main-card", ' ', "");

            // A live branch posts a card; the owner rejects a draft of its on the branch's page.
            let posted = branch(&tree, "tools/posted", "posted");
            write(
                &posted,
                &(main_card.clone() + &card("posted-card", ' ', "")),
            );
            commit(&posted, "post a card", 2);
            write(
                &posted,
                &(main_card.clone()
                    + &card("posted-card", ' ', "")
                    + &card("posted-draft", '-', REJECTED)),
            );
            // A branch whose task is done holds a card only it carries; the owner rejects it there.
            let done = branch(&tree, "tools/done", "done");
            write(
                &done,
                &(main_card.clone() + &card("abandoned-card", ' ', "")),
            );
            commit(&done, "post a card", 2);
            write(
                &done,
                &(main_card.clone() + &card("abandoned-card", '-', REJECTED)),
            );

            let finished = BTreeSet::from(["tools/done".to_string()]);
            let seen = shown(&tree.root, &finished);
            assert_eq!(seen.waiting, ["main-card", "posted-card"]);
            assert_eq!(
                agent_pages(&seen),
                [
                    ("abandoned-card".to_string(), format!("@tools~done/{DOC}")),
                    ("posted-draft".to_string(), format!("@tools~posted/{DOC}")),
                ],
                "each waits on an agent, read on its branch's page"
            );

            // The owner asks on the posted card, then its task is done too: the question and the
            // rejected draft still wait on an agent; the card itself is no longer posted.
            write(
                &posted,
                &(main_card.clone()
                    + &card("posted-card", '?', ASKED)
                    + &card("posted-draft", '-', REJECTED)),
            );
            let both = BTreeSet::from(["tools/done".to_string(), "tools/posted".to_string()]);
            let seen = shown(&tree.root, &both);
            assert_eq!(seen.waiting, ["main-card"]);
            assert_eq!(
                seen.agent,
                ["abandoned-card", "posted-card", "posted-draft"]
            );
            assert_eq!(seen.counts, (1, 1, 1));

            // An agent answers in the done worktree: the owner's line is answered there, and the
            // done branch's card leaves until it lands.
            write(
                &done,
                &(main_card.clone()
                    + &card(
                        "abandoned-card",
                        '-',
                        &format!("{REJECTED}  [2026-11-11 13:00] agent — Dropped it.\n"),
                    )),
            );
            let seen = shown(&tree.root, &both);
            assert_eq!(seen.agent, ["posted-card", "posted-draft"]);

            // An agent answers the draft: it is the agent's to post now. Back in progress, the
            // posted card is posted again, the owner's question on it.
            write(
                &posted,
                &(main_card.clone()
                    + &card("posted-card", '?', ASKED)
                    + &card(
                        "posted-draft",
                        '-',
                        &format!("{REJECTED}  [2026-11-11 13:00] agent — Replaced it.\n"),
                    )),
            );
            let seen = shown(&tree.root, &finished);
            assert_eq!(seen.agent, ["posted-card"]);
            assert_eq!(
                seen.snapshot.queue.agent[0].path,
                format!("@tools~posted/{DOC}")
            );
        },
    );
}

pub(super) fn branch_answer_reaches_the_owner_whatever_mains_pictures() {
    isolated(
        "a_branch_answer_to_a_main_card_with_a_picture_only_main_holds_reaches_the_owner",
        |_| {
            let tree = repository();
            tree.write(".gitignore", "/artifacts/\n");
            let shared = "- [ ] **shared** — Pick one: ![S](/artifacts/shared.png)\n  [2026-11-11 09:00] agent — Posted.\n";
            tree.write(DOC, &register(shared));
            tree.write("artifacts/shared.png", "png");
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "tools/posted", "posted");
            assert!(!wt.join("artifacts/shared.png").exists());
            // The owner asks on main; the branch posts the answer.
            tree.write(
                DOC,
                &register(&format!("{}{QUESTION}", shared.replace("[ ]", "[?]"))),
            );
            commit(&tree.root, "ask", 2);
            write(&wt, &format!("{shared}{ANSWER}"));
            commit(&wt, "answer", 2);

            let none = BTreeSet::new();
            let seen = shown(&tree.root, &none);
            assert_eq!(
                seen.waiting,
                ["shared"],
                "answered, it is the owner's again"
            );
            assert!(seen.agent.is_empty());
            let cards = seen.snapshot.pages[DOC].clone();
            let doc =
                docview::document_showing(&Render::new(&tree.root), DOC, "", &[], &cards).unwrap();
            let thread = serde_json::to_string(&doc.threads["shared"]).unwrap();
            assert!(thread.contains("Why B?") && thread.contains("Because B is faster."));
            assert!(
                symphony::docs_audit::audit(&wt)
                    .errors
                    .iter()
                    .all(|e| !e.contains("shared")),
                "main's picture is not the answering branch's to put on disk"
            );

            // An answer showing a picture of its own not on disk waits for it, and comes when it
            // lands.
            write(
                &wt,
                &format!(
                    "{shared}  [2026-11-11 17:59] agent — Because B is faster: ![B](/artifacts/b.png)\n"
                ),
            );
            commit(&wt, "answer with a picture", 3);
            let scan = branches::scan_pages(&tree.root, &none);
            assert_eq!(scan.awaited.len(), 1);
            assert!(
                scan.awaited[0].ends_with("posted/artifacts/b.png"),
                "{:?}",
                scan.awaited
            );
            let held = shown(&tree.root, &none);
            assert_eq!(
                held.agent,
                ["shared"],
                "main's question still waits on an agent"
            );
            assert!(held.waiting.is_empty());
            tree.write("posted/artifacts/b.png", "png");
            assert_eq!(shown(&tree.root, &none).waiting, ["shared"]);
            assert!(branches::scan_pages(&tree.root, &none).awaited.is_empty());
        },
    );
}

pub(super) fn branch_page_marks_drafts_and_draws_mains_cards() {
    isolated(
        "a_branch_page_marks_its_drafts_and_draws_mains_card_for_every_card_it_did_not_post",
        |_| {
            let tree = repository();
            let base = card("main-card", ' ', "") + &card("settled", ' ', "");
            tree.write(DOC, &register(&base));
            commit(&tree.root, "base", 1);
            // The branch posts a copy of `settled` the owner rejected, then drafts a card.
            let stale = branch(&tree, "tools/stale-copy", "stale");
            let too_dark = "  [2026-11-11 12:00] owner · rejected — Too dark.\n";
            let posted = card("main-card", ' ', "") + &card("settled", '-', too_dark);
            write(&stale, &posted);
            commit(&stale, "an old copy", 2);
            write(&stale, &(posted.clone() + &card("draft-card", ' ', "")));
            // Main resolves `settled` after that copy, and the owner approves main's card there.
            let approved = "  [2026-11-11 18:30] owner · approved\n";
            tree.write(
                DOC,
                &register(
                    &(card("main-card", 'x', approved)
                        + &card("settled", 'r', &format!("{too_dark}{RESOLVED}"))),
                ),
            );
            commit(&tree.root, "main moves on", 3);

            let none = BTreeSet::new();
            let scan = branches::scan_pages(&tree.root, &none);
            let page = format!("@tools~stale-copy/{DOC}");
            let marks = scan.pages[&page].clone();
            assert_eq!(marks.drafts, BTreeSet::from(["draft-card".to_string()]));
            assert_eq!(marks.own, BTreeSet::from(["settled".to_string()]));
            let seen = shown(&tree.root, &none);
            assert!(
                !seen
                    .waiting
                    .iter()
                    .any(|slug| slug == "draft-card" || slug == "settled")
            );
            let pages = &seen.snapshot.pages;
            let cards = branches::page_cards(
                &tree.root,
                &stale,
                DOC,
                pages.get(&page).map_or(&[][..], Vec::as_slice),
                pages.get(DOC).map_or(&[][..], Vec::as_slice),
                &marks,
            );
            let doc = docview::document_marked(
                &Render::new(&stale),
                DOC,
                "tools~stale-copy",
                &[],
                &cards,
                &marks.drafts,
            )
            .unwrap();
            let state = |slug: &str| {
                let card = doc.cards.iter().find(|c| c.slug == slug).unwrap();
                (card.state.clone(), card.group.clone())
            };
            assert_eq!(
                state("settled").0,
                "resolved",
                "main's resolution, not the stale copy"
            );
            assert_eq!(
                state("main-card").0,
                "approved",
                "main's card as main has it"
            );
            let thread = serde_json::to_string(&doc.threads["main-card"]).unwrap();
            assert!(thread.contains("approved"), "{thread}");
            let drawn = |slug: &str| {
                doc.sections
                    .iter()
                    .find(|s| s.html.contains(&format!("data-slug=\"{slug}\"")))
                    .map(|s| s.html.clone())
                    .unwrap_or_default()
            };
            assert!(
                drawn("draft-card")
                    .contains(&format!("<time class=\"posted\">{} · ", docview::DRAFT)),
                "{}",
                drawn("draft-card")
            );
            assert!(!drawn("main-card").contains(docview::DRAFT));
        },
    );
}
