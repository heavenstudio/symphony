//! No line of the owner's and no posted card ever disappears: what the owner said in any worktree
//! counts until an agent answers it, and on a finished branch the answer shows under it on main's
//! card or the branch's own; a branch's answer to main's card reaches the owner whatever pictures
//! main's card shows; and a branch's page marks its drafts and draws main's card for each card the
//! branch did not post.

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
    symphony_board::order::sort_by(&mut pages, &Ord::cmp);
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

            // An agent answers in the done worktree: the rejection is answered, so it waits on
            // nobody, and the owner's line and the answer both stay on the card at its page.
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
            let page = &seen.snapshot.pages[&format!("@tools~done/{DOC}")];
            let abandoned = page.iter().find(|c| c.slug == "abandoned-card").unwrap();
            let lines = abandoned.raw.join("\n");
            assert!(
                lines.contains("Not this one.") && lines.contains("Dropped it."),
                "{lines}"
            );

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

/// The thread of the card `slug` as main's page draws it, with `cards` as main's one cards.
fn mains_thread(root: &std::path::Path, cards: &[symphony::Decision], slug: &str) -> String {
    let doc = docview::document_showing(&Render::new(root), DOC, "", &[], cards).unwrap();
    serde_json::to_string(&doc.threads[slug]).unwrap()
}

