//! The herdr client (`Binary`, `Herdr`'s calls) against the fake as a real binary, and the fake itself.

use crate::{FAKE, World};
use serde_json::json;
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use tasks::cli::Env;
use tasks::launch::LaunchError;
use tasks::launch::herdr::{Binary, Herdr, text, truthy};
use tasks::sys::Command;

fn binary(named: &str) -> Binary {
    let mut vars = BTreeMap::new();
    vars.insert("HERDR_BIN".to_string(), named.to_string());
    Binary::of(&Env {
        vars,
        cwd: std::env::temp_dir(),
    })
}

#[test]
fn a_call_returns_the_result_object() {
    let w = World::new();
    w.add_tab("w1:t5", "x", 2);
    let tabs = w.herdr().tabs().unwrap();
    let seen: Vec<(String, i64)> = tabs
        .iter()
        .map(|t| (text(t.get("tab_id")), t["pane_count"].as_i64().unwrap()))
        .collect();
    assert_eq!(seen, [("w1:t5".to_string(), 2)]);
}

#[test]
fn a_failing_call_names_the_command_and_the_reason() {
    let mut w = World::new();
    w.set("FAKE_HERDR_FAIL", "tab list");
    assert_eq!(
        w.herdr().tabs(),
        Err(LaunchError(
            r#"herdr tab list failed: {"error":{"code":"fake_failure","message":"tab list failed"}}"#.into()
        ))
    );
}

#[test]
fn a_missing_or_unsplittable_binary_is_a_launch_error() {
    let missing = binary("/nonexistent/herdr").tabs();
    assert!(
        matches!(&missing, Err(LaunchError(why)) if why.starts_with("herdr tab list: ")),
        "{missing:?}"
    );
    assert_eq!(
        binary("'unclosed").tabs(),
        Err(LaunchError("HERDR_BIN: No closing quotation".into()))
    );
    assert_eq!(
        binary("   ").tabs(),
        Err(LaunchError("HERDR_BIN names no command".into()))
    );
}

#[test]
fn the_binary_comes_from_herdr_bin_split_as_a_shell_splits_it() {
    let w = World::new();
    let mut env = w.env.clone();
    env.vars
        .insert("HERDR_BIN".into(), format!("sh -c '\"$0\" \"$@\"' {FAKE}"));
    assert_eq!(Binary::of(&env).panes().unwrap(), vec![]);
    assert_eq!(w.calls(), [["pane", "list"].map(String::from).to_vec()]);
}

#[test]
fn an_answer_that_is_not_json_is_named_and_an_empty_one_is_no_result() {
    let w = World::new();
    let mut env = w.env.clone();
    env.vars.insert("HERDR_BIN".into(), "echo not-json".into());
    assert_eq!(
        Binary::of(&env).panes(),
        Err(LaunchError("herdr pane list printed no JSON".into()))
    );
    env.vars.insert("HERDR_BIN".into(), "true".into());
    assert_eq!(Binary::of(&env).panes().unwrap(), vec![]);
    env.vars
        .insert("HERDR_BIN".into(), "sh -c 'echo said >&2; exit 3'".into());
    assert_eq!(
        Binary::of(&env).panes(),
        Err(LaunchError("herdr pane list failed: said".into()))
    );
    env.vars
        .insert("HERDR_BIN".into(), "sh -c 'echo out; exit 3'".into());
    assert_eq!(
        Binary::of(&env).panes(),
        Err(LaunchError("herdr pane list failed: out".into()))
    );
}

#[test]
fn agents_of_another_workspace_are_left_out_and_those_naming_none_kept() {
    let mut w = World::new();
    w.set_state(json!({"tabs": [], "next": 1, "panes": [
        {"pane_id": "a", "agent": "claude", "workspace_id": "w1"},
        {"pane_id": "b", "agent": "claude", "workspace_id": "w2"},
        {"pane_id": "c", "agent": "claude"},
        {"pane_id": "d"},
    ]}));
    let ids = |w: &World| -> Vec<String> {
        w.herdr()
            .agents()
            .unwrap()
            .iter()
            .map(|a| text(a.get("pane_id")))
            .collect()
    };
    assert_eq!(ids(&w), ["a", "b", "c"]);
    w.set("HERDR_WORKSPACE_ID", "w1");
    assert_eq!(ids(&w), ["a", "c"]);
}

#[test]
fn json_values_read_as_python_reads_them() {
    assert_eq!(text(Some(&json!("w1:p1"))), "w1:p1");
    assert_eq!(text(Some(&json!(12))), "12");
    assert_eq!(text(Some(&json!(null))), "");
    assert_eq!(text(None), "");
    for (value, truth) in [
        (json!(null), false),
        (json!(false), false),
        (json!(0), false),
        (json!(""), false),
        (json!([]), false),
        (json!({}), false),
        (json!(true), true),
        (json!(2), true),
        (json!("claude"), true),
        (json!([1]), true),
        (json!({"a": 1}), true),
    ] {
        assert_eq!(truthy(Some(&value)), truth, "{value}");
    }
    assert!(!truthy(None));
}

// -- the fake ---------------------------------------------------------------------------

