//! `restore`: a closed session, or one whose terminal is gone (a herdr pane from before hc, or a
//! terminal closed outside the board), comes back in a new hc terminal, in its worktree, resuming
//! its conversation.

#[path = "recovery.rs"]
mod recovery;
#[path = "restore_review.rs"]
mod review_instructions;
#[path = "restore_startup.rs"]
mod startup;

use crate::common::refused;
use crate::{World, git};
use std::fs;
use std::path::Path;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::{Ask, GONE, Started, restore};
use symphony_board::rows::TaskSessionRow;
use symphony_board::{Error, NewLaunch};

/// A Claude executor of sim/jump: its conversation id is the launch's own.
fn launched(w: &World) -> Started {
    w.try_launch(&Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/jump")
    })
    .unwrap()
}

/// Its terminal goes, and the board records the closure.
fn close_terminal(w: &World, s: &TaskSessionRow) {
    w.hc().close(&s.pane).unwrap();
    w.store()
        .close_task_session(&s.slug, &s.handle, "idle", "restore line")
        .unwrap();
}

/// A reviewer recorded on `slug`, a review instruction, as such a record stands: the instruction
/// reserved, the reviewer's role on its parent, an idle Claude agent in a live terminal of its own
/// and the record on the instruction, its worktree under the worktree base. No launch makes one:
/// it sends a reviewer to the task itself.
fn instruction_reviewer(w: &World, slug: &str) -> TaskSessionRow {
    let handle = w.f.session("reviewer");
    let parent = w.store().get(slug).unwrap().parent.unwrap();
    w.store().reserve(slug, &handle, 3600.0).unwrap();
    // An older board let a second reviewer take the parent's role: beside one still there now.
    w.store()
        .take_role_beside(
            &parent,
            &handle,
            "reviewer",
            Some("an older board's record"),
        )
        .unwrap();
    let pane = w.add_terminal(true, Some(&handle), serde_json::json!({}));
    let worktree = w.wt.join(format!("{}-review", slug.replace('/', "-")));
    w.store()
        .record_launch(&NewLaunch {
            claude_session: format!("conversation-of-{handle}"),
            worktree: worktree.display().to_string(),
            pane,
            model: "sonnet".into(),
            ..NewLaunch::new(slug, &handle, "reviewer")
        })
        .unwrap()
}

fn restored(w: &World, slug: &str, handle: Option<&str>) -> Result<Vec<Started>, Error> {
    restore(w.store(), &w.hc(), &w.machine(), slug, handle, None)
}

/// The program the last `hc run` started, past `env` and its settings.
fn last_program(w: &World) -> Vec<String> {
    let run = w.calls_of(&["run"]).pop().unwrap();
    let argv = run[run.iter().position(|a| a == "--").unwrap() + 1..].to_vec();
    symphony_board::launch::hc::program(&argv).to_vec()
}

fn told(handle: &str) -> String {
    told_to(handle, "work")
}

/// What a restored session is told, to go on with the skill it works by.
fn told_to(handle: &str, skill: &str) -> String {
    format!(
        "You were restored after your pane was closed. First run `just tasks callback --as {handle}`, \
         then `just tasks inbox --as {handle}` and continue /{skill} sim/jump."
    )
}

#[test]
fn restore_resumes_the_recorded_conversation_in_a_new_terminal_in_the_recorded_worktree() {
    let w = World::new();
    let s = launched(&w).row;
    close_terminal(&w, &s);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    assert_eq!(
        last_program(&w),
        [
            "claude",
            "--resume",
            &s.claude_session,
            "-n",
            &s.handle,
            "--permission-mode",
            "auto",
            "--model",
            "sonnet",
        ]
    );
    assert_eq!(w.sent(&again.row.pane), [told(&s.handle)]);
    assert_ne!(again.row.pane, s.pane);
    assert_eq!(w.live(), [again.row.pane.as_str()]);
    let t = w.terminal(&again.row.pane);
    assert_eq!(t["cwd"].as_str(), Some(s.worktree.as_str()));
    assert_eq!(t["name"].as_str(), Some(s.handle.as_str()));
    assert_eq!(
        t["labels"]["symphony.handle"].as_str(),
        Some(s.handle.as_str())
    );
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (read.closed, read.pane.as_str()),
        (None, again.row.pane.as_str())
    );
    assert_eq!(read, again.row);
}

