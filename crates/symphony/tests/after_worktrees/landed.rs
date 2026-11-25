//! The owner's line on a card main holds is main's whenever the branch's page drew main's card,
//! whichever branch's page it was given on: a landed branch's page left open, a live branch's page
//! drawing main's card (one it did not post, or one it posted that main closed since), a page whose
//! worktree is gone, read at its branch while it is kept. It is saved in main's register and
//! committed there, never left in a worktree a land does not carry, so `just symphony-status` reads
//! on main what the page shows. A card the branch posted keeps its line with the branch.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;
use symphony::{docview, render::Render};

use super::posted::{RESOLVED, card, register, shown, write};
use super::saving::branch_page;
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
        symphony_board::order::sort_by(&mut cards, &Ord::cmp);
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
    symphony_board::order::sort_by(&mut cards, &Ord::cmp);
    cards
}

fn states(cards: &[(&str, &str)]) -> Vec<(String, String)> {
    cards
        .iter()
        .map(|(slug, state)| (slug.to_string(), state.to_string()))
        .collect()
}

/// A repository on main whose own identity the verdict committer commits main's saves as.
pub(super) fn committing() -> super::common::Tree {
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

            // The branch's page draws main's answered card in the group its answer puts it in, as
            // main's page does, though the worktree's file keeps it under the owner's group; its
            // place is still the file's lines, where a save from the page lands.
            let page = branch_page(&tree.root, &live, "feat~live");
            assert_eq!(
                under(&page, "old-card"),
                Some(symphony::Group::Agent.title())
            );
            assert_eq!(under(&page, "card-b"), Some(symphony::Group::Agent.title()));
            let old = page
                .sections
                .iter()
                .find(|section| section.key == "card:old-card")
                .unwrap();
            let line = saved
                .lines()
                .position(|line| line.starts_with("- [ ] **old-card**"))
                .unwrap()
                + 1;
            assert_eq!(old.start, line, "the worktree file's line of old-card");
            let card = page.cards.iter().find(|c| c.slug == "old-card").unwrap();
            assert_eq!(card.state, "approved");
        },
    );
}

/// The register group heading the page `page` draws above the card `slug`.
fn under(page: &symphony_api::Document, slug: &str) -> Option<&'static str> {
    let at = page
        .sections
        .iter()
        .position(|section| section.key == format!("card:{slug}"))?;
    page.sections[..at].iter().rev().find_map(|section| {
        symphony::Group::ALL
            .iter()
            .map(|group| group.title())
            .find(|title| section.html.contains(&format!(">{title} (")))
    })
}

/// The variable naming the file that, while it exists, names the words of the git command the
/// test's `git` fails.
const FAILING: &str = "SYMPHONY_TEST_FAILING";

/// The variable naming the real `git` the test's runs.
const GIT: &str = "SYMPHONY_TEST_GIT";

/// A `git` that runs the real one ([`GIT`]), but fails a command holding the words the file
/// [`FAILING`] names while it exists: as a spawn failure under load or a git error would.
const FAILING_GIT: &str = r#"#!/bin/sh
if [ -f "$SYMPHONY_TEST_FAILING" ]; then
    case " $* " in
        *" $(cat "$SYMPHONY_TEST_FAILING") "*) exit 128 ;;
    esac
fi
exec "$SYMPHONY_TEST_GIT" "$@"
"#;

/// The variables that put [`FAILING_GIT`], written in `bin`, before the suite's `PATH`: that `PATH`,
/// the real `git` as the suite's `PATH` finds it before the stand-in is put on it, and the file
/// naming the command to fail.
fn failing_git(bin: &super::common::Tree) -> [(&'static str, std::ffi::OsString); 3] {
    let path = std::env::var_os("PATH").unwrap();
    let git = std::env::split_paths(&path)
        .map(|dir| dir.join("git"))
        .find(|git| git.is_file())
        .expect("a git on PATH");
    super::executable(bin, "bin/git", FAILING_GIT);
    let path =
        std::env::join_paths(std::iter::once(bin.path("bin")).chain(std::env::split_paths(&path)))
            .unwrap();
    [
        ("PATH", path),
        (GIT, git.into_os_string()),
        (FAILING, bin.path("failing").into_os_string()),
    ]
}

/// Makes the test's `git` fail the command holding `words`, or, given none, fail none.
fn fail(words: Option<&str>) {
    let flag = std::env::var_os(FAILING).unwrap();
    match words {
        Some(words) => std::fs::write(flag, words).unwrap(),
        None => match std::fs::remove_file(flag) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => panic!("{error}"),
            _ => {}
        },
    }
}

