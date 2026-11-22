//! The commands that drive agents' hc terminals: `launch`, `restore`, `cleanup`, `clean`, `close`,
//! `reap` and `finish`, and the cleanup after a task finishes, each a call into [`crate::launch`] and the
//! lines it says. The caller's own terminal (`HC_TERMINAL_ID`) is never closed. A refusal (an hc or
//! git step that failed, or the board's) is exit 1, a board that cannot be reached exit 2.

use super::{Context, Stop};
use crate::Store;
use crate::launch::hc::Binary;
use crate::launch::reap::{self, REAP_SECONDS, Reap};
use crate::launch::{self, Ask, Defaults, Machine, procs, running};
use std::collections::HashSet;

/// `launch SLUG --model M ...`: the agent and model named, else the project's `[agents] launch` on
/// the model `[agents.models]` names for the role ([`Defaults`]), else the board's own. Refused
/// while this machine runs as many agents as `[limits] machine_agents` allows, or they cannot be
/// counted under it ([`running::refusing_launch`]).
pub(super) fn launch(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let actor = c.actor()?;
    let machine = Machine::of(c.env);
    let role = p.or("role", "executor");
    let resolved = Defaults::configured_with(&machine.root, &store.stored())
        .map_err(Stop::Refused)?
        .resolve_as(Some(role), p.get("kind"), p.get("model"));
    let hc = Binary::of(c.env);
    if let Some(why) = running::refusing_launch(store, c.env, &hc) {
        return Err(Stop::Refused(format!("no launch: {why}")));
    }
    let ask = Ask {
        slug: c.arg("slug"),
        actor,
        model: resolved.model.clone(),
        effort: resolved.effort.clone(),
        kind: Some(p.get("kind").unwrap_or(resolved.agent).to_string()),
        role: role.into(),
        branch: p.get("branch").map(str::to_string),
        detached: p.flag("detached"),
        worktree: p.get("worktree").map(|w| c.env.cwd.join(w)),
        prompt: p.or("prompt", "").into(),
        tab: p.get("tab").map(str::to_string),
        beside: p.get("beside").map(str::to_string),
    };
    let started = launch::launch(store, &hc, &machine, &ask)?;
    let s = &started.row;
    if started.resumed {
        // The task's recorded reviewer, on its own agent and model: not what a launch would start.
        c.say(&format!(
            "resumed {}, {}'s recorded {}: pane {}, agent session {}, worktree {}",
            s.handle, s.slug, s.role, s.pane, s.claude_session, s.worktree
        ));
        return Ok(());
    }
    c.say(&format!(
        "launched {} on {} as {}: {}, pane {}, agent session {}, worktree {}",
        s.handle,
        s.slug,
        s.role,
        resolved.said(),
        s.pane,
        s.claude_session,
        s.worktree
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

/// `clean [--dry-run]`: cleanup, then one idle sweep, at once (`just tasks clean`), under their own rules
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
/// What a sweep says of its gives ([`Store::give_unowned`]): each task given, to whom and from whom
/// or why, as its thread's line says it; then the tasks it marked waiting for an orchestrator, none
/// that stays being there; a dry run, every task that would wait, marked before or not.
fn given_lines(given: &crate::Given, dry_run: bool) -> Vec<String> {
    let verb = if dry_run { "would give" } else { "gave" };
    let mut lines: Vec<String> = (given.gave.iter().zip(&given.lines))
        .map(|((slug, to), line)| {
            let how = line
                .split_once(crate::orchestration::GIVEN)
                .map_or("", |(_, how)| how);
            format!("{verb} {slug} to {to}{how}")
        })
        .collect();
    let waiting = if dry_run {
        &given.waiting
    } else {
        &given.marked
    };
    if !waiting.is_empty() {
        let would = if dry_run { "would wait" } else { "wait" };
        lines.push(format!(
            "{would} for an orchestrator under its limit that stays: {}",
            waiting.join(", ")
        ));
    }
    lines
}

pub(super) fn reap(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let dry_run = c.parsed.flag("dry_run");
    // Every open task has an orchestrator: what nobody orchestrates goes to one that stays first, so
    // staffing and the sweep read it held.
    let given = match dry_run {
        true => store.would_give_unowned(),
        false => store.give_unowned(None),
    };
    match given {
        Ok(given) => {
            for line in given_lines(&given, dry_run) {
                c.say(&line);
            }
        }
        Err(problem) => {
            let _ = writeln!(
                c.err,
                "giving open tasks an orchestrator failed; idle sweep continues: {problem}"
            );
        }
    }
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
