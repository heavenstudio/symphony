//! The docs audit refuses a card whose pictures do not load, judged on what the checkout wrote: the
//! text and lines of an open card that its fork from main did not hold. A branch answering a card of
//! main's is never refused for main's pictures, which `/artifacts/` (each checkout's own, ignored by
//! git) does not hold in a new worktree. What the checkout wrote is refused too when git does not
//! track its picture, as a clone shows only what git tracks; what a clone lacks elsewhere, and on
//! main itself, is warned of.

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

/// The warning of a card picture git does not track, where no fork of the checkout's own judges it.
fn warned(line: usize, slug: &str, shown: &str) -> String {
    format!(
        "{DOC}:{line}: card **{slug}** shows a picture git does not track: {shown}: not in a clone"
    )
}

pub(super) fn a_cards_picture_is_a_file_git_tracks() {
    let tree = repository();
    tree.write(".gitignore", "/artifacts/\n");
    // Lines 4 to 7: main's card, an agent's pictures git does not track, the owner's upload.
    let main = card(
        "older",
        ' ',
        "![A](/artifacts/older.png) ![G](older.png)",
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
    tree.write("docs/older.png", "png");

    // Main itself has no fork of its own, so its cards refuse nothing: what a clone lacks is warned
    // of, once a page for the artifacts folder and once a card for a picture beside its document,
    // and the file this machine lacks as before. Detached at main's tip, as the nightly checks main
    // out, it is main itself too.
    let on_main = [
        not_in_a_clone(DOC, 5, "/artifacts/older.png and 1 more"),
        warned(5, "older", "older.png"),
        "docs/guide.md:5: /artifacts/gone.log: not on this machine".to_string(),
        not_in_a_clone("docs/guide.md", 3, "../artifacts/shot.png and 1 more"),
    ];
    for detached in [false, true] {
        if detached {
            command(&tree.root, &["checkout", "-q", "--detach"], 1);
        }
        let result = symphony::docs_audit::audit(&tree.root);
        assert_eq!(shown_pictures(&result.errors), Vec::<&String>::new());
        assert_eq!(result.warnings, on_main, "detached: {detached}");
    }
    command(&tree.root, &["checkout", "-q", "main"], 1);
    // An upload git tracks is in a clone.
    command(&tree.root, &["add", "-f", &UPLOAD[1..]], 1);
    assert_eq!(
        unclonable(&tree.root).1,
        [
            not_in_a_clone(DOC, 5, "/artifacts/older.png"),
            warned(5, "older", "older.png"),
            not_in_a_clone("docs/guide.md", 3, "../artifacts/shot.png"),
        ]
    );
    command(&tree.root, &["rm", "-q", "--cached", &UPLOAD[1..]], 1);

    // A branch answers main's card with a picture git does not track, beside main's picture, on
    // disk there too and untracked as well: only its own line is refused, named by its line.
    let wt = branch(&tree, "tools/drawn", "drawn");
    tree.write("drawn/artifacts/older.png", "png");
    tree.write("drawn/docs/older.png", "png");
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
/// picture git does not track instead of refusing it, once a card for those beside its document and
/// once a page for those in the artifacts folder; an upload is judged by its link, whatever the
/// project's artifacts folder is called.
pub(super) fn with_no_fork_an_untracked_picture_is_warned_of() {
    let tree = super::common::Tree::new();
    command(&tree.root, &["init", "-q", "-b", "trunk"], 1);
    tree.write("symphony.toml", "[project]\nartifacts = \"out\"\n");
    tree.write(".gitignore", "/out/\n");
    let shown = card(
        "lone",
        ' ',
        "![A](/out/lone.png) ![C](lone.png)",
        &format!("  [2026-11-11 10:00] agent — And ![B]({UPLOAD}) ![D](lone-2.png)\n"),
    );
    tree.write(DOC, &register(&shown));
    tree.write("out/lone.png", "png");
    commit(&tree.root, "trunk's card", 1);
    tree.write("docs/lone.png", "png");
    tree.write("docs/lone-2.png", "png");
    let result = symphony::docs_audit::audit(&tree.root);
    assert_eq!(shown_pictures(&result.errors), Vec::<&String>::new());
    assert_eq!(
        result.warnings,
        [
            not_in_a_clone(DOC, 5, "/out/lone.png and 1 more"),
            warned(5, "lone", "lone.png and 1 more"),
        ]
    );
}

/// The audit's refusals and warnings of a document's own pictures (not a card's).
fn documents(root: &std::path::Path) -> (Vec<String>, Vec<String>) {
    let result = symphony::docs_audit::audit(root);
    let own = |line: &String| line.contains(": shows a picture");
    (
        result.errors.into_iter().filter(own).collect(),
        result.warnings.into_iter().filter(own).collect(),
    )
}

fn document_refused(rel: &str, line: usize, shown: &str) -> String {
    format!(
        "{rel}:{line}: shows a picture this checkout wrote that git does not track: {shown}; commit it beside the document, as a clone shows only what git tracks (an upload is for a thread, never a document)"
    )
}

fn document_warned(rel: &str, line: usize, shown: &str) -> String {
    format!("{rel}:{line}: shows a picture git does not track: {shown}: not in a clone")
}

const GUIDE: &str = "docs/guide.md";

/// A document's own picture is judged as a card's is, on disk or not: warned of on main, refused on
/// a branch for the lines it wrote, passed once git tracks it. Main's older line is never the
/// branch's; history folders are passed over; an owner's callout is the owner's (its upload only
/// warned of, by the page), and an agent's resolution nested in it is the document's.
pub(super) fn a_documents_own_picture_is_a_file_git_tracks() {
    let tree = repository();
    tree.write(".gitignore", "/artifacts/\n");
    // Lines 1 to 5: a picture git never tracked (absent here) and one it tracks.
    let guide = "# Guide\n\n![old](old.png)\n\n![kept](kept.png)\n";
    tree.write(GUIDE, guide);
    tree.write("docs/kept.png", "png");
    tree.write("docs/history/old.md", "# Old\n\n![gone](gone.png)\n");
    commit(&tree.root, "main's guide", 1);
    assert_eq!(
        documents(&tree.root),
        (vec![], vec![document_warned(GUIDE, 3, "old.png")])
    );

    // A branch writes a picture on disk that git does not track and one not on disk at all (line
    // 7), and shows the tracked one again (line 9): only line 7 is refused, main's line 3 never.
    let wt = branch(&tree, "tools/guide", "guide");
    tree.write("guide/docs/new.png", "png");
    tree.write("guide/docs/old.png", "png");
    let added = format!("{guide}\n![new](new.png) ![absent](absent.png)\n\n![again](kept.png)\n");
    std::fs::write(wt.join(GUIDE), &added).unwrap();
    assert_eq!(
        documents(&wt).0,
        [document_refused(GUIDE, 7, "new.png and 1 more")]
    );
    // Committed, the line is still the branch's own; once git tracks what it shows, it passes.
    commit(&wt, "the guide's pictures", 2);
    assert_eq!(documents(&wt).0.len(), 1);
    let tracked = added.replace(" ![absent](absent.png)", "");
    std::fs::write(wt.join(GUIDE), &tracked).unwrap();
    commit(&wt, "only what is committed", 2);
    assert_eq!(documents(&wt).0, Vec::<String>::new());

    // An owner's callout naming an upload, as a verdict writes it, refuses nothing; an agent's
    // resolution nested in it (line 13) is the document's own.
    let callout = format!(
        "{tracked}\n> [!comment] Like this\n> ![sketch.png]({UPLOAD})\n> > [!resolved] Drawn: ![r](resolved.png)\n"
    );
    std::fs::write(wt.join(GUIDE), &callout).unwrap();
    assert_eq!(
        documents(&wt).0,
        [document_refused(GUIDE, 13, "resolved.png")]
    );
}

/// The picture reader sees every form a picture takes, on a card's line and in a document's text
/// alike: a path with a title, a reference to a definition (refused when the branch wrote the
/// definition too), raw `<img src>`, and this server's own URL for a file of this project. Another
/// project's URL and a bare anchor name no file here.
pub(super) fn every_form_of_a_picture_is_read() {
    let tree = repository();
    tree.write("symphony.toml", "[project]\nid = \"proj\"\n");
    tree.write(".gitignore", "/artifacts/\n");
    let posted = card("forms", ' ', "posted.", "");
    tree.write(DOC, &register(&posted));
    let guide = "# Guide\n\n[shared]: shared.png\n";
    tree.write(GUIDE, guide);
    commit(&tree.root, "main's card and guide", 1);
    let wt = branch(&tree, "tools/forms", "forms");
    for name in ["t1", "t2", "t3", "t4", "shared"] {
        tree.write(&format!("forms/docs/{name}.png"), "png");
    }
    let forms = [
        ("![A](t1.png \"a title\")", "t1.png", ""),
        ("<img alt=\"B\" src=\"t2.png\">", "t2.png", ""),
        (
            "![C](http://127.0.0.1:8765/proj/docs/t3.png)",
            "http://127.0.0.1:8765/proj/docs/t3.png",
            "",
        ),
        ("![E][e]", "t4.png", "\n[e]: t4.png\n"),
    ];
    for (form, shown, definition) in forms {
        // On a card's line (6), the definition below the register.
        let answer = format!("  [2026-11-11 11:00] agent — {form}\n");
        let text = register(&format!("{posted}{answer}")) + definition;
        std::fs::write(wt.join(DOC), &text).unwrap();
        assert_eq!(unclonable(&wt).0, [untracked(6, "forms", shown)], "{form}");
        std::fs::write(wt.join(DOC), register(&posted)).unwrap();
        // In a document's own text (line 5).
        let text = format!("{guide}\n{form}\n{definition}");
        std::fs::write(wt.join(GUIDE), &text).unwrap();
        assert_eq!(
            documents(&wt).0,
            [document_refused(GUIDE, 5, shown)],
            "{form}"
        );
        std::fs::write(wt.join(GUIDE), guide).unwrap();
    }
    // A reference to main's definition, written by the branch, is the branch's picture; main's
    // definition alone is not. Another project's URL and a bare anchor show nothing of this one.
    let text = "# Guide\n\n[shared]: shared.png\n\n![s][shared] ![x](http://127.0.0.1:8765/other/x.png) ![y](#top)\n";
    std::fs::write(wt.join(GUIDE), text).unwrap();
    assert_eq!(documents(&wt).0, [document_refused(GUIDE, 5, "shared.png")]);
    std::fs::write(
        wt.join(GUIDE),
        "# Guide\n\n[shared]: shared.png\n\nNo picture.\n",
    )
    .unwrap();
    assert_eq!(documents(&wt).0, Vec::<String>::new());
}

/// A picture git never tracked reaches main by a branch the audit did not refuse (a land runs
/// none): a clone of main has no such file, and its audit, main itself there, warns of the card's
/// and the document's alike.
pub(super) fn a_clone_is_warned_of_a_picture_git_never_tracked() {
    let tree = super::common::Tree::new();
    tree.write("origin/.gitignore", "/artifacts/\n");
    let origin = tree.path("origin");
    command(&origin, &["init", "-q", "-b", "main"], 1);
    tree.write(
        &format!("origin/{DOC}"),
        &register(&card("cloned", ' ', "![P](pic.png)", "")),
    );
    tree.write(&format!("origin/{GUIDE}"), "# Guide\n\n![g](guide.png)\n");
    commit(&origin, "pictures git never tracked", 1);
    command(&tree.root, &["clone", "-q", "origin", "clone"], 1);
    let clone = tree.path("clone");
    assert!(!clone.join("docs/pic.png").exists());
    let result = symphony::docs_audit::audit(&clone);
    assert_eq!(shown_pictures(&result.errors), Vec::<&String>::new());
    assert_eq!(
        result.warnings,
        [
            warned(5, "cloned", "pic.png"),
            document_warned(GUIDE, 3, "guide.png"),
        ]
    );
}
