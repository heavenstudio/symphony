//! Whose approve a land goes by, and what it does with one that does not do: never one from a
//! session that executed the task, nor its orchestrator's but on a fast fix; an approve naming no
//! commit covers none made during its review; a tip merged by hand is no land to finish; and an
//! approved task goes back to review while its tip is not on main.

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
        "{SLUG}'s approve is {exe}'s, which executed it or work under it: a fresh reviewer, one \
         that did not write or execute it, approves it"
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

/// A reviewer staffed as the skills say, on the instruction `SLUG/review` it claimed, is a fresh
/// reviewer: its approve lands the task. The executor of a work slice `SLUG/part` wrote the branch:
/// the board refuses its verdict, and the land the approve an older board took from it.
#[test]
fn the_reviewer_of_its_review_instruction_lands_it_and_a_slices_executor_does_not() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let store = &r.board.store;
    let (orch, exe) = (r.board.session("orchestrator"), r.board.session("worker"));
    let (part, rev) = (r.board.session("worker"), r.board.session("reviewer"));
    r.task(SLUG);
    store.take_role(SLUG, &orch, "orchestrator").unwrap();
    store.claim(SLUG, &exe).unwrap();
    let (slice, review) = (format!("{SLUG}/part"), format!("{SLUG}/review"));
    for (slug, instruction, who) in [(&slice, false, &part), (&review, true, &rev)] {
        store
            .create(&NewTask {
                parent: Some(SLUG.into()),
                instruction,
                assignee: Some(who.clone()),
                ..NewTask::new(slug, "A step", &orch)
            })
            .unwrap();
        store.claim(slug, who).unwrap();
    }
    store
        .done(&slice, &part, "integrated on the branch")
        .unwrap();
    store.move_to(SLUG, "review", &exe).unwrap();
    store.take_role(SLUG, &part, "reviewer").unwrap();
    assert!(store.verdict(SLUG, &part, "approve", &tip).is_err());
    approved_by_an_older_board(&r, &part, &format!("{tip}: my slice is good"));
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &orch);
    let line = format!(
        "{SLUG}'s approve is {part}'s, which executed it or work under it: a fresh reviewer, one \
         that did not write or execute it, approves it"
    );
    assert_refused(&r, &result, &line, &before);
    assert_eq!(store.get(SLUG).unwrap().status, "review");
    assert!(runs(&r).is_empty(), "nothing is merged or checked");
    store.take_role(SLUG, &rev, "reviewer").unwrap();
    store
        .verdict(SLUG, &rev, "approve", &format!("{tip}: its tests pass"))
        .unwrap();
    store.done(&review, &rev, "approved the tip").unwrap();
    let (said, result) = landed(&r, SLUG, &orch);
    result.unwrap();
    assert!(
        said.starts_with(&format!("land {SLUG}: approved by {rev} at ")),
        "{said}"
    );
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
        "{SLUG}'s approve is {orch}'s, which orchestrated it: a fresh reviewer approves it, and its \
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

/// The task the land sends back to review starts the project's review command, as a move to
/// review on the command line does, and its line on the thread says so.
#[test]
fn a_land_sending_the_task_back_to_review_starts_its_review_command() {
    use std::os::unix::fs::PermissionsExt as _;
    let r = Repo::new();
    let record = r.dir.join("record");
    write(
        &r.dir,
        "on-review.sh",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > '{0}.part'\nmv '{0}.part' '{0}'\n",
            record.display()
        ),
    );
    let script = r.dir.join("on-review.sh");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    manifest(
        &r,
        &format!(
            "\n[board]\non_review = [{}]\n",
            toml_string(&script.to_string_lossy())
        ),
    );
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let approved_tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{approved_tip} is good"));
    write(&wt, "sim/jump.gd", "extends Node2D\n");
    git(&wt, &["commit", "-qam", "one more thing"]);
    let late = head(&wt, "HEAD");
    // The board's review command is let start, as on the live board.
    let mut env = r.env(&r.root);
    env.vars.insert("TASKS_ON_REVIEW".into(), "on".into());
    let mut out = Vec::new();
    let refused = land(&r.board.store, &env, SLUG, &people.orch, None, &mut out);
    assert!(refused.is_err());
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "review");
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert!(
        told.text.contains(&format!(
            "; {SLUG} is back in review, its verify started ({} {SLUG} {SLUG} {late}). Read what",
            script.display()
        )),
        "{}",
        told.text
    );
    let until = Instant::now() + Duration::from_secs(60);
    while !record.exists() {
        assert!(Instant::now() < until, "the review command never ran");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(
        fs::read_to_string(&record).unwrap(),
        format!("{SLUG} {SLUG} {late}\n")
    );
}

/// `SLUG`'s approved tip merged into main by hand, as an orchestrator lands a task outside land:
/// the merge.
fn merged_by_hand(r: &Repo) -> String {
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
    head(&r.root, "main")
}

#[test]
fn a_tip_merged_by_hand_is_no_land_to_finish_and_does_not_go_back_to_review() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    let merge = merged_by_hand(&r);
    let pushed = head(&bare, "main");
    let thread = r.board.said(SLUG);
    let line = format!(
        "{SLUG}'s tip {} is on main already, merged outside land: nothing to finish",
        &tip[..7]
    );
    let refused = |r: &Repo| {
        let (_, result) = landed(r, SLUG, &people.orch);
        assert_eq!(result.as_ref().map_err(String::as_str), Err(line.as_str()));
        assert_eq!(head(&r.root, "main"), merge, "main stays where it was");
        assert_eq!(head(&bare, "main"), pushed, "nothing pushed");
        assert_eq!(
            r.board.said(SLUG),
            thread,
            "no landing recorded, nothing said"
        );
        assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
        assert!(runs(r).is_empty(), "nothing checked");
    };
    refused(&r);
    // A record of another land of the task, of a tip the branch has no more, is not this one's.
    let folder = r.root.join(".git/sf-tasks/land");
    Record {
        slug: SLUG.into(),
        tip: head(&r.root, "main^1"),
        merge: merge.clone(),
        checked: merge.clone(),
    }
    .write(&folder)
    .unwrap();
    refused(&r);
    // Nor does it go back to review, by the command line that reads the repository.
    let (code, _, err) = cli(&r, &["move", SLUG, "review", "--as", &people.exe]);
    assert_ne!(code, 0);
    assert_eq!(
        err.trim_end().lines().last().unwrap(),
        format!(
            "✖ {SLUG}'s tip {} is on main already, landed or merged by hand: it does not go back \
             to review, and what it still needs goes in a new task",
            &tip[..7]
        ),
        "{err}"
    );
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
}

/// `tasks ARGS` run in this process as the command line runs it, from the main checkout: its exit
/// code, what it printed and what it printed on stderr.
pub(super) fn cli(r: &Repo, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(
        &args,
        &r.env(&r.root),
        &mut "".as_bytes(),
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn an_approved_task_whose_tip_is_not_on_main_goes_back_to_review_by_the_command_line() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let (code, out, err) = cli(&r, &["move", SLUG, "review", "--as", &people.exe]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.lines().next().unwrap(), format!("{SLUG} is review"));
    // Its executor and its reviewer are told, where their inbox and PULL digest find it.
    let thread = r.board.store.thread(SLUG).unwrap();
    let told: Vec<(&str, Option<&str>)> = thread
        .iter()
        .rev()
        .take(2)
        .map(|m| (m.sender.as_str(), m.recipient.as_deref()))
        .collect();
    assert_eq!(
        told,
        [("board", Some("@reviewer")), ("board", Some("@executor"))]
    );
}
