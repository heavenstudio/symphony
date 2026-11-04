use crate::common::{Tree, plain};
use symphony::{board, dashboard, postpone};

const PATH: &str = "docs/TOPIC.md";
const CARDS: &str = "# Topic\n\n- [ ] **alpha** — First.\n- [ ] **bravo** — Second.\n- [ ] **charlie** — Third.\n- [?] **confirm** — A question.\n  [2026-10-20 10:00] owner · question — Why?\n  [2026-10-20 11:00] agent — Because.\n- [x] **done** — Answered.\n";

fn fixture() -> Tree {
    let tree = Tree::new();
    tree.write(PATH, CARDS);
    tree.write("docs/DECISIONS.md", "# Decisions\n");
    tree
}

fn ordered(tree: &Tree, owner: &str, now: u64) -> Vec<(String, bool)> {
    let docs = board::scan(&tree.root).unwrap();
    let mut waiting: Vec<symphony_api::Card> = symphony::triage::queue(&plain(), &docs, usize::MAX, "").into_iter()
        .map(|item| serde_json::from_value(serde_json::json!({"slug":item.slug,"doc":item.doc,"doc_title":"Topic","title":"Card","why":item.reason})).unwrap()).collect();
    postpone::apply(&tree.root, owner, &docs, &mut waiting, now).unwrap();
    waiting
        .into_iter()
        .map(|card| (card.slug, card.postponed))
        .collect()
}

fn slugs(rows: Vec<(String, bool)>) -> Vec<String> {
    rows.into_iter().map(|(slug, _)| slug).collect()
}

#[test]
fn later_is_persistent_owner_order_with_unchanged_documents_and_groups() {
    let tree = fixture();
    let now = postpone::now();
    let before = slugs(ordered(&tree, "owner", now));
    assert_eq!(before, ["alpha", "bravo", "charlie", "confirm"]);
    postpone::act(&tree.root, "owner", PATH, "alpha", "seen", now).unwrap();
    postpone::act(&tree.root, "owner", PATH, "alpha", "undo", now).unwrap();
    postpone::answered(&tree.root, "owner", "alpha").unwrap();
    assert!(!tree.path(postpone::FILE).exists());
    postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap();
    assert_eq!(
        ordered(&tree, "owner", now),
        [
            ("bravo".into(), false),
            ("charlie".into(), false),
            ("alpha".into(), true),
            ("confirm".into(), false)
        ]
    );
    assert_eq!(slugs(ordered(&tree, "another-owner", now)), before);
    assert_eq!(tree.read(PATH), CARDS);
    assert_eq!(tree.read("docs/DECISIONS.md"), "# Decisions\n");
    let persisted = tree.read(postpone::FILE);
    for (owner, path, action) in [
        ("another-owner", PATH, "seen"),
        ("another-owner", PATH, "undo"),
        ("owner", "docs/OTHER.md", "undo"),
    ] {
        assert!(!postpone::act(&tree.root, owner, path, "alpha", action, now).unwrap());
        assert_eq!(tree.read(postpone::FILE), persisted);
    }
    // A fresh directory, as on a restarted process: no in-memory cache can supply the order.
    let restarted = fixture();
    restarted.write(postpone::FILE, &persisted);
    let snapshot = dashboard::snapshot(&restarted.root, &[], "").unwrap();
    assert_eq!(snapshot.board.counts.waiting, 4);
    assert_eq!(snapshot.board.next[0].slug, "bravo");
    assert_eq!(
        slugs(
            snapshot
                .queue
                .waiting
                .into_iter()
                .map(|c| (c.slug, c.postponed))
                .collect()
        ),
        ["bravo", "charlie", "confirm", "alpha"]
    );
    postpone::act(&tree.root, "owner", PATH, "alpha", "undo", now).unwrap();
    assert_eq!(slugs(ordered(&tree, "owner", now)), before);
}

