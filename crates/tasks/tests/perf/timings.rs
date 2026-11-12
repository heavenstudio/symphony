//! The timings: say, claim, inbox, a look of `tasks wait` and the `tasks api` answers land asks
//! for, each the `tasks` binary run as an agent runs it on the generated board, three times, the
//! median of the CPU each run cost within a bound: the `tasks` process's own and the board's
//! server's while it ran ([`crate::cpu`]). That is the work the command did, which the machine's
//! load does not multiply as it multiplies a wall: at load 170 on 16 cores a command gets about a
//! tenth of a core, so its wall stretches some tenfold while its work stays what it was. The walls
//! are printed beside, for reading.
//! A say dispatches in the write (`TASKS_DISPATCH=inline`), so its cost is the write's and the
//! delivery to its recipients', 100 of them on this board.
//!
//! The bounds are stated once, at the end, with what justifies them. On 2026-11-07 the live board
//! answered a say in 6 to 9 minutes and a claim in 2: each recipient's inbox read cost 7 s of the
//! server's time, all of it work, which a bound on CPU holds at any load.

use crate::board::board;
use crate::generated;
use serde_json::{Value, json};
use std::time::Duration;

/// What is timed and its bound. A command line with its stdin, and the most CPU its median may take.
struct Timed {
    name: String,
    args: Vec<String>,
    input: String,
    bound: Duration,
}

/// One timed name's runs, each its wall and its CPU (the `tasks` process's and the server's), and
/// the bound of their median CPU.
struct Runs {
    walls: Vec<Duration>,
    cpus: Vec<(Duration, Duration)>,
    bound: Duration,
}

/// The middle of `values`, sorted.
fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

fn command(name: &str, args: &[&str], bound: Duration) -> Timed {
    Timed {
        name: name.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        input: String::new(),
        bound,
    }
}

fn api(method: &str, args: Value, bound: Duration) -> Timed {
    Timed {
        name: format!("api {method}"),
        args: vec!["api".into()],
        input: json!({"method": method, "args": args}).to_string(),
        bound,
    }
}

#[test]
fn say_claim_inbox_and_the_api_answer_within_their_bounds_on_a_board_at_scale() {
    let mut board = board();
    let thread = generated::thread();
    let writer = generated::writer();
    let recipient = generated::recipient();
    let orchestrator = generated::orchestrator();
    let mut timed = Vec::new();
    for n in 0..3 {
        timed.push(command(
            "say",
            &[
                "say",
                &thread,
                &format!("timed post {n}"),
                "--to",
                &recipient,
                "--as",
                &writer,
            ],
            SAY,
        ));
    }
    for _ in 0..3 {
        let slug = board.unclaimed();
        timed.push(command("claim", &["claim", &slug, "--as", &writer], CLAIM));
    }
    for _ in 0..3 {
        timed.push(command("inbox", &["inbox", "--as", &orchestrator], INBOX));
    }
    // One look each: the first says the whole digest, the others what changed since.
    let waiter = generated::waiter();
    for _ in 0..3 {
        let look = ["wait", "--timeout", "0", "--as", &waiter];
        timed.push(command("wait", &look, WAIT));
    }
    for _ in 0..3 {
        timed.extend([
            api("get", json!({"slug": thread}), API),
            api("tasks", json!({"status": "review"}), API),
            api("ever_executed", json!({"slug": thread}), API),
            api("sessions", json!({}), API),
            api("thread", json!({"slug": thread}), API),
            api("landings_posted", json!({}), API),
            api("last_messages", json!({}), API),
            api("to_owner", json!({"limit": 40}), API),
            api("inbox", json!({"handle": orchestrator}), API),
        ]);
    }
    let mut timings: std::collections::BTreeMap<String, Runs> = std::collections::BTreeMap::new();
    for step in &timed {
        let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
        let ran = board.run(&board.addr, &args, &step.input);
        assert_eq!(ran.code, 0, "{}: {}", step.name, ran.err);
        let entry = (timings.entry(step.name.clone())).or_insert(Runs {
            walls: Vec::new(),
            cpus: Vec::new(),
            bound: step.bound,
        });
        entry.walls.push(ran.wall);
        entry.cpus.push((ran.tasks_cpu, ran.server_cpu));
    }
    assert!(
        board
            .listeners
            .hears(&recipient, "timed post 2", Duration::from_secs(5)),
        "the last post reached its recipient: {:?}",
        board.listeners.heard(&recipient)
    );
    let mut over = Vec::new();
    for (name, Runs { walls, cpus, bound }) in timings {
        let wall = median(walls.clone());
        let cpu = median(
            cpus.iter()
                .map(|(tasks, server)| *tasks + *server)
                .collect(),
        );
        eprintln!(
            "perf: {name}: median CPU {cpu:.3?} of {cpus:.3?} (tasks, server), bound {bound:.1?}; median wall {wall:.3?} of {walls:.3?}"
        );
        // A write reads the board for each of its 100 recipients on the server: a meter reading
        // no work there reads another process.
        if ["say", "claim"].contains(&name.as_str()) {
            let idle: Vec<_> = cpus.iter().filter(|(_, server)| server.is_zero()).collect();
            assert!(idle.is_empty(), "{name}: the server took no CPU: {cpus:?}");
        }
        if cpu > bound {
            over.push(format!(
                "{name}: median CPU {cpu:.3?} over its bound {bound:.1?}"
            ));
        }
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// The bounds of the median CPU, for the test's own (debug) build of `tasks` on a machine other
/// agents' runs share. Measured on 2026-11-12 in five runs at load 23 to 140, one with a workspace
/// build beside it, the medians of CPU (wall): say 5.6 to 6.9 s (8.7 to 31.8 s), of which the
/// server's 1.6 to 2.1 s; claim 14.9 to 18.7 s (17.6 to 29.5 s), the server's 9.9 to 13.0 s;
/// inbox 0.12 to 0.16 s; the slowest api answer 0.08 to 0.11 s (`inbox`). Load beyond the cores
/// spreads a command over the efficiency cores and caches others share: it moved a say's CPU by
/// under a quarter where it moved its wall nearly fourfold. Each bound is three to six times the
/// highest of those medians, and far under what the regression guarded costs, all of it the
/// server's CPU: a say and a claim each read the inbox of every one of the 100 recipients (100 of
/// a say's 823 requests), an inbox or an api `inbox` the orchestrator's once, and the
/// orchestrators' sessions read in one as `session WHERE handle IN (SELECT VALUE handle FROM role
/// WHERE role = 'orchestrator')`, the role scan again for each session, take 3.7 s of the server's
/// CPU on this board, where fetched by id they take 5 ms: 370 s for a say or a claim, 3.7 s for an
/// inbox.
const SAY: Duration = Duration::from_secs(20);
const CLAIM: Duration = Duration::from_secs(60);
const INBOX: Duration = Duration::from_secs(1);
const API: Duration = Duration::from_millis(500);
/// One look of `tasks wait` by an orchestrator without a channel, its process's start included.
/// Measured in the same runs: 10.6 to 13.2 s of CPU for the first, which says the whole digest,
/// then medians of 0.49 to 0.63 s (walls 0.49 to 1.29 s); the bound is five times the highest.
const WAIT: Duration = Duration::from_secs(3);
