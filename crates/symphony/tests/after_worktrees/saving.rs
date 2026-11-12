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
    let none = BTreeSet::new();
    let scan = branches::scan_pages(root, &none);
    let marks = scan
        .pages
        .get(&format!("@tools~mid/{DOC}"))
        .cloned()
        .unwrap_or_default();
    let seen = shown(root, &none);
    let pages = &seen.snapshot.pages;
    let cards = branches::page_cards(
        root,
        mid,
        DOC,
        pages
            .get(&format!("@tools~mid/{DOC}"))
            .map_or(&[][..], Vec::as_slice),
        pages.get(DOC).map_or(&[][..], Vec::as_slice),
        &marks,
    );
    docview::document_marked(
        &Render::new(mid),
        DOC,
        "tools~mid",
        &[],
        &cards,
        &marks.drafts,
    )
    .unwrap()
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
