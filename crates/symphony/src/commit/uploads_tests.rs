//! [`super::saved_uploads`] and [`super::shown_uploads`]: which uploads an owner's save names.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Edit, saved_uploads, shown_uploads};
use crate::uploads::{Stored, store};

fn png(n: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.push(n);
    bytes
}

fn edit(base: &str, new: &str) -> Edit {
    Edit {
        rel: "docs/DECISIONS.md".into(),
        base: base.into(),
        new: new.into(),
    }
}

/// The paths `found` names, from the repository root.
fn paths(found: &BTreeMap<String, PathBuf>) -> Vec<&str> {
    found.keys().map(String::as_str).collect()
}

fn path(stored: &Stored) -> String {
    stored.url()[1..].to_string()
}

fn stored_at(root: &Path, stored: &Stored) -> PathBuf {
    crate::uploads::dir(root).join(format!("{}.{}", stored.sha, stored.kind.ext))
}

/// A line the save added names an upload wherever it sits on it: a note's own line, the dated
/// line's words, a callout's body or title. Each comes with its path from the root and its stored
/// file.
#[test]
fn every_line_the_owner_added_names_its_uploads_wherever_they_sit() {
    let tree = crate::test_tree::Tree::new();
    let [note, dated, body, title] =
        [1, 2, 3, 4].map(|n| store(&tree.root, "a.png", &png(n)).unwrap());
    let base = "- [ ] **one** — proposal.\n";
    let new = format!(
        "{base}  [2026-11-12 18:00] owner · approved — {}\n    words\n    {}\n\n> [!comment] {}\n> {}\n",
        dated.markup(),
        note.markup(),
        title.markup(),
        body.markup()
    );
    let found = saved_uploads(&tree.root, &[edit(base, &new)]);
    let mut want: Vec<(String, PathBuf)> = [&note, &dated, &body, &title]
        .into_iter()
        .map(|s| (path(s), stored_at(&tree.root, s)))
        .collect();
    want.sort();
    assert_eq!(found.into_iter().collect::<Vec<_>>(), want);
    assert!(
        want.iter()
            .all(|(p, _)| p.starts_with("artifacts/uploads/"))
    );
}

/// A line the base held is not the owner's new line, unless the save added another copy of it; a
/// later edit's base is the earlier edit's text, and each edit's new lines count.
#[test]
fn a_line_the_base_held_names_nothing_new_but_a_second_copy_does() {
    let tree = crate::test_tree::Tree::new();
    let [old, again, later] = [1, 2, 3].map(|n| store(&tree.root, "a.png", &png(n)).unwrap());
    let line = |s: &Stored| format!("    {}\n", s.markup());
    let base = format!("card\n{}{}", line(&old), line(&again));
    let first = format!("{base}{}", line(&again));
    let second = format!("{first}{}", line(&later));
    assert_eq!(
        paths(&saved_uploads(&tree.root, &[edit(&base, &base)])),
        Vec::<&str>::new()
    );
    let mut want = vec![path(&again), path(&later)];
    want.sort();
    assert_eq!(
        paths(&saved_uploads(
            &tree.root,
            &[edit(&base, &first), edit(&first, &second)]
        )),
        want
    );
}

/// Only what is an upload reference, of a file the store holds, is named: a name an upload cannot
/// have, a picture of a document, a hash too short, an unknown kind or a file not stored name
/// nothing; a plain link to a stored document does.
#[test]
fn only_a_reference_to_a_stored_file_names_an_upload() {
    let tree = crate::test_tree::Tree::new();
    let picture = store(&tree.root, "a.png", &png(1)).unwrap();
    let pdf = store(&tree.root, "paper.pdf", b"%PDF-1.7\n").unwrap();
    let route = crate::uploads::ROUTE;
    let short = &picture.sha[1..];
    let absent = "b".repeat(64);
    let lines = [
        format!("![a*b]({})", picture.url()),
        format!("![paper.pdf]({})", pdf.url()),
        format!("![a.png]({route}{short}.png)"),
        format!("![a.png]({route}{}.svg)", picture.sha),
        format!("![a.png]({route}{absent}.png)"),
        format!("`see` [paper.pdf]({})", pdf.url()),
    ];
    let new = lines.join("\n") + "\n";
    let found = saved_uploads(&tree.root, &[edit("", &new)]);
    assert_eq!(paths(&found), [path(&pdf)]);
    assert_eq!(found[&path(&pdf)], stored_at(&tree.root, &pdf));
}

/// Under another artifacts folder the file is stored there, and named by the path its link names.
#[test]
fn under_another_artifacts_folder_the_link_names_the_path() {
    let tree = crate::test_tree::Tree::new();
    tree.write("symphony.toml", "[project]\nartifacts = \"out\"\n");
    let stored = store(&tree.root, "a.png", &png(1)).unwrap();
    let file = tree.path(&format!("out/uploads/{}.png", stored.sha));
    assert!(file.is_file());
    let found = saved_uploads(&tree.root, &[edit("", &(stored.markup() + "\n"))]);
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        [(path(&stored), file)]
    );
}

/// What the commit shows is what a text still names, by the link's whole path.
#[test]
fn the_commit_shows_the_uploads_a_text_still_names() {
    let uploads: BTreeMap<String, PathBuf> = ["artifacts/uploads/a.png", "artifacts/uploads/b.png"]
        .into_iter()
        .map(|p| (p.to_string(), PathBuf::from(p)))
        .collect();
    let shown = shown_uploads(
        &uploads,
        &[
            "![a](/artifacts/uploads/a.png)",
            "no link to artifacts/uploads/b.png",
        ],
    );
    assert_eq!(paths(&shown), ["artifacts/uploads/a.png"]);
    assert_eq!(paths(&shown_uploads(&uploads, &[])), Vec::<&str>::new());
}
