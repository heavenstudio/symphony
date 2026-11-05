//! `finish`: a task that becomes done or cancelled closes its own sessions' panes, and those of the
//! sessions launched below it; it never fails, since the status change must stand. (What becomes of
//! their worktrees is the worktree sweep's: `tasks sweep --task SLUG`.)

use crate::World;
use symphony_board::Store;
use symphony_board::launch::finish;
use symphony_board::rpc::Client;

impl World {
    pub fn finish(&self, slug: &str) -> Vec<String> {
        let m = self.machine();
        finish(self.store(), &self.herdr(), &*m.processes, slug, None)
    }
}

#[test]
fn the_panes_of_a_done_task_close_and_say_how_to_restore_them() {
    let w = World::new();
    let (handle, pane) = w.worker("sim/jump", Some("done"));
    assert_eq!(
        w.finish("sim/jump"),
        [format!(
            "closed pane {pane} of {handle} (its tasks are finished); restore: just tasks restore sim/jump {handle}"
        )]
    );
    assert!(!w.has_pane(&pane));
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert!(read.closed.is_some());
    // Its worktree is the sweep's, which the binary starts next: finishing touches none.
    let worktree = w.wt.join("sim-jump");
    assert!(worktree.is_dir());
    assert_eq!(read.worktree, worktree.display().to_string());
}

#[test]
fn a_busy_pane_keeps_its_session() {
    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    w.run_under(w.shell_pid(&pane), &["just verify"]);
    assert_eq!(w.finish("sim/jump"), Vec::<String>::new());
    assert!(w.has_pane(&pane));
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
}

#[test]
fn a_task_that_is_not_finished_or_had_no_session_is_left_alone() {
    let w = World::new();
    let (handle, pane) = w.worker("sim/jump", None);
    w.store().claim("sim/jump", &handle).unwrap();
    assert_eq!(w.finish("sim/jump"), Vec::<String>::new());
    assert!(w.has_pane(&pane));
    w.task("sim/dock", None);
    w.store().move_to("sim/dock", "cancelled", &w.orch).unwrap();
    let before = w.calls().len();
    assert_eq!(w.finish("sim/dock"), Vec::<String>::new());
    assert_eq!(w.calls().len(), before, "no session, no herdr call");
}

#[test]
fn sessions_already_closed_ask_nothing_of_herdr() {
    let w = World::new();
    let (_, pane) = w.worker("sim/jump", Some("done"));
    w.clean(false, None, &[]);
    assert!(!w.has_pane(&pane));
    let before = w.calls().len();
    assert_eq!(w.finish("sim/jump"), Vec::<String>::new());
    assert_eq!(w.calls().len(), before);
}

#[test]
fn only_the_finished_tasks_panes_close() {
    let w = World::new();
    w.task("sim/dock", None);
    let (_, jump) = w.worker("sim/jump", Some("done"));
    let (_, dock) = w.worker("sim/dock", Some("done"));
    assert_eq!(w.finish("sim/dock").len(), 1);
    assert!(w.has_pane(&jump));
    assert!(!w.has_pane(&dock));
}

#[test]
fn a_parent_done_closes_an_idle_executor_launched_on_its_subtask_but_not_a_busy_one() {
    for busy in [false, true] {
        let w = World::new();
        w.task("sim/jump/do", Some("sim/jump"));
        let s = w.launch("sim/jump/do").row;
        let pid = w.new_pid();
        w.edit_pane(&s.pane, |p| {
            p.insert("agent_status".into(), "idle".into());
            p.insert("shell_pid".into(), pid.into());
        });
        if busy {
            w.run_under(pid, &["just verify"]);
        }
        w.store().claim("sim/jump/do", &s.handle).unwrap();
        w.store().done("sim/jump/do", &s.handle, "did it").unwrap();
        let other = w.f.session("worker");
        w.finish_task("sim/jump", &other, "done");
        let lines = w.finish("sim/jump");
        assert_eq!(w.has_pane(&s.pane), busy, "{lines:?}");
        assert_eq!(
            w.store().task_sessions("sim/jump/do").unwrap()[0]
                .closed
                .is_some(),
            !busy
        );
    }
}

#[test]
fn finishing_never_fails_when_herdr_is_down() {
    let mut w = World::new();
    w.worker("sim/jump", Some("done"));
    w.set("FAKE_HERDR_FAIL", "pane list");
    let [line] = <[_; 1]>::try_from(w.finish("sim/jump")).unwrap();
    assert!(
        line.starts_with("could not clean up the sessions of sim/jump: herdr pane list failed: "),
        "{line}"
    );
}

#[test]
fn finish_on_a_board_it_cannot_reach_says_so_and_never_fails() {
    let w = World::new();
    let gone = Store::unchecked(
        Client::new("127.0.0.1:1", "sf", "gone"),
        Box::new(|| crate::common::NOW),
        Box::new(|_| false),
        crate::common::HOST,
    );
    let m = w.machine();
    let [line] =
        <[_; 1]>::try_from(finish(&gone, &w.herdr(), &*m.processes, "sim/jump", None)).unwrap();
    assert!(
        line.starts_with("could not clean up the sessions of sim/jump: SurrealDB: "),
        "{line}"
    );
    assert_eq!(w.calls(), Vec::<Vec<String>>::new());
}
