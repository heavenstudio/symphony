//! A reviewer launch goes back to the task's recorded reviewer: its own conversation, handle,
//! worktree, agent and model, in a new terminal; refused while it still runs; a new reviewer only
//! where none can be resumed, or beside it saying why (`--beside WHY`); never on an instruction
//! subtask; and, through `just tasks`, only in the project.

use crate::World;
use crate::common::refused;
use serde_json::json;
use std::fs;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::{Ask, Started, launch};
use symphony_board::rows::TaskSessionRow;
use symphony_board::{Error, NewLaunch};

/// sim/jump in review by an executor of its own.
fn in_review() -> World {
    let w = World::new();
    let executor = w.f.session("worker");
    w.store().claim("sim/jump", &executor).unwrap();
    w.store().move_to("sim/jump", "review", &executor).unwrap();
    w
}

/// A reviewer launch on `slug`, the agent `kind` (Claude for `None`).
fn reviewer(w: &World, slug: &str, kind: Option<&str>) -> Ask {
    Ask {
        role: "reviewer".into(),
        kind: Some(kind.unwrap_or("claude").into()),
        ..w.ask(slug)
    }
}

/// A Claude reviewer launched on sim/jump; with `codex`, a Codex one whose thread `codex:THREAD`
/// its callback registered.
fn reviewing(w: &World, codex: bool) -> TaskSessionRow {
    let kind = codex.then_some("codex");
    let s = w.try_launch(&reviewer(w, "sim/jump", kind)).unwrap().row;
    if !codex {
        return s;
    }
    w.store()
        .record_launch(&NewLaunch {
            claude_session: "codex:0abc-def".into(),
            worktree: s.worktree,
            pane: s.pane,
            model: s.model,
            ..NewLaunch::new("sim/jump", &s.handle, "reviewer")
        })
        .unwrap()
}

/// Its terminal goes, and the board records the closure, as the idle sweep does.
fn closed(w: &World, s: &TaskSessionRow) {
    w.hc().close(&s.pane).unwrap();
    w.store()
        .close_task_session(&s.slug, &s.handle, "idle an hour", "restore line")
        .unwrap();
}

/// The board's reviewer sessions: a resume registers none.
fn reviewers(w: &World) -> Vec<String> {
    let sessions = w.store().sessions().unwrap().into_iter();
    sessions
        .filter(|s| s.row.kind == "reviewer")
        .map(|s| s.row.handle)
        .collect()
}

/// The last `hc run`, past its `--`: what it started, `env` and its settings first.
fn last_started(w: &World) -> Vec<String> {
    let run = w.calls_of(&["run"]).pop().unwrap();
    run[run.iter().position(|a| a == "--").unwrap() + 1..].to_vec()
}

#[test]
fn a_reviewer_launch_resumes_the_tasks_recorded_reviewer_in_its_own_conversation() {
    for codex in [false, true] {
        let w = in_review();
        let first = reviewing(&w, codex);
        closed(&w, &first);
        // Asked for another agent and model: the reviewer resumed is still its own.
        let again = w
            .try_launch(&Ask {
                model: "other-model".into(),
                ..reviewer(&w, "sim/jump", Some(if codex { "claude" } else { "codex" }))
            })
            .unwrap();
        assert!(again.resumed, "{codex}");
        let row = &again.row;
        assert_eq!(
            (row.handle.as_str(), row.claude_session.as_str(), row.closed),
            (first.handle.as_str(), first.claude_session.as_str(), None),
            "{codex}"
        );
        assert_ne!(row.pane, first.pane);
        assert_eq!(w.live(), [row.pane.as_str()]);
        assert_eq!(
            reviewers(&w),
            [first.handle.as_str()],
            "no handle registered"
        );
        let started = last_started(&w);
        assert_eq!(started[1], format!("TASKS_SESSION={}", first.handle));
        let program = w.program(&row.pane);
        if codex {
            assert_eq!(program[0], "codex");
            assert_eq!(program[program.len() - 2..], ["resume", "0abc-def"]);
            assert!(program.contains(&"sonnet".to_string()), "{program:?}");
        } else {
            assert_eq!(
                program,
                [
                    "claude",
                    "--resume",
                    &first.claude_session,
                    "-n",
                    &first.handle,
                    "--permission-mode",
                    "auto",
                    "--model",
                    "sonnet",
                ]
            );
        }
        let review = if codex {
            "$review-task"
        } else {
            "/review-task"
        };
        assert_eq!(
            w.sent(&row.pane),
            [format!("{review} sim/jump {}", first.handle)]
        );
        let t = w.terminal(&row.pane);
        assert_eq!(t["cwd"].as_str(), Some(first.worktree.as_str()));
        assert_eq!(
            w.tab_name(t["labels"]["herdr.tab"].as_str().unwrap()).1,
            "review"
        );
        let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!(&record, row);
    }
}

