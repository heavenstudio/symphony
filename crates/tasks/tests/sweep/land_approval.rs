//! Whose approve a land goes by, and what it does with one that does not do: never one from a
//! session that executed the task, nor its orchestrator's but on a fast fix; an approve naming no
//! commit covers none made during its review; and a land interrupted after its merge is finished by
//! running it again.

use super::*;
use symphony_board::NewTask;

/// What the land tells the orchestrator when the approve is not one it goes by.
const STAFF: &str = "Staff a fresh reviewer, which approves naming the tip it read";

/// An approve the board wrote before it refused a former executor's verdict, as an older board
/// holds one: `by`'s, closing `SLUG` from review.
fn approved_by_an_older_board(r: &Repo, by: &str, text: &str) {
    r.board
        .store
        .landed(SLUG, by, &format!("approve: {text}"), true, &["review"])
        .unwrap();
}

#[test]
fn an_approve_from_a_session_that_executed_the_task_sends_it_back_to_review() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let (orch, exe) = (r.board.session("orchestrator"), r.board.session("worker"));
    let store = &r.board.store;
    r.task(SLUG);
    store.take_role(SLUG, &orch, "orchestrator").unwrap();
    store.claim(SLUG, &exe).unwrap();
    store.move_to(SLUG, "review", &exe).unwrap();
    // The executor lets its role go and reviews its own work: the board refuses its verdict now,
    // and the land refuses the one an older board took.
    store.release(SLUG, &exe, Some("executor")).unwrap();
    store.take_role(SLUG, &exe, "reviewer").unwrap();
    assert!(store.verdict(SLUG, &exe, "approve", &tip).is_err());
    approved_by_an_older_board(&r, &exe, &format!("{tip} is mine and good"));
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &orch);
    let line = format!(
        "{SLUG}'s approve is {exe}'s, which executed it: a fresh reviewer, one that did not write \
         or execute it, approves it"
    );
    assert_refused(&r, &result, &line, &before);
    let told = store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(
        told.text,
        format!("land refused: {line}; {SLUG} is back in review. {STAFF}")
    );
    assert_eq!(told.recipient.as_deref(), Some("@orchestrator"));
    assert_eq!(store.get(SLUG).unwrap().status, "review");
    assert!(runs(&r).is_empty(), "nothing is merged or checked");
    // A fresh reviewer's approve lands it.
    let rev = r.board.session("reviewer");
    store.take_role(SLUG, &rev, "reviewer").unwrap();
    store
        .verdict(SLUG, &rev, "approve", &format!("{tip}: good"))
        .unwrap();
    landed(&r, SLUG, &orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
}

#[test]
fn its_orchestrators_approve_lands_a_fast_fix_and_no_other_task() {
    let r = Repo::new();
    manifest(&r, "");
    let store = &r.board.store;
    let orch = r.board.session("orchestrator");
    let mut tips = Vec::new();
    for (slug, fast_fix) in [
        (SLUG, None),
        ("sim/fall", Some("a one-line fix, its test posted")),
    ] {
        let exe = r.board.session("worker");
        let wt = branch(&r, slug, &format!("{slug}.gd"), "extends Node\n");
        tips.push(head(&wt, "HEAD"));
        store
            .create(&NewTask {
                fast_fix: fast_fix.map(str::to_string),
                ..NewTask::new(slug, "A fix", &orch)
            })
            .unwrap();
        store.take_role(slug, &orch, "orchestrator").unwrap();
        store.claim(slug, &exe).unwrap();
        store.move_to(slug, "review", &exe).unwrap();
    }
    // On a task that is no fast fix the orchestrator approves only by taking the reviewer's role,
    // and the land refuses it: the orchestrator is no fresh reviewer.
    assert!(store.verdict(SLUG, &orch, "approve", "").is_err());
    store.take_role(SLUG, &orch, "reviewer").unwrap();
    store
        .verdict(SLUG, &orch, "approve", &format!("{}: fine", tips[0]))
        .unwrap();
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &orch);
    let line = format!(
        "{SLUG}'s approve is {orch}'s, its orchestrator's: a fresh reviewer approves it, and its \
         orchestrator only a fast fix (filed with `just tasks create --fast-fix WHY`)"
    );
    assert_refused(&r, &result, &line, &before);
    assert_eq!(store.get(SLUG).unwrap().status, "review");
    // The fast fix: its orchestrator stamps its posted evidence, and that is what lands it.
    store
        .verdict(
            "sim/fall",
            &orch,
            "approve",
            &format!("{}: its test is red before, green after", tips[1]),
        )
        .unwrap();
    let (said, result) = landed(&r, "sim/fall", &orch);
    let line = result.unwrap();
    assert!(
        said.starts_with(&format!("land sim/fall: approved by {orch} at ")),
        "{said}"
    );
    assert_eq!(head(&r.root, "main^2"), tips[1]);
    assert_eq!(last(&r, "sim/fall"), line);
}

