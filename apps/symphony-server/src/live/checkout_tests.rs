//! The hub's two small rules every read shares: the checkout a worktree's scope names, and the
//! board reader kept or forgotten by what a read on it answered.

use super::Hub;
use crate::testing::{Tree, board::Fixture, repository, store_on};
use axum::http::StatusCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

#[test]
fn a_scope_names_the_projects_checkout_a_worktrees_or_none() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = Hub::new(&root, &root.join("build/client"));
    assert_eq!(hub.checkout(""), Some(root.clone()));
    // A worktree that is not there is none, never the project's own checkout in its place.
    assert_eq!(hub.checkout("gone"), None);
    let linked = root.join("../linked");
    hub.trees
        .write()
        .unwrap()
        .insert("linked".into(), linked.clone());
    assert_eq!(hub.checkout("linked"), Some(linked));
    assert_eq!(hub.checkout(""), Some(root));
}

/// A hub whose board reader is a scratch board's, and how many times it was opened.
fn reading(tree: &Tree) -> (Hub, Arc<AtomicUsize>) {
    let root = repository(tree);
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let fixture = Fixture::empty();
    let opened = Arc::new(AtomicUsize::new(0));
    let count = opened.clone();
    hub.connect_reader = Box::new(move |_| {
        count.fetch_add(1, SeqCst);
        Ok(store_on(&fixture.other_client()))
    });
    (hub, opened)
}

#[test]
fn a_read_the_board_failed_forgets_the_reader_and_any_other_answer_keeps_it() {
    let tree = Tree::new();
    let (hub, opened) = reading(&tree);
    let store = hub.reader().unwrap();
    // What was made, and a refusal of what was asked, leave as they are and keep the reader.
    assert_eq!(hub.read_on(&store, Ok(7)), Ok(7));
    for status in [
        StatusCode::NOT_FOUND,
        StatusCode::BAD_REQUEST,
        StatusCode::CONFLICT,
        StatusCode::INTERNAL_SERVER_ERROR,
    ] {
        let refused: Result<(), _> = Err((status, "no".to_string()));
        assert_eq!(hub.read_on(&store, refused.clone()), refused, "{status}");
        assert!(Arc::ptr_eq(&store, &hub.reader().unwrap()), "{status}");
    }
    assert_eq!(opened.load(SeqCst), 1);
    // The board failing the read, down or too slow, leaves as it is and forgets the reader.
    let mut store = store;
    for (nth, status) in [StatusCode::SERVICE_UNAVAILABLE, StatusCode::GATEWAY_TIMEOUT]
        .into_iter()
        .enumerate()
    {
        let failed: Result<(), _> = Err((status, "down".to_string()));
        assert_eq!(hub.read_on(&store, failed.clone()), failed, "{status}");
        let next = hub.reader().unwrap();
        assert!(!Arc::ptr_eq(&store, &next), "{status}: opened anew");
        assert_eq!(opened.load(SeqCst), nth + 2, "{status}");
        // A failure on the reader already forgotten leaves the one that replaced it.
        let late: Result<(), _> = Err((status, "down".to_string()));
        assert!(hub.read_on(&store, late).is_err());
        assert!(Arc::ptr_eq(&next, &hub.reader().unwrap()), "{status}");
        store = next;
    }
    assert_eq!(opened.load(SeqCst), 3);
}