#[test]
fn a_reviewer_launch_is_refused_while_the_recorded_reviewer_still_runs() {
    let still = |w: &World, handle: &str, terminal: &str| {
        let board = w.store().board_command();
        format!(
            "{handle} reviews sim/jump and still runs in terminal {terminal}: give it the new tip \
             on the thread ({board} say sim/jump TEXT --to @reviewer), or close it ({board} close \
             sim/jump {handle}) and launch again to resume it, or launch another beside it saying \
             why ({board} launch sim/jump --role reviewer --beside WHY)"
        )
    };
    // In the terminal its launch recorded; a Codex one whose thread never arrived among them, for
    // it runs all the same.
    for codex in [false, true] {
        let w = in_review();
        let kind = codex.then_some("codex");
        let first = w.try_launch(&reviewer(&w, "sim/jump", kind)).unwrap().row;
        let records = w.store().task_sessions("sim/jump").unwrap();
        assert_eq!(
            refused(w.try_launch(&reviewer(&w, "sim/jump", None))),
            still(&w, &first.handle, &first.pane)
        );
        assert_eq!(w.calls_of(&["run"]).len(), 1);
        assert_eq!(reviewers(&w), [first.handle.as_str()]);
        assert_eq!(w.store().task_sessions("sim/jump").unwrap(), records);
    }
    // Its record closed while its handle still runs in a terminal.
    let w = in_review();
    let first = reviewing(&w, false);
    w.store()
        .close_task_session("sim/jump", &first.handle, "idle", "restore line")
        .unwrap();
    assert_eq!(
        refused(w.try_launch(&reviewer(&w, "sim/jump", None))),
        still(&w, &first.handle, &first.pane)
    );
    // Its open record naming a live terminal that bears neither its name nor its label (one it was
    // given in, as staffing gives an idle agent a job).
    let w = in_review();
    let handle = w.f.session("reviewer");
    let pane = w.add_terminal(true, None, json!({}));
    w.store()
        .record_launch(&NewLaunch {
            claude_session: "conversation-given".into(),
            worktree: w.dir().display().to_string(),
            pane: pane.clone(),
            ..NewLaunch::new("sim/jump", &handle, "reviewer")
        })
        .unwrap();
    assert_eq!(
        refused(w.try_launch(&reviewer(&w, "sim/jump", None))),
        still(&w, &handle, &pane)
    );
    assert!(w.calls_of(&["run"]).is_empty());
}

#[test]
fn a_new_reviewer_starts_only_where_none_can_be_resumed() {
    // No conversation recorded, a Codex thread that never arrived, a reviewer that left.
    for case in ["no conversation", "no thread", "left"] {
        let w = in_review();
        let first = reviewing(&w, false);
        if case != "left" {
            let given = if case == "no thread" { "codex:" } else { "" };
            w.store()
                .record_launch(&NewLaunch {
                    claude_session: given.into(),
                    worktree: first.worktree.clone(),
                    pane: first.pane.clone(),
                    model: first.model.clone(),
                    ..NewLaunch::new("sim/jump", &first.handle, "reviewer")
                })
                .unwrap();
        }
        closed(&w, &first);
        if case == "left" {
            w.store().leave(&first.handle).unwrap();
        }
        // Gone past the lost line, as one the idle sweep closed is.
        w.f.advance(11.0 * 60.0);
        let again = w.try_launch(&reviewer(&w, "sim/jump", None)).unwrap();
        assert!(!again.resumed, "{case}");
        let row = &again.row;
        assert_ne!(row.handle, first.handle, "{case}");
        assert_eq!(
            &last_started(&w)[2..5],
            ["claude", "--session-id", &row.claude_session]
        );
        let task = w.store().get("sim/jump").unwrap();
        assert!(
            task.holders("reviewer").contains(&row.handle.as_str()),
            "{case}"
        );
        assert_eq!(reviewers(&w), [first.handle.as_str(), row.handle.as_str()]);
    }
}

#[test]
fn the_newest_reviewer_that_can_be_resumed_is_the_one_resumed() {
    let w = in_review();
    let older = reviewing(&w, false);
    closed(&w, &older);
    // A newer reviewer whose conversation nobody recorded, gone too.
    w.f.advance(60.0);
    let newer = w.f.session("reviewer");
    w.store()
        .record_launch(&NewLaunch {
            worktree: w.dir().display().to_string(),
            ..NewLaunch::new("sim/jump", &newer, "reviewer")
        })
        .unwrap();
    let again = w.try_launch(&reviewer(&w, "sim/jump", None)).unwrap();
    assert!(again.resumed);
    assert_eq!(again.row.handle, older.handle);
    assert_eq!(again.row.claude_session, older.claude_session);
}

