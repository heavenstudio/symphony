//! What follows a task's move: a finished task's terminals close and its sweep starts; a task in
//! review starts its tip's verify, the board's `on_review` command, said in one line.

use super::{Context, panes};
use crate::health::FINISHED;
use crate::on_review::{self, Started};
use crate::sweep::trigger;
use crate::{Store, Task};

/// `task` has moved: what its new status starts.
pub(super) fn after_move(store: &Store, c: &mut Context, task: &Task) {
    after_finish(store, c, task);
    if task.status != "review"
        || !on_review::delivery_task(task.parent.as_deref(), task.instruction)
    {
        return;
    }
    match on_review::trigger(c.env, &c.env.cwd, &task.slug) {
        Started::Command(args) => c.say(&format!(
            "→ the tip's verify started detached: {}",
            args.join(" ")
        )),
        Started::Refused(why) => c.say(&format!("→ the tip's verify did not start: {why}")),
        Started::None => {}
    }
}

/// A finished task's terminals close and the worktree sweep is asked for.
pub(super) fn after_finish(store: &Store, c: &mut Context, task: &Task) {
    if !FINISHED.contains(&task.status.as_str()) {
        return;
    }
    panes::close_finished(store, c, &task.slug);
    let why = format!("{} is {}", task.slug, task.status);
    trigger::trigger(c.env, &c.env.cwd, &why, Some(&task.slug));
}
