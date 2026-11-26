//! An owner's save commits the uploads its new line names, at the path the link names from the
//! repository root (`artifacts/uploads/<sha256>.<ext>`), in the verdict's own commit, on main or on
//! a generated verdict branch; nothing else of the checkout, and never an ask's upload.
use super::*;

/// A picture the store takes, distinct by `n`.
fn png(n: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.push(n);
    bytes
}

/// The path an upload's link names from the repository root.
fn linked(stored: &symphony::uploads::Stored) -> String {
    stored.url()[1..].to_string()
}

/// The files `commit` changed, sorted.
fn changed(root: &Path, commit: &str) -> Vec<String> {
    let mut names: Vec<String> = git(
        root,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", commit],
    )
    .lines()
    .map(str::to_owned)
    .collect();
    symphony_board::order::sort_by(&mut names, &Ord::cmp);
    names
}

/// Whether main holds `path` as the bytes of the file at `file`.
fn committed_as(root: &Path, path: &str, file: &Path) -> bool {
    git(root, &["rev-parse", &format!("main:{path}")])
        == git(root, &["hash-object", file.to_str().unwrap()])
}

pub(super) fn an_owner_save_commits_the_uploads_its_new_line_names(f: &Fixture) {
    let root = f.repo();
    let store = |name: &str, n: u8| symphony::uploads::store(&root, name, &png(n)).unwrap();
    let (verdict, message, comment, asked, unnamed) = (
        store("verdict.png", 1),
        store("message.png", 2),
        store("comment.png", 3),
        store("asked.png", 4),
        store("unnamed.png", 5),
    );
    // Someone else's staged work and an untracked file beside the store stay as they are.
    write(&root.join("other"), "someone else's staged work\n");
    git(&root, &["add", "other"]);
    let staged = git(&root, &["ls-files", "-s", "other"]);
    write(&root.join("docs/draft.png"), "not the owner's\n");
    let base = git(&root, &["rev-parse", "main"]);

    let saves = ClosingSaves::start(&root);
    saves.post(
        "",
        "/api/decision",
        json!({"path":"docs/a.md", "slug":"one", "state":"approved",
            "note": format!("use this shape\n{}", verdict.markup())}),
    );
    saves.post(
        "",
        "/api/message",
        json!({"path":"docs/a.md", "slug":"one", "text": format!("and this\n{}", message.markup())}),
    );
    saves.post(
        "",
        "/api/comment",
        json!({"path":"docs/a.md", "after_line":1, "kind":"comment", "title":"Like this",
            "body": comment.markup()}),
    );
    saves.post(
        "",
        "/api/ask",
        json!({"path":"docs/a.md", "question": format!("Why this?\n{}", asked.markup()),
            "anchor":"one", "quote":"proposal."}),
    );
    saves.0.close();

    // Each save is one commit: its document and the upload its line names, nothing else.
    let commits: Vec<String> = git(&root, &["rev-list", "--reverse", &format!("{base}..main")])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(commits.len(), 3, "{commits:?}");
    for (commit, stored) in commits.iter().zip([&verdict, &message, &comment]) {
        assert_eq!(
            changed(&root, commit),
            [linked(stored), "docs/a.md".to_string()]
        );
    }
    let text = f.main("docs/a.md");
    for stored in [&verdict, &message, &comment] {
        assert!(text.contains(&stored.markup()), "{text}");
        let file = root.join(linked(stored));
        assert!(committed_as(&root, &linked(stored), &file));
    }
    // The ask's upload and the one nothing names stay in the store alone.
    let tracked = git(&root, &["ls-tree", "-r", "--name-only", "main"]);
    for stored in [&asked, &unnamed] {
        assert!(root.join(linked(stored)).is_file());
        assert!(!tracked.contains(&stored.sha), "{tracked}");
    }
    assert!(!tracked.contains("draft.png"));
    // The checkout took each commit: only the other work is left, as it was.
    assert_eq!(f.main("other"), "original\n");
    assert_eq!(git(&root, &["ls-files", "-s", "other"]), staged);
    assert_eq!(
        git(&root, &["status", "--porcelain"]),
        "M  other\n?? docs/draft.png"
    );
    f.clean_workers();
}

pub(super) fn under_another_artifacts_folder_the_upload_is_committed_where_its_link_points(
    f: &Fixture,
) {
    let root = f.repo();
    write(
        &root.join(symphony::project::FILE),
        &format!("[project]\nartifacts = \"out\"\n\n{VERDICTS}"),
    );
    write(&root.join(".gitignore"), "/out/\n");
    git(&root, &["commit", "-qam", "the artifacts folder is out"]);
    let stored = symphony::uploads::store(&root, "sketch.png", &png(1)).unwrap();
    let file = root.join(format!("out/uploads/{}.png", stored.sha));
    assert!(file.is_file());
    let saved = symphony::edit::set_decision(
        DOC,
        "one",
        "approved",
        &format!("like this\n{}", stored.markup()),
        "2026-11-12 18:00",
        &common::plain(),
    )
    .unwrap();
    let outcome = commit_verdicts(
        &root,
        &[f.edit("docs/a.md", DOC, &saved)],
        "docs: owner verdict",
    );
    assert!(
        outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
        "{} {:?}",
        outcome.error,
        outcome.notes
    );
    assert_eq!(
        changed(&root, "main"),
        [linked(&stored), "docs/a.md".to_string()]
    );
    assert!(committed_as(&root, &linked(&stored), &file));
    // The checkout has the committed file where the link points, and is clean.
    assert_eq!(std::fs::read(root.join(linked(&stored))).unwrap(), png(1));
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    f.clean_workers();
}