#[test]
fn an_approve_naming_no_commit_covers_none_made_during_its_review() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let store = &r.board.store;
    let people = People {
        orch: r.board.session("orchestrator"),
        exe: r.board.session("worker"),
        rev: r.board.session("reviewer"),
    };
    r.task(SLUG);
    store.take_role(SLUG, &people.orch, "orchestrator").unwrap();
    store.claim(SLUG, &people.exe).unwrap();
    store.move_to(SLUG, "review", &people.exe).unwrap();
    store.take_role(SLUG, &people.rev, "reviewer").unwrap();
    // A commit while it is in review, maybe after its reviewer read the branch, then the approve.
    write(&wt, "sim/jump.gd", "extends Node2D\n");
    let when = format!("@{:.0} +0000", r.board.now() + 100.0);
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["commit", "-qam", "during the review"])
            .env("GIT_COMMITTER_DATE", &when)
            .env("GIT_AUTHOR_DATE", &when)
            .current_dir(&wt),
    )
    .unwrap();
    assert!(done.status.success());
    let tip = head(&wt, "HEAD");
    r.board.advance(600.0);
    store.verdict(SLUG, &people.rev, "approve", "good").unwrap();
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.clone().unwrap_err();
    assert!(
        line.starts_with(&format!(
            "{SLUG} has 1 commit made during its review (it went to review at "
        )),
        "{line}"
    );
    assert!(
        line.ends_with(&format!(
            "which an approval naming none of its commits does not cover: {} during the review; \
             its reviewer approves naming the tip",
            &tip[..7]
        )),
        "{line}"
    );
    assert_refused(&r, &result, &line, &before);
    let told = store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(told.recipient.as_deref(), Some("@reviewer"));
    assert!(
        told.text.ends_with(&format!(
            "; {SLUG} is back in review. Read what the approve does not cover, then approve the \
             tip naming it: `just tasks verdict {SLUG} approve \"{tip}: …\"`"
        )),
        "{}",
        told.text
    );
    store
        .verdict(SLUG, &people.rev, "approve", &format!("{tip}: read it all"))
        .unwrap();
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
}

/// A land killed after main moved to its merge, before its push and its landed line: main holds
/// the merge, origin does not, and the task's approve still stands.
fn interrupted(r: &Repo) -> (People, String) {
    let wt = branch(r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(r, SLUG, &format!("{tip} is good"));
    git(
        &r.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "--no-edit",
            "-m",
            &format!("Merge branch '{SLUG}'"),
            SLUG,
        ],
    );
    (people, tip)
}

#[test]
fn a_land_interrupted_after_its_merge_is_finished_by_running_it_again() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    let (people, tip) = interrupted(&r);
    let merge = head(&r.root, "main");
    // Main moved on past the merge meanwhile: the merge is still found, under it.
    write(&r.root, "notes.md", "later\n");
    git(&r.root, &["add", "notes.md"]);
    git(&r.root, &["commit", "-qm", "later on main"]);
    let top = head(&r.root, "main");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, finishing a land interrupted after its merge; pushed \
             to origin",
            &merge[..12]
        )
    );
    assert!(
        said.contains(&format!(
            "land {SLUG}: its tip {} is on main already, by {}: finishing the land (the push, \
             the landed line, the sweep)",
            &tip[..7],
            &merge[..7]
        )),
        "{said}"
    );
    assert_eq!(head(&r.root, "main"), top, "nothing merged again");
    assert_eq!(head(&bare, "main"), top);
    assert!(runs(&r).is_empty(), "nothing checked again");
    assert_eq!(last(&r, SLUG), line);
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    // Finished, it is closed: a land again finds no approve standing.
    let again = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert!(
        again.starts_with(&format!(
            "{SLUG} was closed after its approval: landed {SLUG} on main"
        )),
        "{again}"
    );
}

#[test]
fn a_merge_on_main_of_a_tip_the_approve_does_not_cover_is_not_finished() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let approved_tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{approved_tip} is good"));
    write(&wt, "sim/jump.gd", "extends Node2D\n");
    git(&wt, &["commit", "-qam", "one more thing"]);
    let late = head(&wt, "HEAD");
    git(
        &r.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "--no-edit",
            "-m",
            "merged by hand",
            SLUG,
        ],
    );
    let merge = head(&r.root, "main");
    let pushed = head(&bare, "main");
    let line = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert_eq!(
        line,
        format!(
            "{SLUG}'s tip {} is on main already, by {}, but {SLUG} has 1 commit after the \
             approved {}: {} one more thing",
            &late[..7],
            &merge[..7],
            &approved_tip[..7],
            &late[..7]
        )
    );
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(
        told.text,
        format!(
            "land refused: {line}. Nothing is pushed or closed: what landed is not what was \
             approved"
        )
    );
    assert_eq!(told.recipient.as_deref(), Some("@orchestrator"));
    assert_eq!(head(&bare, "main"), pushed, "nothing pushed");
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
}
