//! The docs audit refuses a card whose pictures do not load: any missing picture of a card the
//! checkout's branch wrote, and a missing picture in the tracked folders of any card still open.
//! `/artifacts/` is each checkout's own, so main's cards missing one there only warn.

use super::{DOC, branch, commit, register, repository};

/// A card posted at 09:00 by an agent line showing `picture`, with `lines` under it.
fn card(slug: &str, mark: char, picture: &str, lines: &str) -> String {
    format!(
        "- [{mark}] **{slug}** — Proposal {slug}.\n  [2026-11-11 09:00] agent — Drawn: {picture}\n{lines}"
    )
}

fn refused(line: usize, slug: &str, missing: &str) -> String {
    format!(
        "{DOC}:{line}: card **{slug}** shows a picture that does not load: {missing} is not on disk; put it there before posting the card"
    )
}

/// The audit's errors about cards' pictures, and whether it warned of `/artifacts/` files.
fn pictures(root: &std::path::Path) -> (Vec<String>, bool) {
    let result = symphony::docs_audit::audit(root);
    let errors = result
        .errors
        .into_iter()
        .filter(|e| e.contains("shows a picture that does not load"))
        .collect();
    let warned = result
        .warnings
        .iter()
        .any(|w| w.contains("not on this machine"));
    (errors, warned)
}

pub(super) fn a_card_whose_pictures_do_not_load_is_refused_at_posting() {
    let tree = repository();
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
    commit(&tree.root, "main's cards", 1);

    // On main the cards are main's own: a tracked picture missing is refused, one in /artifacts/
    // is this machine's and only warned of; a closed card, the owner's attachment and the owner's
    // own lines, a picture in code and a plain link ask nothing.
    let (errors, warned) = pictures(&tree.root);
    assert_eq!(errors, [refused(6, "tracked", "gone.png")]);
    assert!(warned);

    // A branch's own card is refused for any picture not on disk, /artifacts/ too; main's cards it
    // inherited are judged as main's.
    let wt = branch(&tree, "tools/drawn", "drawn");
    let own = card(
        "own",
        ' ',
        "![G](/artifacts/own.png) ![H](/artifacts/own-2.png)",
        "",
    );
    std::fs::write(wt.join(DOC), register(&(main.clone() + &own))).unwrap();
    let (errors, _) = pictures(&wt);
    assert_eq!(
        errors,
        [
            refused(6, "tracked", "gone.png"),
            refused(15, "own", "/artifacts/own.png and 1 more"),
        ]
    );
    // Committed, it is still the branch's own.
    commit(&wt, "post own", 2);
    assert_eq!(pictures(&wt).0.len(), 2);

    // With its pictures on disk the card posts; so does main's once its picture is tracked.
    tree.write("drawn/artifacts/own.png", "png");
    tree.write("drawn/artifacts/own-2.png", "png");
    tree.write("drawn/docs/gone.png", "png");
    assert_eq!(pictures(&wt).0, Vec::<String>::new());

    // An inherited card the branch revised is its own too.
    let revised = main.replace("Proposal inherited.", "Revised inherited.");
    std::fs::write(wt.join(DOC), register(&(revised + &own))).unwrap();
    assert_eq!(
        pictures(&wt).0,
        [refused(4, "inherited", "/artifacts/inherited.png")]
    );
}
