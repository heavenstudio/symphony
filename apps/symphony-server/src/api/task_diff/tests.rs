use super::*;
use crate::testing::{Tree, git as command, repository};

fn deadline() -> Instant {
    Instant::now() + test_time::backstop(Duration::from_secs(30))
}

fn input() -> Input {
    Input {
        branch: "tools/change".into(),
        default: "main".into(),
        landing: None,
    }
}

fn save(root: &Path, path: &str, bytes: impl AsRef<[u8]>) {
    std::fs::write(root.join(path), bytes).unwrap();
    command(root, &["add", "--", path]);
    command(root, &["commit", "-qm", "change"]);
}

#[tokio::test]
async fn a_branch_uses_its_merge_base_and_a_landing_uses_its_own_first_parent() {
    let tree = Tree::new();
    let root = repository(&tree);
    command(&root, &["checkout", "-qb", "tools/change"]);
    save(&root, "changed.txt", "one\ntwo\n");
    command(&root, &["checkout", "-q", "main"]);
    save(&root, "unrelated.txt", "main only\n");
    let before = summary(&root, &input(), deadline()).await;
    assert_eq!(before.source, TaskDiffSource::Branch);
    assert_eq!(
        before.files,
        vec![TaskDiffFile {
            path: "changed.txt".into(),
            added: Some(2),
            removed: Some(0)
        }]
    );
    let pair = (
        before.base.as_deref().unwrap(),
        before.head.as_deref().unwrap(),
    );
    let patch = file(&root, &input(), pair, "changed.txt", deadline())
        .await
        .unwrap();
    assert!(patch.patch.contains("@@ -0,0 +1,2 @@\n+one\n+two"));
    assert!(!patch.truncated);
    command(&root, &["merge", "--no-ff", "-qm", "land", "tools/change"]);
    let landing = commit(&root, "HEAD", deadline()).await.unwrap();
    save(&root, "later.txt", "later main\n");
    command(&root, &["branch", "-D", "tools/change"]);
    let landed = Input {
        landing: Some(landing.clone()),
        ..input()
    };
    let after = summary(&root, &landed, deadline()).await;
    assert_eq!(after.source, TaskDiffSource::Landing);
    assert_eq!(after.head.as_deref(), Some(landing.as_str()));
    assert_eq!(after.files, before.files);
    let patch = file(
        &root,
        &landed,
        (after.base.as_deref().unwrap(), &landing),
        "changed.txt",
        deadline(),
    )
    .await
    .unwrap();
    assert!(patch.patch.contains("+two"));
    assert!(!patch.patch.contains("unrelated"));
}

#[tokio::test]
async fn gone_branches_and_unreadable_landings_are_said_without_an_empty_success() {
    let tree = Tree::new();
    let root = repository(&tree);
    let gone = summary(&root, &input(), deadline()).await;
    assert_eq!(gone.source, TaskDiffSource::Unavailable);
    assert!(gone.reason.unwrap().contains("gone"));
    for landing in ["--help", "0123456789abcdef", "HEAD"] {
        let diff = summary(
            &root,
            &Input {
                landing: Some(landing.into()),
                ..input()
            },
            deadline(),
        )
        .await;
        assert_eq!(diff.source, TaskDiffSource::Unavailable);
        assert!(diff.reason.is_some());
    }
    let diff = summary(&root, &input(), Instant::now()).await;
    assert!(diff.reason.unwrap().contains("time limit"));
}

