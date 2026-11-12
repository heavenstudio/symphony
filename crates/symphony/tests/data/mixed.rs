use crate::common::Tree;

#[test]
fn the_home_and_full_queue_mix_topics_oldest_first_with_later_last() {
    let tree = Tree::new();
    for (folder, count) in [("pictures", 30), ("engineering", 3)] {
        let cards: String = (0..count).rev().map(|n| format!(
            "- [ ] **{folder}-{n}** — Decision {n}.\n  [2026-10-01 10:{n:02}] agent — Ready for review.\n"
        )).collect();
        tree.write(
            &format!("docs/{folder}/CARDS.md"),
            &format!("# {folder}\n\n{cards}"),
        );
    }
    let snapshot = || symphony::dashboard::snapshot(&tree.root, &[], "test").unwrap();
    let before = snapshot();
    assert_eq!(before.queue.waiting.len(), 33);
    assert_eq!(before.board.counts.waiting, 33);
    assert_eq!(
        before.board.next,
        before.queue.waiting[..before.board.next.len()]
    );
    assert_eq!(
        before.queue.waiting[..6]
            .iter()
            .map(|c| c.slug.as_str())
            .collect::<Vec<_>>(),
        [
            "engineering-0",
            "pictures-0",
            "engineering-1",
            "pictures-1",
            "engineering-2",
            "pictures-2"
        ]
    );
    assert_eq!(
        before
            .queue
            .waiting
            .iter()
            .filter(|c| symphony_api::queue_category(&c.doc) == "pictures")
            .map(|c| c.slug.clone())
            .collect::<Vec<_>>(),
        (0..30).map(|n| format!("pictures-{n}")).collect::<Vec<_>>()
    );
    symphony::postpone::act(
        &tree.root,
        "owner",
        "docs/pictures/CARDS.md",
        "pictures-1",
        "later",
        symphony::postpone::now(),
    )
    .unwrap();
    let after = snapshot();
    assert_eq!(after.queue.waiting.len(), 33);
    let picture: Vec<_> = after
        .queue
        .waiting
        .iter()
        .filter(|c| symphony_api::queue_category(&c.doc) == "pictures")
        .collect();
    assert_eq!(picture[0].slug, "pictures-0");
    assert_eq!(picture.last().unwrap().slug, "pictures-1");
    assert!(picture.last().unwrap().postponed);
    symphony::postpone::act(
        &tree.root,
        "owner",
        "docs/pictures/CARDS.md",
        "pictures-0",
        "later",
        symphony::postpone::now(),
    )
    .unwrap();
    let later = snapshot();
    let tail: Vec<_> = later
        .queue
        .waiting
        .iter()
        .rev()
        .take(2)
        .map(|c| c.slug.as_str())
        .collect();
    assert_eq!(
        tail,
        ["pictures-0", "pictures-1"],
        "Later keeps its preference order at the category's tail"
    );
    tree.write(crate::postpone::ORDER, "corrupt");
    let fallback = snapshot();
    assert_eq!(
        fallback.queue.waiting, before.queue.waiting,
        "unreadable preferences preserve the mixed queue"
    );
}

#[test]
fn the_activity_lists_each_agent_reply_once_in_its_words_without_markup() {
    let tree = Tree::new();
    tree.write(
        "docs/DECISIONS.md",
        "# Decision register\n\n\
         - [r] **client-cache** — A page draws at once.\n  \
         [2026-11-12 07:30] agent — Posted for the board task `perf/client-cache/design`.\n\n    \
         **Today.** Every data read is answered `no-store`.\n  \
         [2026-11-12 07:48] owner · approved\n  \
         [2026-11-12 08:05] agent — Recorded: **A.** built by `perf/client-cache/build`; see [[analysis/x|the analysis]].\n  \
         [2026-11-12 08:05] agent · resolved\n",
    );
    let snapshot = symphony::dashboard::snapshot(&tree.root, &[], "test").unwrap();
    let shown: Vec<_> = snapshot
        .activity
        .iter()
        .map(|item| (item.id.as_str(), item.kind.as_str(), item.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (
                "docs/DECISIONS.md#client-cache#2",
                "reply",
                "Recorded: A. built by perf/client-cache/build; see the analysis."
            ),
            (
                "docs/DECISIONS.md#client-cache#0",
                "new card",
                "Posted for the board task perf/client-cache/design. Today. Every data read is answered no-store."
            ),
        ],
        "the status line that resolves the card is no reply of its own"
    );
}

#[test]
fn a_callout_waiting_on_the_agent_is_named_by_its_title_and_read_in_its_document() {
    let tree = Tree::new();
    tree.write(
        "docs/design/lighting.md",
        "# Lighting direction\n\n\
         > [!comment] 141 lamps\n> Far too many to validate.\n\n\
         > [!question]\n> Do the **stations** need five size bands?\n\n\
         > [!decision] Drop the brass finish.\n> > [!resolved] Dropped.\n\n\
         > [!like] The warm palette\n",
    );
    let snapshot = symphony::dashboard::snapshot(&tree.root, &[], "test").unwrap();
    let rows: Vec<_> = snapshot
        .queue
        .agent
        .iter()
        .map(|item| (item.title.as_str(), item.kind.as_str(), item.text.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("141 lamps", "comment", "Lighting direction"),
            // one with no title is named by the first line of its body
            (
                "Do the stations need five size bands?",
                "question",
                "Lighting direction"
            ),
        ],
        "the resolved decision and the like wait on no one"
    );
}

#[test]
fn later_order_crosses_computed_groups_inside_a_document_category() {
    let tree = Tree::new();
    let path = "docs/engineering/CARDS.md";
    tree.write(path, "# Engineering\n\n- [?] **c1** — Confirm first.\n  [2026-10-20 10:00] owner · question — Why?\n  [2026-10-20 11:00] agent — Because.\n- [?] **c2** — Confirm second.\n  [2026-10-20 10:00] owner · question — Why?\n  [2026-10-20 11:00] agent — Because.\n- [ ] **d1** — Decide first.\n  [2026-10-20 12:00] agent — Ready.\n- [ ] **d2** — Decide second.\n  [2026-10-20 12:01] agent — Ready.\n");
    let now = symphony::postpone::now();
    for slug in ["c1", "d1"] {
        symphony::postpone::act(&tree.root, "owner", path, slug, "later", now).unwrap();
    }
    let snapshot = symphony::dashboard::snapshot(&tree.root, &[], "test").unwrap();
    assert_eq!(
        snapshot
            .queue
            .waiting
            .iter()
            .map(|c| (c.slug.as_str(), c.postponed))
            .collect::<Vec<_>>(),
        [("c2", false), ("d2", false), ("c1", true), ("d1", true)],
        "the latest Later goes last even when it belongs to a different group"
    );
}
