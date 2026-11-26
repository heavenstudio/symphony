//! The generated board's rows, at the live board's scale of 2026-11-07 or above (49.3k messages,
//! 36.4k deliveries, 2.6k sessions, 3.3k roles of which 598 orchestrator roles held by 7 handles,
//! 2.2k tasks, 95 recipients with a callback, most of them sessions last seen days ago), in the
//! shape the copy takes ([`dump`]), and who is who on it.

use symphony_board::rows::{
    DeliveryRow, Dump, LinkRow, MessageRow, RoleRow, SessionRow, TaskRow, TaskSessionRow,
};

/// The board's clock, `TASKS_NOW` for every command.
pub const NOW: f64 = 1_800_000_000.0;
const HOUR: f64 = 3_600.0;
const DAY: f64 = 86_400.0;

pub const TASKS: usize = 2_200;
pub const SESSIONS: usize = 2_600;
pub const MESSAGES: usize = 50_000;
pub const DELIVERIES: usize = 36_400;
/// Recipients with a native callback: the live orchestrator, three live workers, and 96 sessions
/// last seen two to five days ago whose registrations stand, as the live board's Codex ones did.
pub const CALLBACKS: usize = 100;
/// Sessions of kind orchestrator, each holding orchestrator roles: the first is live.
const ORCHESTRATORS: usize = 7;
/// The first task still open: the board's history before it is done or cancelled.
const OPEN: usize = 1_970;

pub fn handle(n: usize) -> String {
    format!("perf-{n:04}")
}

pub fn slug(n: usize) -> String {
    format!("area{:02}/task-{n:04}", n % 40)
}

/// The live orchestrator, which holds a callback.
pub fn orchestrator() -> String {
    handle(0)
}

/// An orchestrator without a callback, last seen days ago: who waits in the timings.
pub fn waiter() -> String {
    handle(1)
}

/// Another orchestrator without a callback, last seen days ago, whom no other test acts as: the
/// plan check's look makes it one that stays, as the timings' looks make the waiter.
pub fn joiner() -> String {
    handle(2)
}

/// A live worker with a callback, who executes nothing: the recipient of the posts timed.
pub fn recipient() -> String {
    handle(ORCHESTRATORS)
}

/// A live worker without a callback: who says, claims and reads in the timings.
pub fn writer() -> String {
    handle(ORCHESTRATORS + CALLBACKS)
}

/// The recipients' handles: the live orchestrator, then sessions `ORCHESTRATORS..`.
pub fn recipients() -> Vec<String> {
    std::iter::once(orchestrator())
        .chain((ORCHESTRATORS..ORCHESTRATORS + CALLBACKS - 1).map(handle))
        .collect()
}

/// What status task `n` is in: the history done (one in seven cancelled), then the open tasks
/// in progress, in review, blocked, in the backlog and ready.
fn status(n: usize) -> &'static str {
    match n.checked_sub(OPEN) {
        None if n.is_multiple_of(7) => "cancelled",
        None => "done",
        Some(open) => match open {
            0..80 => "in_progress",
            80..140 => "review",
            140..160 => "blocked",
            160..170 => "backlog",
            _ => "ready",
        },
    }
}

/// Whether open task `n` waits on the open task before it, as one open task in three does.
fn waits(n: usize) -> bool {
    n > OPEN && (n - OPEN - 1).is_multiple_of(3)
}

/// The ready tasks that wait on nothing and that nobody holds: each claim the suite makes takes
/// one of them.
pub fn ready() -> Vec<String> {
    (OPEN..TASKS)
        .filter(|&n| status(n) == "ready" && !waits(n))
        .map(slug)
        .collect()
}

/// An open task in progress the live orchestrator oversees: where the timed posts go.
pub fn thread() -> String {
    slug((OPEN..TASKS).find(|&n| n % ORCHESTRATORS == 0).unwrap())
}

