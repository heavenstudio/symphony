//! An approve standing over the one merge of the default branch made on the commit it names
//! (docs/workflow/TASKS.md, "An approve over one merge"), in scratch repositories on a board of their own:
//! a conflict in documents is settled by its executor's one merge and lands on the approve it had
//! and its tests green at the merge, no verdict between; a commit beyond the merge, a merge made
//! on another commit than the approved one, a merge of a branch the default branch does not hold,
//! a merge that settles a conflict anywhere but in a document (code main deleted, a guide, the
//! rules), a merge that changes a path that did not conflict, a document among them, and a merge
//! with no green run are each refused as before; and the refusal of a conflict says to merge once
//! and land again only where the approve names the tip.
//!
//! Whether an approve stands at a tip is one answer, [`covered`]'s, read from the repository alone,
//! and whatever it refuses the land sends back to the task's reviewer by one path. So one refusal
//! is tried through the land, on a board, with what the land then does
//! ([`a_merge_that_settles_code_goes_back_to_its_reviewer_naming_it`]), and each other where it is
//! decided: on that answer over a repository ([`answer`]), with no board, approve or check.

use super::stamps::{green, landed_by, told};
use super::*;
use symphony_board::land::{Approval, covered};

const DOC: &str = "docs/DECISIONS.md";

/// [`approved_on`] with the document [`DOC`].
fn approved_branch(r: &Repo, code: &str) -> (PathBuf, String, People) {
    approved_on(r, DOC, code)
}

/// The words a conflict's refusal ends on where a reviewer reads the merge.
fn fix_and_review() -> String {
    format!(
        "Merge main into {SLUG} and fix it there, then move it back to review (`just tasks \
         move {SLUG} review`) and ask its reviewer to approve the new tip naming it"
    )
}

/// A repository whose main holds the document `doc`, and `SLUG`'s branch changing it and `code`:
/// the worktree, the tip and its people, the approve naming the tip.
fn approved_on(r: &Repo, doc: &str, code: &str) -> (PathBuf, String, People) {
    manifest(r, "");
    let (wt, tip) = branched(r, doc, code);
    let people = approved(r, SLUG, &format!("{tip}: read it"));
    (wt, tip, people)
}

/// A repository whose main holds the document `doc`, and `SLUG`'s branch changing it and `code`:
/// the worktree and the tip, on no board.
fn branched(r: &Repo, doc: &str, code: &str) -> (PathBuf, String) {
    write(&r.root, doc, "# Decisions\n\nA card.\n");
    write(&r.root, "sim/world.gd", "extends Node\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "a document and a world"]);
    let wt = r.add(&SLUG.replace('/', "-"), Some(SLUG), "main");
    write(&wt, doc, "# Decisions\n\nA card, answered by the branch.\n");
    write(&wt, code, "extends Node2D\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", &format!("work on {SLUG}")]);
    let tip = head(&wt, "HEAD");
    (wt, tip)
}

/// [`branched`] with the document [`DOC`].
fn branch_read(r: &Repo, code: &str) -> (PathBuf, String) {
    branched(r, DOC, code)
}

/// What the land's own rule ([`covered`]) answers of `SLUG`'s tip in `wt` over main as it is, under
/// an approve naming `approved`: the repository alone is read. An answer that refuses starts with
/// `line`; what the land does with a refusal is
/// [`a_merge_that_settles_code_goes_back_to_its_reviewer_naming_it`]'s.
#[track_caller]
fn assert_unread(r: &Repo, wt: &Path, approved: &str, line: &str) {
    let approval = Approval {
        by: "a-reviewer".into(),
        at: 0.0,
        text: format!("{approved}: read it"),
        review_from: None,
    };
    let (base, tip) = (head(&r.root, "main"), head(wt, "HEAD"));
    let refused = covered(&r.root, SLUG, &base, &tip, &approval).unwrap_err();
    assert!(refused.starts_with(line), "{refused}");
}

/// Main moves `file` to `text` its own way: its new tip.
fn main_moves(r: &Repo, file: &str, text: &str) -> String {
    write(&r.root, file, text);
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", &format!("main moved {file}")]);
    head(&r.root, "main")
}