#[test]
fn seen_round_and_three_day_expiry_restore_normal_order() {
    let tree = fixture();
    let now = postpone::now();
    postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap();
    postpone::act(&tree.root, "owner", PATH, "confirm", "seen", now).unwrap();
    postpone::act(&tree.root, "owner", PATH, "bravo", "seen", now).unwrap();
    assert!(
        ordered(&tree, "owner", now)
            .iter()
            .any(|(slug, marked)| slug == "alpha" && *marked)
    );
    postpone::act(&tree.root, "owner", PATH, "charlie", "seen", now).unwrap();
    assert_eq!(
        slugs(ordered(&tree, "owner", now)),
        ["alpha", "bravo", "charlie", "confirm"]
    );
    postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap();
    assert!(!postpone::expired(&tree.root, now + 3 * 86_400 - 1));
    assert!(postpone::expired(&tree.root, now + 3 * 86_400));
    assert_eq!(
        slugs(ordered(&tree, "owner", now + 3 * 86_400)),
        ["alpha", "bravo", "charlie", "confirm"]
    );
    assert!(!tree.read(postpone::FILE).contains("alpha"));
}

#[test]
fn answered_deleted_and_invalid_cards_cannot_keep_a_postponement() {
    let tree = fixture();
    let now = postpone::now();
    for slug in ["done", "missing"] {
        assert!(postpone::act(&tree.root, "owner", PATH, slug, "later", now).is_err());
    }
    assert!(postpone::act(&tree.root, "owner", "docs/OTHER.md", "alpha", "later", now).is_err());
    assert!(postpone::act(&tree.root, "owner", PATH, "alpha", "unknown", now).is_err());
    postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap();
    tree.write(PATH, &CARDS.replace("- [ ] **alpha** — First.\n", ""));
    assert_eq!(
        slugs(ordered(&tree, "owner", now)),
        ["bravo", "charlie", "confirm"]
    );
    assert!(!tree.read(postpone::FILE).contains("alpha"));
    tree.write(PATH, CARDS);
    postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap();
    postpone::answered(&tree.root, "owner", "bravo").unwrap();
    assert!(
        ordered(&tree, "owner", now)
            .iter()
            .any(|(slug, marked)| slug == "alpha" && *marked)
    );
    postpone::act(&tree.root, "owner", PATH, "charlie", "seen", now).unwrap();
    assert!(!tree.read(postpone::FILE).contains("alpha"));
    postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap();
    postpone::answered(&tree.root, "owner", "alpha").unwrap();
    assert!(!tree.read(postpone::FILE).contains("alpha"));
    assert_eq!(tree.read(PATH), CARDS);
}

#[test]
fn repeated_later_finishes_a_round_and_state_errors_preserve_the_file() {
    let tree = fixture();
    let now = postpone::now();
    for slug in ["bravo", "alpha"] {
        postpone::act(&tree.root, "owner", PATH, slug, "later", now).unwrap();
    }
    assert_eq!(
        slugs(ordered(&tree, "owner", now)),
        ["charlie", "bravo", "alpha", "confirm"]
    );
    postpone::act(&tree.root, "owner", PATH, "charlie", "later", now).unwrap();
    assert_eq!(
        slugs(ordered(&tree, "owner", now)),
        ["alpha", "bravo", "charlie", "confirm"]
    );
    tree.write(postpone::FILE, "corrupt");
    let error = postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap_err();
    assert!(error.to_string().contains(postpone::FILE));
    assert_eq!(tree.read(postpone::FILE), "corrupt");
    assert_eq!(tree.read(PATH), CARDS);
}