/// `count` words of a 4000-word vocabulary drawn by `seed`: text the full-text index takes term by
/// term, as it takes a thread's.
fn words(seed: usize, count: usize) -> String {
    let mut state = seed as u64;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            format!("w{}", (state >> 33) % 4000)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a session is and when it was last about: its kind, when it was last seen, and whether it
/// has left.
struct Presence {
    kind: &'static str,
    last_seen: f64,
    left: bool,
}

/// Session `n`'s presence: the orchestrators (the first live, two of them gone), three live
/// workers, the recipients last seen days ago, the live writer, then the board's history.
fn presence(n: usize) -> Presence {
    let live = NOW - 10.0;
    let days = |count: usize| NOW - count as f64 * DAY;
    if n < ORCHESTRATORS {
        let last_seen = if n == 0 { live } else { days(3) };
        Presence {
            kind: "orchestrator",
            last_seen,
            left: n >= 5,
        }
    } else if n < ORCHESTRATORS + 3 || n == ORCHESTRATORS + CALLBACKS {
        Presence {
            kind: "worker",
            last_seen: live,
            left: false,
        }
    } else if n < ORCHESTRATORS + CALLBACKS {
        Presence {
            kind: if n.is_multiple_of(3) {
                "worker"
            } else {
                "reviewer"
            },
            last_seen: days(2 + n % 4),
            left: false,
        }
    } else {
        Presence {
            kind: if n % 5 < 3 { "reviewer" } else { "worker" },
            last_seen: days(1 + n % 40),
            left: n.is_multiple_of(3),
        }
    }
}

fn session(n: usize) -> SessionRow {
    let presence = presence(n);
    SessionRow {
        handle: handle(n),
        kind: presence.kind.into(),
        worktree: format!("/w/perf-{n}"),
        branch: format!("perf/{n}"),
        model: "m".into(),
        host: "elsewhere".into(),
        connected: presence.last_seen.min(NOW - 5.0 * DAY),
        last_seen: presence.last_seen,
        left_at: presence.left.then_some(presence.last_seen),
        ..SessionRow::default()
    }
}

fn task(n: usize) -> TaskRow {
    TaskRow {
        id: n as i64 + 1,
        slug: slug(n),
        title: format!("Task {n} {}", words(n, 6)),
        body: words(TASKS + n, 30),
        status: status(n).into(),
        priority: (n % 4) as i64,
        parent: (!n.is_multiple_of(10)).then(|| slug(n - n % 10)),
        created_by: "owner".into(),
        created: NOW - 40.0 * DAY + n as f64,
        updated: NOW - 2.0 * DAY + n as f64,
        ..TaskRow::default()
    }
}

/// Who executes task `n`: a recipient while it is open, so posts to `@executor` reach one.
fn executor(n: usize) -> String {
    if n >= OPEN {
        handle(ORCHESTRATORS + 3 + n % (CALLBACKS - 4))
    } else {
        handle(
            ORCHESTRATORS + CALLBACKS + 1 + (n * 13) % (SESSIONS - ORCHESTRATORS - CALLBACKS - 1),
        )
    }
}

fn reviewer(n: usize) -> String {
    if n >= OPEN {
        handle(ORCHESTRATORS + 3 + (n * 7) % (CALLBACKS - 4))
    } else {
        handle(
            ORCHESTRATORS + CALLBACKS + 1 + (n * 17) % (SESSIONS - ORCHESTRATORS - CALLBACKS - 1),
        )
    }
}

/// The roles: an orchestrator on each of the last 600 tasks (every open one among them), 7
/// handles between them; an executor on each task from 760 that is or was worked; a reviewer on
/// each from 1000 that was reviewed. About 3k in all.
fn roles() -> Vec<RoleRow> {
    let mut roles = Vec::new();
    let mut held = |n: usize, handle: String, role: &str| {
        roles.push(RoleRow {
            slug: slug(n),
            handle,
            role: role.into(),
            since: NOW - 2.0 * DAY + n as f64,
            expires: None,
        });
    };
    for n in 0..TASKS {
        let status = status(n);
        if n >= TASKS - 600 {
            held(n, handle(n % ORCHESTRATORS), "orchestrator");
        }
        if n >= 760 && matches!(status, "done" | "cancelled" | "in_progress" | "review") {
            held(n, executor(n), "executor");
        }
        if n >= 1_000 && matches!(status, "done" | "review") {
            held(n, reviewer(n), "reviewer");
        }
    }
    roles
}

/// Message `n`: the oldest 30 days ago, the newest an hour ago; the last 5000 on open threads.
fn message(n: usize) -> MessageRow {
    let at = if n >= MESSAGES - 5_000 {
        OPEN + n % (TASKS - OPEN)
    } else {
        (n * 7_919) % TASKS
    };
    let slug = slug(at);
    let sender = if n.is_multiple_of(600) {
        "owner".to_string()
    } else {
        handle((n * 31) % SESSIONS)
    };
    let line = |kind: &str, recipient: Option<String>, text: String| MessageRow {
        id: n as i64 + 1,
        slug: slug.clone(),
        sender: sender.clone(),
        recipient,
        kind: kind.into(),
        text,
        ts: NOW - 30.0 * DAY + n as f64 * (30.0 * DAY - HOUR) / MESSAGES as f64,
        person: false,
    };
    match n % 100 {
        0..28 => line(
            "system",
            None,
            match n % 4 {
                0 => format!("{sender} moved it to review"),
                1 => format!("{sender} claimed it"),
                2 => format!("{sender} joined as reviewer"),
                _ => format!("{sender} released it"),
            },
        ),
        28..33 => line(
            "verdict",
            None,
            format!("{}: {}", ["approve", "changes"][n % 2], words(n, 12)),
        ),
        _ if n % 70 == 35 => line("msg", None, format!("landed {slug} on main at 3e0c661{n}")),
        _ => {
            let to = match n % 10 {
                0 | 1 => Some("@orchestrator".to_string()),
                2 => Some("@executor".to_string()),
                3 => Some("@reviewer".to_string()),
                4 | 5 => Some(handle((n * 17) % SESSIONS)),
                6 if n % 600 == 6 => Some("owner".to_string()),
                _ => None,
            };
            line("msg", to, words(n, 16))
        }
    }
}

/// The whole board, in the dump's shape.
pub fn dump() -> Dump {
    Dump {
        tasks: (0..TASKS).map(task).collect(),
        task_links: (OPEN..TASKS)
            .filter(|&n| waits(n))
            .map(|n| LinkRow {
                slug: slug(n),
                kind: "dep".into(),
                target: slug(n - 1),
            })
            .collect(),
        sessions: (0..SESSIONS).map(session).collect(),
        roles: roles(),
        messages: (0..MESSAGES).map(message).collect(),
        deliveries: (0..DELIVERIES)
            .map(|k| DeliveryRow {
                message: k as i64 + 1,
                handle: handle((k * 7) % SESSIONS),
                acked: NOW - 29.0 * DAY + k as f64,
            })
            .collect(),
        task_sessions: (0..900)
            .map(|i| TaskSessionRow {
                slug: slug(1_300 + i),
                handle: handle(ORCHESTRATORS + CALLBACKS + 1 + i),
                role: "executor".into(),
                claude_session: format!("c-{i}"),
                worktree: format!("/w/perf-{i}"),
                branch: slug(1_300 + i),
                pane: format!("p{i}"),
                model: "m".into(),
                started: NOW - 20.0 * DAY,
                closed: (i < 850).then_some(NOW - 10.0 * DAY),
                closed_why: String::new(),
                restore: String::new(),
            })
            .collect(),
        ..Dump::default()
    }
}

/// How many rows of a kind the generated board has, and how many the live board had (2026-11-07).
struct Count {
    rows: &'static str,
    made: usize,
    live: usize,
}

/// A role as the board keys it: its task, its holder and what it is.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Held<'a> {
    slug: &'a str,
    handle: &'a str,
    role: &'a str,
}

