//! `finish`: a task that becomes done or cancelled closes its own sessions' terminals, and those of
//! the sessions launched below it; it never fails, since the status change must stand. (What becomes
//! of their worktrees is the worktree sweep's: `tasks sweep --task SLUG`.) `approved`: a task an
//! approve left done and not landed keeps its executors' terminals until the land's `finish`.

use crate::{Hooked, World};
use symphony_board::launch::hc::Hc as _;
use symphony_board::launch::{LaunchError, NOT_LANDED, approved, finish, landed};
use symphony_board::{Opening, Store};

impl World {
    pub fn finish(&self, slug: &str) -> Vec<String> {
        let m = self.machine();
        finish(self.store(), &self.hc(), &*m.processes, slug)
    }
}

#[test]
fn the_terminals_of_a_done_task_close_and_say_how_to_restore_them() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    assert_eq!(
        w.finish("sim/jump"),
        [format!(
            "closed pane {terminal} of {handle} (its tasks are finished); restore: just tasks restore sim/jump {handle}"
        )]
    );
    assert!(!w.has_terminal(&terminal));
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert!(read.closed.is_some());
    // Its worktree is the sweep's, which the binary starts next: finishing touches none.
    let worktree = w.wt.join("sim-jump");
    assert!(worktree.is_dir());
    assert_eq!(read.worktree, worktree.display().to_string());
}

#[test]
fn an_approve_closes_the_reviewers_terminal_and_keeps_the_executors_until_the_land_finishes() {
    let w = World::new();
    let (executor, worked) = w.worker("sim/jump", None);
    let store = w.store();
    store.claim("sim/jump", &executor).unwrap();
    store.move_to("sim/jump", "review", &executor).unwrap();
    let reviewer = w.review("sim/jump").row;
    w.edit_terminal(&reviewer.pane, |t| {
        t.insert("status".into(), "idle".into());
    });
    store
        .verdict("sim/jump", &reviewer.handle, "approve", "ok")
        .unwrap();
    let m = w.machine();
    let lines = approved(w.store(), &w.hc(), &*m.processes, "sim/jump");
    assert_eq!(
        lines,
        [
            format!(
                "closed pane {} of {} (its tasks are finished); restore: just tasks restore {} {}",
                reviewer.pane, reviewer.handle, reviewer.slug, reviewer.handle
            ),
            format!("kept pane {worked} of {executor} ({NOT_LANDED})"),
        ]
    );
    assert!(w.has_terminal(&worked) && !w.has_terminal(&reviewer.pane));
    let open = |slug: &str, handle: &str| {
        let sessions = w.store().task_sessions(slug).unwrap();
        let session = sessions.iter().find(|s| s.handle == handle).unwrap();
        session.closed.is_none()
    };
    assert!(open("sim/jump", &executor) && !open(&reviewer.slug, &reviewer.handle));
    // A land that ended with the approve still waiting (a pull request not merged yet) closes
    // nothing; one whose landing closed the task closes it, as any finished task's.
    assert_eq!(
        landed(w.store(), &w.hc(), &*m.processes, "sim/jump"),
        Vec::<String>::new()
    );
    assert!(w.has_terminal(&worked));
    w.store()
        .landed("sim/jump", &w.orch, "landed at abc1234", true, &["done"])
        .unwrap();
    assert_eq!(
        landed(w.store(), &w.hc(), &*m.processes, "sim/jump"),
        [format!(
            "closed pane {worked} of {executor} (its tasks are finished); restore: just tasks restore sim/jump {executor}"
        )]
    );
    assert!(!w.has_terminal(&worked));
}

#[test]
fn an_approve_records_an_executors_terminal_that_is_gone_as_closed_and_keeps_none() {
    let w = World::new();
    let (executor, worked) = w.worker("sim/jump", Some("done"));
    w.hc().close(&worked).unwrap();
    let m = w.machine();
    let lines = approved(w.store(), &w.hc(), &*m.processes, "sim/jump");
    let [line] = <[_; 1]>::try_from(lines).unwrap();
    assert!(
        line.starts_with(&format!("closed pane {worked} of {executor} (")),
        "{line}"
    );
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_some()
    );
    // A task that is not finished, as one a changes verdict sent back, is left alone.
    w.task("sim/dock", None);
    let (_, docked) = w.worker("sim/dock", None);
    assert_eq!(
        approved(w.store(), &w.hc(), &*m.processes, "sim/dock"),
        Vec::<String>::new()
    );
    assert!(w.has_terminal(&docked));
    // A land of a slug the board does not hold says so, and closes nothing.
    let [line] = <[_; 1]>::try_from(landed(w.store(), &w.hc(), &*m.processes, "sim/none")).unwrap();
    assert!(
        line.starts_with("could not clean up the sessions of sim/none: "),
        "{line}"
    );
    assert!(w.has_terminal(&docked));
}

