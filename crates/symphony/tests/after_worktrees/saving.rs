//! What the owner saves on a branch's page: never a verdict that closes a draft (a card its branch
//! has not committed), and an answer on a worktree a merge stopped in, whose page shows the
//! branch's last committed copy instead of the conflict markers.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;
use symphony::merging::Conflict;
use symphony::{branches, docview, render::Render};
use symphony_board::sys::Command;

use super::posted::{card, register, shown, write};
use super::{DOC, branch, commit, isolated, post, repository};

/// A decision the save route refuses: its status and words.
fn refused(
    saves: &symphony::save::Saves,
    scope: &str,
    route: &str,
    payload: serde_json::Value,
) -> (u16, String) {
    match saves.post(scope, route, &payload) {
        Ok(saved) => panic!("saved: {saved}"),
        Err(failure) => (failure.status, failure.error),
    }
}

pub(super) fn a_closing_verdict_on_a_draft_waits_for_its_post() {
    isolated(
        "a_verdict_that_closes_a_draft_is_refused_until_its_branch_posts_it",
        |_| {
            let tree = repository();
            tree.write(DOC, &register(&card("main-card", ' ', "")));
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "tools/draft", "draft");
            write(
                &wt,
                &(card("main-card", ' ', "") + &card("draft-card", ' ', "")),
            );
            let saves = symphony::save::Saves::start(&tree.root);
            for state in ["approved", "dismissed"] {
                let (status, error) = refused(
                    &saves,
                    "tools~draft",
                    "/api/decision",
                    json!({"path":DOC, "slug":"draft-card", "state":state, "note":""}),
                );
                assert_eq!(status, 409);
                assert!(
                    error.contains("draft-card is a draft, not posted yet"),
                    "{error}"
                );
            }
            assert!(
                !std::fs::read_to_string(wt.join(DOC))
                    .unwrap()
                    .contains("owner ·")
            );
            // A card its branch holds as it forked is posted: an approval of it stands.
            post(
                &saves,
                "tools~draft",
                "/api/decision",
                json!({"path":DOC, "slug":"main-card", "state":"approved", "note":""}),
            );
            // A rejection of the draft is saved, and counts.
            post(
                &saves,
                "tools~draft",
                "/api/decision",
                json!({"path":DOC, "slug":"draft-card", "state":"rejected", "note":"Not this way."}),
            );
            let saved = std::fs::read_to_string(wt.join(DOC)).unwrap();
            assert!(
                saved.contains("owner · rejected — Not this way."),
                "{saved}"
            );
            let none = BTreeSet::new();
            let seen = shown(&tree.root, &none);
            assert!(
                seen.agent.contains(&"draft-card".to_string()),
                "{:?}",
                seen.agent
            );

            // Posted, the card takes an approval.
            commit(&wt, "post the card", 2);
            post(
                &saves,
                "tools~draft",
                "/api/decision",
                json!({"path":DOC, "slug":"draft-card", "state":"approved", "note":""}),
            );
            saves.close();
            assert!(
                std::fs::read_to_string(wt.join(DOC))
                    .unwrap()
                    .contains("- [x] **draft-card**")
            );
        },
    );
}

pub(super) fn a_closing_verdict_waits_while_its_agent_changes_a_posted_card() {
    isolated(
        "a_verdict_that_closes_a_posted_card_its_agent_is_changing_is_refused_until_committed",
        |_| {
            let tree = repository();
            tree.write(DOC, &register(&card("main-card", ' ', "")));
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "tools/rewrite", "rewrite");
            let posted = card("main-card", ' ', "")
                + &card("kept-card", ' ', "")
                + &card("rewrite-card", ' ', "")
                + &card("line-card", ' ', "");
            write(&wt, &posted);
            commit(&wt, "post three cards", 2);
            // The agent rewrites one card's text and adds a line under another, uncommitted.
            write(
                &wt,
                &posted
                    .replace("Proposal rewrite-card.", "Rewritten: drop the old menu entirely.")
                    .replace(
                        "**line-card** — Proposal line-card.\n  [2026-11-11 09:00] agent — Posted.\n",
                        "**line-card** — Proposal line-card.\n  [2026-11-11 09:00] agent — Posted.\n  [2026-11-11 10:00] agent — Still changing it.\n",
                    ),
            );
            let saves = symphony::save::Saves::start(&tree.root);
            // A card whose copy differs only by the owner's own lines takes an approval.
            for state in ["question", "approved"] {
                post(
                    &saves,
                    "tools~rewrite",
                    "/api/decision",
                    json!({"path":DOC, "slug":"kept-card", "state":state, "note":"Fine?"}),
                );
            }
            for (slug, state) in [("rewrite-card", "approved"), ("line-card", "dismissed")] {
                let (status, error) = refused(
                    &saves,
                    "tools~rewrite",
                    "/api/decision",
                    json!({"path":DOC, "slug":slug, "state":state, "note":""}),
                );
                assert_eq!(status, 409);
                assert!(
                    error.contains(&format!("{slug} has changes its branch has not posted")),
                    "{error}"
                );
            }
            // A question on the rewritten card is saved, and counts.
            post(
                &saves,
                "tools~rewrite",
                "/api/decision",
                json!({"path":DOC, "slug":"rewrite-card", "state":"question", "note":"Which menu?"}),
            );
            let saved = std::fs::read_to_string(wt.join(DOC)).unwrap();
            assert!(saved.contains("- [x] **kept-card**"), "{saved}");
            assert!(
                saved.contains("- [?] **rewrite-card** — Rewritten"),
                "{saved}"
            );
            assert!(saved.contains("- [ ] **line-card**"), "{saved}");
            let none = BTreeSet::new();
            assert!(
                shown(&tree.root, &none)
                    .agent
                    .contains(&"rewrite-card".to_string())
            );

            // Committed, the rewrite is posted: the owner sees it, and may approve it.
            commit(&wt, "post the rewrite", 3);
            post(
                &saves,
                "tools~rewrite",
                "/api/decision",
                json!({"path":DOC, "slug":"rewrite-card", "state":"approved", "note":""}),
            );
            saves.close();
            assert!(
                std::fs::read_to_string(wt.join(DOC))
                    .unwrap()
                    .contains("- [x] **rewrite-card** — Rewritten")
            );
        },
    );
}