#[test]
fn the_fake_makes_tabs_and_splits_and_starts_and_closes_panes() {
    let w = World::new();
    let herdr = w.herdr();
    let made = herdr
        .call(&[
            "tab",
            "create",
            "--label",
            "code",
            "--cwd",
            "/w",
            "--no-focus",
        ])
        .unwrap();
    assert_eq!(made["tab"]["tab_id"], "w1:t101");
    assert_eq!(made["root_pane"]["pane_id"], "w1:p101");
    let split = herdr
        .call(&[
            "pane",
            "split",
            "w1:p101",
            "--direction",
            "right",
            "--cwd",
            "/x",
        ])
        .unwrap();
    assert_eq!(split["pane"]["pane_id"], "w1:p102");
    assert_eq!(split["pane"]["tab_id"], "w1:t101");
    herdr
        .call(&[
            "agent", "start", "amber", "--kind", "claude", "--pane", "w1:p102", "--", "--resume",
            "sid",
        ])
        .unwrap();
    let [agent] = <[_; 1]>::try_from(herdr.agents().unwrap()).unwrap();
    assert_eq!(
        (
            agent["name"].as_str(),
            agent["agent_status"].as_str(),
            agent["agent_session"]["value"].as_str()
        ),
        (Some("amber"), Some("idle"), Some("sid"))
    );
    herdr.call(&["agent", "prompt", "amber", "go"]).unwrap();
    assert_eq!(herdr.agents().unwrap()[0]["agent_status"], "working");
    assert_eq!(w.state()["prompts"], json!([["amber", "go"]]));
    herdr.close("w1:p101").unwrap();
    assert_eq!(w.pane_ids(), ["w1:p102"]);
    let gone = herdr.close("w1:p101");
    assert!(
        matches!(&gone, Err(LaunchError(why)) if why.contains("pane_not_found")),
        "{gone:?}"
    );
    let info = herdr.process_info("w1:p102").unwrap();
    assert_eq!(
        (
            info["shell_pid"].as_i64(),
            info["foreground_processes"].clone()
        ),
        (Some(999_999), json!([]))
    );
}

#[test]
fn the_fake_shows_a_stuck_prompt_in_the_input_line_until_enter_sends_it() {
    let mut w = World::new();
    w.set("FAKE_HERDR_STUCK", "1");
    w.set_state(json!({"tabs": [], "next": 1, "panes": [
        {"pane_id": "p", "agent": "claude", "name": "amber", "agent_status": "idle", "screen": "welcome"},
    ]}));
    let herdr = w.herdr();
    herdr
        .call(&["agent", "prompt", "amber", "/work sim/a"])
        .unwrap();
    assert_eq!(herdr.read("p", 12).unwrap(), "welcome\n> /work sim/a\n");
    assert_eq!(herdr.agents().unwrap()[0]["agent_status"], "idle");
    herdr.enter("p").unwrap();
    assert_eq!(herdr.read("p", 12).unwrap(), "welcome\n");
    assert_eq!(herdr.agents().unwrap()[0]["agent_status"], "working");
    // the next prompt is taken
    herdr.call(&["agent", "prompt", "amber", "again"]).unwrap();
    assert_eq!(herdr.read("p", 12).unwrap(), "welcome\n");
}

#[test]
fn a_key_press_to_the_fakes_pending_chooser_moves_what_reads_show_and_writes_no_state() {
    let mut w = World::new();
    let screen = w.dir().join("hooks-screen.txt");
    std::fs::write(&screen, include_str!("fixtures/codex-hooks.txt")).unwrap();
    w.set("FAKE_HERDR_START_SCREEN", screen.to_str().unwrap());
    w.set("FAKE_HERDR_START_WAIT_MS", "1");
    let herdr = w.herdr();
    herdr.call(&["tab", "create", "--label", "x"]).unwrap();
    // The fake alone, not Binary, whose observer would press keys: its wait runs out at once and
    // leaves the chooser pending.
    let start = tasks::sys::output(
        Command::new(FAKE)
            .args([
                "agent", "start", "probe", "--kind", "codex", "--pane", "w1:p101",
            ])
            .env_clear()
            .envs(&w.env.vars),
    )
    .unwrap();
    assert!(
        String::from_utf8_lossy(&start.stderr).contains("timed out waiting for agent startup"),
        "{start:?}"
    );
    let state = w.dir().join("herdr.json");
    let stored = || (std::fs::metadata(&state).unwrap().ino(), w.state());
    let saved = stored();
    let selected = || {
        let shown = herdr.read("w1:p101", 80).unwrap();
        let line = shown.lines().map(str::trim).find(|l| l.starts_with('›'));
        line.unwrap_or_default().to_string()
    };
    assert_eq!(selected(), "› 1. Review hooks");
    herdr
        .call(&["pane", "send-keys", "w1:p101", "down"])
        .unwrap();
    assert_eq!(selected(), "› 2. Trust all and continue");
    let refused = herdr.call(&["pane", "send-keys", "w1:p101", "down", "sideways"]);
    assert!(
        matches!(&refused, Err(LaunchError(why)) if why.contains("unknown startup key sideways")),
        "{refused:?}"
    );
    assert_eq!(selected(), "› 2. Trust all and continue");
    herdr
        .call(&["pane", "send-keys", "w1:p101", "up", "up"])
        .unwrap();
    assert_eq!(
        selected(),
        "› 3. Continue without trusting (hooks won't run)"
    );
    herdr
        .call(&["pane", "send-keys", "w1:p101", "enter"])
        .unwrap();
    assert_eq!(selected(), "› Ask Codex to do anything");
    assert_eq!(stored(), saved);
}

#[test]
fn the_fake_refuses_what_it_does_not_speak() {
    let w = World::new();
    let said = w.herdr().call(&["workspace", "close"]);
    assert!(
        matches!(&said, Err(LaunchError(why)) if why.contains("unsupported")),
        "{said:?}"
    );
    let read = w.herdr().read("nowhere", 3);
    assert!(
        matches!(&read, Err(LaunchError(why)) if why.contains("pane_not_found")),
        "{read:?}"
    );
}