/// `git merge REV` begun in `wt`, left uncommitted: whether it merged without a conflict.
fn merging(wt: &Path, rev: &str) -> bool {
    symphony_board::sys::output(
        symphony_board::sys::Command::of("git")
            .args(["merge", "-q", "--no-ff", "--no-commit", rev])
            .current_dir(wt),
    )
    .unwrap()
    .status
    .success()
}

/// The merge begun in `wt` committed with `files` written first: the merge commit.
fn settled(wt: &Path, files: &[(&str, &str)]) -> String {
    for (file, text) in files {
        write(wt, file, text);
    }
    git(wt, &["add", "-A"]);
    git(wt, &["commit", "-qm", "Merge main, settled"]);
    head(wt, "HEAD")
}

/// The land refused `SLUG`, its approve not standing on the tip: `line` said to its reviewer, the
/// task back in review and main at `before`.
fn assert_sent_back(r: &Repo, result: Result<String, String>, line: &str, before: &str) {
    let refused = result.unwrap_err();
    assert!(refused.starts_with(line), "{refused}");
    assert_eq!(head(&r.root, "main"), before, "main stays where it was");
    let (text, to) = told(r, SLUG);
    assert!(
        text.starts_with(&format!(
            "land refused: {refused}; {SLUG} is back in review. "
        )),
        "{text}"
    );
    assert_eq!(to.as_deref(), Some("@reviewer"));
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "review");
}

/// The owner's case: approved at A, the land conflicts in a document. The refusal tells the
/// executor to merge once and land again, with no reviewer; the merge settling the document lands
/// on the approve of A once a test run is green at it, and not before.
#[test]
fn a_conflict_in_documents_settled_by_one_merge_lands_on_the_approve_it_had() {
    let r = Repo::new();
    let (wt, tip, people) = approved_branch(&r, "sim/jump.gd");
    let moved = main_moves(&r, DOC, "# Decisions\n\nA card, answered on main.\n");
    let (_, result) = landed_by(&r, SLUG, &people.orch);
    let line = format!("{SLUG} conflicts with main at {}: {DOC}", &moved[..7]);
    assert_eq!(result.as_ref().map_err(String::as_str), Err(line.as_str()));
    let (text, to) = told(&r, SLUG);
    assert_eq!(
        text,
        format!(
            "land refused: {line}. Merge main into {SLUG} once, settling the conflict in those \
             documents and changing nothing else, run its tests at the new tip through the \
             project's recorded recipes, and land it again: an approve naming the commit merged \
             into stands over that one merge, so {SLUG} stays as it is and no reviewer reads it \
             again. A change beyond those conflicts, or a move back to review, asks for a fresh \
             approve of the new tip"
        )
    );
    assert_eq!(to.as_deref(), Some("@executor"));
    let store = &r.board.store;
    assert_eq!(store.get(SLUG).unwrap().status, "done");

    assert!(!merging(&wt, "main"), "the branch meets the conflict");
    let merge = settled(&wt, &[(DOC, "# Decisions\n\nA card, answered by both.\n")]);
    // No run at the merge: the approve stands, and the tests it needs there are missing.
    let (_, result) = landed_by(&r, SLUG, &people.orch);
    let refused = result.unwrap_err();
    assert!(
        refused.starts_with(&format!(
            "{SLUG} lacks at its tip {} what it needs: tests (No build or test recorded for it \
             at {}",
            &merge[..7],
            &merge[..7]
        )),
        "{refused}"
    );
    assert_eq!(head(&r.root, "main"), moved, "main stays where it was");
    assert_eq!(told(&r, SLUG).1.as_deref(), Some("@executor"));
    assert_eq!(
        store.get(SLUG).unwrap().status,
        "done",
        "its approve stands"
    );
    // A run at the commit approved is of another commit.
    green(&r, SLUG, &tip);
    landed_by(&r, SLUG, &people.orch).1.unwrap_err();
    assert_eq!(head(&r.root, "main"), moved);

    green(&r, SLUG, &merge);
    let verdicts = |r: &Repo| {
        let thread = r.board.store.thread(SLUG).unwrap();
        let approves =
            |m: &&symphony_board::Message| m.kind == "verdict" && m.text.starts_with("approve");
        thread.iter().filter(approves).count()
    };
    let before = verdicts(&r);
    let (said, result) = landed_by(&r, SLUG, &people.orch);
    let landed = result.unwrap();
    let evidence = format!("land {SLUG}: approved by {} at ", people.rev);
    let carried = format!(
        ", naming {}, which stands over the one merge {} of main at {} into it, which settles \
         conflicts in documents alone ({DOC}) and is git's own merge of the two elsewhere; its \
         tests are read at {}\n",
        &tip[..7],
        &merge[..7],
        &moved[..7],
        &merge[..7]
    );
    assert!(
        said.starts_with(&evidence) && said.contains(&carried),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "land {SLUG}: it needs review, tests at {} (the catalogue's defaults, no \
             assessment); tests: Green at",
            &merge[..7]
        )),
        "{said}"
    );
    assert_eq!(verdicts(&r), before, "no new approve");
    assert_eq!(head(&r.root, "main^1"), moved);
    assert_eq!(head(&r.root, "main^2"), merge);
    assert_eq!(
        fs::read_to_string(r.root.join(DOC)).unwrap(),
        "# Decisions\n\nA card, answered by both.\n"
    );
    assert_eq!(store.get(SLUG).unwrap().status, "done");
    assert!(
        last(&r, SLUG).contains(&format!(
            "; its approve of {} stands over the one merge {} of main into it",
            &tip[..7],
            &merge[..7]
        )),
        "{}",
        last(&r, SLUG)
    );
    assert!(landed.starts_with(&format!("landed {SLUG} on main at ")));
}

