//! `symphony attach` on a scratch repository: a main checkout and a linked worktree of it, made by
//! git, and a folder in none.

use super::*;
use crate::test_layouts::repository;
use crate::test_tree::Tree;
use sha2::{Digest, Sha256};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03";
const PDF: &[u8] = b"%PDF-1.7\n";

/// `bytes` written at `path`, its folders made: the path.
fn file(path: PathBuf, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The name `bytes` are stored under: their SHA-256 and the extension.
fn stored_as(bytes: &[u8], ext: &str) -> String {
    let sha: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{sha}.{ext}")
}

/// What `attach` said from `here`: its exit code, stdout and stderr.
fn attached(here: &Path, files: &[PathBuf]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = attach(here, files, &mut out, &mut err);
    let text = |bytes| String::from_utf8(bytes).unwrap();
    (code, text(out), text(err))
}

/// The files in `root`'s uploads, sorted.
fn store(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(uploads::dir(root)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    symphony_board::order::sort_by(&mut names, &Ord::cmp);
    names
}

#[test]
fn a_file_attached_in_the_main_checkout_is_stored_there_and_its_line_printed() {
    let tree = Tree::new();
    let (main, _) = repository(&tree.root);
    let shot = file(main.join("artifacts/some-task/board.png"), PNG);
    let (code, out, err) = attached(&main, std::slice::from_ref(&shot));
    let name = stored_as(PNG, "png");
    assert_eq!(
        (code, out, err),
        (
            0,
            format!("![board.png](/artifacts/uploads/{name})\n"),
            String::new()
        )
    );
    assert_eq!(store(&main), std::slice::from_ref(&name));
    assert_eq!(
        std::fs::read(uploads::dir(&main).join(&name)).unwrap(),
        PNG,
        "the file's own bytes"
    );
    assert!(shot.is_file(), "the file attached stays where it was");
    // The line is the one an upload's reference reads back, as the thread draws it.
    let line = format!("![board.png](/artifacts/uploads/{name})");
    assert_eq!(
        uploads::reference(&line).map(|found| found.url),
        Some(format!("/artifacts/uploads/{name}"))
    );
}

#[test]
fn a_file_attached_in_a_linked_worktree_is_stored_in_its_main_checkout_from_any_folder_in_it() {
    let tree = Tree::new();
    let (main, linked) = repository(&tree.root);
    let shot = file(linked.join("artifacts/some-task/after.png"), PNG);
    let paper = file(linked.join("notes/measure.pdf"), PDF);
    // From a folder below the worktree's top, one file by its path from there.
    let (code, out, err) = attached(
        &linked.join("docs"),
        &[shot, PathBuf::from("../notes/measure.pdf")],
    );
    assert_eq!(
        (code, out, err),
        (
            0,
            format!(
                "![after.png](/artifacts/uploads/{})\n[measure.pdf](/artifacts/uploads/{})\n",
                stored_as(PNG, "png"),
                stored_as(PDF, "pdf")
            ),
            String::new()
        )
    );
    assert!(paper.is_file());
    let mut both = vec![stored_as(PNG, "png"), stored_as(PDF, "pdf")];
    symphony_board::order::sort_by(&mut both, &Ord::cmp);
    assert_eq!(store(&main), both, "the main checkout's store");
    assert_eq!(store(&linked), Vec::<String>::new(), "none in the worktree");
}

#[test]
fn the_same_bytes_attached_twice_are_stored_once_and_each_named_as_given() {
    let tree = Tree::new();
    let (main, linked) = repository(&tree.root);
    let first = file(main.join("a.png"), PNG);
    let again = file(linked.join("artifacts/t/b c.png"), PNG);
    let (code, out, err) = attached(&linked, &[first, again]);
    let name = stored_as(PNG, "png");
    assert_eq!(
        (code, out, err),
        (
            0,
            format!("![a.png](/artifacts/uploads/{name})\n![b c.png](/artifacts/uploads/{name})\n"),
            String::new()
        )
    );
    assert_eq!(store(&main), [name]);
}

#[test]
fn a_file_the_uploads_refuse_is_named_with_why_and_the_rest_are_still_attached() {
    let tree = Tree::new();
    let (main, linked) = repository(&tree.root);
    let svg = file(
        linked.join("artifacts/t/chart.svg"),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    );
    let page = file(linked.join("artifacts/t/page.txt"), b"<html><body>x</body>");
    let empty = file(linked.join("artifacts/t/empty.png"), b"");
    let large = linked.join("artifacts/t/full.png");
    // Sparse: its size alone refuses it, so the test writes and the command reads none of it.
    std::fs::File::create(&large)
        .unwrap()
        .set_len(MAX_BYTES as u64 + 1)
        .unwrap();
    let missing = linked.join("artifacts/t/gone.png");
    let folder = linked.join("artifacts/t");
    let good = file(linked.join("artifacts/t/good.png"), PNG);
    let files = [
        svg.clone(),
        good,
        page.clone(),
        empty.clone(),
        large.clone(),
        missing.clone(),
        folder.clone(),
    ];
    let (code, out, err) = attached(&linked, &files);
    assert_eq!(code, 1, "{err}");
    assert_eq!(
        out,
        format!(
            "![good.png](/artifacts/uploads/{})\n",
            stored_as(PNG, "png")
        )
    );
    let said = |path: &Path, why: &str| format!("symphony attach: {}: {why}", path.display());
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(
        lines,
        [
            said(
                &svg,
                "chart.svg is not a picture (PNG, JPEG, GIF, WebP) or a document (PDF, text, \
                 Word, Excel, PowerPoint)"
            ),
            said(
                &page,
                "page.txt holds markup or a script, which is never stored"
            ),
            said(&empty, "the file is empty"),
            said(&large, "the file is 10.0 MB; uploads are at most 10 MB"),
            said(&missing, "No such file or directory (os error 2)"),
            said(&folder, "not a file"),
        ]
    );
    assert_eq!(store(&main), [stored_as(PNG, "png")], "only the good one");
    for refused in [&svg, &page, &empty, &large] {
        assert!(refused.is_file(), "{} stays", refused.display());
    }
}

#[test]
fn a_folder_in_no_git_checkout_has_no_store_and_nothing_is_written() {
    let tree = Tree::new();
    let shot = file(tree.path("loose/a.png"), PNG);
    let (code, out, err) = attached(&tree.path("loose"), &[shot]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        format!(
            "symphony attach: {} is in no git checkout, so there is no main checkout to store \
             in\n",
            tree.path("loose").display()
        )
    );
    assert!(!tree.path("loose/artifacts").exists());
}

#[test]
fn no_file_is_a_command_line_it_cannot_run_and_help_says_what_it_does() {
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(&args, &mut out, &mut err);
        let text = |bytes| String::from_utf8(bytes).unwrap();
        (code, text(out), text(err))
    };
    assert_eq!(
        run(&[]),
        (
            2,
            String::new(),
            format!(
                "{USAGE}\nsymphony attach: error: the following arguments are required: FILE\n"
            )
        )
    );
    let (code, out, err) = run(&["--frobnicate", "a.png"]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(
        err.ends_with("unrecognized arguments: --frobnicate\n"),
        "{err}"
    );
    for args in [&["--help"][..], &["-h"], &["a.png", "--help"]] {
        let (code, out, err) = run(args);
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
        assert!(out.starts_with(&format!("{USAGE}\n\n")), "{out}");
        assert!(out.contains("main checkout"), "{out}");
    }
}
