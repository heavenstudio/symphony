//! What an outline's check (`/api/document?view=outline` with `If-None-Match`) costs the hub: one
//! that names the tag of the copy the page follows renders nothing and is answered `304` at the
//! feed's cursor; a check with another tag or none, of a document not followed yet, held as
//! missing or naming a file from outside the entrypoints' folders renders, and a document a
//! refresh rendered again is answered with its new outline. Renders are counted, never timed.

use super::Hub;
use super::read_paths_tests::{Answer, Move, moved, observe, served_with};
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    git, repository,
};
use axum::http::StatusCode;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::Ordering;

const OUTLINE: &str = "/api/document?path=docs/TOPIC.md&view=outline";

/// The tag an answer names, as its `ETag` sends it.
fn tag(answer: &Answer) -> String {
    answer.1["etag"].to_str().unwrap().to_string()
}

/// The feed's cursor an answer names.
fn cursor(answer: &Answer) -> u64 {
    answer.1[symphony_api::VERSION_HEADER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap()
}

fn says(answer: &Answer, words: &str) -> bool {
    String::from_utf8_lossy(&answer.2).contains(words)
}

/// A hub over `root` with main's page read, served: the hub, and a way to ask for `path`'s
/// outline, naming a tag or none.
fn serving(
    root: &Path,
) -> (
    std::sync::Arc<Hub>,
    impl Fn(&str, Option<&str>) -> Answer + use<>,
) {
    let hub = Hub::new(root, &root.join("build/client"));
    hub.refresh_docs(None).unwrap();
    let (shared, send) = served_with(root, hub);
    let hub = shared.api.get().unwrap().clone();
    let ask = move |path: &str, named: Option<&str>| match named {
        Some(named) => send(path, &[("If-None-Match", named)]),
        None => send(path, &[]),
    };
    (hub, ask)
}

fn renders(hub: &Hub) -> u64 {
    hub.renders.load(Ordering::SeqCst)
}

#[test]
fn a_check_naming_the_followed_copys_tag_renders_nothing_and_is_answered_at_the_cursor() {
    let tree = Tree::new();
    let root = repository(&tree);
    let (hub, ask) = serving(&root);

    // A check of a document not followed yet renders: there is no copy to answer from.
    let unknown = ask(OUTLINE, Some("\"0000000000000000\""));
    assert_eq!(unknown.0, StatusCode::OK);
    let named = tag(&unknown);
    assert!(named.starts_with("W/\""), "{named}");
    let rendered = renders(&hub);
    assert!(rendered > 0, "the first read rendered the document");

    // The weak tag as sent, its strong form, one of a list and any (`*`): each names the copy.
    let strong = named.trim_start_matches("W/");
    let listed = format!("\"1111111111111111\", {named}");
    for asked in [named.as_str(), strong, listed.as_str(), "*"] {
        let checked = ask(OUTLINE, Some(asked));
        assert_eq!(checked.0, StatusCode::NOT_MODIFIED, "{asked}");
        assert!(checked.2.is_empty(), "{asked}: a 304 carries no body");
        assert_eq!(tag(&checked), named, "{asked}");
        assert_eq!(
            cursor(&checked),
            hub.pages.read().unwrap()[""].board.version,
            "{asked}: the feed's cursor the copy stands at"
        );
        assert_eq!(checked.1["cache-control"], "no-store", "{asked}");
    }
    assert_eq!(renders(&hub), rendered, "a matching tag renders nothing");

    // The cursor moves with the page, the tag with the document alone.
    {
        let mut pages = hub.pages.write().unwrap();
        let page = pages.get_mut("").unwrap();
        hub.publish(page, vec![serde_json::json!({"region":"probe"})]);
    }
    let later = ask(OUTLINE, Some(&named));
    assert_eq!(later.0, StatusCode::NOT_MODIFIED);
    assert_eq!(cursor(&later), cursor(&unknown) + 1);
    assert_eq!(renders(&hub), rendered);

    // Another tag, or none, is a read: it renders, as it did.
    let other = ask(OUTLINE, Some("\"0000000000000000\""));
    assert_eq!(other.0, StatusCode::OK);
    assert_eq!(tag(&other), named, "the same outline");
    assert_eq!(renders(&hub), rendered + 1);
    let plain = ask(OUTLINE, None);
    assert_eq!(plain.0, StatusCode::OK);
    assert_eq!(renders(&hub), rendered + 2);

    // Each read placed a copy of its own: the tag is made again for it, and still renders nothing.
    let again = ask(OUTLINE, Some(&named));
    assert_eq!(again.0, StatusCode::NOT_MODIFIED);
    assert_eq!(renders(&hub), rendered + 2);

    // Only the outline's check is answered so: the whole document's renders.
    let whole = ask("/api/document?path=docs/TOPIC.md", Some(&named));
    assert_eq!(whole.0, StatusCode::OK);
    assert_eq!(renders(&hub), rendered + 3);
}

#[test]
fn a_document_a_refresh_rendered_again_is_answered_with_its_new_outline() {
    let tree = Tree::new();
    let root = repository(&tree);
    let (hub, ask) = serving(&root);
    let first = ask(OUTLINE, None);
    let named = tag(&first);

    // Its own file, told by the watcher.
    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n## A new heading\n\nWords.\n",
    );
    hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/TOPIC.md")])))
        .unwrap();
    let rendered = renders(&hub);
    let edited = ask(OUTLINE, Some(&named));
    assert_eq!(edited.0, StatusCode::OK);
    assert!(says(&edited, "A new heading"), "the new outline");
    let renamed = tag(&edited);
    assert_ne!(renamed, named);
    assert_eq!(
        ask(OUTLINE, Some(&renamed)).0,
        StatusCode::NOT_MODIFIED,
        "the new tag names the new copy"
    );
    assert_eq!(
        renders(&hub),
        rendered + 1,
        "the tag that no longer matches rendered, the one that does did not"
    );

    // Another file of the documents renders it again, as a render reads more than its own file:
    // the copy is another, its outline the same, and its tag is made again without a render.
    tree.write("main/docs/OTHER.md", "# Other\n\nWords of its own.\n");
    hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/OTHER.md")])))
        .unwrap();
    let rendered = renders(&hub);
    assert_eq!(ask(OUTLINE, Some(&renamed)).0, StatusCode::NOT_MODIFIED);
    assert_eq!(renders(&hub), rendered);
}