#[test]
fn a_codex_conversation_resumes_with_its_thread_without_the_daemon() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let s = w
        .store()
        .record_launch(&NewLaunch {
            claude_session: "codex:0abc-def".into(),
            worktree: s.worktree,
            branch: s.branch,
            pane: s.pane,
            model: "chosen-model".into(),
            ..NewLaunch::new("sim/jump", &s.handle, &s.role)
        })
        .unwrap();
    close_terminal(&w, &s);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    // Through the wrapper that hands Codex's shell the new terminal's address.
    let run = w.calls_of(&["run"]).pop().unwrap();
    let wrapper = symphony_board::launch::hc::IN_ITS_TERMINAL;
    assert!(
        run.windows(4).any(|w| w[..3] == wrapper && w[3] == "codex"),
        "{run:?}"
    );
    assert_eq!(
        last_program(&w),
        [
            "codex",
            "--no-daemon",
            "-s",
            "workspace-write",
            "-m",
            "chosen-model",
            "-c",
            "model_reasoning_effort=\"xhigh\"",
            "-c",
            "sandbox_workspace_write.network_access=true",
            "-c",
            &format!(
                "shell_environment_policy.set.TASKS_SESSION=\"{}\"",
                s.handle
            ),
            "resume",
            "0abc-def",
        ]
    );
    assert_eq!(w.sent(&again.row.pane), [told(&s.handle)]);
    assert_eq!(again.row.claude_session, "codex:0abc-def");
    assert_eq!(again.row.closed, None);
    assert_eq!(w.live(), [again.row.pane]);
}

#[test]
fn the_restored_terminal_shows_in_its_streams_tab_or_the_one_given() {
    let w = World::new();
    let s = launched(&w).row;
    let tab = w.terminal(&s.pane)["labels"]["herdr.tab"].clone();
    close_terminal(&w, &s);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    // The stream's tab has room again: the terminal joins it, and no tab is made.
    assert_eq!(w.terminal(&again.row.pane)["labels"]["herdr.tab"], tab);
    assert_eq!(w.calls_of(&["app", "do"]).len(), 1);

    close_terminal(&w, &again.row);
    let [elsewhere] = <[_; 1]>::try_from(
        restore(
            w.store(),
            &w.hc(),
            &w.machine(),
            "sim/jump",
            None,
            Some("infra"),
        )
        .unwrap(),
    )
    .unwrap();
    let infra = w.terminal(&elsewhere.row.pane)["labels"]["herdr.tab"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(w.tab_name(&infra), ("lamp-works".into(), "infra".into()));
}

#[test]
fn a_record_of_a_herdr_pane_is_gone_and_restored_into_an_hc_terminal() {
    let w = World::new();
    let s = launched(&w).row;
    // Herdr is no more: the record still names its pane, and hc never ran it.
    w.edit_terminals(|all| all.clear());
    w.store()
        .reopen_task_session("sim/jump", &s.handle, "w1:p3")
        .unwrap();
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    assert!(again.row.pane.starts_with('t'), "{}", again.row.pane);
    assert_eq!(w.live(), [again.row.pane.as_str()]);
    assert_eq!(
        (again.row.closed, again.row.claude_session.as_str()),
        (None, s.claude_session.as_str())
    );
    let said = w.f.said("sim/jump");
    let restore_line = format!("just tasks restore sim/jump {}", s.handle);
    assert!(
        said.iter().any(|m| m
            == &format!(
                "closed the pane of {} ({GONE}); restore: {restore_line}",
                s.handle
            )),
        "{said:?}"
    );
    assert!(
        said.iter()
            .any(|m| m == &format!("restored {} in pane {}", s.handle, again.row.pane)),
        "{said:?}"
    );
}

#[test]
fn an_open_record_whose_terminal_was_closed_outside_the_board_is_restored() {
    let w = World::new();
    let s = launched(&w).row;
    w.hc().close(&s.pane).unwrap();
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", Some(&s.handle)).unwrap()).unwrap();
    assert_eq!(w.live(), [again.row.pane]);
}

#[test]
fn a_session_whose_terminal_runs_is_not_restored() {
    let w = World::new();
    launched(&w);
    assert_eq!(restored(&w, "sim/jump", None).unwrap(), vec![]);
    assert_eq!(w.calls_of(&["run"]).len(), 1);
}

#[test]
fn a_closed_record_whose_handle_still_runs_in_a_terminal_is_refused() {
    let w = World::new();
    let s = launched(&w).row;
    w.store()
        .close_task_session("sim/jump", &s.handle, "idle", "restore line")
        .unwrap();
    assert_eq!(
        refused(restored(&w, "sim/jump", None)),
        format!(
            "{} still runs in terminal {}: close it before restoring",
            s.handle, s.pane
        )
    );
    assert_eq!(w.calls_of(&["run"]).len(), 1);
}

#[test]
fn only_the_named_handle_is_restored_and_every_closed_one_by_default() {
    let w = World::new();
    let a = launched(&w).row;
    let b = w.review("sim/jump").row;
    close_terminal(&w, &a);
    close_terminal(&w, &b);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", Some(&b.handle)).unwrap()).unwrap();
    assert_eq!(again.row.handle, b.handle);
    let open: Vec<String> = w
        .store()
        .task_sessions("sim/jump")
        .unwrap()
        .into_iter()
        .filter(|s| s.closed.is_none())
        .map(|s| s.handle)
        .collect();
    assert_eq!(open, std::slice::from_ref(&b.handle));

    let w = World::new();
    let a = launched(&w).row;
    let b = w.review("sim/jump").row;
    w.store()
        .record_launch(&NewLaunch {
            claude_session: "codex:0abc".into(),
            worktree: b.worktree.clone(),
            pane: b.pane.clone(),
            model: b.model.clone(),
            ..NewLaunch::new("sim/jump", &b.handle, &b.role)
        })
        .unwrap();
    close_terminal(&w, &a);
    close_terminal(&w, &b);
    let mut handles: Vec<String> = restored(&w, "sim/jump", None)
        .unwrap()
        .into_iter()
        .map(|s| s.row.handle)
        .collect();
    handles.sort();
    let mut both = vec![a.handle, b.handle];
    both.sort();
    assert_eq!(handles, both);
}

#[test]
fn a_missing_worktree_is_made_again_on_its_branch_or_detached_without_one() {
    let w = World::new();
    let s = launched(&w).row;
    close_terminal(&w, &s);
    git(&w.repo, &["worktree", "remove", "--force", &s.worktree]);
    restored(&w, "sim/jump", None).unwrap();
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "sim/jump"
    );

    let w = World::new();
    let s = w
        .try_launch(&Ask {
            role: "reviewer".into(),
            kind: Some("claude".into()),
            ..w.ask("sim/jump")
        })
        .unwrap()
        .row;
    close_terminal(&w, &s);
    git(&w.repo, &["worktree", "remove", "--force", &s.worktree]);
    restored(&w, "sim/jump", None).unwrap();
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "HEAD"
    );
}