#[test]
fn the_generated_board_is_at_the_live_scale_or_above() {
    use std::collections::BTreeSet;
    let dump = dump();
    let orchestrator_roles: Vec<&RoleRow> = (dump.roles.iter())
        .filter(|r| r.role == "orchestrator")
        .collect();
    let orchestrators: BTreeSet<&str> = (orchestrator_roles.iter())
        .map(|r| r.handle.as_str())
        .collect();
    let counted = |rows: &'static str, made: usize, live: usize| Count { rows, made, live };
    let counts = [
        counted("messages", dump.messages.len(), 49_293),
        counted("deliveries", dump.deliveries.len(), 36_395),
        counted("sessions", dump.sessions.len(), 2_588),
        counted("roles", dump.roles.len(), 2_758),
        counted("orchestrator roles", orchestrator_roles.len(), 597),
    ];
    let short: Vec<String> = (counts.iter())
        .filter(|count| count.made < count.live)
        .map(|count| {
            format!(
                "{}: {}, the live board {}",
                count.rows, count.made, count.live
            )
        })
        .collect();
    assert!(short.is_empty(), "{short:?}");
    assert_eq!(orchestrators.len(), ORCHESTRATORS, "{orchestrators:?}");
    assert_eq!(recipients().len(), CALLBACKS);
    let held: BTreeSet<Held> = (dump.roles.iter())
        .map(|r| Held {
            slug: &r.slug,
            handle: &r.handle,
            role: &r.role,
        })
        .collect();
    assert_eq!(held.len(), dump.roles.len(), "each role is held once");
}
