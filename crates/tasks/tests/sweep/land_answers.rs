//! The owner's answer a land would leave behind: an owner's dated line under a card that the
//! Symphony saved into a worktree and no commit has. A worktree at a commit the land brings that
//! holds one refuses the land, said to the executor, before the merge and again right before main
//! moves; one saved while main moved is said to the orchestrator once it moved. A worktree git
//! cannot read refuses it, said to the one landing. A line a regroup only moved, an agent's own
//! note and a worktree at no commit the land brings refuse nothing.

use super::*;

const DOC: &str = "docs/DECISIONS.md";

/// A register with one card an agent posted.
const REGISTER: &str = "# Decisions\n\n<!-- register:start -->\n- [ ] **card-b** — B.\n  \
                        [2026-11-14 01:00] agent — Posted.\n<!-- register:end -->\n";

/// `register` with the owner's approve of its last card, as the Symphony saves it.
fn answered(register: &str) -> String {
    register.replacen("- [ ]", "- [x]", 1).replace(
        "<!-- register:end",
        "  [2026-11-14 01:52] owner · approved — B\n<!-- register:end",
    )
}

/// The line the land refuses on, said to the executor, while the worktrees `held` (each its
/// path's last name and what follows it) hold the owner's answer.
fn refused_for(held: &str) -> String {
    format!(
        "{held} holds the owner's answer no commit has, which a land of its commits would leave \
         behind"
    )
}

/// What the executor is told to do after the refusal `line`; `elsewhere` when a worktree not on the
/// branch holds the answer.
fn told_to(line: &str, elsewhere: bool) -> String {
    format!(
        "land refused: {line}. Commit it on {SLUG} there{}, then move it back to review (`just \
         tasks move {SLUG} review`) and ask its reviewer to approve the new tip naming it",
        if elsewhere {
            format!(" (from a worktree not on it, copy the line onto its card on {SLUG})")
        } else {
            String::new()
        }
    )
}

/// The worktree `wt`'s name.
fn name(wt: &Path) -> String {
    wt.file_name().unwrap().to_string_lossy().into_owned()
}

/// The answer committed on the branch in `wt`, the task back in review and approved again naming
/// the new tip: that tip.
fn committed_and_approved(r: &Repo, wt: &Path, people: &People) -> String {
    git(wt, &["commit", "-qam", "the owner's answer"]);
    let fixed = head(wt, "HEAD");
    let store = &r.board.store;
    store.move_to(SLUG, "review", &people.exe).unwrap();
    store
        .verdict(
            SLUG,
            &people.rev,
            "approve",
            &format!("{fixed}: carries it"),
        )
        .unwrap();
    fixed
}

/// The owner's answer on a card only the branch holds is saved into its worktree's register,
/// uncommitted: a land of its commits would leave it behind, so it is refused, the executor told,
/// until the answer is committed on the branch and its new tip approved; then it lands with it.
#[test]
fn an_owner_answer_no_commit_has_in_the_branchs_worktree_is_refused_until_committed() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, DOC, REGISTER);
    let people = approved(&r, SLUG, &format!("{} is good", head(&wt, "HEAD")));
    write(&wt, DOC, &answered(REGISTER));
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    assert!(line.starts_with(&format!("{SLUG}'s worktree ")), "{line}");
    assert!(
        line.ends_with(&refused_for(&format!("{} ({DOC})", name(&wt)))),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before, "nothing landed");
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(told.text, told_to(&line, false));
    assert_eq!(told.recipient.as_deref(), Some("@executor"));
    assert_eq!(
        fs::read_to_string(wt.join(DOC)).unwrap(),
        answered(REGISTER)
    );
    assert!(runs(&r).is_empty());

    // Committed on the branch and approved again, it lands, the owner's answer with it.
    let fixed = committed_and_approved(&r, &wt, &people);
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), fixed);
    assert_eq!(
        fs::read_to_string(r.root.join(DOC)).unwrap(),
        answered(REGISTER)
    );
}

/// An agent's own edit of the branch's documents, uncommitted, holds no owner line: the land is
/// not refused for it.
#[test]
fn an_agents_uncommitted_docs_edit_without_an_owner_line_lands() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, DOC, REGISTER);
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    let edited = REGISTER.replace(
        "<!-- register:end",
        "  [2026-11-14 01:52] agent — A note no commit has.\n<!-- register:end",
    );
    write(&wt, DOC, &edited);
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(fs::read_to_string(r.root.join(DOC)).unwrap(), REGISTER);
}

