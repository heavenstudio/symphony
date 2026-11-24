//! The task diff's native routes on a scratch board and repository, with validators and errors.
use super::Hub;
use super::read_paths_tests::{observe, served_with};
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    git, repository, store_on,
};
use axum::http::StatusCode;
use symphony_api::{TaskDiff, TaskDiffPatch, TaskDiffSource};

#[test]
fn task_diff_routes_read_only_the_requested_task_and_open_file() {
    let tree = Tree::new();
    let root = repository(&tree);
    git(&root, &["checkout", "-qb", "tools/change"]);
    std::fs::write(root.join("file.txt"), "changed\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "change"]);
    let fixture = Fixture::new(Seed::default().task("tools/change", "in_progress"));
    let store = store_on(fixture.store.database());
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let board = fixture.other_database();
    hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
    hub.refresh_page("", &root).unwrap();
    observe(&hub, &store, &[]);
    let (_shared, ask) = served_with(&root, hub);
    for route in ["task-diff", "task-diff-file"] {
        assert_eq!(
            ask(&format!("/api/{route}"), &[]).0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ask(&format!("/api/{route}?slug=tools/missing"), &[]).0,
            StatusCode::NOT_FOUND
        );
    }
    let (status, headers, bytes) = ask("/api/task-diff?slug=tools/change", &[]);
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(headers["cache-control"], "no-store");
    assert!(headers.contains_key(symphony_api::VERSION_HEADER));
    let diff: TaskDiff = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(diff.source, TaskDiffSource::Branch);
    assert_eq!(diff.files.len(), 1);
    assert!(!String::from_utf8(bytes).unwrap().contains("+changed"));
    assert_eq!(
        ask(
            "/api/task-diff?slug=tools/change",
            &[("if-none-match", headers["etag"].to_str().unwrap())]
        )
        .0,
        StatusCode::NOT_MODIFIED
    );
    let path = format!(
        "/api/task-diff-file?slug=tools/change&path=file.txt&base={}&head={}",
        diff.base.unwrap(),
        diff.head.unwrap()
    );
    let (status, _, bytes) = ask(&path, &[]);
    assert_eq!(status, StatusCode::OK);
    let patch: TaskDiffPatch = serde_json::from_slice(&bytes).unwrap();
    assert!(patch.patch.contains("+changed"));
    assert_eq!(
        ask(
            "/api/task-diff-file?slug=tools/change&path=../other&base=x&head=y",
            &[]
        )
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ask(
            "/api/task-diff-file?slug=tools/change&path=file.txt&base=x&head=y",
            &[]
        )
        .0,
        StatusCode::CONFLICT
    );
    std::fs::create_dir(root.join("@scope")).unwrap();
    std::fs::write(root.join("@scope/file.txt"), "literal at-path\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "at-path"]);
    let (_, _, bytes) = ask("/api/task-diff?slug=tools/change", &[]);
    let moved: TaskDiff = serde_json::from_slice(&bytes).unwrap();
    let target = format!(
        "/api/task-diff-file?slug=tools/change&path=%40scope%2Ffile.txt&base={}&head={}",
        moved.base.unwrap(),
        moved.head.unwrap()
    );
    assert_eq!(ask(&target, &[]).0, StatusCode::OK);
    git(&root, &["checkout", "-q", "main"]);
    git(&root, &["branch", "-D", "tools/change"]);
    let (status, _, bytes) = ask("/api/task-diff?slug=tools/change", &[]);
    assert_eq!(status, StatusCode::OK);
    let gone: TaskDiff = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(gone.source, TaskDiffSource::Unavailable);
    assert!(gone.reason.unwrap().contains("gone"));
}
