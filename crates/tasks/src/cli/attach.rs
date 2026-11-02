//! The attach monitor (`just tasks-attach`): a session's ears. It heartbeats and, every few
//! seconds, prints what reached the session: messages and verdicts, roles taken and dropped on its
//! tasks, their status changes, new ready work for an orchestrator, and a child session going stale
//! or lost with work another could take. An orchestrator also hears the PULL digest
//! ([`super::pull`]) only for actionable entries absent from its last delivered digest.

use super::agents::{Probes, View, describe};
use super::pull::digest;
use super::text::event;
use crate::health::{Health, LIVE_SECONDS};
use crate::{Board, Error, Store, Task};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// What a session's monitor sees of the board at one poll.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Seen {
    #[serde(default)]
    pub latest_message: Option<i64>,
    /// Verdicts on these tasks reach this recipient, so their status need not be repeated.
    #[serde(default)]
    pub verdict_tasks: BTreeSet<String>,
    /// Canonical active staffing inputs invalidate PULL without widening ROLE notifications.
    #[serde(default)]
    pub staffing: String,
    /// (slug, handle, role) on each task the session holds a role on.
    pub roles: BTreeSet<(String, String, String)>,
    /// The status of each task it holds a role on, and, for one it orchestrates, of every task below.
    pub statuses: BTreeMap<String, String>,
    /// For an orchestrator, the ready tasks, slug -> title.
    pub ready: BTreeMap<String, String>,
    /// The health of each session it is the parent of.
    pub children: BTreeMap<String, Health>,
}

/// The session's view of the board, in one read however many tasks it orchestrates.
pub fn observe(store: &Store, handle: &str, kind: &str) -> Result<Seen, Error> {
    Ok(seen(&store.board()?, handle, kind))
}

pub(crate) fn seen(board: &Board, handle: &str, kind: &str) -> Seen {
    let mut kids: HashMap<Option<&str>, Vec<&Task>> = HashMap::new();
    for t in &board.tasks {
        kids.entry(t.parent.as_deref()).or_default().push(t);
    }
    let mine: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|t| t.roles.iter().any(|r| r.handle == handle))
        .collect();
    let mut watched: BTreeMap<&str, &Task> = mine.iter().map(|t| (t.slug.as_str(), *t)).collect();
    let mut followed: HashSet<&str> = HashSet::new();
    for t in &mine {
        // What an orchestrator of a task follows includes everything below it.
        if t.roles
            .iter()
            .any(|r| r.handle == handle && r.role == "orchestrator")
        {
            let mut todo = kids
                .get(&Some(t.slug.as_str()))
                .cloned()
                .unwrap_or_default();
            while let Some(below) = todo.pop() {
                if followed.insert(&below.slug) {
                    watched.insert(&below.slug, below);
                    todo.extend(kids.get(&Some(below.slug.as_str())).into_iter().flatten());
                }
            }
        }
    }
    let ready = if kind == "orchestrator" {
        board
            .tasks
            .iter()
            .filter(|t| t.status == "ready")
            .map(|t| (t.slug.clone(), t.title.clone()))
            .collect()
    } else {
        BTreeMap::new()
    };
    let staffing = if kind == "orchestrator" {
        let roles: BTreeSet<_> = board
            .tasks
            .iter()
            .filter(|t| {
                matches!(t.status.as_str(), "ready" | "review")
                    || crate::health::EXECUTING.contains(&t.status.as_str())
            })
            .flat_map(|t| {
                t.roles
                    .iter()
                    .filter(|r| matches!(r.role.as_str(), "executor" | "reviewer" | "reserved"))
                    .map(|r| {
                        (
                            t.slug.as_str(),
                            r.handle.as_str(),
                            r.role.as_str(),
                            r.health.holds(),
                        )
                    })
            })
            .collect();
        serde_json::to_string(&roles).expect("strings and booleans serialize")
    } else {
        String::new()
    };
    Seen {
        latest_message: Some(board.latest_message),
        staffing,
        verdict_tasks: mine
            .iter()
            .filter(|t| {
                t.roles.iter().any(|r| {
                    r.handle == handle && matches!(r.role.as_str(), "executor" | "orchestrator")
                })
            })
            .map(|t| t.slug.clone())
            .chain(followed.into_iter().map(str::to_owned))
            .collect(),
        roles: mine
            .iter()
            .flat_map(|t| {
                t.roles
                    .iter()
                    .map(|r| (t.slug.clone(), r.handle.clone(), r.role.clone()))
            })
            .collect(),
        statuses: watched
            .iter()
            .map(|(slug, t)| (slug.to_string(), t.status.clone()))
            .collect(),
        ready,
        children: board
            .sessions
            .iter()
            .filter(|s| s.row.parent.as_deref() == Some(handle))
            .map(|s| (s.row.handle.clone(), s.health))
            .collect(),
    }
}

pub(crate) fn pull_changed(before: Option<&Seen>, now: &Seen) -> bool {
    before.is_none_or(|before| {
        before.roles != now.roles
            || before.statuses != now.statuses
            || before.children != now.children
            || before.staffing != now.staffing
    })
}

fn latest_status_action<'a>(
    messages: &'a [crate::Message],
    slug: &str,
) -> Option<&'a crate::Message> {
    messages.iter().rev().find(|m| {
        m.slug == slug
            && (m.kind == "verdict"
                || (m.kind == "system"
                    && (m.text.starts_with("created by ")
                        || m.text.contains(" moved it ")
                        || m.text.contains(" claimed it")
                        || m.text.contains(" released "))))
    })
}

