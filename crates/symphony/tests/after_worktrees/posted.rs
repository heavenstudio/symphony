//! A card reaches the owner once, when its branch posts it (commits it): never from a worktree's
//! unsaved draft, never again from a finished branch's old copy, and main's copy and a branch's
//! read as one card wherever the owner reads it.

use std::collections::BTreeSet;
use std::path::Path;

use super::{DOC, branch, command, commit, isolated, repository};
use symphony::{branches, docview, render::Render};

/// A register as a page draws it: its cards one block, a blank line under the region's start.
pub(super) fn register(body: &str) -> String {
    format!("# Decisions\n\n<!-- register:start -->\n\n{body}\n<!-- register:end -->\n")
}

/// A card posted at 09:00 with `lines` under it.
pub(super) fn card(slug: &str, mark: char, lines: &str) -> String {
    format!(
        "- [{mark}] **{slug}** — Proposal {slug}.\n  [2026-11-11 09:00] agent — Posted.\n{lines}"
    )
}

pub(super) fn write(at: &Path, body: &str) {
    std::fs::write(at.join(DOC), register(body)).unwrap();
}

pub(super) const QUESTION: &str = "  [2026-11-11 17:42] owner · question — Why B?\n";
pub(super) const ANSWER: &str = "  [2026-11-11 17:59] agent — Because B is faster.\n";
pub(super) const RESOLVED: &str =
    "  [2026-11-11 18:00] owner · approved\n  [2026-11-11 19:00] agent · resolved\n";

/// What the owner's queue and counts show: the waiting cards' slugs, the cards waiting on an agent,
/// and the counts.
pub(super) struct Shown {
    pub(super) waiting: Vec<String>,
    pub(super) agent: Vec<String>,
    pub(super) counts: (u32, u32, u32),
    pub(super) snapshot: symphony::dashboard::Snapshot,
}

pub(super) fn shown(root: &Path, finished: &BTreeSet<String>) -> Shown {
    let cards = branches::scan(root, finished);
    let snapshot = symphony::dashboard::snapshot(root, &cards, "test").unwrap();
    let mut waiting: Vec<String> = snapshot
        .queue
        .waiting
        .iter()
        .map(|c| c.slug.clone())
        .collect();
    waiting.sort();
    let mut agent: Vec<String> = snapshot
        .queue
        .agent
        .iter()
        .map(|c| c.slug.clone())
        .collect();
    agent.sort();
    let counts = (
        snapshot.board.counts.waiting,
        snapshot.board.counts.docs_waiting,
        snapshot.board.progress.decisions_waiting,
    );
    Shown {
        waiting,
        agent,
        counts,
        snapshot,
    }
}

