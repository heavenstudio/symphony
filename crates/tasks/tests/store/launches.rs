//! What `just tasks launch` records on a task, so a closed session can be restored later.
use crate::common::Seed;

use crate::common::{Fixture, NOW, refused};
use symphony_board::NewLaunch;
use symphony_board::rows::TaskSessionRow;

/// A launch of a new worker's handle on `slug` as `role`, in terminal t9: its record.
pub(crate) fn launch(f: &Fixture, slug: &str, role: &str) -> TaskSessionRow {
    let handle = f.session("worker");
    f.store
        .record_launch(&NewLaunch {
            claude_session: "11111111-2222-3333-4444-555555555555".into(),
            worktree: "/wt/sim-jump".into(),
            branch: "sim/jump".into(),
            pane: "t9".into(),
            model: "sonnet".into(),
            ..NewLaunch::new(slug, &handle, role)
        })
        .unwrap()
}

#[test]
fn a_launch_is_read_back_whole_and_open_on_the_record() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    f.advance(5.0);
    let launched = launch(&f, "sim/jump", "executor");
    let read = f.store.task_sessions("sim/jump").unwrap();
    assert_eq!(read, std::slice::from_ref(&launched));
    assert_eq!(
        (
            read[0].role.as_str(),
            read[0].claude_session.as_str(),
            read[0].worktree.as_str(),
            read[0].branch.as_str(),
            read[0].pane.as_str(),
            read[0].model.as_str()
        ),
        (
            "executor",
            "11111111-2222-3333-4444-555555555555",
            "/wt/sim-jump",
            "sim/jump",
            "t9",
            "sonnet"
        )
    );
    assert_eq!(
        (
            read[0].started,
            read[0].closed,
            read[0].closed_why.as_str(),
            read[0].restore.as_str()
        ),
        (f.now(), None, "", "")
    );
    let h = &launched.handle;
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(
        (last.sender.as_str(), last.kind.as_str(), last.text),
        (
            h.as_str(),
            "system",
            format!(
                "launched {h} as executor: pane t9, agent session 11111111-2222-3333-4444-555555555555, worktree /wt/sim-jump, branch sim/jump"
            )
        )
    );
}

#[test]
fn a_session_found_running_is_recorded_as_found() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("reviewer");
    f.store
        .record_launch(&NewLaunch {
            worktree: "/wt/x".into(),
            pane: "t2".into(),
            how: "found".into(),
            ..NewLaunch::new("sim/jump", &h, "reviewer")
        })
        .unwrap();
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("found {h} running as reviewer: pane t2, agent session unknown, worktree /wt/x")
    );
}

#[test]
fn a_session_recorded_without_its_line_leaves_the_thread_as_it_was() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("reviewer");
    let before = f.said("sim/jump");
    let row = f
        .store
        .record_launch(&NewLaunch {
            pane: "t2".into(),
            how: "found".into(),
            log: false,
            ..NewLaunch::new("sim/jump", &h, "reviewer")
        })
        .unwrap();
    assert_eq!(f.said("sim/jump"), before);
    assert_eq!(f.store.task_sessions("sim/jump").unwrap(), vec![row]);
}

#[test]
fn a_session_of_an_unknown_task_or_handle_is_refused() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("worker");
    assert_eq!(
        refused(
            f.store
                .record_launch(&NewLaunch::new("sim/none", &h, "executor"))
        ),
        "no task 'sim/none'"
    );
    assert_eq!(
        refused(
            f.store
                .record_launch(&NewLaunch::new("sim/jump", "ghost-owl", "executor"))
        ),
        "no session 'ghost-owl'"
    );
}

#[test]
fn a_role_outside_executor_and_reviewer_is_refused() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("worker");
    assert_eq!(
        refused(
            f.store
                .record_launch(&NewLaunch::new("sim/jump", &h, "orchestrator"))
        ),
        "a task session is an executor or a reviewer, not 'orchestrator'"
    );
}

#[test]
fn closing_keeps_the_record_and_the_restore_line() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let launched = launch(&f, "sim/jump", "executor");
    let h = &launched.handle;
    f.advance(60.0);
    let restore = format!("just tasks restore sim/jump {h}");
    f.store
        .close_task_session("sim/jump", h, "task done", &restore)
        .unwrap();
    let read = f.store.task_sessions("sim/jump").unwrap().remove(0);
    assert_eq!(
        (read.closed, read.closed_why.as_str(), read.restore.as_str()),
        (Some(f.now()), "task done", restore.as_str())
    );
    assert_eq!(
        (read.claude_session, read.worktree),
        (launched.claude_session, launched.worktree)
    );
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("closed the pane of {h} (task done); restore: {restore}")
    );
    assert!(f.store.open_task_sessions().unwrap().is_empty());
    assert_eq!(f.store.all_task_sessions().unwrap().len(), 1);
}

#[test]
fn reopening_gives_the_new_pane_and_clears_the_closure() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let launched = launch(&f, "sim/jump", "executor");
    let h = &launched.handle;
    f.store
        .close_task_session("sim/jump", h, "idle", "restore line")
        .unwrap();
    f.store.reopen_task_session("sim/jump", h, "t77").unwrap();
    let read = f.store.task_sessions("sim/jump").unwrap().remove(0);
    assert_eq!(
        (read.pane.as_str(), read.closed, read.closed_why.as_str()),
        ("t77", None, "")
    );
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("restored {h} in pane t77")
    );
    assert_eq!(f.store.open_task_sessions().unwrap(), [read]);
}