/// A worktree of the branch git cannot read may hold the owner's answer: the land cannot tell, so
/// it refuses, said to the one landing alone (no commit of the branch fixes git), nothing merged,
/// checked or said on the thread; once git reads it, the land goes on.
#[test]
fn a_worktree_git_cannot_read_refuses_the_land_to_the_one_landing() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, DOC, REGISTER);
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    // Its `.git` names no repository: git still lists it from main's, and cannot read it.
    let link = fs::read_to_string(wt.join(".git")).unwrap();
    fs::write(wt.join(".git"), "gitdir: /nowhere/at/all\n").unwrap();
    let before = head(&r.root, "main");
    let said = r.board.said(SLUG);
    let line = landed(&r, SLUG, &people.orch).1.unwrap_err();
    let named = format!(
        "{} could not be read for the owner's answers no commit has: git diff failed in ",
        name(&wt)
    );
    assert!(
        line.starts_with("the worktree ") && line.contains(&named),
        "{line}"
    );
    assert!(
        line.ends_with(&format!(
            ": nothing landed, as {SLUG} may hold the owner's answer there. Run the land again \
             once git reads it"
        )),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(r.board.said(SLUG), said, "nothing said on the thread");
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    assert!(runs(&r).is_empty());

    fs::write(wt.join(".git"), link).unwrap();
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
}

/// The owner's answers are read in the folders the project's entrypoints serve documents from,
/// not in `docs/` by name: a register under another entrypoint's folder refuses the land, and a
/// line under a card in a folder the project serves nothing from is none the Symphony wrote.
#[test]
fn the_answers_are_read_in_the_projects_entrypoint_folders() {
    let r = Repo::new();
    manifest(&r, "\n[[entrypoints]]\nurl = \"spec\"\nfolder = \"spec\"\n");
    let wt = branch(&r, SLUG, "spec/DECISIONS.md", REGISTER);
    let people = approved(&r, SLUG, &format!("{} is good", head(&wt, "HEAD")));
    write(&wt, "spec/DECISIONS.md", &answered(REGISTER));
    // Not a document of this project: an agent's file, whatever it holds.
    write(&wt, "docs/NOTES.md", &answered(REGISTER));
    let line = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert!(
        line.ends_with(&refused_for(&format!("{} (spec/DECISIONS.md)", name(&wt)))),
        "{line}"
    );
    let fixed = committed_and_approved(&r, &wt, &people);
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), fixed);
    assert!(wt.join("docs/NOTES.md").is_file(), "left where it was");
}

/// A worktree at a commit the land brings holds the owner's answer wherever the Symphony saved it:
/// a reviewer's on a branch of its own at the tip, or one detached there after. Each refuses the
/// land, named with where it is; a worktree at no commit the land brings is not its to read. The
/// executor copies the line onto its card on the branch and commits it there: the tip then holds
/// it, so the copies the other worktrees hold are carried, and the land goes on.
#[test]
fn a_worktree_at_the_tip_on_another_branch_or_detached_holding_an_answer_refuses_the_land() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, DOC, REGISTER);
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    let review = r.add("review", Some("review/jump"), &tip);
    let look = r.add("look", None, &tip);
    // At main: none of the land's.
    let other = r.add("other", Some("other/x"), "main");
    for at in [&review, &look, &other] {
        write(at, DOC, &answered(REGISTER));
    }
    let line = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert!(
        line.contains(&format!("{} on review/jump ({DOC})", name(&review))),
        "{line}"
    );
    assert!(
        line.contains(&format!(
            "{} detached at {} ({DOC})",
            name(&look),
            &tip[..7]
        )),
        "{line}"
    );
    assert!(!line.contains(&name(&other)), "{line}");
    assert!(!line.contains(&format!("{} (", name(&wt))), "{line}");
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(told.text, told_to(&line, true));
    assert_eq!(told.recipient.as_deref(), Some("@executor"));

    write(&wt, DOC, &answered(REGISTER));
    let fixed = committed_and_approved(&r, &wt, &people);
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), fixed);
    assert_eq!(
        fs::read_to_string(r.root.join(DOC)).unwrap(),
        answered(REGISTER)
    );
}