/// `git merge main` in `at`, which stops on a conflict.
fn merge_stops(at: &Path) {
    let done = symphony_board::sys::output(
        Command::new("git")
            .args(["-c", "commit.gpgsign=false", "merge", "-q", "main"])
            .current_dir(at)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
            .env("GIT_COMMITTER_NAME", "Test fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.com"),
    )
    .unwrap();
    assert!(!done.status.success(), "the merge was meant to stop");
    assert!(
        String::from_utf8_lossy(&done.stdout).contains("CONFLICT"),
        "{}",
        String::from_utf8_lossy(&done.stdout)
    );
}

/// The branch page `@tools~mid/DOC` as the app draws it.
fn mid_page(root: &Path, mid: &Path) -> symphony_api::Document {
    branch_page(root, mid, "tools~mid")
}

/// The page `DOC` of the worktree at `wt`, whose branch's id is `id`, as the app draws it.
pub(super) fn branch_page(root: &Path, wt: &Path, id: &str) -> symphony_api::Document {
    let none = BTreeSet::new();
    let page = format!("@{id}/{DOC}");
    let scan = branches::scan_pages(root, &none);
    let marks = scan.pages.get(&page).cloned().unwrap_or_default();
    let seen = shown(root, &none);
    let pages = &seen.snapshot.pages;
    let cards = branches::page_cards(
        root,
        wt,
        DOC,
        pages.get(&page).map_or(&[][..], Vec::as_slice),
        pages.get(DOC).map_or(&[][..], Vec::as_slice),
        &marks,
    );
    docview::document_marked(&Render::new(wt), DOC, id, &[], &cards, &marks.drafts).unwrap()
}

pub(super) fn a_line_git_left_by_a_shared_line_is_not_the_branch_cards() {
    isolated(
        "while_merging_a_line_git_left_after_a_line_two_cards_share_is_not_the_branch_cards",
        |_| {
            let tree = repository();
            tree.write(
                DOC,
                &register(&(card("main-card", ' ', "") + &card("old-card", ' ', ""))),
            );
            commit(&tree.root, "base", 1);
            // Each side puts a card of its own in the old card's place, under the same "Posted."
            // line; main's is approved and resolved.
            let edge = branch(&tree, "tools/edge", "edge");
            write(
                &edge,
                &(card("main-card", ' ', "") + &card("edge-card", ' ', "")),
            );
            commit(&edge, "post", 2);
            let approved =
                "  [2026-11-11 18:00] owner · approved\n  [2026-11-11 19:00] agent · resolved\n";
            tree.write(
                DOC,
                &register(&(card("main-card", ' ', "") + &card("merged-card", 'r', approved))),
            );
            commit(&tree.root, "main posts and settles a card", 3);
            merge_stops(&edge);
            // Git took the shared line and main's lines after it out of the conflict, so the
            // branch's side reads them under its own card.
            let working = std::fs::read_to_string(edge.join(DOC)).unwrap();
            let ours = Conflict::parse(&working).expect("a conflict").ours();
            assert!(
                ours.contains(&format!(
                    "**edge-card** — Proposal edge-card.\n  [2026-11-11 09:00] agent — Posted.\n{approved}"
                )),
                "{working}"
            );

            let page = branch_page(&tree.root, &edge, "tools~edge");
            let edge_card = page.cards.iter().find(|c| c.slug == "edge-card").unwrap();
            assert_eq!(
                edge_card.state, "pending",
                "main's approval is not edge-card's"
            );
            let thread = serde_json::to_string(&page.threads["edge-card"]).unwrap();
            assert!(!thread.contains("approved"), "{thread}");
            let none = BTreeSet::new();
            let seen = shown(&tree.root, &none);
            assert!(
                seen.waiting.contains(&"edge-card".to_string()),
                "{:?}",
                seen.waiting
            );
            assert!(seen.agent.is_empty(), "{:?}", seen.agent);

            // Its lines cross a marker: it is answered once the merge is done.
            let saves = symphony::save::Saves::start(&tree.root);
            let (status, error) = refused(
                &saves,
                "tools~edge",
                "/api/decision",
                json!({"path":DOC, "slug":"edge-card", "state":"question", "note":"Why?"}),
            );
            saves.close();
            assert_eq!(status, 409);
            assert!(error.contains("sits across a conflict"), "{error}");
            assert_eq!(std::fs::read_to_string(edge.join(DOC)).unwrap(), working);
        },
    );
}

