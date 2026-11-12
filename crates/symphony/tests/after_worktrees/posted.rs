//! A card reaches the owner once, when its branch posts it (commits it): never from a worktree's
//! unsaved draft, never again from a finished branch's old copy, and main's copy and a branch's
//! read as one card wherever the owner reads it.

use std::collections::BTreeSet;
use std::path::Path;

use super::{DOC, branch, command, commit, isolated, repository};
use symphony::{branches, docview, render::Render};

/// A register as a page draws it: its cards one block, a blank line under the region's start.
fn register(body: &str) -> String {
    format!("# Decisions\n\n<!-- register:start -->\n\n{body}\n<!-- register:end -->\n")
}

/// A card posted at 09:00 with `lines` under it.
fn card(slug: &str, mark: char, lines: &str) -> String {
    format!(
        "- [{mark}] **{slug}** — Proposal {slug}.\n  [2026-11-11 09:00] agent — Posted.\n{lines}"
    )
}

fn write(at: &Path, body: &str) {
    std::fs::write(at.join(DOC), register(body)).unwrap();
}

const QUESTION: &str = "  [2026-11-11 17:42] owner · question — Why B?\n";
const ANSWER: &str = "  [2026-11-11 17:59] agent — Because B is faster.\n";
const RESOLVED: &str =
    "  [2026-11-11 18:00] owner · approved\n  [2026-11-11 19:00] agent · resolved\n";

/// What the owner's queue and counts show: the waiting cards' slugs, the cards waiting on an agent,
/// and the counts.
struct Shown {
    waiting: Vec<String>,
    agent: Vec<String>,
    counts: (u32, u32, u32),
    snapshot: symphony::dashboard::Snapshot,
}

fn shown(root: &Path, finished: &BTreeSet<String>) -> Shown {
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
                    .map(|c| (c.tree.branch.as_str(), c.card.slug.as_str()))
                    .collect::<Vec<_>>(),
                [("tools/posted", "shared"), ("tools/posted", "posted-card")],
                "no draft, no card before its picture, nothing of a merged or a done branch"
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
            // The slug's own task decides, whatever another task records for its branch.
            task("tools/live/work", "done", Some("tools/live"), None),
            task("tools/cancelled", "cancelled", None, None),
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
    // Every done task's own branch is finished too, `b/one` among them; `tools/live` and
    // `worktree-two` are not.
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
}