pub(super) fn cards_reach_the_owner_once_posted() {
    isolated(
        "a_card_reaches_the_owner_once_posted_and_never_from_a_draft_or_a_finished_branch",
        |_| {
            let tree = repository();
            let main_cards =
                card("main-card", ' ', "") + &card("shared", ' ', "") + &card("settled", ' ', "");
            tree.write(DOC, &register(&main_cards));
            commit(&tree.root, "base", 1);

            // A draft: a new card the executor is still writing, its picture not on disk yet.
            let draft = branch(&tree, "tools/draft", "draft");
            write(
                &draft,
                &(main_cards.clone()
                    + &card(
                        "draft-card",
                        ' ',
                        "  [2026-11-11 10:00] agent — A: ![A](/artifacts/a.png)\n",
                    )),
            );

            // A posted card, an answer posted on main's card, and drafts on top of both.
            let posted = branch(&tree, "tools/posted", "posted");
            let posted_body = card("main-card", ' ', "")
                + &card("shared", ' ', ANSWER)
                + &card("settled", ' ', "")
                + &card("posted-card", ' ', "");
            write(&posted, &posted_body);
            commit(&posted, "post a card and an answer", 2);
            write(
                &posted,
                &(posted_body.replace(
                    "Proposal posted-card.\n  [2026-11-11 09:00] agent — Posted.\n",
                    "Proposal posted-card.\n  [2026-11-11 09:00] agent — Posted.\n  [2026-11-11 10:00] agent — A draft line ![B](/artifacts/b.png).\n",
                ) + &card("posted-draft", ' ', "")),
            );

            // Posted before its picture is on disk.
            let pictures = branch(&tree, "tools/pictures", "pictures");
            write(
                &pictures,
                &(main_cards.clone()
                    + &card(
                        "picture-card",
                        ' ',
                        "  [2026-11-11 10:00] agent — A: ![A](/artifacts/late.png)\n",
                    )),
            );
            commit(&pictures, "post a picture card", 2);

            // A branch whose task is done, never merged.
            let stale = branch(&tree, "tools/stale", "stale");
            write(
                &stale,
                &(card("main-card", ' ', "")
                    + &card("shared", ' ', "")
                    + &card(
                        "settled",
                        '-',
                        "  [2026-11-11 12:00] owner · rejected — Too dark.\n",
                    )
                    + &card("abandoned-card", ' ', "")),
            );
            commit(&stale, "an old copy", 2);

            // A merged branch: its card landed, then main resolved it.
            let merged = branch(&tree, "tools/merged", "merged");
            write(
                &merged,
                &(main_cards.clone() + &card("merged-card", ' ', "")),
            );
            commit(&merged, "post a card", 2);
            command(
                &tree.root,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    "land",
                    "tools/merged",
                ],
                3,
            );
            tree.write(
                DOC,
                &register(
                    &(card("main-card", ' ', "")
                        + &card("shared", '?', QUESTION)
                        + &card("settled", 'r', RESOLVED)
                        + &card("merged-card", 'r', RESOLVED)),
                ),
            );
            commit(
                &tree.root,
                "the owner asks and two cards resolve on main",
                3,
            );

            let finished = BTreeSet::from(["tools/stale".to_string()]);
            let scanned = branches::scan(&tree.root, &finished);
            assert_eq!(
                scanned
                    .iter()
                    .map(|c| (c.tree.branch.as_str(), c.card.slug.as_str(), c.on_main))
                    .collect::<Vec<_>>(),
                [
                    ("tools/posted", "shared", false),
                    ("tools/posted", "posted-card", false),
                    // The owner's rejection on the done branch's copy is carried to main's card,
                    // where main's later resolution leaves it nothing to add (below).
                    ("tools/stale", "settled", true),
                ],
                "no draft, no card before its picture, nothing else of a merged or a done branch"
            );
            let posted_card = &scanned[1].card;
            assert!(
                !posted_card.raw.iter().any(|l| l.contains("A draft line")),
                "the agent's unsaved line is a draft"
            );

            let first = shown(&tree.root, &finished);
            assert_eq!(first.waiting, ["main-card", "posted-card", "shared"]);
            assert_eq!(first.counts, (3, 1, 3));
            assert!(first.agent.is_empty());
            let tags: Vec<(&str, Option<&str>)> = first
                .snapshot
                .queue
                .waiting
                .iter()
                .map(|c| (c.slug.as_str(), c.task.as_deref()))
                .collect();
            assert!(tags.contains(&("posted-card", Some("tools/posted"))));
            assert!(
                !first
                    .snapshot
                    .activity
                    .iter()
                    .any(|a| a.slug.contains("draft") || a.text.contains("draft")),
                "a draft raises no banner"
            );
            // Main's question and the branch's answer are one answered card, on both pages.
            let one = first
                .snapshot
                .queue
                .waiting
                .iter()
                .find(|c| c.slug == "shared")
                .unwrap();
            assert_eq!(one.href(), "/@tools~posted/docs/DECISIONS.md#shared");
            for (page, root, scope) in [
                (DOC.to_string(), tree.root.clone(), ""),
                (
                    format!("@tools~posted/{DOC}"),
                    posted.clone(),
                    "tools~posted",
                ),
            ] {
                let cards = first.snapshot.pages.get(&page).cloned().unwrap_or_default();
                let doc = docview::document_showing(&Render::new(&root), DOC, scope, &[], &cards)
                    .unwrap();
                let thread = serde_json::to_string(&doc.threads["shared"]).unwrap();
                assert!(thread.contains("Why B?"), "{page}: the owner's question");
                assert!(
                    thread.contains("Because B is faster."),
                    "{page}: the answer"
                );
                let shared = doc.cards.iter().find(|c| c.slug == "shared").unwrap();
                assert_eq!(
                    (shared.state.as_str(), shared.group.as_str()),
                    ("question", "confirm"),
                    "{page}"
                );
                // The card is drawn as one too, answered (main's file alone has the question open,
                // waiting on an agent), at the file's lines.
                let drawn = doc
                    .sections
                    .iter()
                    .find(|s| s.html.contains("data-slug=\"shared\""))
                    .unwrap_or_else(|| {
                        let seen: Vec<_> = doc
                            .sections
                            .iter()
                            .map(|s| (&s.key, s.start, s.end, &s.html))
                            .collect();
                        panic!("{page}: {seen:#?}")
                    });
                assert!(
                    drawn.html.contains("data-open=\"false\"")
                        && drawn.html.contains("data-group=\"confirm\""),
                    "{page}: {}",
                    drawn.html
                );
                assert_eq!(
                    (drawn.start, drawn.key.as_str()),
                    (7, "card:shared"),
                    "{page}"
                );
            }

            // The done branch would have listed its abandoned card and its stale copy.
            let unfinished = branches::scan(&tree.root, &BTreeSet::new());
            assert!(unfinished.iter().any(|c| c.card.slug == "abandoned-card"));
            let without = shown(&tree.root, &BTreeSet::new());
            assert!(without.waiting.contains(&"abandoned-card".to_string()));
            assert!(
                !without.waiting.contains(&"settled".to_string()),
                "main's resolution after the stale copy still wins"
            );

            // The picture arrives: the card is posted to the owner.
            std::fs::create_dir_all(pictures.join("artifacts")).unwrap();
            std::fs::write(pictures.join("artifacts/late.png"), b"png").unwrap();
            let arrived = shown(&tree.root, &finished);
            assert_eq!(
                arrived.waiting,
                ["main-card", "picture-card", "posted-card", "shared"]
            );
            assert_eq!(arrived.counts.0, 4);

            // Committing the draft posts it.
            commit(&posted, "post the drafts", 4);
            let committed = shown(&tree.root, &finished);
            assert!(committed.waiting.contains(&"posted-draft".to_string()));
        },
    );
}

