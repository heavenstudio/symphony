//! The owner's line on a card main holds is main's, whichever branch's page it was given on: a
//! landed branch's page left open, a live branch's page drawing main's card, a page whose worktree
//! is gone. It is saved in main's register and committed there, never left in a worktree a land
//! does not carry, so `just symphony-status` reads on main what the page shows. A card the branch
//! posted keeps its line with the branch.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;
use symphony::{docview, render::Render};

use super::posted::{card, register, shown, write};
use super::{DOC, branch, command, commit, isolated, post, repository};

/// Each card of main's register by slug, with its state: as `symphony status` reads main's file,
/// and as main's page draws it (the file's copy, or the one card the scan makes of it with the
/// worktrees' lines); they must agree. The slugs waiting on the owner, as the queue lists them.
fn status_and_page(root: &Path) -> (Vec<(String, String)>, Vec<String>) {
    let status: Vec<(String, String)> = symphony::board::scan(root)
        .unwrap()
        .iter()
        .filter(|doc| doc.path == DOC)
        .flat_map(|doc| &doc.decisions)
        .map(|card| (card.slug.clone(), card.state.as_str().to_string()))
        .collect();
    let seen = shown(root, &BTreeSet::new());
    let cards = seen.snapshot.pages.get(DOC).cloned().unwrap_or_default();
    let page = docview::document_showing(&Render::new(root), DOC, "", &[], &cards).unwrap();
    let drawn: Vec<(String, String)> = page
        .cards
        .iter()
        .map(|card| (card.slug.clone(), card.state.clone()))
        .collect();
    let sorted = |mut cards: Vec<(String, String)>| {
        cards.sort();
        cards
    };
    assert_eq!(
        sorted(status.clone()),
        sorted(drawn),
        "symphony status and main's page"
    );
    let pending: Vec<String> = sorted(status)
        .into_iter()
        .filter(|(_, state)| state == "pending")
        .map(|(slug, _)| slug)
        .collect();
    assert_eq!(
        pending, seen.waiting,
        "status's pending cards and the queue"
    );
    (main_states(root), seen.waiting)
}

/// Each card of main's committed register, by slug, with its state.
fn main_states(root: &Path) -> Vec<(String, String)> {
    let text = command(root, &["show", &format!("main:{DOC}")], 9);
    let mut cards: Vec<(String, String)> =
        symphony::register::parse_decisions(&text.lines().collect::<Vec<_>>())
            .into_iter()
            .map(|card| (card.slug, card.state.as_str().to_string()))
            .collect();
    cards.sort();
    cards
}

fn states(cards: &[(&str, &str)]) -> Vec<(String, String)> {
    cards
        .iter()
        .map(|(slug, state)| (slug.to_string(), state.to_string()))
        .collect()
}

/// A repository on main whose own identity the verdict committer commits main's saves as.
fn committing() -> super::common::Tree {
    let tree = repository();
    for (key, value) in [
        ("user.name", "Test fixture"),
        ("user.email", "fixture@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        command(&tree.root, &["config", key, value], 1);
    }
    tree
}

/// `git status --porcelain` in `at` (trimmed, so a file changed but not staged reads `M PATH`).
fn changes(at: &Path) -> String {
    command(at, &["status", "--porcelain"], 9)
}

pub(super) fn a_verdict_on_a_landed_branchs_page_is_mains() {
    isolated(
        "a_verdict_on_a_landed_branchs_page_is_saved_on_main_and_status_agrees_with_the_page",
        |_| {
            let tree = committing();
            tree.write(DOC, &register(&card("old-card", ' ', "")));
            commit(&tree.root, "base", 1);
            let landed = branch(&tree, "feat/landed", "landed");
            write(
                &landed,
                &(card("card-a", ' ', "") + &card("old-card", ' ', "")),
            );
            commit(&landed, "post card-a", 2);
            // The land: one merge commit of the branch on main.
            command(
                &tree.root,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    "Merge branch 'feat/landed'",
                    "feat/landed",
                ],
                3,
            );
            let (_, waiting) = status_and_page(&tree.root);
            assert_eq!(waiting, ["card-a", "old-card"]);

            // The owner approves card-a on the branch's page, open since before the land.
            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "feat~landed",
                "/api/decision",
                json!({"path":DOC, "slug":"card-a", "state":"approved", "note":"Go"}),
            );
            saves.close();
            assert_eq!(changes(&landed), "", "nothing is left in the worktree");
            assert!(tree.read(DOC).contains("owner · approved — Go"));
            let (committed, waiting) = status_and_page(&tree.root);
            assert_eq!(
                committed,
                states(&[("card-a", "approved"), ("old-card", "pending")]),
                "main's register holds the verdict, committed"
            );
            assert_eq!(waiting, ["old-card"]);
        },
    );
}