// -- through `just tasks`, which an agent runs unasked -----------------------------------------

fn unasked(w: &World) -> Result<Vec<Started>, Error> {
    restore(w.store(), &w.hc(), &w.unasked(), "sim/jump", None, None)
}

#[test]
fn unasked_a_record_of_a_worktree_outside_the_project_is_neither_made_again_nor_started_in() {
    let w = World::new();
    // An older record, or one the binary run by itself made: a Claude executor outside the project.
    let elsewhere = w.dir().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let s = w
        .try_launch(&Ask {
            kind: Some("claude".into()),
            worktree: Some(elsewhere.clone()),
            ..w.ask("sim/jump")
        })
        .unwrap()
        .row;
    close_terminal(&w, &s);
    let closed = w.store().task_sessions("sim/jump").unwrap();
    assert_eq!(
        refused(unasked(&w)),
        w.outside(&format!(
            "{}'s recorded worktree {} is",
            s.handle,
            elsewhere.display()
        ))
    );
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), closed);
    // One that is gone is not made again there, and a record whose terminal is gone stays as it
    // was: nothing is written before the refusal.
    let gone = w.dir().join("gone");
    w.store()
        .record_launch(&NewLaunch {
            claude_session: s.claude_session.clone(),
            worktree: gone.display().to_string(),
            branch: "sim/jump".into(),
            pane: s.pane.clone(),
            model: s.model.clone(),
            ..NewLaunch::new("sim/jump", &s.handle, &s.role)
        })
        .unwrap();
    let reopened = w.store().task_sessions("sim/jump").unwrap();
    assert_eq!(reopened[0].closed, None);
    assert_eq!(
        refused(unasked(&w)),
        w.outside(&format!(
            "{}'s recorded worktree {} is",
            s.handle,
            gone.display()
        ))
    );
    assert!(!gone.exists());
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), reopened);
    assert_eq!(w.calls_of(&["run"]).len(), 1);
    // The binary run by itself makes it and starts the agent there.
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    assert!(gone.is_dir());
    assert_eq!(
        w.terminal(&again.row.pane)["cwd"].as_str(),
        Some(gone.display().to_string().as_str())
    );
}

#[test]
fn unasked_a_record_in_the_project_is_restored_and_made_again_under_the_worktree_base() {
    let w = World::new();
    let s = launched(&w).row;
    close_terminal(&w, &s);
    git(&w.repo, &["worktree", "remove", "--force", &s.worktree]);
    let [again] = <[_; 1]>::try_from(unasked(&w).unwrap()).unwrap();
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "sim/jump"
    );
    assert_eq!(
        w.terminal(&again.row.pane)["cwd"].as_str(),
        Some(s.worktree.as_str())
    );
    assert_eq!(
        last_program(&w)[..3],
        ["claude", "--resume", s.claude_session.as_str()]
    );
}

