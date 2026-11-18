//! What goes wrong, on the board in memory, each fault planted exactly: an answer lost after
//! the write it answered, a run cut short, a token the board refuses, a board restored, the
//! state lost, a repository that is no longer the one bound, an issue or a credential refused,
//! a rate limit, issues that move while they are listed, and the adapter turned off mid-run.

use crate::github::Fault;
use crate::{Rig, memory};
use symphony_github::plan::{import_slug, marker_for};
use symphony_github::state::{Event, Flag};

const IMPORT: &str = "issues = \"import\"\nimport_labels = [\"symphony\"]";
const EXCHANGE: &str = "issues = \"exchange\"\nimport_labels = [\"symphony\"]";

fn draft(rig: &Rig, slug: &str, title: &str, body: &str) {
    let file = rig.dir.0.join("public.md");
    std::fs::write(&file, body).unwrap();
    rig.run(&[
        "publish",
        slug,
        "--title",
        title,
        "--body-file",
        file.to_str().unwrap(),
    ])
    .ok();
}

#[test]
fn a_publication_whose_answer_was_lost_is_found_by_its_marker_and_not_made_twice() {
    let rig = Rig::new("lost-create", EXCHANGE);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    let digest = rig.preview().digest().unwrap();
    rig.github
        .fault("POST", "/repos/o/n/issues", Fault::LoseAnswer);

    let cut = rig.reconcile(&digest);
    assert_eq!(cut.code, 1);
    assert!(cut.err.contains("result unknown"), "{}", cut.err);
    assert_eq!(rig.github.issues().len(), 1, "GitHub made the issue");
    assert!(
        rig.state().mappings.is_empty(),
        "and nothing is mapped on an unknown result"
    );
    assert_eq!(rig.journal().last().unwrap().event, Event::Unknown);

    // Run again with the same digest: the issue is looked for before anything is created.
    rig.reconcile(&digest).ok();
    assert_eq!(rig.github.issues().len(), 1, "no second issue");
    let creates = rig
        .github
        .asked()
        .iter()
        .filter(|(method, ..)| method == "POST")
        .count();
    assert_eq!(creates, 1);
    assert_eq!(rig.state().mappings["tools/a"].issue_number, 1);
    assert_eq!(rig.preview().digest(), None);
}

#[test]
fn a_publication_never_sent_is_made_by_the_next_run_and_duplicates_stop_it() {
    // The request never left: the next run looks, finds none, and creates one.
    let rig = Rig::new("unsent-create", EXCHANGE);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    let digest = rig.preview().digest().unwrap();
    rig.github
        .fault("POST", "/repos/o/n/issues", Fault::Status(502, vec![]));
    assert_eq!(rig.reconcile(&digest).code, 1);
    assert_eq!(rig.github.issues(), Vec::<serde_json::Value>::new());
    rig.reconcile(&digest).ok();
    assert_eq!(rig.github.issues().len(), 1);

    // Two issues carry the marker: neither is adopted, and none is made.
    let rig = Rig::new("duplicate-create", EXCHANGE);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    let digest = rig.preview().digest().unwrap();
    rig.github
        .fault("POST", "/repos/o/n/issues", Fault::LoseAnswer);
    assert_eq!(rig.reconcile(&digest).code, 1);
    let marker = marker_for(77, "tools/a");
    rig.github.open(
        "Public",
        &format!("Body.\n\n<!-- symphony-github:{marker} -->"),
        &[],
    );
    let stopped = rig.reconcile(&digest).ok();
    assert!(
        stopped.out.contains("2 issues carry its marker"),
        "{}",
        stopped.out
    );
    assert_eq!(rig.github.issues().len(), 2, "no third");
    assert!(rig.state().mappings.is_empty());
    // And a later preview maps neither and publishes nothing.
    let preview = rig.preview();
    assert!(
        preview
            .out
            .contains("more than one issue carries its marker"),
        "{}",
        preview.out
    );
    assert_eq!(preview.digest(), None);
}

