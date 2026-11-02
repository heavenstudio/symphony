use crate::common::Tree;

#[test]
fn the_home_and_full_queue_mix_topics_oldest_first_with_later_last() {
    let tree = Tree::new();
    for (folder, count) in [("art", 30), ("engineering", 3)] {
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
            "art-0",
            "engineering-0",
            "art-1",
            "engineering-1",
            "art-2",
            "engineering-2"
        ]
    );
    assert_eq!(
        before
            .queue
            .waiting
            .iter()
            .filter(|c| symphony_api::queue_category(&c.doc) == "art")
            .map(|c| c.slug.clone())
            .collect::<Vec<_>>(),
        (0..30).map(|n| format!("art-{n}")).collect::<Vec<_>>()
    );
    symphony::postpone::act(
        &tree.root,
        "owner",
        "docs/art/CARDS.md",
        "art-1",
        "later",
        symphony::postpone::now(),
    )
    .unwrap();
    let after = snapshot();
    assert_eq!(after.queue.waiting.len(), 33);
    let art: Vec<_> = after
        .queue
        .waiting
        .iter()
        .filter(|c| symphony_api::queue_category(&c.doc) == "art")
        .collect();
    assert_eq!(art[0].slug, "art-0");
    assert_eq!(art.last().unwrap().slug, "art-1");
    assert!(art.last().unwrap().postponed);
    symphony::postpone::act(
        &tree.root,
        "owner",
        "docs/art/CARDS.md",
        "art-0",
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
        ["art-0", "art-1"],
        "Later keeps its preference order at the category's tail"
    );
    tree.write(symphony::postpone::FILE, "corrupt");
    let fallback = snapshot();
    assert_eq!(
        fallback.queue.waiting, before.queue.waiting,
        "unreadable preferences preserve the mixed queue"
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