pub(super) fn owner_lines_reach_main_from_a_finished_worktree() {
    isolated(
        "the_owners_unsaved_line_in_a_finished_worktree_shows_on_mains_card_until_main_closes_it_later",
        |_| {
            let tree = repository();
            tree.write(DOC, &register(&card("landed", ' ', "")));
            commit(&tree.root, "base", 1);
            let merged = branch(&tree, "tools/landed", "landed");
            let built = "  [2026-11-11 10:00] agent — Built.\n";
            write(&merged, &card("landed", ' ', built));
            commit(&merged, "work", 2);
            command(
                &tree.root,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    "land",
                    "tools/landed",
                ],
                3,
            );
            // The owner asked on the branch's page after its last commit; it landed without it.
            let asked = "  [2026-11-11 14:34] owner · question — Does it run on a phone?\n";
            write(&merged, &card("landed", '?', &format!("{built}{asked}")));
            let none = BTreeSet::new();
            let open = shown(&tree.root, &none);
            assert_eq!(
                open.agent,
                ["landed"],
                "the owner's question waits on an agent"
            );
            assert!(open.waiting.is_empty());
            let item = &open.snapshot.queue.agent[0];
            assert_eq!(
                item.path, DOC,
                "read on main's page, not the finished worktree's"
            );
            let cards = open.snapshot.pages[DOC].clone();
            let doc =
                docview::document_showing(&Render::new(&tree.root), DOC, "", &[], &cards).unwrap();
            let thread = serde_json::to_string(&doc.threads["landed"]).unwrap();
            assert!(
                thread.contains("Does it run on a phone?"),
                "the owner's words are not lost"
            );
            assert_eq!(doc.cards[0].state, "question");

            // Main answers and closes the card after it: the old line adds nothing.
            tree.write(
                DOC,
                &register(&card(
                    "landed",
                    'r',
                    &format!(
                        "{built}  [2026-11-11 15:00] owner · question — Does it run on a phone?\n  [2026-11-11 15:10] agent — Yes.\n  [2026-11-11 18:00] owner · approved\n  [2026-11-11 19:00] agent · resolved\n"
                    ),
                )),
            );
            commit(&tree.root, "resolved on main", 3);
            let quiet = shown(&tree.root, &none);
            assert!(
                quiet.waiting.is_empty() && quiet.agent.is_empty(),
                "{:?}",
                quiet.agent
            );
            assert_eq!(quiet.counts, (0, 0, 0));
            assert!(!quiet.snapshot.pages.contains_key(DOC));

            // A line the owner wrote there after main closed it reopens the card on main.
            write(
                &merged,
                &card(
                    "landed",
                    '-',
                    &format!("{built}  [2026-11-11 20:00] owner · rejected — It broke.\n"),
                ),
            );
            let reopened = shown(&tree.root, &none);
            assert_eq!(reopened.agent, ["landed"]);
            assert_eq!(reopened.snapshot.queue.agent[0].path, DOC);
        },
    );
}

