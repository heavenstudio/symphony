//! The commands that drive agents' hc terminals: `launch`, `restore`, `cleanup`, `clean`, `close`,
//! `reap` and `finish`, and the cleanup after a task finishes, each a call into [`crate::launch`] and the
//! lines it says. The caller's own terminal (`HC_TERMINAL_ID`) is never closed. A refusal (an hc or
//! git step that failed, or the board's) is exit 1, a board that cannot be reached exit 2.

use super::{Context, Stop};
use crate::Store;
use crate::launch::hc::Binary;
use crate::launch::reap::{self, REAP_SECONDS, Reap};
use crate::launch::{self, Ask, Machine, procs};
use std::collections::HashSet;

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
    let closures = launch::cleanup(
        store,
        &Binary::of(c.env),
        &*procs::table(c.env),
        dry_run,
        &[],
    )?;
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

/// `close SLUG HANDLE`: one agent's terminal, whatever it holds, recorded with its restore line.
pub(super) fn close(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let (slug, handle) = (c.arg("slug"), c.arg("handle"));
    let actor = c.actor()?;
    let closed = launch::close(
        store,
        &Binary::of(c.env),
        &*procs::table(c.env),
        &actor,
        &slug,
        &handle,
    )?;
    c.say(&format!(
        "closed pane {} of {handle} ({}){}",
        closed.pane,
        closed.why,
        restore_part(&closed.restore)
    ));
    Ok(())
}

fn restore_part(line: &str) -> String {
    if line.is_empty() {
        String::new()
    } else {
        format!("; restore: {line}")
    }
}

/// ` of HANDLE`, or nothing for a terminal of no known session.
fn of(handle: &str) -> String {
    if handle.is_empty() {
        String::new()
    } else {
        format!(" of {handle}")
    }
}

/// `VERB pane T of H (WHY); restore: R`, for a terminal a sweep closed or would close.
fn closed_line(verb: &str, closed: &reap::Closed) -> String {
    format!(
        "{verb} pane {}{} ({}){}",
        closed.pane,
        of(&closed.handle),
        closed.why,
        restore_part(&closed.restore)
    )
}

/// `VERB pane T of H (WHY)`, for a terminal a sweep left open.
fn kept_line(verb: &str, kept: &reap::Kept) -> String {
    format!(
        "{verb} pane {}{} ({})",
        kept.pane,
        of(&kept.handle),
        kept.why
    )
}

/// `clean [--dry-run]`: cleanup, then one idle sweep, at once (`just clean`), under their own rules
/// and never closing the caller's terminal or session: what closed, and why each terminal of the
/// board left open stays.
pub(super) fn clean(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let dry_run = c.parsed.flag("dry_run");
    let caller = c.actor().ok();
    let processes = procs::table(c.env);
    let hc = Binary::of(c.env);
    let closures = launch::cleanup_as(store, &hc, &*processes, dry_run, &[], caller.as_deref())?;
    // The terminals cleanup removed (or would): the sweep says nothing more of those. A record
    // whose pane is gone removes no terminal, and one hc refused stays: the sweep says why a
    // terminal it names stays, on screen say.
    let mut removed = HashSet::new();
    let mut said = false;
    for closure in closures.iter().filter(|closure| !closure.kept) {
        said = true;
        if closure.why != launch::GONE && (dry_run || closure.closed) {
            removed.insert(closure.pane.clone());
        }
        let verb = match (dry_run, closure.closed) {
            (true, _) => "would close",
            (false, true) => "closed",
            (false, false) => "could not close",
        };
        let owner = closure.handle.as_deref().map_or(String::new(), of);
        c.say(&format!(
            "{verb} pane {}{owner} ({}){}",
            closure.pane,
            closure.why,
            restore_part(&closure.restore)
        ));
    }
    let how = Reap {
        state: reap::state_path(c.env, &c.env.cwd).map_err(Stop::Board)?,
        apply: !dry_run,
        processes: &*processes,
        caller: caller.as_deref(),
        min_idle: REAP_SECONDS,
    };
    let Some(swept) = reap::sweep(store, &hc, &how, &mut *c.err)? else {
        for closure in closures.iter().filter(|closure| closure.kept) {
            let owner = closure.handle.as_deref().map_or(String::new(), of);
            c.say(&format!(
                "kept pane {}{owner} ({})",
                closure.pane, closure.why
            ));
        }
        c.say("another sweep is running (the Symphony's timer): no idle sweep now");
        return Ok(());
    };
    let verb = if dry_run { "would close" } else { "closed" };
    for closed in swept.closed.iter().filter(|s| !removed.contains(&s.pane)) {
        c.say(&closed_line(verb, closed));
    }
    for kept in swept.kept.iter().filter(|s| !removed.contains(&s.pane)) {
        c.say(&kept_line("kept", kept));
    }
    if !said && swept.closed.is_empty() && swept.kept.is_empty() {
        c.say("nothing to clean up");
    }
    Ok(())
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
    let processes = procs::table(c.env);
    let how = Reap {
        state: reap::state_path(c.env, &c.env.cwd).map_err(Stop::Board)?,
        apply: !dry_run,
        processes: &*processes,
        caller: caller.as_deref(),
        min_idle: REAP_SECONDS,
    };
    let Some(swept) = reap::sweep(store, &Binary::of(c.env), &how, &mut *c.err)? else {
        c.say("another sweep is running (the Symphony's timer): nothing done");
        return Ok(());
    };
    let verb = if dry_run { "would close" } else { "closed" };
    for closed in &swept.closed {
        c.say(&closed_line(verb, closed));
    }
    if dry_run {
        // every reason a sweep would keep a terminal for, said now
        for kept in &swept.kept {
            c.say(&kept_line("keeps", kept));
        }
    } else {
        // An idle terminal's reason, once per change: what the Symphony's log shows (its stderr).
        for kept in swept.kept.iter().filter(|kept| kept.told) {
            let _ = writeln!(c.err, "· idle sweep {}", kept_line("keeps", kept));
        }
        if !swept.idle.is_empty() {
            c.say(&format!(
                "idle once, closed if still idle next sweep: {}",
                swept.idle.join(", ")
            ));
        }
    }
    if swept.closed.is_empty() && (dry_run || swept.idle.is_empty()) {
        c.say("nothing to close");
    }
    if !dry_run {
        match launch::restore_unanswered(
            store,
            &Binary::of(c.env),
            &Machine::of(c.env),
            &mut *c.err,
        ) {
            Ok(lines) => {
                for line in lines {
                    c.say(&line);
                }
            }
            Err(problem) => {
                let _ = writeln!(c.err, "unanswered recovery unavailable: {problem}");
            }
        }
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
    let lines = launch::finish(store, &Binary::of(c.env), &*procs::table(c.env), slug);
    for line in lines {
        c.say(&format!("→ {line}"));
    }
}