/// What the store holds under an upload's name goes into the verdict only when it is that upload:
/// a link there (to a file elsewhere in the ignored folder) and bytes planted under another
/// upload's name are left out, each with a note, and the verdict is committed with the upload that
/// is what its name says. The checkout stays clean.
pub(super) fn a_store_file_that_is_not_its_upload_is_left_out_and_the_verdict_saved(f: &Fixture) {
    let root = f.repo();
    let store = |name: &str, n: u8| symphony::uploads::store(&root, name, &png(n)).unwrap();
    let (kept, linked_one, planted) = (
        store("kept.png", 1),
        store("linked.png", 2),
        store("planted.png", 3),
    );
    // The link's target holds the very bytes its name was made from: a link is refused as such.
    let elsewhere = root.join("artifacts/elsewhere.png");
    write(&elsewhere, "");
    std::fs::write(&elsewhere, png(2)).unwrap();
    let link = root.join(linked(&linked_one));
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
    std::fs::write(root.join(linked(&planted)), png(4)).unwrap();
    let note = format!(
        "like these\n{}\n{}\n{}",
        kept.markup(),
        linked_one.markup(),
        planted.markup()
    );
    let saved = symphony::edit::set_decision(
        DOC,
        "one",
        "approved",
        &note,
        "2026-11-12 18:00",
        &common::plain(),
    )
    .unwrap();
    let outcome = commit_verdicts(
        &root,
        &[f.edit("docs/a.md", DOC, &saved)],
        "docs: owner verdict",
    );
    assert!(
        outcome.committed && outcome.error.is_empty(),
        "{}",
        outcome.error
    );
    let left_out = |stored: &symphony::uploads::Stored, why: &str| {
        format!(
            "{}: not committed with the verdict, which is saved without it: {why}",
            linked(stored)
        )
    };
    assert_eq!(
        outcome.notes,
        [
            left_out(
                &linked_one,
                "the store holds a link or another kind of file there, not the upload"
            ),
            left_out(
                &planted,
                "its bytes are not the ones its name was made from"
            ),
        ]
    );
    assert_eq!(f.main("docs/a.md"), saved);
    assert_eq!(
        changed(&root, "main"),
        [linked(&kept), "docs/a.md".to_string()]
    );
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    f.clean_workers();
}

pub(super) fn a_generated_verdict_branch_carries_the_uploads_its_line_names(f: &Fixture) {
    use symphony::edit::set_decision;
    let root = f.repo();
    f.generate();
    let stored = symphony::uploads::store(&root, "sketch.png", &png(1)).unwrap();
    let asked = symphony::uploads::store(&root, "asked.png", &png(2)).unwrap();
    // Bytes planted under an upload's name the line names too: left out of the branch.
    let planted = symphony::uploads::store(&root, "planted.png", &png(3)).unwrap();
    std::fs::write(root.join(linked(&planted)), png(4)).unwrap();
    let saved = set_decision(
        DOC,
        "one",
        "approved",
        &format!("like this\n{}\n{}", stored.markup(), planted.markup()),
        "2026-11-12 18:00",
        &common::plain(),
    )
    .unwrap();
    let committer = ClosingCommitter::start(&root);
    committer
        .0
        .submit(f.edit("docs/a.md", DOC, &saved))
        .unwrap();
    committer.0.close();
    assert_eq!(f.main("docs/a.md"), saved);
    assert_eq!(f.main("content/one"), "approved\n");
    assert_eq!(
        changed(&root, "main"),
        [
            linked(&stored),
            "content/one".to_string(),
            "docs/a.md".to_string(),
            "view/lamp_atlas".to_string()
        ]
    );
    assert!(committed_as(
        &root,
        &linked(&stored),
        &root.join(linked(&stored))
    ));
    let tracked = git(&root, &["ls-tree", "-r", "--name-only", "main"]);
    assert!(
        !tracked.contains(&asked.sha),
        "an upload the line does not name stays out"
    );
    assert!(
        !tracked.contains(&planted.sha),
        "bytes that are not their name's stay out"
    );
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    // The upload left out is said where a verdict's trouble is: on the board, once.
    let filed: Vec<String> = f
        .store()
        .tasks(None)
        .unwrap()
        .into_iter()
        .filter(|task| task.slug.starts_with("tools/review-app-verdict-"))
        .map(|task| task.body)
        .collect();
    assert_eq!(
        filed,
        [format!(
            "Symphony: {}: not committed with the verdict, which is saved without it: its bytes are not the ones its name was made from",
            linked(&planted)
        )]
    );
    f.clean_publication_state();
}