pub(super) fn finished_branches_follow_the_board() {
    use symphony_board::rows::SessionRow;
    use symphony_board::{Board, Health, Role, Session, Task};
    let task = |slug: &str, status: &str, branch: Option<&str>, executor: Option<&str>| Task {
        branch: branch.map(str::to_string),
        id: 0,
        slug: slug.into(),
        title: slug.into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: "test".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: executor
            .map(|handle| Role {
                handle: handle.into(),
                role: "executor".into(),
                since: 0.0,
                expires: None,
                health: Health::Live,
            })
            .into_iter()
            .collect(),
        instruction: false,
        fields: Default::default(),
    };
    let session = |handle: &str, branch: &str| Session {
        row: SessionRow {
            handle: handle.into(),
            branch: branch.into(),
            ..Default::default()
        },
        health: Health::Live,
    };
    let board = Board {
        tasks: vec![
            task("tools/done", "done", None, None),
            task("tools/live", "in_progress", None, None),
            // A subtask done on its parent's branch finishes nothing while the parent is live.
            task("tools/live/work", "done", Some("tools/live"), None),
            task("tools/cancelled", "cancelled", None, None),
            // A branch named for a done task that a live task records as its own is live.
            task("tools/reused", "done", None, None),
            task(
                "tools/reused-fix",
                "in_progress",
                Some("tools/reused"),
                None,
            ),
            // A branch named for no task: the tasks recording it decide, all of them.
            task("a/one", "done", Some("worktree-one"), None),
            task("a/two", "done", None, Some("silver")),
            // One of them still open keeps the branch live.
            task("b/one", "done", Some("worktree-two"), None),
            task("b/two", "review", Some("worktree-two"), None),
        ],
        sessions: vec![session("silver", "worktree-one")],
        latest_message: 0,
    };
    // Every done task's own branch is finished too, `b/one` among them; `tools/live`,
    // `tools/reused` and `worktree-two` are not.
    assert_eq!(
        branches::finished(&board),
        BTreeSet::from(
            [
                "a/one",
                "a/two",
                "b/one",
                "tools/cancelled",
                "tools/done",
                "tools/live/work",
                "worktree-one"
            ]
            .map(String::from)
        )
    );
    // A branch's cards are tagged with the live task it names, the first it names when none is.
    let names = branches::names(&board);
    for (branch, task) in [
        ("worktree-two", "b/two"),
        ("tools/reused", "tools/reused-fix"),
        ("tools/live", "tools/live"),
        ("worktree-one", "a/one"),
        ("tools/done", "tools/done"),
    ] {
        assert_eq!(names[branch], task, "{branch}");
    }
}

/// A reference document whose cards sit inline, each under the section it decides, with no
/// register region in the file.
fn lamps(count: &str, finishes: &str) -> String {
    format!(
        "# Lamps\n\n## How many\n\nThere are 141 lamps.\n\n{count}\nThree sizes.\n\n## Finishes\n\nBrass and steel.\n\n{finishes}"
    )
}

