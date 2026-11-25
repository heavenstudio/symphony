//! [`super::saved_uploads`] and [`super::shown_uploads`]: which uploads an owner's save names, and
//! which of them the store holds as their names promise.

use std::collections::BTreeMap;

use sha2::Digest;

use super::{Edit, saved_uploads, shown_uploads};
use crate::uploads::{MAX_BYTES, ROUTE, Stored, store};

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
fn paths<T>(found: &BTreeMap<String, T>) -> Vec<&str> {
    found.keys().map(String::as_str).collect()
}

fn path(stored: &Stored) -> String {
    stored.url()[1..].to_string()
}

fn sha(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The note of an upload at `path` left out of the verdict, `why`.
fn left_out(path: &str, why: &str) -> String {
    format!("{path}: not committed with the verdict, which is saved without it: {why}")
}

/// A line the save added names an upload wherever it sits on it: a note's own line, the dated
/// line's words, a callout's body or title. Each comes with its path from the root and its stored
/// bytes.
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
    let (found, refused) = saved_uploads(&tree.root, &[edit(base, &new)]);
    let mut want: Vec<(String, Vec<u8>)> = [(&note, 1), (&dated, 2), (&body, 3), (&title, 4)]
        .into_iter()
        .map(|(s, n)| (path(s), png(n)))
        .collect();
    symphony_board::order::sort_by(&mut want, &Ord::cmp);
    assert_eq!(found.into_iter().collect::<Vec<_>>(), want);
    assert_eq!(refused, Vec::<String>::new());
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
        paths(&saved_uploads(&tree.root, &[edit(&base, &base)]).0),
        Vec::<&str>::new()
    );
    let mut want = vec![path(&again), path(&later)];
    symphony_board::order::sort_by(&mut want, &Ord::cmp);
    assert_eq!(
        paths(&saved_uploads(&tree.root, &[edit(&base, &first), edit(&first, &second)]).0),
        want
    );
}

/// Only what is an upload reference names one: a name an upload cannot have, a picture of a
/// document, a hash too short or an unknown kind name nothing; a plain link to a stored document
/// does. A reference to a file the store does not hold is left out with a note, once however often
/// it is named.
#[test]
fn only_a_reference_to_a_stored_file_names_an_upload() {
    let tree = crate::test_tree::Tree::new();
    let picture = store(&tree.root, "a.png", &png(1)).unwrap();
    let pdf = store(&tree.root, "paper.pdf", b"%PDF-1.7\n").unwrap();
    let short = &picture.sha[1..];
    let absent = "b".repeat(64);
    let lines = [
        format!("![a*b]({})", picture.url()),
        format!("![paper.pdf]({})", pdf.url()),
        format!("![a.png]({ROUTE}{short}.png)"),
        format!("![a.png]({ROUTE}{}.svg)", picture.sha),
        format!("![a.png]({ROUTE}{absent}.png)"),
        format!("[again]({ROUTE}{absent}.png)"),
        format!("`see` [paper.pdf]({})", pdf.url()),
    ];
    let new = lines.join("\n") + "\n";
    let (found, refused) = saved_uploads(&tree.root, &[edit("", &new)]);
    assert_eq!(paths(&found), [path(&pdf)]);
    assert_eq!(found[&path(&pdf)], b"%PDF-1.7\n");
    let missing = format!("artifacts/uploads/{absent}.png");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].starts_with(&left_out(&missing, "not in the store (")),
        "{refused:?}"
    );
}

/// Under another artifacts folder the file is stored there, and named by the path its link names.
#[test]
fn under_another_artifacts_folder_the_link_names_the_path() {
    let tree = crate::test_tree::Tree::new();
    tree.write("symphony.toml", "[project]\nartifacts = \"out\"\n");
    let stored = store(&tree.root, "a.png", &png(1)).unwrap();
    assert!(
        tree.path(&format!("out/uploads/{}.png", stored.sha))
            .is_file()
    );
    let (found, _) = saved_uploads(&tree.root, &[edit("", &(stored.markup() + "\n"))]);
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        [(path(&stored), png(1))]
    );
}