#[test]
fn a_busy_terminal_keeps_its_session() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.run_under(w.pid_of(&terminal), &["just verify"]);
    assert_eq!(w.finish("sim/jump"), Vec::<String>::new());
    assert!(w.has_terminal(&terminal));
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
}

#[test]
fn a_task_that_is_not_finished_or_had_no_session_is_left_alone() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", None);
    w.store().claim("sim/jump", &handle).unwrap();
    assert_eq!(w.finish("sim/jump"), Vec::<String>::new());
    assert!(w.has_terminal(&terminal));
    w.task("sim/dock", None);
    w.store().move_to("sim/dock", "cancelled", &w.orch).unwrap();
    let before = w.calls().len();
    assert_eq!(w.finish("sim/dock"), Vec::<String>::new());
    assert_eq!(w.calls().len(), before, "no session, no hc call");
}

#[test]
fn sessions_already_closed_ask_nothing_of_hc() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.clean(false, &[]);
    assert!(!w.has_terminal(&terminal));
    let before = w.calls().len();
    assert_eq!(w.finish("sim/jump"), Vec::<String>::new());
    assert_eq!(w.calls().len(), before);
}

#[test]
fn a_finish_whose_process_table_cannot_be_read_keeps_the_terminal_and_says_why() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    let broken = |_: i64| -> Result<Vec<String>, LaunchError> {
        Err(LaunchError("ps: no answer in 30 s".into()))
    };
    assert_eq!(
        finish(w.store(), &w.hc(), &broken, "sim/jump"),
        [format!(
            "kept pane {terminal} of {handle} (never closed on no information: ps: no answer in 30 s)"
        )]
    );
    assert!(w.has_terminal(&terminal));
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_none()
    );
}

#[test]
fn a_terminal_gone_between_its_listing_and_its_read_is_nothing_to_close() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    let mut hooked = Hooked::new(&w);
    hooked.instead = Box::new(|args| {
        (args[0] == "get").then(|| Err(LaunchError(format!("hc get {}: not_found: gone", args[1]))))
    });
    let m = w.machine();
    assert_eq!(
        finish(w.store(), &hooked, &*m.processes, "sim/jump"),
        Vec::<String>::new()
    );
    assert!(w.calls_of(&["kill"]).is_empty() && w.has_terminal(&terminal));
}

#[test]
fn only_the_finished_tasks_terminals_close() {
    let w = World::new();
    w.task("sim/dock", None);
    let (_, jump) = w.worker("sim/jump", Some("done"));
    let (_, dock) = w.worker("sim/dock", Some("done"));
    assert_eq!(w.finish("sim/dock").len(), 1);
    assert!(w.has_terminal(&jump));
    assert!(!w.has_terminal(&dock));
}

#[test]
fn a_parent_done_closes_an_idle_executor_launched_on_its_subtask_but_not_a_busy_one() {
    for busy in [false, true] {
        let w = World::new();
        crate::git(&w.repo, &["branch", "sim/jump"]);
        w.task("sim/jump/do", Some("sim/jump"));
        let s = w.launch("sim/jump/do").row;
        w.edit_terminal(&s.pane, |t| {
            t.insert("status".into(), "idle".into());
        });
        if busy {
            w.run_under(w.pid_of(&s.pane), &["just verify"]);
        }
        w.store().claim("sim/jump/do", &s.handle).unwrap();
        w.store().done("sim/jump/do", &s.handle, "did it").unwrap();
        let other = w.f.session("worker");
        w.finish_task("sim/jump", &other, "done");
        let lines = w.finish("sim/jump");
        assert_eq!(w.has_terminal(&s.pane), busy, "{lines:?}");
        assert_eq!(
            w.store().task_sessions("sim/jump/do").unwrap()[0]
                .closed
                .is_some(),
            !busy
        );
    }
}

#[test]
fn finishing_never_fails_when_hc_is_down() {
    let mut w = World::new();
    w.worker("sim/jump", Some("done"));
    w.set("FAKE_HC_FAIL", "ls");
    let [line] = <[_; 1]>::try_from(w.finish("sim/jump")).unwrap();
    assert_eq!(
        line,
        "could not clean up the sessions of sim/jump: hc ls: fake_failure: ls failed"
    );
}

#[test]
fn finish_on_a_board_it_cannot_reach_says_so_and_never_fails() {
    let w = World::new();
    // A PostgreSQL socket folder no server listens in.
    let nowhere = crate::Folder::new();
    let gone = Store::unchecked_on(
        symphony_board::pg::Client::new(&nowhere.0, "gone").into(),
        Box::new(|| crate::common::NOW),
        Box::new(|_| false),
        crate::common::HOST,
        &Opening::write(),
    );
    let m = w.machine();
    let [line] = <[_; 1]>::try_from(finish(&gone, &w.hc(), &*m.processes, "sim/jump")).unwrap();
    assert!(
        line.starts_with("could not clean up the sessions of sim/jump: the board's database: "),
        "{line}"
    );
    assert_eq!(w.calls(), Vec::<Vec<String>>::new());
}