pub(super) fn inline_cards_are_the_branchs_only_when_it_posted_them() {
    isolated(
        "an_inline_card_a_branch_posted_is_its_own_and_mains_inline_cards_stay_mains",
        |_| {
            const LAMPS: &str = "docs/LAMPS.md";
            let tree = repository();
            let main_card = card("main-inline", ' ', "");
            tree.write(LAMPS, &lamps(&main_card, ""));
            commit(&tree.root, "base", 1);

            // The branch rewrites a paragraph of the page and posts a card under another section.
            let posted = branch(&tree, "docs/inline", "inline");
            let branch_card = card("branch-inline", ' ', "");
            std::fs::write(
                posted.join(LAMPS),
                lamps(&main_card, &branch_card)
                    .replace("Brass and steel.", "Brass, steel and tin."),
            )
            .unwrap();
            commit(&posted, "post an inline card", 2);

            let shown = shown(&tree.root, &BTreeSet::new());
            assert_eq!(shown.waiting, ["branch-inline", "main-inline"]);
            assert_eq!(shown.counts.0, 2, "each card counts once");
            let queued = |slug: &str| {
                shown
                    .snapshot
                    .queue
                    .waiting
                    .iter()
                    .find(|card| card.slug == slug)
                    .unwrap()
            };
            // Main's card is main's: no task proposes it, and its place is main's page.
            assert_eq!(queued("main-inline").task, None);
            assert_eq!(queued("main-inline").href(), "/docs/LAMPS.md#main-inline");
            // The branch's card is read from its page, at the place it sits.
            assert_eq!(
                queued("branch-inline").href(),
                "/@docs~inline/docs/LAMPS.md#branch-inline"
            );
            let cards = branches::scan(&tree.root, &BTreeSet::new());
            assert_eq!(
                cards
                    .iter()
                    .map(|c| c.card.slug.as_str())
                    .collect::<Vec<_>>(),
                ["branch-inline"],
                "the branch posted one card, not every card of the page it changed"
            );
        },
    );
}

/// The whole of an inline card's life on main, as a card in a register has it: read on its page at
/// the place it sits, queued with a link to that place, answered from the page and resolved.
pub(super) fn an_inline_card_lives_as_a_registers_does() {
    isolated(
        "an_inline_card_is_read_queued_answered_from_its_page_and_resolved_where_it_sits",
        |_| {
            const LAMPS: &str = "docs/guide/LAMPS.md";
            let tree = repository();
            let text = lamps(&card("lamp-count", ' ', ""), "");
            tree.write(LAMPS, &text);
            commit(&tree.root, "base", 1);

            // Read: the page draws the card as a section of its own between the two paragraphs
            // around it, where the file holds it.
            let render = Render::new(&tree.root);
            let page = docview::document(&render, LAMPS, "", &[]).unwrap();
            let at = page
                .sections
                .iter()
                .position(|section| section.key == "card:lamp-count")
                .expect("the card is a section of its page");
            assert_eq!((page.sections[at].start, page.sections[at].end), (7, 8));
            assert!(page.sections[at - 1].html.contains("There are 141 lamps."));
            assert!(page.sections[at + 1].html.contains("Three sizes."));
            assert!(
                page.sections[at].html.contains("id=\"lamp-count\""),
                "the queue's link lands on the card: {}",
                page.sections[at].html
            );
            assert_eq!(page.waiting, ["lamp-count"]);
            assert_eq!(page.cards[0].group, "decide");

            // Queued, its link to its place and its category its document's folder.
            let shown_before = shown(&tree.root, &BTreeSet::new());
            assert_eq!(shown_before.waiting, ["lamp-count"]);
            let queued = &shown_before.snapshot.queue.waiting[0];
            assert_eq!(queued.href(), "/docs/guide/LAMPS.md#lamp-count");
            assert_eq!(symphony_api::queue_category(&queued.doc), "guide");

            // Answered from the page: the verdict is written under the card, nothing else moves.
            let saves = symphony::save::Saves::start(&tree.root);
            super::post(
                &saves,
                "",
                "/api/decision",
                serde_json::json!({"path":LAMPS, "slug":"lamp-count", "state":"approved", "note":""}),
            );
            saves.close();
            let saved = tree.read(LAMPS);
            let line = saved
                .lines()
                .find(|line| line.contains("owner · approved"))
                .expect("the owner's line")
                .to_string();
            assert_eq!(
                saved,
                text.replace("- [ ] **lamp-count**", "- [x] **lamp-count**")
                    .replace("agent — Posted.\n", &format!("agent — Posted.\n{line}\n"))
            );
            assert!(shown(&tree.root, &BTreeSet::new()).waiting.is_empty());

            // Resolved by the tidy, where it sits.
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code =
                symphony::board::tidy(&tree.root, Some("2026-11-22 08:00"), &mut out, &mut err);
            assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
            assert_eq!(
                tree.read(LAMPS),
                saved
                    .replace("- [x] **lamp-count**", "- [r] **lamp-count**")
                    .replace(
                        &line,
                        &format!("{line}\n  [2026-11-22 08:00] agent · resolved")
                    )
            );
            let page = docview::document(&Render::new(&tree.root), LAMPS, "", &[]).unwrap();
            assert_eq!(page.cards[0].group, "resolved");
            assert!(page.waiting.is_empty());
        },
    );
}