#[test]
fn board_answers_lost_after_the_write_are_settled_without_a_second_write() {
    let rig = Rig::new("lost-board", IMPORT);
    let number = rig.github.open("Title", "Text.", &["symphony"]);
    let slug = import_slug(77, number);

    // The proposal is made and its answer lost.
    let digest = rig.preview().digest().unwrap();
    rig.board.fault("propose", memory::Fault::LoseAnswer);
    let cut = rig.reconcile(&digest);
    assert_eq!(cut.code, 1, "{}", cut.out);
    assert_eq!(rig.board.slugs(), std::slice::from_ref(&slug));
    assert!(rig.state().mappings.is_empty());
    rig.reconcile(&digest).ok();
    assert_eq!(
        rig.board.slugs(),
        std::slice::from_ref(&slug),
        "one proposal"
    );
    assert_eq!(rig.state().mappings[&slug].task_id, 1);

    // The edit is made and its answer lost.
    rig.github.edit(number, "Title 2", "Text 2.");
    let digest = rig.preview().digest().unwrap();
    rig.board.fault("edit", memory::Fault::LoseAnswer);
    assert_eq!(rig.reconcile(&digest).code, 1);
    assert_eq!(rig.board.task(&slug).unwrap().title, "Title 2");
    let edits = |rig: &Rig| {
        rig.board
            .writes()
            .iter()
            .filter(|w| w.starts_with("edit"))
            .count()
    };
    assert_eq!(edits(&rig), 1);
    rig.reconcile(&digest).ok();
    assert_eq!(edits(&rig), 1, "the edit is read back, not made again");
    assert_eq!(rig.preview().digest(), None);
}

#[test]
fn a_run_cut_short_goes_on_from_its_journal_and_a_bounded_run_stops_at_its_limit() {
    let rig = Rig::new("interrupted", IMPORT);
    for n in 0..4 {
        rig.github
            .open(&format!("Issue {n}"), "Text.", &["symphony"]);
    }
    let digest = rig.preview().digest().unwrap();
    // Bounded: the binding and one proposal, then it stops.
    let first = rig
        .run(&[
            "reconcile",
            "--preview",
            &digest,
            "--as",
            "amber-fox-1",
            "--limit",
            "2",
        ])
        .ok();
    assert!(
        first.out.contains("2 applied, 0 skipped, 3 wait"),
        "{}",
        first.out
    );
    assert_eq!(rig.board.slugs().len(), 1);

    // Cut short after the acknowledgment and before the state was kept: the state file is
    // put back to what it was before the run, and the journal still holds the results.
    let state_file = rig.dir.0.join("state/state.json");
    let before = std::fs::read(&state_file).unwrap();
    rig.run(&[
        "reconcile",
        "--preview",
        &digest,
        "--as",
        "amber-fox-1",
        "--limit",
        "1",
    ])
    .ok();
    assert_eq!(rig.board.slugs().len(), 2);
    std::fs::write(&state_file, &before).unwrap();
    assert_eq!(rig.state().mappings.len(), 1);

    rig.reconcile(&digest).ok();
    assert_eq!(rig.board.slugs().len(), 4, "each proposal made once");
    let proposals = rig
        .board
        .writes()
        .iter()
        .filter(|w| w.starts_with("propose"))
        .count();
    assert_eq!(proposals, 4);
    assert_eq!(
        rig.state().mappings.len(),
        4,
        "the acknowledged one was settled from the journal"
    );
    assert!(rig.reconcile(&digest).ok().out.contains("already applied"));
    assert_eq!(rig.preview().digest(), None);
}

#[test]
fn a_token_the_board_refuses_ends_the_run_and_nothing_is_tried_on_the_newer_task() {
    let rig = Rig::new("refused-token", IMPORT);
    let first = rig.github.open("One", "Text.", &["symphony"]);
    let second = rig.github.open("Two", "Text.", &["symphony"]);
    let (one, two) = (import_slug(77, first), import_slug(77, second));
    rig.sync();
    rig.github.edit(first, "One edited", "Remote.");
    rig.github.edit(second, "Two edited", "Remote.");
    let digest = rig.preview().digest().unwrap();
    // Between the preview and the reconcile the first task is rewritten here.
    rig.board.rewrite(&one, "One", "Rewritten here meanwhile.");

    let writes = rig.board.writes();
    let ended = rig.reconcile(&digest);
    assert_eq!(ended.code, 1);
    assert!(
        ended.err.contains("the board refused the edit"),
        "{}",
        ended.err
    );
    assert!(ended.err.contains("Preview again"), "{}", ended.err);
    assert_eq!(
        rig.board.writes(),
        writes,
        "nothing was written, the second edit included"
    );
    assert_eq!(
        rig.board.task(&one).unwrap().body,
        "Rewritten here meanwhile."
    );
    assert_eq!(rig.board.task(&two).unwrap().title, "Two");

    // The preview is gone with the refusal: the same digest is no retry.
    let gone = rig.reconcile(&digest);
    assert_eq!(gone.code, 1);
    assert!(gone.err.contains("no preview"), "{}", gone.err);
    assert_eq!(rig.board.writes(), writes);

    // A new preview reads the task again: the first is a conflict, the second is written.
    rig.sync();
    assert_eq!(rig.state().mappings[&one].flag, Some(Flag::Conflict));
    assert_eq!(
        rig.board.task(&one).unwrap().body,
        "Rewritten here meanwhile."
    );
    assert_eq!(rig.board.task(&two).unwrap().title, "Two edited");
}

