//! The docs audit refuses a card whose pictures do not load, judged on what the checkout wrote: the
//! text and lines of an open card that its fork from main did not hold. A branch answering a card of
//! main's is never refused for main's pictures, which `/artifacts/` (each checkout's own, ignored by
//! git) does not hold in a new worktree. What the checkout wrote is refused too when git does not
//! track its picture, as a clone shows only what git tracks; what a clone lacks elsewhere is warned
//! of.

use super::{DOC, branch, command, commit, register, repository};

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

/// An owner's upload as the store names it.
const UPLOAD: &str =
    "/artifacts/uploads/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";

/// The audit's refusals of pictures git does not track, and its warnings of what a clone lacks.
fn unclonable(root: &std::path::Path) -> (Vec<String>, Vec<String>) {
    let result = symphony::docs_audit::audit(root);
    let errors = result
        .errors
        .into_iter()
        .filter(|e| e.contains("git does not track"))
        .collect();
    let warnings = result
        .warnings
        .into_iter()
        .filter(|w| w.contains("not in a clone"))
        .collect();
    (errors, warnings)
}

/// The errors about a card's pictures, whether they load or git tracks them.
fn shown_pictures(errors: &[String]) -> Vec<&String> {
    errors
        .iter()
        .filter(|e| e.contains("shows a picture"))
        .collect()
}

fn untracked(line: usize, slug: &str, shown: &str) -> String {
    format!(
        "{DOC}:{line}: card **{slug}** shows a picture this checkout wrote that git does not track: {shown}; commit it beside its document, as a clone shows only what git tracks (an upload is for a thread, never a card)"
    )
}

fn not_in_a_clone(rel: &str, line: usize, shown: &str) -> String {
    format!("{rel}:{line}: {shown}: not in a clone, as git does not track it")
}

pub(super) fn a_cards_picture_is_a_file_git_tracks() {
    let tree = repository();
    tree.write(".gitignore", "/artifacts/\n");
    // Lines 4 to 7: main's card, an agent's picture git does not track, the owner's upload.
    let main = card(
        "older",
        ' ',
        "![A](/artifacts/older.png)",
        &format!("  [2026-11-11 10:00] owner · comment — like this\n    ![sketch.png]({UPLOAD})\n"),
    );
    tree.write(DOC, &register(&main));
    tree.write("artifacts/older.png", "png");
    tree.write(&UPLOAD[1..], "png");
    // A document showing a picture in the ignored folder, linking a file this machine lacks, and an
    // owner's callout naming an upload; a history folder holds the same and is passed over.
    let guide = format!(
        "# Guide\n\n![shot](../artifacts/shot.png)\n\nSee [the log](/artifacts/gone.log).\n\n> [!comment] Like this\n> ![sketch.png]({UPLOAD})\n"
    );
    tree.write("docs/guide.md", &guide);
    tree.write("artifacts/shot.png", "png");
    tree.write(
        "docs/history/old.md",
        &format!("{guide}\n{}", register(&main)),
    );
    commit(&tree.root, "main's card", 1);

    // On main its committed lines refuse nothing; what a clone lacks is warned of, once a page, and
    // the file this machine lacks as before.
    let result = symphony::docs_audit::audit(&tree.root);
    assert_eq!(shown_pictures(&result.errors), Vec::<&String>::new());
    assert_eq!(
        result.warnings,
        [
            not_in_a_clone(DOC, 5, "/artifacts/older.png and 1 more"),
            "docs/guide.md:5: /artifacts/gone.log: not on this machine".to_string(),
            not_in_a_clone("docs/guide.md", 3, "../artifacts/shot.png and 1 more"),
        ]
    );
    // An upload git tracks is in a clone.
    command(&tree.root, &["add", "-f", &UPLOAD[1..]], 1);
    assert_eq!(
        unclonable(&tree.root).1,
        [
            not_in_a_clone(DOC, 5, "/artifacts/older.png"),
            not_in_a_clone("docs/guide.md", 3, "../artifacts/shot.png"),
        ]
    );
    command(&tree.root, &["rm", "-q", "--cached", &UPLOAD[1..]], 1);

    // A branch answers main's card with a picture git does not track, beside main's picture, on
    // disk there too and untracked as well: only its own line is refused, named by its line.
    let wt = branch(&tree, "tools/drawn", "drawn");
    tree.write("drawn/artifacts/older.png", "png");
    tree.write("drawn/artifacts/drawn.png", "png");
    let answer = "  [2026-11-11 11:00] agent — Drawn: ![B](/artifacts/drawn.png)\n";
    std::fs::write(wt.join(DOC), register(&(main.clone() + answer))).unwrap();
    assert_eq!(
        unclonable(&wt).0,
        [untracked(8, "older", "/artifacts/drawn.png")]
    );
    // Committed, the line is still the branch's own; once git tracks the picture, it passes.
    commit(&wt, "answer", 2);
    assert_eq!(unclonable(&wt).0.len(), 1);
    command(&wt, &["add", "-f", "artifacts/drawn.png"], 2);
    assert_eq!(unclonable(&wt).0, Vec::<String>::new());

    // A card of the branch's own: its text and a line show pictures beside its document that git
    // does not track, and a line shows the owner's upload, which counts as untracked whether git
    // tracks it or not.
    let own = card(
        "own",
        ' ',
        "![C](own.png) ![D](own-2.png)",
        &format!("  [2026-11-11 12:00] agent — As you drew: ![E]({UPLOAD})\n"),
    )
    .replace("Proposal own.", "Proposal own: ![F](own.png)");
    tree.write("drawn/docs/own.png", "png");
    tree.write("drawn/docs/own-2.png", "png");
    std::fs::write(wt.join(DOC), register(&(main.clone() + answer + &own))).unwrap();
    assert_eq!(
        unclonable(&wt).0,
        [untracked(9, "own", "own.png and 2 more")]
    );
    command(&wt, &["add", "docs/own.png", "docs/own-2.png"], 2);
    assert_eq!(unclonable(&wt).0, [untracked(11, "own", UPLOAD)]);
    tree.write(&format!("drawn{UPLOAD}"), "png");
    command(&wt, &["add", "-f", &UPLOAD[1..]], 2);
    assert_eq!(unclonable(&wt).0, [untracked(11, "own", UPLOAD)]);
}

/// With no fork from main (here no main at all), the audit judges every open card and warns of a
/// picture git does not track instead of refusing it; an upload is judged by its link, whatever the
/// project's artifacts folder is called.
pub(super) fn with_no_fork_an_untracked_picture_is_warned_of() {
    let tree = super::common::Tree::new();
    command(&tree.root, &["init", "-q", "-b", "trunk"], 1);
    tree.write("symphony.toml", "[project]\nartifacts = \"out\"\n");
    tree.write(".gitignore", "/out/\n");
    let shown = card(
        "lone",
        ' ',
        "![A](/out/lone.png)",
        &format!("  [2026-11-11 10:00] agent — And ![B]({UPLOAD})\n"),
    );
    tree.write(DOC, &register(&shown));
    tree.write("out/lone.png", "png");
    commit(&tree.root, "trunk's card", 1);
    let result = symphony::docs_audit::audit(&tree.root);
    assert_eq!(shown_pictures(&result.errors), Vec::<&String>::new());
    assert_eq!(
        result.warnings,
        [
            not_in_a_clone(DOC, 5, "/out/lone.png and 1 more"),
            format!(
                "{DOC}:5: card **lone** shows a picture git does not track: /out/lone.png and 1 more: not in a clone"
            ),
        ]
    );
}