pub(super) fn a_verdict_when_the_worktrees_cannot_be_listed_stays_the_pages() {
    let name =
        "a_verdict_when_the_worktrees_cannot_be_listed_stays_the_pages_and_main_takes_nothing";
    let bin = super::common::Tree::new();
    super::isolated_with(name, &failing_git(&bin), |_| {
        let tree = committing();
        tree.write(DOC, &register(&card("old-card", ' ', "")));
        commit(&tree.root, "base", 1);
        // The branch posts its own revision of main's card: a decision of its own.
        let live = branch(&tree, "feat/live", "live");
        let revised = card("old-card", ' ', "").replace("Proposal old-card.", "Revised old-card.");
        write(&live, &revised);
        commit(&live, "revise old-card", 2);
        fail(Some("worktree list"));
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
            (503, "the worktrees could not be listed")
        );
        fail(None);
        assert_eq!(main_states(&tree.root), states(&[("old-card", "pending")]));
        assert!(!tree.read(DOC).contains("owner"), "{}", tree.read(DOC));
        assert_eq!(changes(&live), "");
    });
}

/// A line from the branch's page, posted on `slug`.
fn message(slug: &str, text: &str) -> serde_json::Value {
    json!({"path":DOC, "slug":slug, "text":text})
}

/// The owner's rejection of the branch's copy of `settled`, its latest line.
const TOO_DARK: &str = "  [2026-11-11 12:00] owner · rejected — Too dark.\n";

/// What main's register at its tip holds.
fn mains_committed(root: &Path) -> String {
    command(root, &["show", &format!("main:{DOC}")], 9)
}

/// Main's resolution of a card, before the branch's 12:00 lines.
const EARLY: &str =
    "  [2026-11-11 10:00] owner · approved\n  [2026-11-11 10:30] agent · resolved\n";

/// The cards the branch page `page` of the worktree at `tree` draws in place of its file's copies
/// ([`symphony::branches::page_cards`]), with their state and whose they are: the branch's one card
/// with main's copy, or main's card.
fn drawn(root: &Path, tree: &Path, page: &str) -> Vec<(String, String, &'static str)> {
    let none = BTreeSet::new();
    let marks = symphony::branches::scan_pages(root, &none).pages[page].clone();
    let seen = shown(root, &none);
    let ones = seen.snapshot.pages.get(page).map_or(&[][..], Vec::as_slice);
    let mains = seen.snapshot.pages.get(DOC).map_or(&[][..], Vec::as_slice);
    symphony::branches::page_cards(root, tree, DOC, ones, mains, &marks)
        .into_iter()
        .map(|card| {
            let whose = if ones.iter().any(|one| one.slug == card.slug) {
                "the branch's"
            } else {
                "main's"
            };
            (card.slug, card.state.as_str().to_string(), whose)
        })
        .collect()
}

pub(super) fn a_line_on_a_card_main_closed_since_the_branch_posted_it_is_mains() {
    isolated(
        "a_line_on_a_card_the_branch_posted_and_main_closed_since_is_saved_on_main_as_its_page_draws_it",
        |_| {
            let tree = committing();
            let base = card("settled", ' ', "") + &card("early", ' ', "");
            tree.write(DOC, &register(&base));
            commit(&tree.root, "base", 1);
            // The branch posts its copies of both cards.
            let stale = branch(&tree, "feat/stale", "stale");
            let revised = "  [2026-11-11 12:00] agent — Revised.\n";
            write(
                &stale,
                &(card("settled", '-', TOO_DARK) + &card("early", ' ', revised)),
            );
            commit(&stale, "post both", 2);
            // Main resolves `settled` after every line of the branch's copy, `early` before them.
            tree.write(
                DOC,
                &register(
                    &(card("settled", 'r', &format!("{TOO_DARK}{RESOLVED}"))
                        + &card("early", 'r', EARLY)),
                ),
            );
            commit(&tree.root, "main resolves both", 3);
            // The page draws main's resolved `settled`, and its own `early` (one card with main's).
            let page = format!("@feat~stale/{DOC}");
            let resolved = |slug: &str, whose| (slug.to_string(), "resolved".to_string(), whose);
            assert_eq!(
                drawn(&tree.root, &stale, &page),
                [
                    resolved("settled", "main's"),
                    resolved("early", "the branch's")
                ]
            );

            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "feat~stale",
                "/api/message",
                message("settled", "Reopen it?"),
            );
            post(
                &saves,
                "feat~stale",
                "/api/message",
                message("early", "And this?"),
            );
            saves.close();
            let main = mains_committed(&tree.root);
            assert!(main.contains("owner — Reopen it?"), "{main}");
            assert!(!main.contains("And this?"), "{main}");
            let saved = std::fs::read_to_string(stale.join(DOC)).unwrap();
            assert!(saved.contains("owner — And this?"), "{saved}");
            assert!(!saved.contains("Reopen it?"), "{saved}");
        },
    );
}