#[test]
fn a_restored_board_suspends_its_mappings_until_the_operator_takes_them_up() {
    let rig = Rig::new("restored", IMPORT);
    let first = rig.github.open("One", "Text.", &["symphony"]);
    let second = rig.github.open("Two", "Text.", &["symphony"]);
    let (one, two) = (import_slug(77, first), import_slug(77, second));
    rig.sync();

    // The board is put back from an older copy, and the second slug now names another task.
    rig.board.restore();
    rig.board.replace(&two, "Something else", "Unrelated.");
    rig.github.edit(first, "One edited", "Remote.");
    rig.github.edit(second, "Two edited", "Remote.");
    let writes = rig.board.writes();
    let said = rig.sync();
    assert!(said.contains("the board was restored or rebound"), "{said}");
    assert!(said.contains("the slug names another task now"), "{said}");
    assert_eq!(
        rig.board.writes(),
        writes,
        "no task was written by a mapping kept before"
    );
    assert_eq!(rig.board.task(&two).unwrap().title, "Something else");
    let state = rig.state();
    assert!(matches!(
        state.mappings[&one].flag,
        Some(Flag::Suspended(_))
    ));
    assert!(matches!(
        state.mappings[&two].flag,
        Some(Flag::Suspended(_))
    ));

    // Suspended mappings are said and left alone.
    let preview = rig.preview();
    assert!(
        preview.out.contains("suspended, not exchanged"),
        "{}",
        preview.out
    );
    assert_eq!(preview.digest(), None);

    // The operator takes the first up again, keeping the remote side.
    rig.run(&["resolve", &one, "remote"]).ok();
    rig.sync();
    assert_eq!(rig.board.task(&one).unwrap().title, "One edited");
    assert_eq!(rig.state().mappings[&one].flag, None);
    assert_eq!(rig.board.task(&two).unwrap().title, "Something else");

    // A task gone from the board suspends its mapping too.
    rig.board.remove(&one);
    rig.github.edit(first, "One again", "Remote.");
    let said = rig.sync();
    assert!(
        said.contains("the task is no longer on the board"),
        "{said}"
    );
    assert_eq!(rig.board.slugs(), [two], "and it is not imported again");
}

#[test]
fn a_board_with_no_recovery_binding_is_written_by_no_mapping() {
    let rig = Rig::new("unbound", IMPORT);
    let number = rig.github.open("One", "Text.", &["symphony"]);
    rig.sync();
    rig.board.unbind();
    rig.github.edit(number, "One edited", "Remote.");
    let preview = rig.preview();
    assert!(
        preview.out.contains("the board gives no token"),
        "{}",
        preview.out
    );
    assert_eq!(preview.digest(), None);
    assert_eq!(
        rig.board.task(&import_slug(77, number)).unwrap().title,
        "One"
    );
}