#[test]
fn a_restored_reviewer_goes_on_with_the_review_skill_and_an_executor_with_work() {
    let w = World::new();
    let reviewer = w.review("sim/jump").row;
    close_terminal(&w, &reviewer);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    assert_eq!(
        w.sent(&again.row.pane),
        [told_to(&reviewer.handle, "review-task")]
    );

    let w = World::new();
    let executor = launched(&w).row;
    close_terminal(&w, &executor);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    assert_eq!(w.sent(&again.row.pane), [told(&executor.handle)]);
}

#[test]
fn a_handle_that_never_ran_on_the_task_or_a_task_never_launched_is_refused() {
    let w = World::new();
    assert_eq!(
        refused(restored(&w, "sim/jump", Some("ghost-owl"))),
        "ghost-owl has no recorded session on sim/jump"
    );
    assert_eq!(
        refused(restored(&w, "sim/jump", None)),
        "sim/jump has no recorded sessions: it was not launched with `just tasks launch`"
    );
    launched(&w);
    assert_eq!(
        refused(restored(&w, "sim/jump", Some("ghost-owl"))),
        "ghost-owl has no recorded session on sim/jump"
    );
}

#[test]
fn a_restore_without_a_recorded_conversation_is_refused() {
    for identity in ["", "codex:"] {
        let w = World::new();
        let s = launched(&w).row;
        w.store()
            .record_launch(&NewLaunch {
                claude_session: identity.into(),
                worktree: s.worktree.clone(),
                branch: s.branch.clone(),
                pane: s.pane.clone(),
                model: s.model.clone(),
                ..NewLaunch::new("sim/jump", &s.handle, &s.role)
            })
            .unwrap();
        close_terminal(&w, &s);
        assert_eq!(
            refused(restored(&w, "sim/jump", None)),
            format!(
                "{} has no recorded conversation id: its conversation cannot be resumed (start it again \
                 with `just tasks launch`)",
                s.handle
            ),
            "{identity:?}"
        );
        assert_eq!(w.calls_of(&["run"]).len(), 1);
    }
}

#[test]
fn a_restored_handle_that_had_left_can_act_again() {
    let w = World::new();
    let s = launched(&w).row;
    w.store().leave(&s.handle).unwrap();
    close_terminal(&w, &s);
    restored(&w, "sim/jump", None).unwrap();
    w.store().touch(&s.handle).unwrap();
    assert_eq!(w.f.health(&s.handle).as_str(), "live");
}

#[test]
fn a_failed_run_starts_nothing_and_reopens_nothing() {
    let mut w = World::new();
    let s = launched(&w).row;
    close_terminal(&w, &s);
    w.set("FAKE_HC_FAIL", "run");
    assert_eq!(
        refused(restored(&w, "sim/jump", None)),
        "hc run: fake_failure: run failed"
    );
    assert!(w.addresses().is_empty());
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_some()
    );
}

#[test]
fn an_unconfirmed_restore_kills_and_removes_its_new_terminal_and_keeps_the_record_closed() {
    for (knob, value, why) in [
        (
            "FAKE_HC_BLOCKED",
            "1",
            "the agent in terminal {t} waits on a person before it took its first message: Quick \
             safety check: Is this a project you created or one you trust?",
        ),
        (
            "FAKE_HC_DEAF",
            "1",
            "the agent in terminal {t} did not start working on its first message (its status \
             when it went in: idle)",
        ),
        (
            "FAKE_HC_SILENT",
            "1",
            "the agent in terminal {t} was never ready for its first message (its last status: \
             none)",
        ),
    ] {
        let mut w = World::new();
        let s = launched(&w).row;
        close_terminal(&w, &s);
        w.store().leave(&s.handle).unwrap();
        let before = w.store().task_sessions("sim/jump").unwrap();
        w.set(knob, value);
        let problem = refused(restored(&w, "sim/jump", Some(&s.handle)));
        let run = w.calls_of(&["run"]).pop().unwrap();
        let address = w
            .calls_of(&["rm"])
            .pop()
            .map(|c| c[1].clone())
            .expect("the new terminal was removed");
        assert_ne!(address, s.pane);
        assert_eq!(problem, why.replace("{t}", &address), "{knob}");
        assert_eq!(run[3], s.handle);
        assert_eq!(w.calls_of(&["kill", &address]).len(), 1, "{knob}");
        assert!(w.addresses().is_empty(), "{knob}");
        assert_eq!(
            w.store().task_sessions("sim/jump").unwrap(),
            before,
            "{knob}"
        );
        assert_eq!(w.f.health(&s.handle).as_str(), "left", "{knob}");
    }
}