pub(super) fn answers_in_a_finished_worktree_stay_under_the_owners_line() {
    isolated(
        "an_answer_in_a_finished_worktree_shows_under_the_owners_line_on_mains_card_and_its_own",
        |_| {
            let tree = repository();
            let main_card = card("main-card", ' ', "");
            // A card the owner rejected on main before the branch forked, answered there.
            let old_card = card(
                "old-card",
                '-',
                &format!("{REJECTED}  [2026-11-11 12:10] agent — Replaced.\n"),
            );
            tree.write(DOC, &register(&(main_card.clone() + &old_card)));
            commit(&tree.root, "base", 1);
            // The branch posts a card and revises the old one, which main then drops; its task is
            // done; the owner asks on two cards on its page.
            let done = branch(&tree, "tools/done", "done");
            write(
                &done,
                &(main_card.clone()
                    + &old_card.replace("Proposal old-card.", "Proposal old-card, revised.")
                    + &card("done-card", ' ', "")),
            );
            commit(&done, "post a card", 2);
            tree.write(DOC, &register(&main_card));
            commit(&tree.root, "drop the old card", 2);
            write(
                &done,
                &(card("main-card", '?', ASKED) + &card("done-card", '?', ASKED)),
            );
            let finished = BTreeSet::from(["tools/done".to_string()]);
            let seen = shown(&tree.root, &finished);
            assert_eq!(
                agent_pages(&seen),
                [
                    ("done-card".to_string(), format!("@tools~done/{DOC}")),
                    ("main-card".to_string(), DOC.to_string()),
                ]
            );
            assert!(
                seen.snapshot.pages[&format!("@tools~done/{DOC}")]
                    .iter()
                    .all(|c| c.slug != "old-card"),
                "what the owner said before the branch forked is not its thread"
            );

            // An agent there answers main's card and commits it, and answers its own card unsaved.
            let yes = "  [2026-11-11 13:00] agent — Yes, it runs on a phone.\n";
            write(
                &done,
                &(card("main-card", '?', &format!("{ASKED}{yes}"))
                    + &card("done-card", '?', ASKED)),
            );
            commit(&done, "answer", 3);
            let fps = "  [2026-11-11 13:10] agent — Yes: 60 fps on a phone.\n";
            write(
                &done,
                &(card("main-card", '?', &format!("{ASKED}{yes}"))
                    + &card("done-card", '?', &format!("{ASKED}{fps}"))),
            );
            let seen = shown(&tree.root, &finished);
            assert!(seen.agent.is_empty(), "{:?}", seen.agent);
            assert_eq!(
                seen.waiting,
                ["done-card", "main-card"],
                "answered, each is the owner's again"
            );
            let paths: Vec<(String, String)> = {
                let mut paths: Vec<_> = seen
                    .snapshot
                    .queue
                    .waiting
                    .iter()
                    .map(|item| (item.slug.clone(), item.doc.clone()))
                    .collect();
                symphony_board::order::sort_by(&mut paths, &Ord::cmp);
                paths
            };
            assert_eq!(
                paths,
                [
                    ("done-card".to_string(), format!("@tools~done/{DOC}")),
                    ("main-card".to_string(), DOC.to_string()),
                ]
            );
            let thread = mains_thread(&tree.root, &seen.snapshot.pages[DOC], "main-card");
            assert!(
                thread.contains("Is it fast on a phone?")
                    && thread.contains("Yes, it runs on a phone."),
                "{thread}"
            );
            let own = &seen.snapshot.pages[&format!("@tools~done/{DOC}")];
            let lines = own
                .iter()
                .find(|c| c.slug == "done-card")
                .unwrap()
                .raw
                .join("\n");
            assert!(
                lines.contains("Is it fast on a phone?") && lines.contains("60 fps"),
                "{lines}"
            );

            // Main holds those lines: nothing is laid on its card twice.
            tree.write(
                DOC,
                &register(&card("main-card", '?', &format!("{ASKED}{yes}"))),
            );
            commit(&tree.root, "main has the answer", 4);
            let seen = shown(&tree.root, &finished);
            assert_eq!(seen.waiting, ["done-card", "main-card"]);
            assert!(!seen.snapshot.pages.contains_key(DOC), "main's own copy");
            // Main closes the card after them: it is settled, the old thread adds nothing.
            tree.write(
                DOC,
                &register(&card("main-card", 'r', &format!("{ASKED}{yes}{RESOLVED}"))),
            );
            commit(&tree.root, "resolved on main", 5);
            let seen = shown(&tree.root, &finished);
            assert_eq!(seen.waiting, ["done-card"]);
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

/// The owner asks, an agent answers and the owner approves, all in the minute 01:19.
const MENU: &str = "  [2026-11-12 01:19] owner · question — Which menu?\n";
const LEFT: &str = "  [2026-11-12 01:19] agent — The left one.\n";
const GO: &str = "  [2026-11-12 01:19] owner · approved\n";

/// The state and group the card `slug` is drawn with on the page `doc` of the checkout at `root`
/// (`scope` the branch's, empty for main), with `cards` as the page's one cards.
fn drawn(
    root: &std::path::Path,
    scope: &str,
    cards: &[symphony::Decision],
    slug: &str,
) -> (String, String) {
    let doc = docview::document_showing(&Render::new(root), DOC, scope, &[], cards).unwrap();
    let card = doc.cards.iter().find(|c| c.slug == slug).unwrap();
    (card.state.clone(), card.group.clone())
}

pub(super) fn a_verdict_in_the_minute_of_the_answer_decides() {
    isolated(
        "an_owners_approval_saved_in_the_minute_of_the_answer_it_follows_closes_the_card",
        |_| {
            let tree = repository();
            let main_card = card("main-card", ' ', "");
            tree.write(DOC, &register(&main_card));
            commit(&tree.root, "base", 1);
            let none = BTreeSet::new();
            let closed = ("approved".to_string(), "resolved".to_string());

            // A branch posts a card. On its page the owner asks, its agent answers and commits,
            // and the owner approves, all in one minute.
            let orphan = branch(&tree, "tools/orphan", "orphan");
            write(
                &orphan,
                &(main_card.clone() + &card("orphan-card", ' ', "")),
            );
            commit(&orphan, "post a card", 2);
            write(
                &orphan,
                &(main_card.clone() + &card("orphan-card", '?', MENU)),
            );
            assert_eq!(shown(&tree.root, &none).agent, ["orphan-card"]);
            let answered = format!("{MENU}{LEFT}");
            write(
                &orphan,
                &(main_card.clone() + &card("orphan-card", '?', &answered)),
            );
            commit(&orphan, "answer", 3);
            let seen = shown(&tree.root, &none);
            assert_eq!(seen.waiting, ["main-card", "orphan-card"], "answered");
            assert!(seen.agent.is_empty(), "{:?}", seen.agent);
            write(
                &orphan,
                &(main_card.clone() + &card("orphan-card", 'x', &format!("{answered}{GO}"))),
            );
            let seen = shown(&tree.root, &none);
            assert!(
                seen.agent.is_empty(),
                "the approval waits on no agent: {:?}",
                seen.agent
            );
            assert_eq!(seen.waiting, ["main-card"]);
            let page = &seen.snapshot.pages[&format!("@tools~orphan/{DOC}")];
            assert_eq!(drawn(&orphan, "tools~orphan", page, "orphan-card"), closed);

            // Main's card, asked on main in that minute: a branch holding main's question answers
            // it, the owner approves on the branch's page, and then its agent commits that too.
            tree.write(DOC, &register(&card("main-card", '?', MENU)));
            commit(&tree.root, "ask on main", 3);
            let answer = branch(&tree, "tools/answer", "answer");
            write(&answer, &card("main-card", '?', &answered));
            commit(&answer, "answer main's card", 4);
            assert_eq!(shown(&tree.root, &none).waiting, ["main-card"], "answered");
            let approved = card("main-card", 'x', &format!("{answered}{GO}"));
            write(&answer, &approved);
            for committed in [false, true] {
                if committed {
                    commit(&answer, "the owner's approval", 5);
                }
                let seen = shown(&tree.root, &none);
                assert!(
                    seen.waiting.is_empty() && seen.agent.is_empty(),
                    "committed={committed}: {:?} {:?}",
                    seen.waiting,
                    seen.agent
                );
                let pages = &seen.snapshot.pages;
                assert_eq!(
                    drawn(&tree.root, "", &pages[DOC], "main-card"),
                    closed,
                    "committed={committed}: main's page"
                );
                assert_eq!(
                    drawn(
                        &answer,
                        "tools~answer",
                        &pages[&format!("@tools~answer/{DOC}")],
                        "main-card"
                    ),
                    closed,
                    "committed={committed}: the branch's page"
                );
            }
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