#[test]
fn lost_state_is_mapped_again_from_both_sides_and_nothing_is_published_or_imported_twice() {
    let rig = Rig::new("state-loss", EXCHANGE);
    let number = rig.github.open("Imported", "Text.", &["symphony"]);
    let slug = import_slug(77, number);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    rig.sync();
    assert_eq!(rig.state().mappings.len(), 2);
    let (issues, tasks) = (rig.github.issues().len(), rig.board.slugs().len());

    // Everything kept is lost.
    for file in ["state.json", "journal.jsonl", "preview.json"] {
        let _ = std::fs::remove_file(rig.dir.0.join("state").join(file));
    }
    let preview = rig.preview();
    assert!(
        preview
            .out
            .contains(&format!("map {slug} to issue #1 again")),
        "{}",
        preview.out
    );
    assert!(
        preview.out.contains("map tools/a to issue #2 again"),
        "{}",
        preview.out
    );
    assert!(!preview.out.contains("publish tools/a"), "{}", preview.out);
    assert!(!preview.out.contains("import issue"), "{}", preview.out);
    rig.reconcile(&preview.digest().unwrap()).ok();
    assert_eq!(
        rig.github.issues().len(),
        issues,
        "nothing was published again"
    );
    assert_eq!(rig.board.slugs().len(), tasks, "nothing was imported again");
    let state = rig.state();
    assert_eq!(state.mappings.len(), 2);
    assert_eq!(
        state.drafts["tools/a"].body, "Body.",
        "the curated text is the issue's"
    );
    assert_eq!(
        rig.preview().digest(),
        None,
        "both agree, so nothing follows"
    );

    // A task that only has the name of an import, and not its issue, is never adopted.
    let rig = Rig::new("collision", IMPORT);
    let number = rig.github.open("Imported", "Text.", &["symphony"]);
    rig.board
        .file(&import_slug(77, number), "Someone's own task", "Unrelated.");
    let preview = rig.preview();
    assert!(
        preview.out.contains("is not issue #1's proposal"),
        "{}",
        preview.out
    );
    rig.sync();
    assert!(rig.state().mappings.is_empty());
    assert_eq!(
        rig.board.task(&import_slug(77, number)).unwrap().body,
        "Unrelated."
    );
}

#[test]
fn lost_state_whose_sides_no_longer_agree_is_a_conflict_not_an_overwrite() {
    let rig = Rig::new("state-loss-diverged", IMPORT);
    let number = rig.github.open("Imported", "Text.", &["symphony"]);
    let slug = import_slug(77, number);
    rig.sync();
    rig.board.rewrite(
        &slug,
        "Imported",
        &format!("{}\nRefined here.", rig.board.task(&slug).unwrap().body),
    );
    std::fs::remove_file(rig.dir.0.join("state/state.json")).unwrap();
    rig.sync();
    rig.sync();
    assert_eq!(rig.state().mappings[&slug].flag, Some(Flag::Conflict));
    assert!(
        rig.board
            .task(&slug)
            .unwrap()
            .body
            .ends_with("Refined here.")
    );
}

#[test]
fn another_repository_at_the_name_pauses_until_an_explicit_rebind() {
    let rig = Rig::new("identity", EXCHANGE);
    let number = rig.github.open("One", "Text.", &["symphony"]);
    let slug = import_slug(77, number);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    rig.sync();

    rig.github.replace_repository(88);
    let paused = rig.run(&["preview"]);
    assert_eq!(paused.code, 1);
    assert!(
        paused.err.contains("repository id 88, not the id 77"),
        "{}",
        paused.err
    );
    assert!(rig.run(&["status"]).ok().out.contains("paused:"));
    let refused = rig.reconcile("0000");
    assert_eq!(refused.code, 1);
    assert_eq!(
        rig.github.writes(),
        1,
        "only the first publication was ever written"
    );

    // The operator binds to it: the mappings are kept and suspended, nothing is published.
    let preview = rig.run(&["preview", "--rebind"]).ok();
    assert!(
        preview
            .out
            .contains("bind this state to the repository o/n (id 88)"),
        "{}",
        preview.out
    );
    rig.reconcile(&preview.digest().unwrap()).ok();
    let state = rig.state();
    assert_eq!(state.binding.unwrap().repository_id, 88);
    assert!(matches!(
        state.mappings[&slug].flag,
        Some(Flag::Suspended(_))
    ));
    assert!(matches!(
        state.mappings["tools/a"].flag,
        Some(Flag::Suspended(_))
    ));
    rig.sync();
    assert_eq!(rig.github.writes(), 1, "the backlog is not published again");
    assert_eq!(rig.board.slugs().len(), 2);
}