/// The lines for what changed between two looks.
pub fn diff(
    store: &Store,
    before: &Seen,
    now: &Seen,
    handle: &str,
    emit: &mut dyn FnMut(&str),
) -> Result<(), Error> {
    let confirmed = store.confirmed_callbacks()?;
    let through = now.latest_message.unwrap_or_default();
    let since = before.latest_message.unwrap_or(through);
    let needs_actions = before.statuses != now.statuses
        || now
            .ready
            .keys()
            .any(|slug| !before.ready.contains_key(slug));
    let messages = if needs_actions && since < through {
        store.messages_after(since, through)?
    } else {
        Vec::new()
    };
    // Joining a task reveals its existing participants. Those roles are context, not new actions
    // for the joining recipient (and must not echo its own join back through the other roles).
    let joined: BTreeSet<&str> = now
        .roles
        .iter()
        .filter(|(slug, who, _)| {
            who == handle
                && !before
                    .roles
                    .iter()
                    .any(|(old_slug, old_who, _)| old_slug == slug && old_who == handle)
        })
        .map(|(slug, _, _)| slug.as_str())
        .collect();
    let mut changed: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (slug, who, _) in now.roles.symmetric_difference(&before.roles) {
        if who != handle
            && now.roles.iter().any(|(s, _, _)| s == slug)
            && !joined.contains(slug.as_str())
        {
            changed.insert((slug, who));
        }
    }
    let mut announced = BTreeSet::new();
    for (slug, who) in changed {
        let roles: Vec<&str> = now
            .roles
            .iter()
            .filter(|(s, h, _)| s == slug && h == who)
            .map(|(_, _, role)| role.as_str())
            .collect();
        let roles = if roles.is_empty() {
            "none".to_string()
        } else {
            roles.join(", ")
        };
        let status = &now.statuses[slug];
        emit(&format!("ROLE {slug} {who} {roles}, {status}"));
        announced.insert(slug);
    }
    for (slug, status) in &now.statuses {
        if let Some(old) = before.statuses.get(slug)
            && old != status
            && !announced.contains(slug.as_str())
            && !latest_status_action(&messages, slug).is_some_and(|m| {
                m.sender == handle
                    || (m.kind == "verdict"
                        && (now.verdict_tasks.contains(slug)
                            || (m.sender == "owner"
                                && now
                                    .roles
                                    .iter()
                                    .any(|(s, who, _)| s == slug && who == handle))))
            })
        {
            emit(&format!("STATUS {slug} {old} → {status}"));
        }
    }
    for (slug, title) in &now.ready {
        if !before.ready.contains_key(slug)
            && !latest_status_action(&messages, slug).is_some_and(|m| m.sender == handle)
            && !announced.contains(slug.as_str())
        {
            emit(&format!("NEW {slug} ready — {title}"));
        }
    }
    for (child, &health) in &now.children {
        if confirmed.contains(child)
            || before.children.get(child) == Some(&health)
            || matches!(health, Health::Live | Health::Left)
        {
            continue;
        }
        let held = store.claimable(child)?;
        if held.is_empty() {
            continue; // a child holding nothing another could take ends quietly
        }
        if health == Health::Unknown {
            emit(&format!(
                "LIVENESS UNKNOWN {child} — native presence sweep overdue or failed; work remains held"
            ));
        } else if health == Health::Stale {
            emit(&format!(
                "STALE {child} — no sign of it for {LIVE_SECONDS} s"
            ));
        } else {
            emit(&format!(
                "LOST {child} — its work is claimable: {}",
                held.join(", ")
            ));
        }
    }
    Ok(())
}

/// How a monitor runs.
pub struct Watch<'a> {
    /// Seconds between looks.
    pub interval: f64,
    /// Seconds between heartbeats.
    pub beat: f64,
    /// Stop after this many waits (tests); None runs until the session is refused a beat.
    pub polls: Option<i64>,
    /// A child whose own verify is running or queued for a slot is never called STALE or LOST while it
    /// is; the PULL digest's idle and verifying agents come from the same view.
    pub probes: Option<&'a Probes<'a>>,
    /// Whether a task's branch is on main.
    pub landed: Option<&'a dyn Fn(&Task) -> bool>,
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
    // One view of the agents a poll, however many children are quiet, shared with the digest.
    let view = match w.probes {
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
        let digest = pull_digest(store, view.as_ref(), w.landed, stamp)?;
        let entries = digest.entries();
        memory.pull_entries.retain(|entry| entries.contains(entry));
        if !entries.is_subset(&memory.pull_entries) {
            for line in digest.event_lines() {
                emit(&line);
            }
            memory.pull_entries = entries;
        }
        memory.last_pull = Some(stamp);
        memory.last_statuses = statuses;
    }
    if let Some(before) = &memory.seen {
        diff(store, before, &now, handle, emit)?;
    }
    memory.seen = Some(now);
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
    now: f64,
) -> Result<super::pull::Digest, Error> {
    let (mut idle, mut busy) = (Vec::new(), Vec::new());
    for a in view.map_or(&[][..], |v| v.agents.as_slice()) {
        if a.state == "idle" && a.kind != "orchestrator" && a.kind != "chat" {
            idle.push(a.handle.clone());
        } else if a.state == "running_test" {
            busy.push(a.handle.clone());
        }
    }
    digest(store, &idle, landed, now, &busy)
}
