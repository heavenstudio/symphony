//! The commands that drive agents' hc terminals: `launch`, `restore`, `cleanup`, `reap` and
//! `finish`, and the cleanup after a task finishes, each a call into [`crate::launch`] and the lines
//! it says. The caller's own terminal (`HC_TERMINAL_ID`) is never closed. A refusal (an hc or git
//! step that failed, or the board's) is exit 1, a board that cannot be reached exit 2.

use super::{Context, Stop};
use crate::Store;
use crate::launch::hc::Binary;
use crate::launch::reap::{self, REAP_SECONDS, Reap};
use crate::launch::{self, Ask, Machine, procs};

/// `launch SLUG --model M ...`.
pub(super) fn launch(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let ask = Ask {
        slug: c.arg("slug"),
        actor: c.actor()?,
        model: p
            .or(
                "model",
                if matches!(p.get("kind"), Some("claude" | "worker" | "reviewer")) {
                    "sonnet"
                } else {
                    "gpt-6.1-sol"
                },
            )
            .into(),
        kind: p.get("kind").map(str::to_string),
        role: p.or("role", "executor").into(),
        branch: p.get("branch").map(str::to_string),
        detached: p.flag("detached"),
        worktree: p.get("worktree").map(|w| c.env.cwd.join(w)),
        prompt: p.or("prompt", "").into(),
        tab: p.get("tab").map(str::to_string),
    };
    let started = launch::launch(store, &Binary::of(c.env), &Machine::of(c.env), &ask)?;
    let s = &started.row;
    c.say(&format!(
        "launched {} on {} as {}: pane {}, agent session {}, worktree {}",
        s.handle, s.slug, s.role, s.pane, s.claude_session, s.worktree
    ));
    Ok(())
}

/// `restore SLUG [HANDLE]`.
pub(super) fn restore(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let slug = c.arg("slug");
    let handle = c.parsed.get("handle");
    let again = launch::restore(
        store,
        &Binary::of(c.env),
        &Machine::of(c.env),
        &slug,
        handle,
        None,
    )?;
    for s in &again {
        let r = &s.row;
        c.say(&format!(
            "restored {} on {}: pane {}, claude session {}",
            r.handle, r.slug, r.pane, r.claude_session
        ));
    }
    if again.is_empty() {
        c.say(&format!(
            "nothing to restore: every recorded session of {slug} is open"
        ));
    }
    Ok(())
}

/// `cleanup [--dry-run]`.
pub(super) fn cleanup(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let dry_run = c.parsed.flag("dry_run");
    let closures = launch::cleanup(store, &Binary::of(c.env), &procs::descendants, dry_run, &[])?;
    for closure in &closures {
        let verb = match (closure.kept, dry_run, closure.closed) {
            (true, _, _) => "kept",
            (false, true, _) => "would close",
            (false, false, true) => "closed",
            (false, false, false) => "could not close",
        };
        let of = closure
            .handle
            .as_ref()
            .map_or(String::new(), |h| format!(" of {h}"));
        c.say(&format!(
            "{verb} pane {}{of} ({}){}",
            closure.pane,
            closure.why,
            restore_part(&closure.restore)
        ));
    }
    if closures.is_empty() {
        c.say("nothing to clean up");
    }
    Ok(())
}

fn restore_part(line: &str) -> String {
    if line.is_empty() {
        String::new()
    } else {
        format!("; restore: {line}")
    }
}

/// `reap [--dry-run]`: one sweep, which never closes the pane or the handle of whoever asked.
pub(super) fn reap(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let dry_run = c.parsed.flag("dry_run");
    // The app's existing five-minute timer invokes reap; staffing stays entirely in Rust.
    if !dry_run {
        match launch::staff::automatic(store, c.env) {
            Ok(lines) => {
                for line in lines {
                    c.say(&line);
                }
            }
            Err(problem) => {
                let _ = writeln!(c.err, "staff failed; idle sweep continues: {problem}");
            }
        }
    }
    let caller = c.actor().ok();
    let how = Reap {
        state: reap::state_path(c.env, &c.env.cwd).map_err(Stop::Board)?,
        apply: !dry_run,
        processes: &procs::descendants,
        caller: caller.as_deref(),
        min_idle: REAP_SECONDS,
    };
    let Some(swept) = reap::sweep(store, &Binary::of(c.env), &how, &mut *c.err)? else {
        c.say("another sweep is running (the Symphony's timer): nothing done");
        return Ok(());
    };
    for closed in &swept.closed {
        c.say(&format!(
            "{} {} ({}){}",
            if dry_run { "would close" } else { "closed" },
            closed.pane,
            closed.handle,
            restore_part(&closed.restore)
        ));
    }
    if !swept.idle.is_empty() {
        c.say(&format!(
            "idle once, closed if still idle next sweep: {}",
            swept.idle.join(", ")
        ));
    }
    if swept.closed.is_empty() && swept.idle.is_empty() {
        c.say("nothing to close");
    }
    Ok(())
}

pub(super) fn staff(store: &Store, c: &mut Context) -> Result<(), Stop> {
    for line in launch::staff::pass(store, c.env, c.parsed.flag("dry_run"))? {
        c.say(&line);
    }
    Ok(())
}

/// `finish SLUG`: the cleanup after a task finishes, for a caller that finished it on the board
/// itself (`just land`).
pub(super) fn finish(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let slug = c.arg("slug");
    close_finished(store, c, &slug);
    Ok(())
}

/// The terminals of the sessions launched on finished `slug` or under it close
/// ([`launch::finish`]), each closed one a line; what went wrong is said, and nothing fails.
pub(super) fn close_finished(store: &Store, c: &mut Context, slug: &str) {
    let lines = launch::finish(store, &Binary::of(c.env), &procs::descendants, slug);
    for line in lines {
        c.say(&format!("→ {line}"));
    }
}