pub(super) fn a_line_on_mains_card_from_a_live_branchs_page_is_mains() {
    isolated(
        "a_line_on_mains_card_from_a_live_branchs_page_is_saved_on_main_and_its_own_card_stays_its",
        |_| {
            let tree = committing();
            tree.write(DOC, &register(&card("old-card", ' ', "")));
            commit(&tree.root, "base", 1);
            // The branch posts a card of its own; its copy of old-card is the one it forked with.
            let live = branch(&tree, "feat/live", "live");
            write(
                &live,
                &(card("card-b", ' ', "") + &card("old-card", ' ', "")),
            );
            commit(&live, "post card-b", 2);
            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "feat~live",
                "/api/message",
                json!({"path":DOC, "slug":"old-card", "text":"Still wanted?"}),
            );
            post(
                &saves,
                "feat~live",
                "/api/decision",
                json!({"path":DOC, "slug":"old-card", "state":"approved", "note":""}),
            );
            // Its own card's verdict stays its, for its land to carry.
            post(
                &saves,
                "feat~live",
                "/api/decision",
                json!({"path":DOC, "slug":"card-b", "state":"approved", "note":"B"}),
            );
            saves.close();
            let main = tree.read(DOC);
            assert!(main.contains("owner — Still wanted?"), "{main}");
            assert!(main.contains("- [x] **old-card**"), "{main}");
            assert!(!main.contains("card-b"), "{main}");
            assert_eq!(main_states(&tree.root), states(&[("old-card", "approved")]));
            assert_eq!(changes(&live), "M docs/DECISIONS.md");
            let saved = std::fs::read_to_string(live.join(DOC)).unwrap();
            assert!(saved.contains("owner · approved — B"), "{saved}");
            assert!(!saved.contains("Still wanted?"), "{saved}");
            assert!(saved.contains("- [ ] **old-card**"), "{saved}");
            status_and_page(&tree.root);
        },
    );
}

/// The variable naming the file whose presence makes the test's `git` refuse to list worktrees.
const NO_LISTING: &str = "SYMPHONY_TEST_NO_LISTING";

/// A `git` that runs the real one, but fails `git worktree list` while the file [`NO_LISTING`]
/// names exists: a listing that could not be made, as a spawn failure under load or a git error.
const FAILING_LISTING: &str = r#"#!/bin/sh
case " $* " in
    *" worktree list "*) [ -f "$SYMPHONY_TEST_NO_LISTING" ] && exit 128 ;;
esac
exec /usr/bin/git "$@"
"#;

pub(super) fn a_verdict_when_the_worktrees_cannot_be_listed_stays_the_pages() {
    let name =
        "a_verdict_when_the_worktrees_cannot_be_listed_stays_the_pages_and_main_takes_nothing";
    let bin = super::common::Tree::new();
    super::executable(&bin, "bin/git", FAILING_LISTING);
    let path = std::env::join_paths(
        std::iter::once(bin.path("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let flag = bin.path("no-listing").into_os_string();
    super::isolated_with(name, &[("PATH", path), (NO_LISTING, flag)], |_| {
        let tree = committing();
        tree.write(DOC, &register(&card("old-card", ' ', "")));
        commit(&tree.root, "base", 1);
        // The branch posts its own revision of main's card: a decision of its own.
        let live = branch(&tree, "feat/live", "live");
        let revised = card("old-card", ' ', "").replace("Proposal old-card.", "Revised old-card.");
        write(&live, &revised);
        commit(&live, "revise old-card", 2);
        std::fs::write(std::env::var_os(NO_LISTING).unwrap(), "").unwrap();
        let saves = symphony::save::Saves::start(&tree.root);
        let failure = saves
            .post(
                "feat~live",
                "/api/decision",
                &json!({"path":DOC, "slug":"old-card", "state":"approved", "note":""}),
            )
            .unwrap_err();
        saves.close();
        // The owner sees the failure; main's card, the other decision, is never answered.
        assert_eq!(
            (failure.status, failure.error.as_str()),
            (404, "no such worktree")
        );
        assert_eq!(main_states(&tree.root), states(&[("old-card", "pending")]));
        assert!(!tree.read(DOC).contains("owner"), "{}", tree.read(DOC));
        assert_eq!(changes(&live), "");
    });
}

pub(super) fn a_verdict_from_a_gone_worktrees_page_is_mains() {
    isolated(
        "a_verdict_from_a_gone_worktrees_page_is_saved_on_main_when_main_holds_the_card",
        |_| {
            let tree = committing();
            tree.write(DOC, &register(&card("old-card", ' ', "")));
            commit(&tree.root, "base", 1);
            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "feat~gone",
                "/api/decision",
                json!({"path":DOC, "slug":"old-card", "state":"rejected", "note":"No."}),
            );
            // A card main does not hold has nowhere to go.
            let failure = saves
                .post(
                    "feat~gone",
                    "/api/decision",
                    &json!({"path":DOC, "slug":"elsewhere", "state":"approved", "note":""}),
                )
                .unwrap_err();
            saves.close();
            assert_eq!(
                (failure.status, failure.error.as_str()),
                (404, "no such worktree")
            );
            assert_eq!(main_states(&tree.root), states(&[("old-card", "rejected")]));
            status_and_page(&tree.root);
        },
    );
}
