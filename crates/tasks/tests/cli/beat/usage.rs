//! A beat hands the board the tokens its session's own transcript gained since its last beat
//! (`symphony_board::usage::transcript`), and `tasks show` reads them back.

use super::{bash, beat, marked, ok, tool};
use crate::{env, run, scratch};
use serde_json::{Value, json};
use std::fs;
use std::io::Write as _;
use std::path::Path;
use symphony_board::cli::Env;

/// An assistant line of message `id`, worth `n`: `n` uncached, `10 n` written to the cache for five
/// minutes, `1000 n` read from it, `2 n` out.
fn assistant(id: &str, n: u64) -> String {
    json!({"type": "assistant", "message": {"id": id, "model": "claude-opus-5-5", "usage": {
        "input_tokens": n, "cache_creation_input_tokens": 10 * n, "cache_read_input_tokens": 1000 * n,
        "output_tokens": 2 * n, "speed": "standard",
        "cache_creation": {"ephemeral_5m_input_tokens": 10 * n, "ephemeral_1h_input_tokens": 0}}}})
    .to_string()
}

fn append(path: &Path, lines: &[String]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
}

/// `sim/jump`'s tokens, all told, as `tasks show --json` gives them.
fn total(env: &Env) -> Value {
    let shown = run(env, &["show", "sim/jump", "--json"], "");
    assert_eq!(shown.code, 0, "{}", shown.err);
    serde_json::from_str::<Value>(&shown.out).unwrap()["tokens"]["total"].clone()
}

fn worth(n: u64, calls: u64) -> Value {
    json!({"input": n, "cache_write_5m": 10 * n, "cache_write_1h": 0, "cache_read": 1000 * n,
           "output": 2 * n, "calls": calls})
}

#[test]
fn a_subagents_beats_give_its_own_transcripts_tokens_once_from_its_binding_on() {
    let dir = scratch("beat-usage");
    let mut env = env();
    env.vars.insert(
        "TASKS_USAGE".into(),
        dir.join("usage").to_string_lossy().into_owned(),
    );
    let h = ok(&env, &["register", "--kind", "worker"]);
    ok(&env, &["--as", &h, "create", "sim/jump", "Jump"]);
    ok(&env, &["--as", &h, "claim", "sim/jump"]);
    let (main, own) = (
        dir.join("s-1.jsonl"),
        dir.join("s-1").join("subagents").join("agent-a1.jsonl"),
    );
    append(&main, &[assistant("parent", 100)]);
    append(&own, &[assistant("m1", 1), assistant("m1", 1)]);
    let at = |mut event: Value| {
        event["transcript_path"] = json!(main);
        event
    };
    // Before a board command binds it, nothing is the board's: what it read waits.
    beat(&env, &at(tool("Read", "a1")));
    assert_eq!(total(&env), worth(0, 0));
    append(&own, &[assistant("m2", 2)]);
    let command = format!("just tasks show sim/jump --as {h}");
    beat(&env, &at(bash(&command).agent("a1").0));
    assert_eq!(
        total(&env),
        worth(3, 2),
        "its own two messages, the parent's none"
    );
    beat(&env, &at(bash(&command).agent("a1").ended().0));
    assert_eq!(total(&env), worth(3, 2), "nothing new, nothing added");
    // An event `just tasks beat` reads is one an agent wrote: the transcript it names is not read.
    append(&own, &[assistant("m3", 4)]);
    beat(&marked(&env), &at(tool("Read", "a1")));
    assert_eq!(total(&env), worth(3, 2));
    beat(&env, &at(tool("Read", "a1")));
    assert_eq!(total(&env), worth(7, 3));
    let shown = ok(&env, &["show", "sim/jump"]);
    for said in [
        "  Tokens: 7k in 3 calls (input 7 · cache write 70 · cache read 7k · output 14)",
        "    by role: executor 7k",
        "    by round: round 1 7k",
    ] {
        assert!(shown.contains(said), "{said:?} not in {shown}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_test_board_with_no_usage_folder_reads_no_transcript() {
    let dir = scratch("beat-no-usage");
    let env = env();
    let h = ok(&env, &["register", "--kind", "worker"]);
    ok(&env, &["--as", &h, "create", "sim/jump", "Jump"]);
    ok(&env, &["--as", &h, "claim", "sim/jump"]);
    let own = dir.join("s-1").join("subagents").join("agent-a1.jsonl");
    append(&own, &[assistant("m1", 1)]);
    let mut event = bash(&format!("just tasks inbox --as {h}")).agent("a1").0;
    event["transcript_path"] = json!(dir.join("s-1.jsonl"));
    beat(&env, &event);
    assert_eq!(total(&env), worth(0, 0));
    assert!(!ok(&env, &["show", "sim/jump"]).contains("Tokens:"));
    let _ = fs::remove_dir_all(&dir);
}