pub(super) fn a_merging_worktree_shows_its_last_committed_copy() {
    isolated(
        "a_branch_page_a_merge_stopped_on_shows_its_last_committed_copy_and_takes_an_answer",
        |_| {
            let tree = repository();
            tree.write(
                DOC,
                &register(&(card("main-card", ' ', "") + &card("settled", ' ', ""))),
            );
            commit(&tree.root, "base", 1);
            // The branch answers `settled` and posts a card; main answers it otherwise.
            let mid = branch(&tree, "tools/mid", "mid");
            write(
                &mid,
                &(card("main-card", ' ', "")
                    + &card(
                        "settled",
                        ' ',
                        "  [2026-11-11 10:00] agent — The branch's take.\n",
                    )
                    + &card("mid-card", ' ', "")),
            );
            commit(&mid, "post", 2);
            tree.write(
                DOC,
                &register(
                    &(card("main-card", ' ', "")
                        + &card(
                            "settled",
                            ' ',
                            "  [2026-11-11 11:00] agent — Main's take.\n",
                        )),
                ),
            );
            commit(&tree.root, "main answers", 3);
            merge_stops(&mid);
            let working = std::fs::read_to_string(mid.join(DOC)).unwrap();
            assert!(working.contains("<<<<<<<"), "{working}");

            let page = mid_page(&tree.root, &mid);
            assert_eq!(page.sections[0].key, "merging");
            assert!(page.sections[0].html.contains("Merging."));
            for section in &page.sections {
                for marker in ["<<<<<<<", "=======", ">>>>>>>"] {
                    assert!(!section.html.contains(marker), "{}", section.html);
                }
            }
            let slugs: Vec<&str> = page.cards.iter().map(|c| c.slug.as_str()).collect();
            assert_eq!(
                slugs,
                ["main-card", "settled", "mid-card"],
                "each card once"
            );
            let drawn = page
                .sections
                .iter()
                .filter(|s| s.html.contains("data-slug=\"settled\""))
                .count();
            assert_eq!(drawn, 1);
            let settled = serde_json::to_string(&page.threads["settled"]).unwrap();
            assert!(settled.contains("The branch"), "{settled}");

            // The owner asks on the branch's card: saved on the branch's side of the conflict.
            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "tools~mid",
                "/api/decision",
                json!({"path":DOC, "slug":"mid-card", "state":"question", "note":"Is it merged yet?"}),
            );
            let (status, error) = refused(
                &saves,
                "tools~mid",
                "/api/comment",
                json!({"path":DOC, "after_line":3, "kind":"note", "title":"A note", "body":""}),
            );
            assert_eq!(status, 409);
            assert!(error.contains("being merged"), "{error}");
            saves.close();
            let working = std::fs::read_to_string(mid.join(DOC)).unwrap();
            let conflict = Conflict::parse(&working).expect("the conflict as it was");
            assert_eq!(working.matches("Main's take.").count(), 1);
            let ours = conflict.ours();
            assert!(ours.contains("- [?] **mid-card**"), "{ours}");
            assert!(
                ours.contains("owner · question — Is it merged yet?"),
                "{ours}"
            );

            // It waits on an agent, and the page shows it on the committed copy.
            let none = BTreeSet::new();
            let seen = shown(&tree.root, &none);
            assert_eq!(seen.agent, ["mid-card"]);
            let page = mid_page(&tree.root, &mid);
            let thread = serde_json::to_string(&page.threads["mid-card"]).unwrap();
            assert!(thread.contains("Is it merged yet?"), "{thread}");
            assert!(page.sections.iter().all(|s| !s.html.contains("<<<<<<<")));

            // The merge is finished on the branch's side: the page is the file again, the
            // owner's question in it.
            std::fs::write(mid.join(DOC), &ours).unwrap();
            commit(&mid, "merge main", 4);
            let page = mid_page(&tree.root, &mid);
            assert_ne!(page.sections[0].key, "merging");
            let thread = serde_json::to_string(&page.threads["mid-card"]).unwrap();
            assert!(thread.contains("Is it merged yet?"), "{thread}");
        },
    );
}
