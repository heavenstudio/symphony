//! What follows a task's move: a finished task's terminals close and its sweep starts; a task in
//! review starts its tip's verify, the board's `on_review` command, said in one line; and, where
//! the project delivers by pull request, its dispatch to the host
//! ([`crate::land::dispatch`]), as each later write that can change its evidence does
//! ([`poke`]).

use super::{Context, panes};
use crate::land::dispatch;
use crate::on_review::{self, Started};
use crate::sweep::trigger;
use crate::{Store, Task};

/// `task` has moved: what its new status starts.
pub(super) fn after_move(store: &Store, c: &mut Context, task: &Task) {
    after_finish(store, c, task);
    if let Ok(actor) = c.actor()
        && let Some(said) = dispatch::on_move(c.env, &c.env.cwd, task, &actor)
    {
        c.say(&format!("→ {said}"));
    }
    if task.status != "review"
        || !on_review::delivery_task(task.parent.as_deref(), task.instruction)
    {
        return;
    }
    match on_review::trigger(c.env, &c.env.cwd, &task.slug, c.program) {
        Started::Command(args) => c.say(&format!(
            "→ the tip's verify started detached: {}",
            args.join(" ")
        )),
        Started::Refused(why) => c.say(&format!("→ the tip's verify did not start: {why}")),
        Started::None => {}
    }
}

/// A write that can change the evidence of `slug` (an assessment, a verdict, a gate decision):
/// its dispatch started again where it was dispatched before, said in one line.
pub(super) fn poke(c: &mut Context, slug: &str) {
    if let Ok(actor) = c.actor()
        && let Some(said) = dispatch::poke(c.env, &c.env.cwd, slug, &actor)
    {
        c.say(&format!("→ {said}"));
    }
}

/// A finished task's terminals close and the worktree sweep is asked for: finished by its
/// recorded facts, so a task a custom workflow holds by its terminal position, never the status it
/// projects to ([`Store::finished`]).
pub(super) fn after_finish(store: &Store, c: &mut Context, task: &Task) {
    if !store.finished(task).unwrap_or(false) {
        return;
    }
    panes::close_finished(store, c, &task.slug);
    let why = format!("{} is {}", task.slug, task.status);
    trigger::trigger(c.env, &c.env.cwd, &why, Some(&task.slug));
}