/// A merge of main that met no conflict is git's own, what the land itself makes of a tip: the
/// approve stands over it, with its tests green there.
#[test]
fn a_merge_of_main_that_met_no_conflict_lands_on_the_approve_too() {
    let r = Repo::new();
    let (wt, tip, people) = approved_branch(&r, "sim/jump.gd");
    let moved = main_moves(&r, "sim/other.gd", "extends Object\n");
    assert!(merging(&wt, "main"));
    let merge = settled(&wt, &[]);
    green(&r, SLUG, &merge);
    let (said, result) = landed_by(&r, SLUG, &people.orch);
    result.unwrap();
    assert!(
        said.contains(&format!(
            "naming {}, which stands over the one merge {} of main at {} into it, which is \
             git's own merge of the two; its tests are read at {}",
            &tip[..7],
            &merge[..7],
            &moved[..7],
            &merge[..7]
        )),
        "{said}"
    );
    assert_eq!(head(&r.root, "main^2"), merge);
}

/// One commit beyond the merge is a commit nobody read: back to its reviewer, as before.
#[test]
fn a_commit_beyond_the_merge_goes_back_to_its_reviewer() {
    let r = Repo::new();
    let (wt, tip) = branch_read(&r, "sim/jump.gd");
    main_moves(&r, DOC, "# Decisions\n\nA card, answered on main.\n");
    assert!(!merging(&wt, "main"));
    settled(&wt, &[(DOC, "# Decisions\n\nBoth.\n")]);
    write(&wt, "sim/jump.gd", "extends Node3D\n");
    git(&wt, &["commit", "-qam", "one more thing"]);
    let line = format!("{SLUG} has 3 commits after the approved {}: ", &tip[..7]);
    assert_unread(&r, &wt, &tip, &line);
}

/// A merge made on a commit after the approved one carries that commit unread.
#[test]
fn a_merge_whose_first_parent_is_not_the_approved_commit_goes_back_to_its_reviewer() {
    let r = Repo::new();
    let (wt, tip) = branch_read(&r, "sim/jump.gd");
    main_moves(&r, DOC, "# Decisions\n\nA card, answered on main.\n");
    write(&wt, "sim/jump.gd", "extends Node3D\n");
    git(&wt, &["commit", "-qam", "one more thing"]);
    assert!(!merging(&wt, "main"));
    settled(&wt, &[(DOC, "# Decisions\n\nBoth.\n")]);
    let line = format!("{SLUG} has 3 commits after the approved {}: ", &tip[..7]);
    assert_unread(&r, &wt, &tip, &line);
}