#[test]
fn a_reviewer_on_an_instruction_subtask_is_refused_naming_the_task_it_judges() {
    let w = in_review();
    w.task("sim/jump/review", Some("sim/jump"));
    let before = w.store().sessions().unwrap().len();
    assert_eq!(
        refused(w.try_launch(&reviewer(&w, "sim/jump/review", None))),
        "sim/jump/review is an instruction subtask: a reviewer judges the task itself \
         (/review-task takes no subtask), so launch it there: just tasks launch sim/jump --role \
         reviewer"
    );
    assert_eq!(w.calls(), Vec::<Vec<String>>::new());
    assert_eq!(w.store().sessions().unwrap().len(), before);
    assert!(
        w.store()
            .task_sessions("sim/jump/review")
            .unwrap()
            .is_empty()
    );
    assert!(
        w.store()
            .get("sim/jump")
            .unwrap()
            .holders("reviewer")
            .is_empty()
    );
}

fn unasked(w: &World) -> Result<Started, Error> {
    launch(
        w.store(),
        &w.hc(),
        &w.unasked(),
        &reviewer(w, "sim/jump", None),
    )
}

#[test]
fn unasked_a_resume_starts_its_reviewer_only_in_the_project_and_never_a_new_one_instead() {
    // Its recorded worktree outside the project (an older record, or the binary run by itself):
    // refused, and no other reviewer is started in its place.
    let w = in_review();
    let elsewhere = w.dir().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let first = w
        .try_launch(&Ask {
            worktree: Some(elsewhere.clone()),
            ..reviewer(&w, "sim/jump", None)
        })
        .unwrap()
        .row;
    closed(&w, &first);
    let records = w.store().task_sessions("sim/jump").unwrap();
    assert_eq!(
        refused(unasked(&w)),
        w.outside(&format!(
            "{}'s recorded worktree {} is",
            first.handle,
            elsewhere.display()
        ))
    );
    assert_eq!(w.calls_of(&["run"]).len(), 1);
    assert_eq!(reviewers(&w), [first.handle.as_str()]);
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), records);

    // One in the project is resumed, under its own handle.
    let w = in_review();
    let first = reviewing(&w, false);
    closed(&w, &first);
    let again = unasked(&w).unwrap();
    assert!(again.resumed);
    assert_eq!(again.row.handle, first.handle);
    assert_eq!(
        w.program(&again.row.pane)[..3],
        ["claude", "--resume", first.claude_session.as_str()]
    );
}

#[test]
fn a_reviewer_launched_beside_the_recorded_one_is_a_new_reviewer_never_a_resume() {
    let w = in_review();
    let first = reviewing(&w, false);
    closed(&w, &first);
    // sessions() orders by connected, then handle: the second connects later.
    w.f.advance(60.0);
    let again = w
        .try_launch(&Ask {
            beside: Some("a second stamp".into()),
            ..reviewer(&w, "sim/jump", None)
        })
        .unwrap();
    assert!(!again.resumed);
    assert_ne!(again.row.handle, first.handle);
    assert_eq!(
        reviewers(&w),
        [first.handle.as_str(), again.row.handle.as_str()]
    );
    let task = w.store().get("sim/jump").unwrap();
    assert_eq!(
        task.holders("reviewer"),
        [first.handle.as_str(), again.row.handle.as_str()]
    );
}

#[test]
fn a_reviewer_there_with_nothing_to_resume_refuses_the_launch_naming_its_own_beside() {
    // One that joined by `tasks role`, as a subagent does: no record a launch can resume.
    let w = in_review();
    let there = w.f.session("reviewer");
    w.store().take_role("sim/jump", &there, "reviewer").unwrap();
    let refusal = refused(w.try_launch(&reviewer(&w, "sim/jump", None)));
    let board = w.store().board_command();
    assert!(
        refusal.starts_with(&format!("sim/jump is reviewed by {there} (")),
        "{refusal}"
    );
    assert!(
        refusal.ends_with(&format!(
            "one joins beside it with: {board} launch sim/jump --role reviewer --beside WHY"
        )),
        "{refusal}"
    );
    // Refused before its terminal: nothing started or recorded, and the role is still the one's.
    assert!(w.calls_of(&["run"]).is_empty());
    assert!(w.store().task_sessions("sim/jump").unwrap().is_empty());
    let task = w.store().get("sim/jump").unwrap();
    assert_eq!(task.holders("reviewer"), [there.as_str()]);
    // Saying why, it joins beside it. Roles are read by when they were taken, then by handle: the
    // fake clock stands still between writes, so the second joins later.
    w.f.advance(60.0);
    let again = w
        .try_launch(&Ask {
            beside: Some("a second stamp".into()),
            ..reviewer(&w, "sim/jump", None)
        })
        .unwrap();
    assert!(!again.resumed);
    let task = w.store().get("sim/jump").unwrap();
    assert_eq!(
        task.holders("reviewer"),
        [there.as_str(), again.row.handle.as_str()]
    );
}