/// `change` made on the fixture's board, then read by the hub as its loop reads it: a board read
/// alone, which asks for no refresh of the documents and renders again only those whose linking
/// tasks moved.
fn board_moves(hub: &Hub, f: &Fixture, change: Move) {
    moved(&f.store, change);
    observe(hub, &f.store, &[]);
}

#[test]
fn a_document_whose_linking_task_moved_on_the_board_alone_is_answered_with_its_new_outline() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/OTHER.md", "# Other\n\nNo task links it.\n");
    let f = Fixture::new(
        Seed::default()
            .task("tools/topic", "ready")
            .link("tools/topic", "doc", "docs/TOPIC.md")
            .task("tools/later", "ready")
            .task("tools/unlinked", "ready"),
    );
    let (hub, ask) = serving(&root);
    observe(&hub, &f.store, &[]);
    let other = "/api/document?path=docs/OTHER.md&view=outline";
    let first = ask(OUTLINE, None);
    assert!(says(&first, "\"ready\""), "the outline names its tasks");
    let mut named = tag(&first);
    let unlinked = tag(&ask(other, None));
    assert_eq!(ask(OUTLINE, Some(&named)).0, StatusCode::NOT_MODIFIED);

    // Each is a change of the board alone: no file, no ref, and no refresh of the documents.
    let moves = [
        (
            "its status",
            Move::Set(
                "tools/topic",
                serde_json::json!({ "status": "in_progress" }),
            ),
            "in_progress",
        ),
        (
            "its title",
            Move::Set(
                "tools/topic",
                serde_json::json!({ "title": "Retitled since" }),
            ),
            "Retitled since",
        ),
        (
            "another task linking it",
            Move::Link("tools/later", "docs/TOPIC.md"),
            "tools/later",
        ),
    ];
    for (case, change, words) in moves {
        let rendered = renders(&hub);
        board_moves(&hub, &f, change);
        // The board read rendered the copy again and told open pages.
        assert_eq!(renders(&hub), rendered + 1, "{case}: rendered once");
        let published = hub.pages.read().unwrap()[""].history.len();
        // A return visit's check names the old copy's tag: it reads, and finds nothing new to tell.
        let moved = ask(OUTLINE, Some(&named));
        assert_eq!(moved.0, StatusCode::OK, "{case}");
        assert!(says(&moved, words), "{case}: the new outline");
        assert_eq!(renders(&hub), rendered + 2, "{case}: a read renders");
        assert_eq!(
            hub.pages.read().unwrap()[""].history.len(),
            published,
            "{case}: the board read told it already"
        );
        assert_ne!(tag(&moved), named, "{case}");
        named = tag(&moved);
        // The copy that render placed lists the board's tasks: its check renders nothing.
        assert_eq!(
            ask(OUTLINE, Some(&named)).0,
            StatusCode::NOT_MODIFIED,
            "{case}"
        );
        assert_eq!(renders(&hub), rendered + 2, "{case}");
        // A document none of those tasks links is answered from its copy throughout.
        assert_eq!(
            ask(other, Some(&unlinked)).0,
            StatusCode::NOT_MODIFIED,
            "{case}"
        );
        assert_eq!(renders(&hub), rendered + 2, "{case}");
    }

    // A task that links nothing of it moved: the check is answered from the copy.
    board_moves(
        &hub,
        &f,
        Move::Set(
            "tools/unlinked",
            serde_json::json!({ "status": "in_progress" }),
        ),
    );
    let rendered = renders(&hub);
    assert_eq!(ask(OUTLINE, Some(&named)).0, StatusCode::NOT_MODIFIED);
    assert_eq!(renders(&hub), rendered);
}