#[test]
fn a_renamed_repository_pauses_until_the_committed_settings_name_it() {
    let rig = Rig::new("renamed", IMPORT);
    rig.github.open("One", "Text.", &["symphony"]);
    rig.sync();
    rig.github.rename("n2");
    let paused = rig.run(&["preview"]);
    assert_eq!(paused.code, 1);
    assert!(
        paused.err.contains("moved to another name"),
        "{}",
        paused.err
    );
    assert!(rig.state().paused.is_some());

    // The settings name it: the same repository by its number, so its mappings go on.
    rig.settings.set(Some(
        crate::settings(&rig.token_file, IMPORT).replace("o/n", "o/n2"),
    ));
    let said = rig.sync();
    assert!(
        said.contains("bind this state to the repository o/n2 (id 77)"),
        "{said}"
    );
    let state = rig.state();
    assert_eq!(state.paused, None);
    assert_eq!(state.binding.unwrap().full_name, "o/n2");
    assert!(
        state
            .mappings
            .values()
            .all(|mapping| mapping.flag.is_none())
    );
}

#[test]
fn an_issue_not_accessible_is_flagged_and_nothing_local_is_deleted_or_published_again() {
    let rig = Rig::new("inaccessible", EXCHANGE);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    rig.sync();
    rig.github.hide(1);
    draft(&rig, "tools/a", "Public", "Body 2.");
    let said = rig.sync();
    assert!(
        said.contains("the issue is missing or not accessible"),
        "{said}"
    );
    assert_eq!(
        rig.state().mappings["tools/a"].flag,
        Some(Flag::Inaccessible)
    );
    assert!(rig.board.task("tools/a").is_some());
    assert_eq!(rig.state().drafts["tools/a"].body, "Body 2.");
    rig.sync();
    assert!(rig.github.issues().is_empty(), "it is not published again");

    // Hidden between the preview and the reconcile: found by the read before the write.
    let rig = Rig::new("inaccessible-late", EXCHANGE);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    rig.sync();
    draft(&rig, "tools/a", "Public", "Body 2.");
    let digest = rig.preview().digest().unwrap();
    rig.github.hide(1);
    let applied = rig.reconcile(&digest).ok();
    assert!(
        applied.out.contains("missing or not accessible"),
        "{}",
        applied.out
    );
    assert_eq!(
        rig.state().mappings["tools/a"].flag,
        Some(Flag::Inaccessible)
    );
    assert_eq!(rig.github.writes(), 1);
}

#[test]
fn an_issue_edited_between_the_preview_and_the_reconcile_is_not_overwritten() {
    let rig = Rig::new("late-edit", EXCHANGE);
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    rig.sync();
    draft(&rig, "tools/a", "Public", "Body 2.");
    let digest = rig.preview().digest().unwrap();
    rig.github.edit(1, "Public", "Edited there meanwhile.");
    let applied = rig.reconcile(&digest).ok();
    assert!(
        applied.out.contains("changed since the preview"),
        "{}",
        applied.out
    );
    assert_eq!(rig.github.issue(1)["body"], "Edited there meanwhile.");
    assert_eq!(rig.state().mappings["tools/a"].flag, Some(Flag::Conflict));
    assert_eq!(
        rig.state().drafts["tools/a"].body,
        "Body 2.",
        "the intended text is kept"
    );
}

#[test]
fn a_refused_credential_pauses_and_keeps_everything() {
    let rig = Rig::new("unauthorized", IMPORT);
    rig.github.open("One", "Text.", &["symphony"]);
    rig.sync();
    let kept = rig.state();
    rig.github
        .fault("GET", "/repos/o/n", Fault::Status(401, vec![]));
    let refused = rig.run(&["preview"]);
    assert_eq!(refused.code, 1);
    assert!(
        refused.err.contains("refused the credential"),
        "{}",
        refused.err
    );
    let state = rig.state();
    assert!(state.paused.is_some());
    assert_eq!(state.mappings, kept.mappings);

    // Refused mid-run: the run ends, paused, and the preview is dropped.
    let rig = Rig::new("unauthorized-run", "issues = \"export\"");
    rig.board.file("tools/a", "A", "brief");
    draft(&rig, "tools/a", "Public", "Body.");
    let digest = rig.preview().digest().unwrap();
    rig.github
        .fault("POST", "/repos/o/n/issues", Fault::Status(401, vec![]));
    let ended = rig.reconcile(&digest);
    assert_eq!(ended.code, 1);
    assert!(rig.state().paused.is_some());
    assert!(rig.reconcile(&digest).err.contains("no preview"));
    // The credential works again: a preview lifts the pause.
    rig.sync();
    assert_eq!(rig.state().paused, None);
    assert_eq!(rig.github.issues().len(), 1);
}