pub(super) fn save_routing_and_page_use_the_registers_closure() {
    isolated(
        "save_routing_and_page_agree_on_which_cards_main_closed",
        |_| {
            let tree = committing();
            let cases = [
                (
                    "rejected-open",
                    '-',
                    "  [2026-11-11 12:00] owner · rejected — Too dark.\n",
                    false,
                ),
                (
                    "approved-open",
                    'x',
                    "  [2026-11-11 12:00] owner · approved — One question.\n",
                    false,
                ),
                (
                    "rejected-answered",
                    '-',
                    "  [2026-11-11 12:00] owner · rejected — Too dark.\n  [2026-11-11 13:00] agent — Replaced.\n",
                    true,
                ),
                (
                    "approved-closed",
                    'x',
                    "  [2026-11-11 12:00] owner · approved\n",
                    true,
                ),
                (
                    "resolved",
                    'r',
                    "  [2026-11-11 12:00] agent · resolved\n",
                    true,
                ),
            ];
            let base: String = cases
                .iter()
                .map(|(slug, _, _, _)| card(slug, ' ', ""))
                .collect();
            tree.write(DOC, &register(&base));
            commit(&tree.root, "base", 1);
            let live = branch(&tree, "feat/closure", "closure");
            write(&live, &base.replace("Proposal", "Revision"));
            commit(&live, "revise", 2);
            let main: String = cases
                .iter()
                .map(|(slug, mark, lines, _)| card(slug, *mark, lines))
                .collect();
            tree.write(DOC, &register(&main));
            commit(&tree.root, "main replies", 3);
            let scan = symphony::branches::scan_pages(&tree.root, &BTreeSet::new());
            let overrides = symphony::branches::page_cards(
                &tree.root,
                &live,
                DOC,
                &[],
                &[],
                &scan.pages[&format!("@feat~closure/{DOC}")],
            );
            for (slug, _, _, closed) in cases {
                assert_eq!(
                    symphony::branches::mains_card(
                        &tree.root,
                        symphony::branches::Page::Tree(&live),
                        DOC,
                        slug
                    ),
                    closed,
                    "{slug}: save routing"
                );
                assert_eq!(
                    overrides
                        .iter()
                        .any(|card| card.slug == slug && card.text.starts_with("Proposal")),
                    closed,
                    "{slug}: page draws main's closure"
                );
            }
        },
    );
}

