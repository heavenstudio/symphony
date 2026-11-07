//! The generated board, at the live board's scale of 2026-11-07 or above (49.3k messages, 36.4k
//! deliveries, 2.6k sessions, 3.3k roles of which 598 orchestrator roles held by 7 handles, 2.2k
//! tasks, 95 recipients with a callback, most of them sessions last seen days ago), on a server of
//! this test binary's own that keeps it in `surrealkv://`, the live board's engine; and the `tasks`
//! binary run on it as an agent runs it.
//!
//! The board is made once for the test binary ([`board`]): its copy and the first look of every
//! recipient take most of the suite's time. Each test holds it while it runs, so a timing never
//! shares the server with another test's commands.

use crate::listeners::Listeners;
use crate::proxy::{Proxy, Request};
use crate::store;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use symphony_board::rows::{
    DeliveryRow, Dump, LinkRow, MessageRow, RoleRow, SessionRow, TaskRow, TaskSessionRow,
};
use symphony_board::rpc::Client;
use symphony_board::sys::{self, Child, Command};
use symphony_board::{NAMESPACE, Store};

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

/// The database the board is in on the suite's server.
pub const DATABASE: &str = "perf";

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

fn session(n: usize) -> SessionRow {
    let (kind, last_seen, left) = if n < ORCHESTRATORS {
        let seen = if n == 0 { NOW - 10.0 } else { NOW - 3.0 * DAY };
        ("orchestrator", seen, n >= 5)
    } else if n < ORCHESTRATORS + 3 || n == ORCHESTRATORS + CALLBACKS {
        ("worker", NOW - 10.0, false)
    } else if n < ORCHESTRATORS + CALLBACKS {
        let kind = if n.is_multiple_of(3) {
            "worker"
        } else {
            "reviewer"
        };
        (kind, NOW - (2 + n % 4) as f64 * DAY, false)
    } else {
        let kind = if n % 5 < 3 { "reviewer" } else { "worker" };
        (kind, NOW - (1 + n % 40) as f64 * DAY, n.is_multiple_of(3))
    };
    SessionRow {
        handle: handle(n),
        kind: kind.into(),
        worktree: format!("/w/perf-{n}"),
        branch: format!("perf/{n}"),
        model: "m".into(),
        host: "elsewhere".into(),
        connected: last_seen.min(NOW - 5.0 * DAY),
        last_seen,
        left_at: left.then_some(last_seen),
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
    let (slug, sid) = (slug(at), n as i64 + 1);
    let sender = if n.is_multiple_of(600) {
        "owner".to_string()
    } else {
        handle((n * 31) % SESSIONS)
    };
    let (kind, recipient, text) = match n % 100 {
        0..28 => (
            "system",
            None,
            match n % 4 {
                0 => format!("{sender} moved it to review"),
                1 => format!("{sender} claimed it"),
                2 => format!("{sender} joined as reviewer"),
                _ => format!("{sender} released it"),
            },
        ),
        28..33 => (
            "verdict",
            None,
            format!("{}: {}", ["approve", "changes"][n % 2], words(n, 12)),
        ),
        _ if n % 70 == 35 => ("msg", None, format!("landed {slug} on main at 3e0c661{n}")),
        _ => {
            let to = match n % 10 {
                0 | 1 => Some("@orchestrator".to_string()),
                2 => Some("@executor".to_string()),
                3 => Some("@reviewer".to_string()),
                4 | 5 => Some(handle((n * 17) % SESSIONS)),
                6 if n % 600 == 6 => Some("owner".to_string()),
                _ => None,
            };
            ("msg", to, words(n, 16))
        }
    };
    MessageRow {
        id: sid,
        slug,
        sender,
        recipient,
        kind: kind.into(),
        text,
        ts: NOW - 30.0 * DAY + n as f64 * (30.0 * DAY - HOUR) / MESSAGES as f64,
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

/// What one run of the `tasks` binary said, and how long it took.
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
    pub wall: Duration,
}

/// The suite's board: its server, the recipients' sockets, and what the recipients' first looks
/// sent.
pub struct Board {
    pub addr: String,
    dir: PathBuf,
    pub listeners: Listeners,
    /// What the write whose dispatch gave every recipient its first look sent the server.
    pub first_looks: Vec<Request>,
    /// The ready tasks not claimed yet.
    unclaimed: Vec<String>,
    shell: Child,
}

static BOARD: OnceLock<Mutex<Board>> = OnceLock::new();

unsafe extern "C" {
    fn atexit(at_exit: extern "C" fn()) -> std::ffi::c_int;
}

/// At the test binary's exit the server ends with its shell, and its folder goes.
extern "C" fn stop_board() {
    if let Some(board) = BOARD.get() {
        let mut board = board.lock().unwrap_or_else(|held| held.into_inner());
        drop(board.shell.stdin.take());
        let _ = board.shell.wait();
        let _ = std::fs::remove_dir_all(&board.dir);
    }
}

/// The suite's board, made on first use and held by the caller until it lets go.
pub fn board() -> MutexGuard<'static, Board> {
    BOARD
        .get_or_init(|| Mutex::new(Board::make()))
        .lock()
        .unwrap_or_else(|held| held.into_inner())
}

/// The `tasks` binary timed: `SF_PERF_TASKS` when it names one (another build's, to time it on the
/// same board), else this crate's.
pub fn program() -> PathBuf {
    std::env::var_os("SF_PERF_TASKS")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_tasks")), PathBuf::from)
}

impl Board {
    fn make() -> Board {
        // Short: the recipients' sockets live here, and a socket's path has room for 104 bytes.
        let dir = std::env::temp_dir().join(format!("sf-perf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ps"), "").unwrap();
        let engine = format!("surrealkv://{}", dir.join("data").display());
        let (addr, shell) = store::launch_on(&store::surreal(), &dir.join("surreal.log"), &engine)
            .unwrap_or_else(|why| panic!("the suite's server did not come up: {why:?}"));
        assert_eq!(unsafe { atexit(stop_board) }, 0, "the server's end is set");
        let began = Instant::now();
        let store = Store::with(
            Client::new(&addr, NAMESPACE, DATABASE).waiting(Duration::from_secs(600)),
            Box::new(|| NOW),
            Box::new(|_| false),
            &sys::hostname(),
        )
        .unwrap();
        store.copy(&dump()).unwrap();
        eprintln!("perf: the board copied in {:.1?}", began.elapsed());
        let recipients = recipients();
        let listeners = Listeners::new(&dir, &recipients);
        let path = |handle: &str| format!("uds:{}", listeners.paths[handle].display());
        // One registration through the store, for the listener's identity as the store records it;
        // the rest as rows like it, so no registration dispatches to those before it.
        store
            .callback(&recipients[0], Some(&path(&recipients[0])), false, false)
            .unwrap();
        let owner = store
            .client()
            .query(
                "RETURN (SELECT VALUE owner FROM ONLY type::record('callback_state', $handle));",
                json!({"handle": recipients[0]}),
            )
            .unwrap()
            .remove(0);
        let rows: Vec<Value> = (recipients[1..].iter())
            .map(|handle| {
                json!({"handle": handle, "address": path(handle), "host": sys::hostname(), "owner": owner,
                    "confirmed_until": 0, "chat": false, "chat_fallback": false, "chat_resume": false,
                    "registration_revision": 1})
            })
            .collect();
        store
            .client()
            .query(
                "FOR $row IN $rows { UPSERT type::record('callback_state', $row.handle) CONTENT $row RETURN NONE; }; RETURN NONE;",
                json!({"rows": rows}),
            )
            .unwrap();
        let mut board = Board {
            addr,
            dir,
            listeners,
            first_looks: Vec::new(),
            unclaimed: ready(),
            shell,
        };
        // Every recipient's first look, through the relay: what a write, and the dispatch it makes
        // to recipients not seen before, sends.
        let relay = Proxy::to(&board.addr);
        let began = Instant::now();
        let (thread, writer) = (thread(), writer());
        let opening = [
            "say",
            &thread,
            "the board opens",
            "--to",
            "@orchestrator",
            "--as",
            &writer,
        ];
        let ran = board.run(&relay.addr, &opening, "");
        assert_eq!(ran.code, 0, "the first looks: {}", ran.err);
        eprintln!(
            "perf: every recipient's first look took {:.1?}",
            began.elapsed()
        );
        board.first_looks = relay.take();
        board
    }

    /// A ready task nobody has claimed, for a claim.
    pub fn unclaimed(&mut self) -> String {
        self.unclaimed.pop().expect("a ready task is left to claim")
    }

    /// `tasks ARGS` as the [`program`], on the board as the server at `server` reaches it, `input`
    /// on its stdin, its clock at [`NOW`] and its dispatch in the write, as `TASKS_DISPATCH=inline`
    /// keeps it: what a write costs is what its command takes.
    pub fn run(&self, server: &str, args: &[&str], input: &str) -> Ran {
        use std::io::Write as _;
        let mut command = Command::new(program());
        command
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.dir)
            .env("TASKS_SERVER", server)
            .env("TASKS_DATABASE", DATABASE)
            .env("TASKS_NOW", NOW.to_string())
            .env("TASKS_DISPATCH", "inline")
            .env("TASKS_PS_TABLE", self.dir.join("ps"))
            .env("HERDR_BIN", "/usr/bin/false")
            .current_dir(&self.dir);
        let began = Instant::now();
        let mut child = sys::spawn_piped(&mut command).expect("the tasks binary starts");
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes()).unwrap();
        }
        let said = sys::wait_output(child).expect("the tasks binary ends");
        Ran {
            code: said.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&said.stdout).into_owned(),
            err: String::from_utf8_lossy(&said.stderr).into_owned(),
            wall: began.elapsed(),
        }
    }

    /// A client of the board, straight to its server.
    pub fn client(&self) -> Client {
        Client::new(&self.addr, NAMESPACE, DATABASE).waiting(Duration::from_secs(600))
    }
}

#[test]
fn the_generated_board_is_at_the_live_scale_or_above() {
    let dump = dump();
    let orchestrators: std::collections::BTreeSet<&str> = (dump.roles.iter())
        .filter(|r| r.role == "orchestrator")
        .map(|r| r.handle.as_str())
        .collect();
    assert!(dump.messages.len() >= 49_293 && dump.deliveries.len() >= 36_395);
    assert!(dump.sessions.len() >= 2_588 && dump.roles.len() >= 2_758);
    assert_eq!(orchestrators.len(), ORCHESTRATORS);
    assert!(
        dump.roles
            .iter()
            .filter(|r| r.role == "orchestrator")
            .count()
            >= 597
    );
    assert_eq!(recipients().len(), CALLBACKS);
    let roles: std::collections::BTreeSet<_> = (dump.roles.iter())
        .map(|r| (&r.slug, &r.handle, &r.role))
        .collect();
    assert_eq!(roles.len(), dump.roles.len(), "each role is held once");
}