/// The approved commit merged into main's, main first, is no merge made on the approved commit.
#[test]
fn a_merge_made_on_mains_commit_goes_back_to_its_reviewer() {
    let r = Repo::new();
    let (wt, tip) = branch_read(&r, "sim/jump.gd");
    main_moves(&r, "sim/other.gd", "extends Object\n");
    git(&wt, &["checkout", "-q", "--detach", "main"]);
    assert!(merging(&wt, &tip));
    let merge = settled(&wt, &[]);
    git(&wt, &["checkout", "-q", "-B", SLUG, &merge]);
    assert_eq!(head(&wt, "HEAD^2"), tip);
    let line = format!("{SLUG} has 2 commits after the approved {}: ", &tip[..7]);
    assert_unread(&r, &wt, &tip, &line);
}

/// A merge of a branch main does not hold brings commits no land read.
#[test]
fn a_merge_of_a_commit_main_does_not_hold_goes_back_to_its_reviewer() {
    let r = Repo::new();
    let (wt, tip) = branch_read(&r, "sim/jump.gd");
    let other = r.add("other", Some("sim/other"), "main");
    write(&other, "sim/other.gd", "extends Object\n");
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-qm", "work nobody landed"]);
    assert!(merging(&wt, "sim/other"));
    settled(&wt, &[]);
    let line = format!("{SLUG} has 2 commits after the approved {}: ", &tip[..7]);
    assert_unread(&r, &wt, &tip, &line);
}

/// A conflict in code is settled by code nobody read: the land's refusal keeps its words, and
/// the merge that settles it goes to its reviewer, the code named.
#[test]
fn a_merge_that_settles_code_goes_back_to_its_reviewer_naming_it() {
    let r = Repo::new();
    let (wt, _, people) = approved_branch(&r, "sim/world.gd");
    let moved = main_moves(&r, "sim/world.gd", "extends Object\n");
    let (_, result) = landed_by(&r, SLUG, &people.orch);
    result.unwrap_err();
    let (text, _) = told(&r, SLUG);
    assert!(
        text.ends_with(&format!(
            "Merge main into {SLUG} and fix it there, then move it back to review (`just tasks \
             move {SLUG} review`) and ask its reviewer to approve the new tip naming it"
        )),
        "{text}"
    );
    assert!(!merging(&wt, "main"));
    let merge = settled(&wt, &[("sim/world.gd", "extends Node2D # and Object\n")]);
    green(&r, SLUG, &merge);
    let (_, result) = landed_by(&r, SLUG, &people.orch);
    let line = format!(
        "{SLUG}'s tip {} is one merge over its approved commit, but that merge settles a \
         conflict in sim/world.gd, no document its executor settles alone (a Markdown file kept \
         as a regular file, no skill, guide or rules): an approve stands over a merge that \
         settles conflicts in documents alone",
        &merge[..7]
    );
    assert_sent_back(&r, result, &line, &moved);
}

/// A conflict in a document and one in code together keep the refusal's old words: the merge
/// needs its reviewer.
#[test]
fn a_conflict_in_a_document_and_in_code_is_the_reviewers_still() {
    let r = Repo::new();
    let (_, _, people) = approved_branch(&r, "sim/world.gd");
    write(&r.root, "sim/world.gd", "extends Object\n");
    main_moves(&r, DOC, "# Decisions\n\nA card, answered on main.\n");
    landed_by(&r, SLUG, &people.orch).1.unwrap_err();
    let (text, _) = told(&r, SLUG);
    assert!(
        text.contains(&format!(
            ": {DOC}, sim/world.gd. Merge main into {SLUG} and fix it there"
        )),
        "{text}"
    );
}

/// A merge that settles its document and changes code by hand beside it, where nothing
/// conflicted, carries code nobody read.
#[test]
fn a_merge_that_changes_code_beside_the_documents_it_settles_goes_back_to_its_reviewer() {
    let r = Repo::new();
    let (wt, tip) = branch_read(&r, "sim/jump.gd");
    main_moves(&r, DOC, "# Decisions\n\nA card, answered on main.\n");
    assert!(!merging(&wt, "main"));
    let merge = settled(
        &wt,
        &[
            (DOC, "# Decisions\n\nBoth.\n"),
            ("sim/jump.gd", "extends Node3D\n"),
        ],
    );
    let line = format!(
        "{SLUG}'s tip {} is one merge over its approved commit, but that merge changes \
         sim/jump.gd where the two did not conflict: an approve stands over a merge that settles \
         conflicts in documents alone",
        &merge[..7]
    );
    assert_unread(&r, &wt, &tip, &line);
}