/// What the store holds under an upload's name is committed only when it is what the name
/// promises: a link there (to the very bytes, even), a folder, bytes that are not the ones the name
/// was made from, or a file over the cap is left out with a note naming why, and the rest of the
/// save's uploads still go in. A file at the cap whose bytes are its name's goes in.
#[test]
fn only_a_regular_file_whose_bytes_are_its_name_and_within_the_cap_is_committed() {
    let tree = crate::test_tree::Tree::new();
    let kept = store(&tree.root, "kept.png", &png(1)).unwrap();
    let dir = crate::uploads::dir(&tree.root);
    let name = |bytes: &[u8]| format!("{}.png", sha(bytes));
    // A link at a name, to a file outside the store holding exactly the bytes the name was made of.
    std::fs::create_dir_all(tree.path("elsewhere")).unwrap();
    let outside = tree.path("elsewhere/linked.png");
    std::fs::write(&outside, png(2)).unwrap();
    let linked_name = name(&png(2));
    std::os::unix::fs::symlink(&outside, dir.join(&linked_name)).unwrap();
    // A folder at a name.
    let folder = name(b"folder");
    std::fs::create_dir(dir.join(&folder)).unwrap();
    // Another upload's bytes planted under a name they were not made from.
    let planted = name(&png(3));
    std::fs::write(dir.join(&planted), png(4)).unwrap();
    // One byte over the cap, named by its own bytes; and exactly at the cap.
    let big = vec![b'x'; MAX_BYTES + 1];
    let over = name(&big);
    std::fs::write(dir.join(&over), &big).unwrap();
    let full = vec![b'y'; MAX_BYTES];
    let at_cap = name(&full);
    std::fs::write(dir.join(&at_cap), &full).unwrap();

    let lines: Vec<String> = [&linked_name, &folder, &planted, &over, &at_cap]
        .iter()
        .map(|file| format!("![shot]({ROUTE}{file})"))
        .chain([kept.markup()])
        .collect();
    let (found, refused) = saved_uploads(&tree.root, &[edit("", &(lines.join("\n") + "\n"))]);
    let at = |file: &str| format!("artifacts/uploads/{file}");
    let mut committed = vec![path(&kept), at(&at_cap)];
    symphony_board::order::sort_by(&mut committed, &Ord::cmp);
    assert_eq!(paths(&found), committed);
    assert_eq!(found[&at(&at_cap)].len(), MAX_BYTES);
    let not_the_upload = "the store holds a link or another kind of file there, not the upload";
    assert_eq!(
        refused,
        [
            left_out(&at(&linked_name), not_the_upload),
            left_out(&at(&folder), not_the_upload),
            left_out(
                &at(&planted),
                "its bytes are not the ones its name was made from"
            ),
            left_out(
                &at(&over),
                &format!(
                    "{} bytes, over the {MAX_BYTES} an upload holds",
                    MAX_BYTES + 1
                )
            ),
        ]
    );
}

/// What the commit shows is what a text still names, by the link's whole path.
#[test]
fn the_commit_shows_the_uploads_a_text_still_names() {
    let uploads: BTreeMap<String, Vec<u8>> = ["artifacts/uploads/a.png", "artifacts/uploads/b.png"]
        .into_iter()
        .map(|p| (p.to_string(), p.as_bytes().to_vec()))
        .collect();
    let shown = shown_uploads(
        &uploads,
        &[
            "![a](/artifacts/uploads/a.png)",
            "no link to artifacts/uploads/b.png",
        ],
    );
    assert_eq!(paths(&shown), ["artifacts/uploads/a.png"]);
    assert_eq!(shown["artifacts/uploads/a.png"], b"artifacts/uploads/a.png");
    assert_eq!(paths(&shown_uploads(&uploads, &[])), Vec::<&str>::new());
}

#[test]
fn shown_uploads_clone_only_named_values_once_and_keep_key_order() {
    use std::cell::Cell;
    use std::rc::Rc;
    struct Counted {
        value: u8,
        calls: Rc<Cell<usize>>,
    }
    impl Clone for Counted {
        fn clone(&self) -> Self {
            self.calls.set(self.calls.get() + 1);
            Self {
                value: self.value,
                calls: self.calls.clone(),
            }
        }
    }
    let calls = Rc::new(Cell::new(0));
    let uploads: BTreeMap<_, _> = [("z", 3), ("m", 2), ("a", 1)]
        .into_iter()
        .map(|(name, value)| {
            (
                format!("artifacts/uploads/{name}.png"),
                Counted {
                    value,
                    calls: calls.clone(),
                },
            )
        })
        .collect();
    let shown = shown_uploads(
        &uploads,
        &[
            "/artifacts/uploads/m.png /artifacts/uploads/a.png /artifacts/uploads/a.png",
            "artifacts/uploads/z.png has no leading slash",
        ],
    );
    assert_eq!(
        paths(&shown),
        ["artifacts/uploads/a.png", "artifacts/uploads/m.png"]
    );
    assert_eq!(shown.values().map(|v| v.value).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(calls.get(), 2);
    assert!(shown_uploads(&uploads, &[]).is_empty());
    assert!(shown_uploads::<Counted>(&BTreeMap::new(), &["/artifacts/uploads/a.png"]).is_empty());
    assert_eq!(calls.get(), 2);
}