#[tokio::test]
async fn binary_files_literal_paths_and_changed_comparisons_are_handled() {
    let tree = Tree::new();
    let root = repository(&tree);
    command(&root, &["checkout", "-qb", "tools/change"]);
    save(&root, "binary.dat", [0, 255, 1]);
    save(&root, "tab\tline\n.txt", "literal\n");
    save(&root, "[magic].txt", "literal magic\n");
    let diff = summary(&root, &input(), deadline()).await;
    assert_eq!(diff.files.len(), 3);
    let binary = diff.files.iter().find(|f| f.path == "binary.dat").unwrap();
    assert_eq!((binary.added, binary.removed), (None, None));
    let pair = (diff.base.as_deref().unwrap(), diff.head.as_deref().unwrap());
    for path in ["binary.dat", "tab\tline\n.txt", "[magic].txt"] {
        let patch = file(&root, &input(), pair, path, deadline()).await.unwrap();
        assert_eq!(patch.path, path);
        assert!(!patch.truncated);
        if path == "binary.dat" {
            assert!(patch.patch.contains("Binary files"));
        }
    }
    for path in ["", "/etc/passwd", "../other", "x/../other", "bad\0path"] {
        assert_eq!(
            file(&root, &input(), pair, path, deadline())
                .await
                .unwrap_err()
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    for path in ["*.txt", ":(glob)*", "missing.txt"] {
        assert_eq!(
            file(&root, &input(), pair, path, deadline())
                .await
                .unwrap_err()
                .0,
            StatusCode::NOT_FOUND
        );
    }
    save(&root, "moved.txt", "new tip\n");
    assert_eq!(
        file(&root, &input(), pair, "binary.dat", deadline())
            .await
            .unwrap_err()
            .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn summaries_and_patches_stop_at_their_bounds() {
    let tree = Tree::new();
    let root = repository(&tree);
    command(&root, &["checkout", "-qb", "tools/change"]);
    for i in 0..=FILES {
        std::fs::write(root.join(format!("file-{i:04}.txt")), "one\n").unwrap();
    }
    std::fs::write(
        root.join("huge.txt"),
        "long changed line\n".repeat(BYTES / 8),
    )
    .unwrap();
    command(&root, &["add", "."]);
    command(&root, &["commit", "-qm", "large diff"]);
    let diff = summary(&root, &input(), deadline()).await;
    assert_eq!(diff.files.len(), FILES);
    assert!(diff.truncated);
    assert!(diff.reason.unwrap().contains("500 files"));
    let patch = file(
        &root,
        &input(),
        (diff.base.as_deref().unwrap(), diff.head.as_deref().unwrap()),
        "huge.txt",
        deadline(),
    )
    .await
    .unwrap();
    assert!(patch.truncated);
    assert!(patch.patch.len() <= BYTES);
    assert!(patch.patch.contains("+long changed line"));
}

#[test]
fn numstat_keeps_only_complete_records_and_reports_unreadable_paths() {
    let read = |bytes: &[u8], truncated| {
        files(&git::Output {
            bytes: bytes.to_vec(),
            truncated,
        })
    };
    assert_eq!(read(b"", false).unwrap(), (vec![], false));
    let (files, truncated) = read(b"2\t1\ta\tb\n.txt\0-\t-\tbinary\0partial", true).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "a\tb\n.txt");
    assert!(truncated);
    assert!(read(b"1\t0\tbad\xff\0", false).unwrap().1);
    for bytes in [b"bad\t0\tx\0".as_slice(), b"1\0", b"-\t0\tx\0", b"1\t0\t\0"] {
        assert!(read(bytes, false).is_err(), "{bytes:?}");
    }
    let (files, truncated) = read(&b"1\t0\tx\0".repeat(FILES + 1), false).unwrap();
    assert_eq!(files.len(), FILES);
    assert!(truncated);
}

#[tokio::test]
async fn renames_are_bounded_as_added_and_removed_paths_without_external_diff_tools() {
    let tree = Tree::new();
    let root = repository(&tree);
    command(&root, &["checkout", "-qb", "tools/change"]);
    command(&root, &["mv", "docs/TOPIC.md", "renamed.md"]);
    command(&root, &["commit", "-qm", "rename"]);
    command(&root, &["config", "diff.external", "false"]);
    command(&root, &["config", "diff.markdown.textconv", "false"]);
    std::fs::write(root.join(".gitattributes"), "*.md diff=markdown\n").unwrap();
    let diff = summary(&root, &input(), deadline()).await;
    assert_eq!(diff.source, TaskDiffSource::Branch);
    assert_eq!(diff.files.len(), 2);
    let deleted = diff
        .files
        .iter()
        .find(|f| f.path == "docs/TOPIC.md")
        .unwrap();
    assert_eq!(deleted.added, Some(0));
    assert!(deleted.removed.unwrap() > 0);
    let pair = (diff.base.as_deref().unwrap(), diff.head.as_deref().unwrap());
    for path in ["docs/TOPIC.md", "renamed.md"] {
        let patch = file(&root, &input(), pair, path, deadline()).await.unwrap();
        assert!(patch.patch.contains("initial-card"));
    }
}
