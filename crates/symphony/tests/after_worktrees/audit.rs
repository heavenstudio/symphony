//! The docs audit refuses a card whose pictures do not load, judged on what the checkout wrote: the
//! text and lines of an open card that its fork from main did not hold. A branch answering a card of
//! main's is never refused for main's pictures, which `/artifacts/` (each checkout's own, ignored by
//! git) does not hold in a new worktree.

use super::{DOC, branch, commit, register, repository};

/// A card posted at 09:00 by an agent line showing `picture`, with `lines` under it.
fn card(slug: &str, mark: char, picture: &str, lines: &str) -> String {
    format!(
        "- [{mark}] **{slug}** — Proposal {slug}.\n  [2026-11-11 09:00] agent — Drawn: {picture}\n{lines}"
    )
}

fn refused(line: usize, slug: &str, missing: &str) -> String {
    format!(
        "{DOC}:{line}: card **{slug}** shows a picture this checkout wrote that does not load: {missing} is not on disk; add the file, or take the picture out, before committing"
    )
}

/// The audit's errors about cards' pictures, and whether it warned of `/artifacts/` files.
fn pictures(root: &std::path::Path) -> (Vec<String>, bool) {
    let result = symphony::docs_audit::audit(root);
    let errors = result
        .errors
        .into_iter()
        .filter(|e| e.contains("that does not load"))
        .collect();
    let warned = result
        .warnings
        .iter()
        .any(|w| w.contains("not on this machine"));
    (errors, warned)
}

pub(super) fn a_card_whose_pictures_do_not_load_is_refused_at_posting() {
    let tree = repository();
    tree.write(".gitignore", "/artifacts/\n");
    // Lines 4 to 14 of the register, two to a card but `uploaded`'s three.
    let main = card("inherited", ' ', "![A](/artifacts/inherited.png)", "")
        + &card("tracked", ' ', "![B](gone.png) ![C](kept.png)", "")
        + &card("settled", 'x', "![D](gone.png)", "")
        + &card(
            "uploaded",
            ' ',
            "![E](/artifacts/uploads/sketch.png)",
            "  [2026-11-11 10:00] owner · question — like ![this](/elsewhere/owner.png)?\n",
        )
        + &card("coded", ' ', "`![F](gone.png)` and [a link](gone.png)", "");
    tree.write(DOC, &register(&main));
    tree.write("docs/kept.png", "png");
    tree.write("artifacts/inherited.png", "png");
    commit(&tree.root, "main's cards", 1);

    // What main posted was judged when it was posted.
    assert_eq!(pictures(&tree.root).0, Vec::<String>::new());
    // A card main is writing is judged: its missing tracked picture is refused. A closed card, the
    // owner's attachment and the owner's own lines, a picture in code and a plain link ask nothing.
    let fresh = card("fresh", ' ', "![G](gone.png)", "");
    tree.write(DOC, &register(&(main.clone() + &fresh)));
    assert_eq!(pictures(&tree.root).0, [refused(15, "fresh", "gone.png")]);
    tree.write(DOC, &register(&main));

    // A branch answers main's card, whose picture only main's `/artifacts/` holds, and posts a card
    // of its own: only its own card's pictures are its to put on disk.
    let wt = branch(&tree, "tools/drawn", "drawn");
    assert!(!wt.join("artifacts/inherited.png").exists());
    let answered = main.replace(
        "Drawn: ![A](/artifacts/inherited.png)\n",
        "Drawn: ![A](/artifacts/inherited.png)\n  [2026-11-11 10:00] agent — Answered.\n",
    );
    let own = card(
        "own",
        ' ',
        "![H](/artifacts/own.png) ![I](/artifacts/own-2.png)",
        "",
    );
    std::fs::write(wt.join(DOC), register(&(answered.clone() + &own))).unwrap();
    let (errors, warned) = pictures(&wt);
    assert_eq!(
        errors,
        [refused(16, "own", "/artifacts/own.png and 1 more")]
    );
    assert!(warned, "main's missing picture is only warned of");
    // Committed, it is still the branch's own.
    commit(&wt, "post own", 2);
    assert_eq!(pictures(&wt).0.len(), 1);

    // With its pictures on disk the card posts.
    tree.write("drawn/artifacts/own.png", "png");
    tree.write("drawn/artifacts/own-2.png", "png");
    assert_eq!(pictures(&wt).0, Vec::<String>::new());

    // An answer showing a picture not on disk is the branch's own, on main's card too; so is a
    // revised text.
    let drawn = answered
        .replace("Answered.", "Answered: ![J](/artifacts/answer.png)")
        .replace(
            "Proposal tracked.",
            "Revised tracked: ![K](/artifacts/revised.png)",
        );
    std::fs::write(wt.join(DOC), register(&(drawn + &own))).unwrap();
    assert_eq!(
        pictures(&wt).0,
        [
            refused(4, "inherited", "/artifacts/answer.png"),
            refused(7, "tracked", "/artifacts/revised.png"),
        ]
    );
}