pub(super) fn a_finished_revision_stays_one_card_until_its_answer_lands(revise_options: bool) {
    isolated(
        if revise_options {
            "a_finished_option_revision_and_its_answer_stay_one_card_until_landed"
        } else {
            "a_finished_revision_and_its_answer_stay_one_card_until_landed"
        },
        |_| {
            let tree = committing();
            let base = card(
                "revised",
                ' ',
                if revise_options {
                    "  [2026-11-11 09:01] agent — A: original option.\n"
                } else {
                    ""
                },
            );
            tree.write(DOC, &register(&base));
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "feat/done", "done");
            let revision = if revise_options {
                base.replace("A: original option.", "A: revised option.")
            } else {
                base.replace("Proposal revised.", "A revised proposal.")
            };
            let heading = if revise_options {
                "Proposal revised."
            } else {
                "A revised proposal."
            };
            write(&wt, &revision);
            commit(&wt, "revise", 2);
            let finished = BTreeSet::from(["feat/done".to_string()]);
            let assert_pages = |finished: &BTreeSet<String>| {
                let seen = shown(&tree.root, finished);
                for (path, at, scope) in [
                    (DOC.to_string(), &tree.root, ""),
                    (format!("@feat~done/{DOC}"), &wt, "feat~done"),
                ] {
                    let cards = seen
                        .snapshot
                        .pages
                        .get(&path)
                        .map_or(&[][..], Vec::as_slice);
                    let page = docview::document_showing(&Render::new(at), DOC, scope, &[], cards)
                        .unwrap();
                    assert_eq!(
                        page.cards.iter().filter(|c| c.slug == "revised").count(),
                        1,
                        "{path}: one rendered card"
                    );
                    assert!(
                        page.sections.iter().any(|s| s.html.contains(heading)),
                        "{path}"
                    );
                    if revise_options {
                        assert!(
                            page.threads["revised"].html.contains("A: revised option."),
                            "{path}: revised option when finished={finished:?}"
                        );
                    }
                }
                seen
            };
            let live = assert_pages(&BTreeSet::new());
            assert_eq!(
                live.snapshot.queue.waiting[0].doc,
                format!("@feat~done/{DOC}")
            );
            let before = assert_pages(&finished);
            assert_eq!(before.waiting, ["revised"]);
            assert_eq!(
                before.snapshot.queue.waiting[0].doc,
                format!("@feat~done/{DOC}"),
                "main's page saves on the revision's branch"
            );
            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "feat~done",
                "/api/decision",
                json!({"path":DOC, "slug":"revised", "state":"approved", "note":"Keep the revision"}),
            );
            saves.close();
            assert!(!tree.read(DOC).contains("owner"));
            assert_eq!(
                symphony::landing::owner_lines_left(&wt),
                [DOC],
                "the landing reader keeps the answer until committed"
            );
            let answered = assert_pages(&finished);
            assert_eq!(answered.agent, ["revised"]);
            for path in [DOC.to_string(), format!("@feat~done/{DOC}")] {
                assert!(
                    answered.snapshot.pages[&path][0]
                        .raw
                        .iter()
                        .any(|line| line.contains("Keep the revision")),
                    "{path}: the owner's answer stays with the revision"
                );
            }
            commit(&wt, "carry owner answer", 3);
            assert!(symphony::landing::owner_lines_left(&wt).is_empty());
            command(
                &tree.root,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "merge",
                    "--no-ff",
                    "-m",
                    "land revision",
                    "feat/done",
                ],
                4,
            );
            let (committed, _) = status_and_page(&tree.root);
            assert_eq!(committed, states(&[("revised", "approved")]));
            assert!(tree.read(DOC).contains(heading));
            if revise_options {
                assert!(tree.read(DOC).contains("A: revised option."));
            }
            assert!(tree.read(DOC).contains("Keep the revision"));
        },
    );
}

pub(super) fn a_verdict_from_a_gone_worktrees_page_reads_its_kept_branch() {
    let name = "a_verdict_from_a_gone_worktrees_page_is_judged_on_its_branch_while_it_is_kept";
    let bin = super::common::Tree::new();
    super::isolated_with(name, &failing_git(&bin), |_| {
        let tree = committing();
        let base = card("old-card", ' ', "") + &card("forked", ' ', "") + &card("settled", ' ', "");
        tree.write(DOC, &register(&base));
        commit(&tree.root, "base", 1);
        // The branch revises old-card, keeps forked as it forked it and posts its copy of settled.
        let kept = branch(&tree, "feat/kept", "kept");
        let revised = card("old-card", ' ', "").replace("Proposal old-card.", "Revised old-card.");
        write(
            &kept,
            &(revised + &card("forked", ' ', "") + &card("settled", '-', TOO_DARK)),
        );
        commit(&kept, "revise old-card", 2);
        let resolved = card("settled", 'r', &format!("{TOO_DARK}{RESOLVED}"));
        let main = card("old-card", ' ', "") + &card("forked", ' ', "") + &resolved;
        tree.write(DOC, &register(&main));
        commit(&tree.root, "main resolves settled", 3);
        command(&tree.root, &["worktree", "remove", "kept"], 4);
        assert!(!kept.exists());

        let saves = symphony::save::Saves::start(&tree.root);
        let refused = |slug: &str| {
            let failure = saves
                .post("feat~kept", "/api/message", &message(slug, "Lost?"))
                .unwrap_err();
            (failure.status, failure.error)
        };
        let nowhere = (404, "no such worktree".to_string());
        // Its own revision is the branch's, as its page drew it: with no worktree, nowhere.
        assert_eq!(refused("old-card"), nowhere);
        // A branch git cannot read says nothing of the card: the page's, nowhere either.
        fail(Some("for-each-ref --format=%(refname)%00%(objectname)"));
        assert_eq!(refused("forked"), nowhere);
        fail(None);
        post(
            &saves,
            "feat~kept",
            "/api/message",
            message("forked", "Still wanted?"),
        );
        post(
            &saves,
            "feat~kept",
            "/api/message",
            message("settled", "Reopen it?"),
        );
        saves.close();
        let main = mains_committed(&tree.root);
        assert!(main.contains("owner — Still wanted?"), "{main}");
        assert!(main.contains("owner — Reopen it?"), "{main}");
        assert!(!tree.read(DOC).contains("Lost?"), "{}", tree.read(DOC));
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