/// A conflict git leaves no marker for: main deleted a code file the branch changed. The merge
/// that keeps the file is, path by path, the tree git's own merge leaves, and is code settled by
/// hand all the same.
#[test]
fn a_merge_that_keeps_code_main_deleted_goes_back_to_its_reviewer() {
    let r = Repo::new();
    let (wt, tip) = branch_read(&r, "sim/world.gd");
    fs::remove_file(r.root.join("sim/world.gd")).unwrap();
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "main deleted the world"]);
    assert!(!merging(&wt, "main"), "a change meets a deletion");
    let merge = settled(&wt, &[("sim/world.gd", "extends Node2D\n")]);
    let line = format!(
        "{SLUG}'s tip {} is one merge over its approved commit, but that merge settles a \
         conflict in sim/world.gd, no document",
        &merge[..7]
    );
    assert_unread(&r, &wt, &tip, &line);
}

/// A document edited in the merge where nothing conflicted is text nobody read, as code is.
#[test]
fn a_merge_that_edits_a_document_that_did_not_conflict_goes_back_to_its_reviewer() {
    let r = Repo::new();
    let (wt, tip) = branch_read(&r, "sim/jump.gd");
    main_moves(&r, DOC, "# Decisions\n\nA card, answered on main.\n");
    assert!(!merging(&wt, "main"));
    let merge = settled(
        &wt,
        &[
            (DOC, "# Decisions\n\nBoth.\n"),
            ("docs/OTHER.md", "# Another page\n"),
        ],
    );
    let line = format!(
        "{SLUG}'s tip {} is one merge over its approved commit, but that merge changes \
         docs/OTHER.md where the two did not conflict",
        &merge[..7]
    );
    assert_unread(&r, &wt, &tip, &line);
}

/// A guide, the rules and a skill instruct every agent: a conflict there keeps its reviewer, in
/// the refusal's words and when the merge that settles it is landed. A skill here; which paths
/// instruct agents is one rule's (`land::carried::tests::a_document_is_a_markdown_file_by_its_extension`).
#[test]
fn a_conflict_in_the_rules_or_a_skill_is_the_reviewers_still() {
    let doc = "shipped/skills/work/SKILL.md";
    let r = Repo::new();
    let (wt, _, people) = approved_on(&r, doc, "sim/jump.gd");
    let moved = main_moves(&r, doc, "# Decisions\n\nA card, answered on main.\n");
    landed_by(&r, SLUG, &people.orch).1.unwrap_err();
    let (text, to) = told(&r, SLUG);
    assert!(text.ends_with(&fix_and_review()), "{text}");
    assert_eq!(to.as_deref(), Some("@executor"));
    assert!(!merging(&wt, "main"));
    let merge = settled(&wt, &[(doc, "# Decisions\n\nBoth.\n")]);
    green(&r, SLUG, &merge);
    let (_, result) = landed_by(&r, SLUG, &people.orch);
    let line = format!(
        "{SLUG}'s tip {} is one merge over its approved commit, but that merge settles a \
         conflict in {doc}, no document",
        &merge[..7]
    );
    assert_sent_back(&r, result, &line, &moved);
}

/// An approve naming no commit stands over no merge, so the refusal of its conflict, in a
/// document too, sends the merge to its reviewer.
#[test]
fn a_conflict_under_an_approve_naming_no_commit_keeps_the_refusals_words() {
    let r = Repo::new();
    manifest(&r, "");
    write(&r.root, DOC, "# Decisions\n\nA card.\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "a document"]);
    branch(
        &r,
        SLUG,
        DOC,
        "# Decisions\n\nA card, answered by the branch.\n",
    );
    let people = approved(&r, SLUG, "good");
    let moved = main_moves(&r, DOC, "# Decisions\n\nA card, answered on main.\n");
    let (_, result) = landed_by(&r, SLUG, &people.orch);
    let line = format!("{SLUG} conflicts with main at {}: {DOC}", &moved[..7]);
    assert_eq!(result.as_ref().map_err(String::as_str), Err(line.as_str()));
    let (text, to) = told(&r, SLUG);
    assert_eq!(text, format!("land refused: {line}. {}", fix_and_review()));
    assert_eq!(to.as_deref(), Some("@executor"));
}