#[test]
fn confirmation_order_and_concurrent_owners_stay_independent() {
    let tree = fixture();
    tree.write(PATH, &format!("{CARDS}- [?] **confirm-again** — Another question.\n  [2026-10-20 10:00] owner · question — Why?\n  [2026-10-20 11:00] agent — Because.\n"));
    let now = postpone::now();
    std::thread::scope(|scope| {
        for (owner, slug) in [("owner", "confirm"), ("another-owner", "alpha")] {
            let root = &tree.root;
            scope.spawn(move || postpone::act(root, owner, PATH, slug, "later", now).unwrap());
        }
    });
    assert_eq!(
        slugs(ordered(&tree, "owner", now)),
        ["alpha", "bravo", "charlie", "confirm-again", "confirm"]
    );
    assert_eq!(
        slugs(ordered(&tree, "another-owner", now)),
        ["bravo", "charlie", "alpha", "confirm", "confirm-again"]
    );
    // An answer outside the app must clear the old entry as well.
    tree.write(
        PATH,
        &tree
            .read(PATH)
            .replace("[?] **confirm**", "[x] **confirm**")
            .replace("owner · question — Why?", "owner · approved — Yes."),
    );
    ordered(&tree, "owner", now);
    let state: serde_json::Value = serde_json::from_str(&tree.read(postpone::FILE)).unwrap();
    assert!(state["owners"]["owner"].is_null());
    assert!(state["owners"]["another-owner"]["alpha"].is_object());
}

#[test]
fn a_new_owner_line_clears_postponement_without_a_group_change() {
    let tree = fixture();
    let now = postpone::now();
    postpone::act(&tree.root, "owner", PATH, "alpha", "later", now).unwrap();
    assert!(
        ordered(&tree, "owner", now)
            .iter()
            .any(|(slug, marked)| slug == "alpha" && *marked)
    );
    let reset =
        symphony::edit::set_decision(CARDS, "alpha", "pending", "", "2026-10-28 12:00", &plain())
            .unwrap();
    tree.write(PATH, &reset);
    let docs = board::scan(&tree.root).unwrap();
    let card = docs
        .iter()
        .flat_map(|doc| &doc.decisions)
        .find(|card| card.slug == "alpha")
        .unwrap();
    assert_eq!(card.group(&plain()), symphony::Group::Decide);
    assert_eq!(card.state, symphony::State::Pending);
    assert!(
        card.entries()
            .iter()
            .any(|entry| entry.who == symphony::Who::Owner)
    );
    let refreshed = ordered(&tree, "owner", now);
    assert!(
        refreshed
            .iter()
            .any(|(slug, marked)| slug == "alpha" && !marked)
    );
    assert!(!tree.read(postpone::FILE).contains("alpha"));
}

#[test]
fn seen_is_best_effort_when_the_existence_check_fails_but_undo_reports_it() {
    let tree = fixture();
    let path = tree.path(postpone::FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("postponed.json", &path).unwrap();
    assert!(
        path.try_exists().is_err(),
        "the symlink loop fails the existence check"
    );
    assert!(!postpone::act(&tree.root, "owner", PATH, "alpha", "seen", 0).unwrap());
    let error = postpone::act(&tree.root, "owner", PATH, "alpha", "undo", 0).unwrap_err();
    assert!(error.to_string().contains(postpone::FILE));
    assert_eq!(
        std::fs::read_link(&path).unwrap(),
        std::path::Path::new("postponed.json")
    );
    assert_eq!(tree.read(PATH), CARDS);
}

#[test]
fn broken_view_preferences_preserve_the_file_and_serve_triage_order() {
    for unreadable in [false, true] {
        let tree = fixture();
        if unreadable {
            std::fs::create_dir_all(tree.path(postpone::FILE)).unwrap();
        } else {
            tree.write(postpone::FILE, "corrupt preferences");
        }
        assert!(
            !postpone::act(&tree.root, "owner", PATH, "alpha", "seen", postpone::now()).unwrap()
        );
        assert!(
            postpone::act(&tree.root, "owner", PATH, "alpha", "undo", postpone::now()).is_err()
        );
        let snapshot = dashboard::snapshot(&tree.root, &[], "").unwrap();
        assert_eq!(
            snapshot
                .queue
                .waiting
                .iter()
                .map(|card| card.slug.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "bravo", "charlie", "confirm"]
        );
        assert!(snapshot.queue.waiting.iter().all(|card| !card.postponed));
        assert_eq!(snapshot.board.counts.waiting, 4);
        assert_eq!(tree.read(PATH), CARDS);
        if unreadable {
            assert!(tree.path(postpone::FILE).is_dir());
        } else {
            assert_eq!(tree.read(postpone::FILE), "corrupt preferences");
        }
    }
}