/// A register's cards regrouped, uncommitted (as the Symphony's save regroups them), only move
/// the owner's committed lines, within the file or to a card moved into another: no answer. One
/// line the owner added among them is, and refuses the land until it is committed.
#[test]
fn an_owner_line_a_regroup_only_moved_is_no_answer() {
    let posted = "# Decisions\n\n<!-- register:start -->\n### Open\n\n- [ ] **card-b** — B.\n  \
                  [2026-11-14 01:00] agent — Posted.\n\n### Done\n\n- [x] **card-a** — A.\n  \
                  [2026-11-11 14:28] owner · approved — A\n- [x] **card-c** — C.\n  \
                  [2026-11-11 14:30] owner · approved — C\n<!-- register:end -->\n";
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, DOC, posted);
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    // card-a goes above card-b, under a heading of its own; card-c to a document of its own.
    let regrouped = "# Decisions\n\n<!-- register:start -->\n### Done\n\n- [x] **card-a** — A.\n  \
                     [2026-11-11 14:28] owner · approved — A\n\n### Open\n\n- [ ] **card-b** — \
                     B.\n  [2026-11-14 01:00] agent — Posted.\n<!-- register:end -->\n";
    let archive = "# Archive\n\n<!-- register:start -->\n- [x] **card-c** — C.\n  [2026-11-11 \
                   14:30] owner · approved — C\n<!-- register:end -->\n";
    write(&wt, DOC, &answered(regrouped));
    write(&wt, "docs/ARCHIVE.md", archive);
    let line = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert!(
        line.ends_with(&refused_for(&format!("{} ({DOC})", name(&wt)))),
        "only the new line's file: {line}"
    );
    // The new line taken out, what is left only moved.
    write(&wt, DOC, regrouped);
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(fs::read_to_string(r.root.join(DOC)).unwrap(), posted);
}

/// The owner answers in the branch's worktree while the land checks the merge: asked again right
/// before main moves, the land refuses, main unmoved, the executor told.
#[test]
fn an_answer_saved_while_the_land_checked_refuses_it_before_main_moves() {
    let r = Repo::new();
    let wt = branch(&r, SLUG, DOC, REGISTER);
    let saved = r.dir.join("answered.md");
    fs::write(&saved, answered(REGISTER)).unwrap();
    // The check is when the owner answers.
    checked_by(
        &r,
        &format!("cp '{}' '{}'", saved.display(), wt.join(DOC).display()),
    );
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    let before = head(&r.root, "main");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    assert!(said.contains(&format!("land {SLUG}: checking")), "{said}");
    assert!(
        line.ends_with(&refused_for(&format!("{} ({DOC})", name(&wt)))),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before, "main never moved");
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(told.text, told_to(&line, false));
    assert!(
        !r.board
            .said(SLUG)
            .iter()
            .any(|l| l.contains("moving main there")),
        "nothing recorded"
    );
}

/// The owner answers in the branch's worktree while main moves, after the land's last look: the
/// land cannot refuse any more, so once main moved it says so on the thread to the orchestrator,
/// naming the worktree, the file and the card, and lands.
#[test]
fn an_answer_saved_while_main_moved_is_said_to_the_orchestrator() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, DOC, REGISTER);
    let saved = r.dir.join("answered.md");
    fs::write(&saved, answered(REGISTER)).unwrap();
    // A post-merge hook in the main checkout alone: git runs it once main moved.
    let root = r.root.display();
    let hooks = r.root.join(".git/hooks");
    write(
        &hooks,
        "post-merge",
        &format!(
            "#!/bin/sh\nif [ \"$(pwd -P)\" = '{root}' ]; then cp '{}' '{}'; fi\n",
            saved.display(),
            wt.join(DOC).display()
        ),
    );
    fs::set_permissions(
        hooks.join("post-merge"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    let (said, result) = landed(&r, SLUG, &people.orch);
    result.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(fs::read_to_string(r.root.join(DOC)).unwrap(), REGISTER);
    let thread = r.board.store.thread(SLUG).unwrap();
    let told = thread
        .iter()
        .find(|m| m.text.contains("saved while"))
        .unwrap_or_else(|| panic!("{said}"));
    assert_eq!(told.recipient.as_deref(), Some("@orchestrator"));
    assert!(
        told.text.starts_with(&format!("land {SLUG}: ")),
        "{}",
        told.text
    );
    assert!(
        told.text.ends_with(&format!(
            "{} ({DOC}) holds the owner's answer on card-b saved while {SLUG} landed, which main \
             does not hold: copy it onto that card on main and commit it there; the sweep keeps \
             that worktree meanwhile",
            name(&wt)
        )),
        "{}",
        told.text
    );
    assert!(said.contains(&told.text), "{said}");
    assert!(
        last(&r, SLUG).starts_with(&format!("landed {SLUG} on main at ")),
        "{}",
        last(&r, SLUG)
    );
}
