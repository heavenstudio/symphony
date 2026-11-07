//! The timings: say, claim, inbox and the `tasks api` answers land asks for, each the `tasks`
//! binary run as an agent runs it on the generated board, three times, its median within a bound.
//! A say dispatches in the write (`TASKS_DISPATCH=inline`), so its time is the write's and the
//! delivery to its recipients', 100 of them on this board.
//!
//! The bounds are stated once, at the end, with what justifies them. On 2026-11-07 the live board
//! answered a say in 6 to 9 minutes and a claim in 2: each recipient's inbox read cost 7 s of the
//! server's time.

use crate::board::{self, board};
use serde_json::{Value, json};
use std::time::Duration;

/// What is timed and its bound. A command line with its stdin, and the most its median may take.
struct Timed {
    name: String,
    args: Vec<String>,
    input: String,
    bound: Duration,
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
    let (thread, writer, recipient, orchestrator) = (
        board::thread(),
        board::writer(),
        board::recipient(),
        board::orchestrator(),
    );
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
    let mut walls: std::collections::BTreeMap<String, (Vec<Duration>, Duration)> =
        std::collections::BTreeMap::new();
    for t in &timed {
        let args: Vec<&str> = t.args.iter().map(String::as_str).collect();
        let ran = board.run(&board.addr, &args, &t.input);
        assert_eq!(ran.code, 0, "{}: {}", t.name, ran.err);
        let entry = walls.entry(t.name.clone()).or_insert((Vec::new(), t.bound));
        entry.0.push(ran.wall);
    }
    assert!(
        board
            .listeners
            .hears(&recipient, "timed post 2", Duration::from_secs(5)),
        "the last post reached its recipient: {:?}",
        board.listeners.heard(&recipient)
    );
    let mut over = Vec::new();
    for (name, (mut runs, bound)) in walls {
        runs.sort();
        let median = runs[runs.len() / 2];
        eprintln!("perf: {name}: median {median:.3?} of {runs:.3?}, bound {bound:.0?}");
        if median > bound {
            over.push(format!(
                "{name}: median {median:.3?} over its bound {bound:.0?}"
            ));
        }
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// The bounds of the medians, for the test's own (debug) build of `tasks` on a machine other
/// agents' runs share. Measured on 2026-11-07 once indexed: at load 20 to 75, say 6.6 s, claim
/// 7.3 s, inbox 0.19 s and the slowest api answer 0.15 s (`inbox`); at load 110 to 140, say 13.1 s,
/// claim 13.9 s, inbox 0.64 s and api 0.31 s (the inbox still on its whole-table read then); a
/// release build answered in 3.0 s, 3.6 s, 0.12 s and 0.10 s. Each bound is three to five times
/// the loaded medians, room for a load twice that, and far under what the same command took before
/// the indexes: main's first say on this board took 23 minutes, the live board's 6 to 9, and
/// main's inbox of an orchestrator 7 s on a copy of the live board.
const SAY: Duration = Duration::from_secs(60);
const CLAIM: Duration = Duration::from_secs(60);
const INBOX: Duration = Duration::from_secs(2);
const API: Duration = Duration::from_secs(1);