#[test]
fn a_rate_limit_stops_everything_until_its_time_and_the_run_goes_on_after() {
    let rig = Rig::new("rate-limit", "issues = \"export\"");
    rig.board.file("tools/a", "A", "brief");
    rig.board.file("tools/b", "B", "brief");
    draft(&rig, "tools/a", "Public A", "Body.");
    draft(&rig, "tools/b", "Public B", "Body.");
    let digest = rig.preview().digest().unwrap();
    rig.github.fault(
        "POST",
        "/repos/o/n/issues",
        Fault::Status(403, vec![("retry-after", "30".into())]),
    );
    let ended = rig.reconcile(&digest);
    assert_eq!(ended.code, 1);
    assert!(
        ended.err.contains("rate limited until 1030"),
        "{}",
        ended.err
    );
    assert_eq!(rig.state().rate_limited_until, Some(1030));

    // Before its time nothing is sent, by a reconcile or a preview.
    let asked = rig.github.asked().len();
    rig.now.set(1029);
    assert!(
        rig.reconcile(&digest)
            .err
            .contains("rate limit holds until 1030")
    );
    assert!(
        rig.run(&["preview"])
            .err
            .contains("rate limit holds until 1030")
    );
    assert_eq!(rig.github.asked().len(), asked);

    // After it the same preview goes on, each issue made once.
    rig.now.set(1030);
    rig.reconcile(&digest).ok();
    assert_eq!(rig.github.issues().len(), 2);

    // A reset time and an exhausted quota read the same; a listing that meets one keeps it.
    rig.github.fault(
        "GET",
        "/repos/o/n/issues",
        Fault::Status(
            403,
            vec![
                ("x-ratelimit-remaining", "0".into()),
                ("x-ratelimit-reset", "5000".into()),
            ],
        ),
    );
    assert!(
        rig.run(&["preview"])
            .err
            .contains("rate limited until 5000")
    );
    assert_eq!(rig.state().rate_limited_until, Some(5000));
}

#[test]
fn every_page_is_read_and_issues_that_move_between_pages_hold_the_cursor() {
    let rig = Rig::new("pages", IMPORT);
    rig.github.page_size(2);
    for n in 1..=5 {
        rig.github
            .open(&format!("Issue {n}"), "Text.", &["symphony"]);
    }
    rig.sync();
    assert_eq!(rig.board.slugs().len(), 5, "all three pages");
    let cursor = rig
        .state()
        .cursor
        .expect("a cursor once every page was read");
    assert_eq!(cursor, rig.github.issue(5)["updated_at"]);

    // The next listing starts at the cursor: one page, the newest issue again.
    let asked = rig.github.asked().len();
    assert_eq!(rig.preview().digest(), None);
    let lists: Vec<_> = rig.github.asked()[asked..]
        .iter()
        .filter(|(_, path, _)| path.contains("/issues?"))
        .map(|(_, path, _)| path.clone())
        .collect();
    assert_eq!(lists.len(), 1, "{lists:?}");
    assert!(lists[0].contains(&format!("since={cursor}")), "{lists:?}");

    // Two issues are edited while the pages are read, so they are listed twice and another
    // could have slipped between pages: what was read is used, and the cursor stays.
    for n in 6..=9 {
        rig.github
            .open(&format!("Issue {n}"), "Text.", &["symphony"]);
    }
    rig.github.shift_after_first_page(&[5, 6]);
    let said = rig.sync();
    assert!(said.contains("issues moved between pages"), "{said}");
    assert_eq!(
        rig.state().cursor,
        Some(cursor),
        "the cursor did not advance"
    );
    // The next, quiet listing reads from the same cursor and then advances.
    rig.sync();
    assert_eq!(rig.board.slugs().len(), 9);
    assert_eq!(
        rig.state().cursor.unwrap(),
        rig.github.issue(6)["updated_at"]
    );
}