#[test]
fn closing_or_reopening_what_was_never_launched_is_refused() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("worker");
    assert_eq!(
        refused(f.store.close_task_session("sim/jump", &h, "idle", "x")),
        format!("{h} has no recorded session on sim/jump")
    );
    assert_eq!(
        refused(f.store.reopen_task_session("sim/jump", &h, "t1")),
        format!("{h} has no recorded session on sim/jump")
    );
}

#[test]
fn launching_again_for_the_same_handle_replaces_the_record() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let first = launch(&f, "sim/jump", "executor");
    f.store
        .close_task_session("sim/jump", &first.handle, "idle", "x")
        .unwrap();
    let again = f
        .store
        .record_launch(&NewLaunch {
            claude_session: "other-id".into(),
            worktree: "/wt/two".into(),
            branch: "b2".into(),
            pane: "t2".into(),
            ..NewLaunch::new("sim/jump", &first.handle, "executor")
        })
        .unwrap();
    let read = f.store.task_sessions("sim/jump").unwrap();
    assert_eq!(read, std::slice::from_ref(&again));
    assert_eq!(
        (
            again.claude_session.as_str(),
            again.worktree.as_str(),
            again.closed,
            again.model.as_str()
        ),
        ("other-id", "/wt/two", None, "")
    );
}

#[test]
fn sessions_of_a_handle_span_its_tasks_in_the_order_started() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    f.task("sim/dock");
    let h = f.session("worker");
    f.store
        .record_launch(&NewLaunch {
            worktree: "/wt/a".into(),
            ..NewLaunch::new("sim/jump", &h, "executor")
        })
        .unwrap();
    f.advance(1.0);
    f.store
        .record_launch(&NewLaunch {
            worktree: "/wt/b".into(),
            ..NewLaunch::new("sim/dock", &h, "reviewer")
        })
        .unwrap();
    let mine: Vec<(String, f64)> = f
        .store
        .sessions_of(&h)
        .unwrap()
        .into_iter()
        .map(|s| (s.slug, s.started))
        .collect();
    assert_eq!(
        mine,
        [
            ("sim/jump".to_string(), NOW),
            ("sim/dock".to_string(), NOW + 1.0)
        ]
    );
    assert_eq!(
        f.store.task_sessions("sim/dock").unwrap()[0].role,
        "reviewer"
    );
    assert!(f.store.sessions_of("ghost-owl").unwrap().is_empty());
}

/// A Codex launch of `handle` on sim/jump whose thread the launch does not know (`codex:`).
fn codex_launch(f: &Fixture, handle: &str) -> TaskSessionRow {
    f.store
        .record_launch(&NewLaunch {
            claude_session: "codex:".into(),
            worktree: "/wt/x".into(),
            pane: "t3".into(),
            ..NewLaunch::new("sim/jump", handle, "executor")
        })
        .unwrap()
}

#[test]
fn a_codex_record_written_after_its_callback_keeps_the_thread_the_callback_registered() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("worker");
    f.store
        .callback(&h, Some("codex:0abc-12"), false, false)
        .unwrap();
    let row = codex_launch(&f, &h);
    assert_eq!(row.claude_session, "codex:0abc-12");
    assert_eq!(
        f.store.task_sessions("sim/jump").unwrap(),
        std::slice::from_ref(&row)
    );
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("launched {h} as executor: pane t3, agent session codex:0abc-12, worktree /wt/x")
    );
    // A conversation the record names is its own, whatever the callback says.
    for given in ["codex:9999-00", "11111111-2222-3333-4444-555555555555", ""] {
        let kept = f
            .store
            .record_launch(&NewLaunch {
                claude_session: given.into(),
                ..NewLaunch::new("sim/jump", &h, "executor")
            })
            .unwrap();
        assert_eq!(kept.claude_session, given);
        assert_eq!(
            f.store.task_sessions("sim/jump").unwrap()[0].claude_session,
            given
        );
    }
}

#[test]
fn a_codex_record_closed_with_no_thread_says_restore_cannot_resume_it() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("worker");
    codex_launch(&f, &h);
    let restore = format!("just tasks restore sim/jump {h}");
    f.store
        .close_task_session("sim/jump", &h, "idle", &restore)
        .unwrap();
    let read = f.store.task_sessions("sim/jump").unwrap().remove(0);
    assert_eq!(
        (read.claude_session.as_str(), read.restore.as_str()),
        ("codex:", restore.as_str())
    );
    assert!(read.closed.is_some());
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!(
            "closed the pane of {h} (idle); restore: {restore}. Its Codex thread id never arrived, \
             so restore cannot resume its conversation: start it again with just tasks launch \
             sim/jump"
        )
    );
}

#[test]
fn a_codex_record_still_unknown_when_it_closes_takes_the_thread_registered_meanwhile() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let h = f.session("worker");
    codex_launch(&f, &h);
    // Registered without completing the record, as a write racing the launch's could leave it.
    (f.store.raw().row("callback_state", h.as_str()))
        .put(serde_json::json!({"handle": h, "address": "codex:0abc-12", "host": "this-host"}))
        .unwrap();
    assert_eq!(
        f.store.task_sessions("sim/jump").unwrap()[0].claude_session,
        "codex:"
    );
    let restore = format!("just tasks restore sim/jump {h}");
    f.store
        .close_task_session("sim/jump", &h, "idle", &restore)
        .unwrap();
    assert_eq!(
        f.store.task_sessions("sim/jump").unwrap()[0].claude_session,
        "codex:0abc-12"
    );
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("closed the pane of {h} (idle); restore: {restore}")
    );
}
