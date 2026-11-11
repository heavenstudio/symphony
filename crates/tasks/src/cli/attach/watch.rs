//! The monitor's loop ([`watch`]): a heartbeat, and at each look the inbox, the PULL digest when
//! due, the event lines ([`super::diff`]) and the pane watch ([`super::pane_watch`]).

use super::{PaneWatch, Seen, diff, overseen, pane_watch, pull_changed, seen};
use crate::cli::agents::{Probes, View, describe};
use crate::cli::pull::{Digest, digest_with_tips};
use crate::cli::text::event;
use crate::health::Health;
use crate::{Error, Store, Task};
use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// How a monitor runs.
pub struct Watch<'a> {
    /// Seconds between looks.
    pub interval: f64,
    /// Seconds between heartbeats.
    pub beat: f64,
    /// Stop after this many waits (tests); None runs until the session is refused a beat.
    pub polls: Option<i64>,
    /// A child whose own verify is running or queued for a slot, or an in-process child with a process
    /// still working in its own worktree, is never called STALE or LOST while it is; the PULL digest's
    /// idle and verifying agents come from the same view. The pane watch asks hc through them; with
    /// none, it does not run.
    pub probes: Option<&'a Probes<'a>>,
    /// Whether a task's branch is on main.
    pub landed: Option<&'a dyn Fn(&Task) -> bool>,
    /// The tip of a task's branch, which the pipeline alarm judges a landing's record of what the
    /// task waits for against; with none, the record is taken as it stands.
    pub tips: Option<crate::alarm::Tips<'a>>,
    /// Seconds between computing PULL inputs; only new actionable entries emit a digest.
    pub pull_every: Option<f64>,
}

/// What the monitor remembers between looks.
#[derive(Default)]
struct Memory {
    seen: Option<Seen>,
    last_pull: Option<f64>,
    last_statuses: BTreeMap<String, String>,
    pull_entries: BTreeSet<(String, String)>,
    panes: PaneWatch,
}

/// Heartbeat every `w.beat` s and print this session's events every `w.interval` s, through `emit`;
/// `sleep` waits. The first look is the baseline (unread messages still print). A look that fails is
/// said and the watch goes on; a refused heartbeat (the session left) ends it.
pub fn watch(
    store: &Store,
    handle: &str,
    w: &Watch,
    emit: &mut dyn FnMut(&str),
    sleep: &mut dyn FnMut(f64),
) -> Result<(), Error> {
    let kind = store.session(handle)?.row.kind;
    let mut memory = Memory::default();
    let mut done: i64 = -1;
    let mut since_beat = 0.0;
    let more = |done: i64| w.polls.is_none_or(|polls| done < polls);
    while more(done) {
        if let Err(problem) = look(store, handle, &kind, w, &mut memory, emit) {
            emit(&format!("WATCH scan failed: {problem}"));
        }
        done += 1;
        if more(done) {
            sleep(w.interval);
            since_beat += w.interval;
            if since_beat >= w.beat {
                since_beat = 0.0;
                match store.touch(handle) {
                    Ok(()) => {}
                    Err(Error::Refused(why)) => {
                        emit(&format!("WATCH heartbeat refused: {why}"));
                        return Ok(());
                    }
                    Err(problem) => emit(&format!("WATCH heartbeat failed: {problem}")),
                }
            }
        }
    }
    Ok(())
}

fn look(
    store: &Store,
    handle: &str,
    kind: &str,
    w: &Watch,
    memory: &mut Memory,
    emit: &mut dyn FnMut(&str),
) -> Result<(), Error> {
    let inbox = store.inbox(handle)?;
    for m in &inbox {
        emit(&event(m));
    }
    if !inbox.is_empty() {
        let ids: Vec<i64> = inbox.iter().map(|m| m.id).collect();
        store.ack(handle, &ids)?;
    }
    let board = store.board()?;
    let mut now = seen(&board, handle, kind);
    let pull = match w.pull_every {
        Some(every) if kind == "orchestrator" => {
            let statuses: BTreeMap<String, String> = board
                .tasks
                .iter()
                .map(|t| (t.slug.clone(), t.status.clone()))
                .collect();
            let stamp = store.now();
            let due = memory.last_pull.is_none_or(|last| stamp - last >= every);
            (due || statuses != memory.last_statuses || pull_changed(memory.seen.as_ref(), &now))
                .then_some((stamp, statuses))
        }
        _ => None,
    };
    let quiet = |h: &Health| matches!(h, Health::Stale | Health::Lost | Health::Unknown);
    let any_quiet = now.children.values().any(quiet);
    // hc is asked at most once a poll, by the agents view and the pane watch together.
    let sample = OnceCell::new();
    let probes = w.probes.map(|probes| Probes {
        panes: Box::new(|| sample.get_or_init(|| (probes.panes)()).clone()),
        runs: Box::new(|| (probes.runs)()),
        branch_of: Box::new(|cwd| (probes.branch_of)(cwd)),
        // The monitor looks every few seconds: the process listing is kept a while.
        processes: Box::new(|| store.kept_scan(&*probes.processes)),
    });
    // One view of the agents a poll, however many children are quiet, shared with the digest.
    let view = match &probes {
        Some(probes) if any_quiet || pull.is_some() => {
            Some(describe(store, probes, store.now(), false)?)
        }
        _ => None,
    };
    if let Some(view) = &view
        && any_quiet
    {
        let verifying = verifying(view);
        for (child, health) in now.children.iter_mut() {
            if quiet(health) && verifying.contains(child.as_str()) {
                *health = Health::Live; // waiting on its own verify: it will beat again when that ends
            }
        }
    }
    if let Some((stamp, statuses)) = pull {
        let digest =
            pull_digest(store, view.as_ref(), w.landed, w.tips, stamp)?.only_for(&board, handle);
        super::pull_updates::emit_new(&digest, &mut memory.pull_entries, emit);
        memory.last_pull = Some(stamp);
        memory.last_statuses = statuses;
    }
    if let Some(before) = &memory.seen {
        diff(store, before, &now, handle, emit)?;
    }
    memory.seen = Some(now);
    if let Some(probes) = &probes {
        let overseen = overseen(&board, handle);
        let open = if overseen.is_empty() {
            Vec::new()
        } else {
            store.open_task_sessions()?
        };
        memory.panes = pane_watch(&board, &overseen, &open, &probes.panes, &memory.panes, emit);
    }
    Ok(())
}

/// The handles the view finds running a test.
fn verifying(view: &View) -> HashSet<&str> {
    view.agents
        .iter()
        .filter(|a| a.state == "running_test")
        .map(|a| a.handle.as_str())
        .collect()
}

fn pull_digest(
    store: &Store,
    view: Option<&View>,
    landed: Option<&dyn Fn(&Task) -> bool>,
    tips: Option<crate::alarm::Tips<'_>>,
    now: f64,
) -> Result<Digest, Error> {
    let (mut idle, mut busy) = (Vec::new(), Vec::new());
    for a in view.map_or(&[][..], |v| v.agents.as_slice()) {
        if a.state == "idle" && a.kind != "orchestrator" && a.kind != "chat" {
            idle.push(a.handle.clone());
        } else if a.state == "running_test" {
            busy.push(a.handle.clone());
        }
    }
    digest_with_tips(store, &idle, landed, tips, now, &busy)
}