#[test]
fn an_edit_no_refresh_told_yet_is_found_by_a_read_and_a_check_says_the_followed_copy() {
    let tree = Tree::new();
    let root = repository(&tree);
    let (hub, ask) = serving(&root);
    let named = tag(&ask(OUTLINE, None));

    // The check says what a page left open shows, at the cursor its feed tells the edit from.
    tree.write("main/docs/TOPIC.md", "# Topic\n\n## Unseen yet\n\nWords.\n");
    let rendered = renders(&hub);
    let checked = ask(OUTLINE, Some(&named));
    assert_eq!(checked.0, StatusCode::NOT_MODIFIED);
    assert_eq!(renders(&hub), rendered);
    let stood = cursor(&checked);

    // The refresh the watcher asks for publishes it past that cursor, and the tag stops matching.
    hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/TOPIC.md")])))
        .unwrap();
    {
        let pages = hub.pages.read().unwrap();
        let event = pages[""].history.back().unwrap();
        assert!(event.version > stood, "the edit is told after the cursor");
        assert!(event.json.contains("Unseen yet"), "{}", event.json);
    }
    let told = ask(OUTLINE, Some(&named));
    assert_eq!(told.0, StatusCode::OK);
    assert!(says(&told, "Unseen yet"));
}

#[test]
fn a_document_naming_a_file_outside_the_documents_folders_renders_at_each_check() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/shots/lamp.png", "a picture");
    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n![The lamp](../shots/lamp.png)\n",
    );
    let (hub, ask) = serving(&root);
    let first = ask(OUTLINE, None);
    assert_eq!(first.0, StatusCode::OK);
    let named = tag(&first);

    // Unchanged, it is still answered 304, by rendering: no refresh watches that folder.
    let rendered = renders(&hub);
    for _ in 0..2 {
        assert_eq!(ask(OUTLINE, Some(&named)).0, StatusCode::NOT_MODIFIED);
    }
    assert_eq!(renders(&hub), rendered + 2);

    // The picture replaced, with no document changed and no refresh: the check finds it.
    tree.write("main/shots/lamp.png", "another picture, of another size");
    let replaced = ask(OUTLINE, Some(&named));
    assert_eq!(replaced.0, StatusCode::OK);
    assert_ne!(tag(&replaced), named);
}