#[test]
fn a_page_that_fails_fails_the_listing_and_advances_nothing() {
    let rig = Rig::new("page-fails", IMPORT);
    rig.github.page_size(2);
    for n in 1..=4 {
        rig.github
            .open(&format!("Issue {n}"), "Text.", &["symphony"]);
    }
    rig.github.fault(
        "GET",
        "/repos/o/n/issues?state=all&sort=updated&direction=asc&per_page=100&page=2",
        Fault::Status(500, vec![]),
    );
    let failed = rig.run(&["preview"]);
    assert_eq!(failed.code, 1);
    assert!(failed.err.contains("listing the issues"), "{}", failed.err);
    assert_eq!(rig.state().cursor, None);
    assert_eq!(rig.board.slugs(), Vec::<String>::new());
    rig.sync();
    assert_eq!(rig.board.slugs().len(), 4);
}

#[test]
fn turned_off_during_a_run_the_next_dispatch_is_not_made_and_turning_on_previews_anew() {
    let rig = Rig::new("disable", "issues = \"export\"");
    rig.board.file("tools/a", "A", "brief");
    rig.board.file("tools/b", "B", "brief");
    draft(&rig, "tools/a", "Public A", "Body.");
    draft(&rig, "tools/b", "Public B", "Body.");
    let digest = rig.preview().digest().unwrap();
    // The settings are read once as the run begins and once before each dispatch: turned off
    // after the first dispatch's check.
    rig.settings.after(2, None);
    let ended = rig.reconcile(&digest);
    assert_eq!(ended.code, 1);
    assert!(ended.err.contains("turned off"), "{}", ended.err);
    assert!(
        ended.err.contains("nothing more was dispatched"),
        "{}",
        ended.err
    );
    assert_eq!(
        rig.github.issues().len(),
        1,
        "the second was not dispatched"
    );

    // Turned off: nothing runs, and work accumulates here.
    assert_eq!(rig.reconcile(&digest).code, 1);
    rig.board.file("tools/c", "C", "brief");
    draft(&rig, "tools/c", "Public C", "Body.");
    assert!(rig.preview().out.contains("turned off"));
    assert_eq!(rig.github.issues().len(), 1);

    // Turned on again: the old preview is not replayed, and a new one shows what waits.
    rig.enable("issues = \"export\"");
    let stale = rig.reconcile(&digest);
    assert_eq!(stale.code, 1);
    assert!(stale.err.contains("no preview"), "{}", stale.err);
    assert_eq!(rig.github.issues().len(), 1, "nothing replayed");
    let preview = rig.preview();
    assert!(preview.out.contains("publish tools/b"), "{}", preview.out);
    assert!(preview.out.contains("publish tools/c"), "{}", preview.out);
    assert!(!preview.out.contains("publish tools/a"), "{}", preview.out);
    rig.reconcile(&preview.digest().unwrap()).ok();
    assert_eq!(rig.github.issues().len(), 3);
}

#[test]
fn turned_off_during_a_run_the_board_is_written_no_further_either() {
    let rig = Rig::new("disable-board", IMPORT);
    for n in 0..3 {
        rig.github
            .open(&format!("Issue {n}"), "Text.", &["symphony"]);
    }
    let digest = rig.preview().digest().unwrap();
    rig.settings.after(2, None);
    let ended = rig.reconcile(&digest);
    assert_eq!(ended.code, 1);
    assert!(
        ended.err.contains("nothing more was dispatched"),
        "{}",
        ended.err
    );
    assert_eq!(
        rig.board.slugs().len(),
        1,
        "one proposal before it was turned off, none after"
    );
}

#[test]
fn one_writer_at_a_time_and_a_kept_preview_that_was_altered_is_refused() {
    let rig = Rig::new("lock", IMPORT);
    rig.github.open("One", "Text.", &["symphony"]);
    let digest = rig.preview().digest().unwrap();
    {
        let _held = rig.store().lock().unwrap();
        let refused = rig.reconcile(&digest);
        assert_eq!(refused.code, 1);
        assert!(
            refused.err.contains("one writer at a time"),
            "{}",
            refused.err
        );
    }
    let file = rig.dir.0.join("state/preview.json");
    let altered = std::fs::read_to_string(&file)
        .unwrap()
        .replace("One", "Another");
    std::fs::write(&file, altered).unwrap();
    let refused = rig.reconcile(&digest);
    assert_eq!(refused.code, 1);
    assert!(
        refused.err.contains("not the one its digest names"),
        "{}",
        refused.err
    );
    assert_eq!(rig.board.slugs(), Vec::<String>::new());
}