#[test]
fn a_document_the_page_holds_as_missing_is_not_answered_from_its_copy() {
    let tree = Tree::new();
    let root = repository(&tree);
    let (hub, ask) = serving(&root);
    let named = tag(&ask(OUTLINE, None));
    assert_eq!(ask(OUTLINE, Some(&named)).0, StatusCode::NOT_MODIFIED);

    std::fs::remove_file(root.join("docs/TOPIC.md")).unwrap();
    hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/TOPIC.md")])))
        .unwrap();
    assert!(
        hub.pages.read().unwrap()[""]
            .missing
            .contains("docs/TOPIC.md")
    );
    assert_eq!(ask(OUTLINE, Some(&named)).0, StatusCode::NOT_FOUND);

    // Put back as it was, it is followed again, and its check renders nothing once more.
    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n- [ ] **initial-card** — Initial question.\n",
    );
    hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/TOPIC.md")])))
        .unwrap();
    let rendered = renders(&hub);
    assert_eq!(ask(OUTLINE, Some(&named)).0, StatusCode::NOT_MODIFIED);
    assert_eq!(renders(&hub), rendered);
}

#[test]
fn a_branch_pages_check_is_answered_from_that_pages_copy() {
    let tree = Tree::new();
    let root = repository(&tree);
    let branch = tree.path("proposal");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "design/policy",
            branch.to_str().unwrap(),
        ],
    );
    let (hub, ask) = serving(&root);
    let scope = "design~policy";
    hub.ensure_page(scope).unwrap();
    let outline = format!("/@{scope}{OUTLINE}");
    let first = ask(&outline, None);
    assert_eq!(first.0, StatusCode::OK);
    let named = tag(&first);
    let rendered = renders(&hub);
    let checked = ask(&outline, Some(&named));
    assert_eq!(checked.0, StatusCode::NOT_MODIFIED);
    assert_eq!(
        cursor(&checked),
        hub.pages.read().unwrap()[scope].board.version
    );
    assert_eq!(renders(&hub), rendered, "the branch's copy answers");

    // Main's copy is another: its page follows nothing of the branch's.
    assert_eq!(ask(OUTLINE, Some(&named)).0, StatusCode::OK);
    assert_eq!(renders(&hub), rendered + 1);
}

#[test]
fn a_tag_made_while_another_copy_was_placed_answers_nothing_and_is_not_kept() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_docs(None).unwrap();
    let tag_of = |_: symphony_api::Document| "\"made\"".to_string();
    assert_eq!(
        hub.outline_kept("", "docs/TOPIC.md", tag_of),
        None,
        "a document not followed has no copy"
    );
    assert_eq!(hub.outline_kept("no~page", "docs/TOPIC.md", tag_of), None);
    hub.document("", "docs/TOPIC.md").unwrap();

    // A read places its copy while the first copy's tag is being made: that tag names a copy the
    // page no longer holds.
    let during = hub.outline_kept("", "docs/TOPIC.md", |_| {
        hub.document("", "docs/TOPIC.md").unwrap();
        "\"of the first copy\"".to_string()
    });
    assert_eq!(during, None);
    let cursor = hub.pages.read().unwrap()[""].board.version;
    assert_eq!(
        hub.outline_kept("", "docs/TOPIC.md", tag_of),
        Some(("\"made\"".to_string(), cursor)),
        "the copy held now has its own tag made"
    );
    // Made once for a copy: the next check makes none.
    assert_eq!(
        hub.outline_kept("", "docs/TOPIC.md", |_| unreachable!("the tag is kept")),
        Some(("\"made\"".to_string(), cursor))
    );
}
