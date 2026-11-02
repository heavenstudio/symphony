//! A stand-in for the `herdr` binary, for the tests of the launcher and the idle sweep
//! (`symphony/crates/tasks/tests/launch/`): never the live panes. `HERDR_BIN` names it.
//!
//! State is a JSON file named by `$FAKE_HERDR_STATE` (`tabs` and `panes`, plus counters); every call
//! is appended, as one JSON list of its arguments per line, to `$FAKE_HERDR_LOG`. It speaks the slice
//! of herdr the launcher uses, with the same JSON shapes (`.result.tabs`, `.result.panes`,
//! `.result.agents`, `.result.tab` + `.result.root_pane`, `.result.pane`; `pane read` prints a pane's
//! `screen` as plain text, then its input line, `> ` and what sits in it). `FAKE_HERDR_FAIL=<subcommand
//! words>` makes that call exit 1 with an error on stderr; `FAKE_HERDR_BUSY=N` makes the first N
//! `agent start` calls fail with `agent_pane_busy`, as a pane whose shell is not up yet does. A prompt
//! the agent takes sets it `working`, as herdr reports it; `FAKE_HERDR_STUCK=N` leaves the first N
//! prompts in their pane's input line instead, unsent, until `pane send-keys PANE enter` sends it.

use serde_json::{Map, Value, json};
use std::io::Write;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}

/// A herdr error, on stderr as herdr prints one.
fn refuse(code: &str, message: &str, exit: i32) -> i32 {
    eprintln!("{}", json!({"error": {"code": code, "message": message}}));
    exit
}

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn count(name: &str) -> u64 {
    var(name).trim().parse().unwrap_or(0)
}

fn run(argv: &[String]) -> i32 {
    let log = var("FAKE_HERDR_LOG");
    if !log.is_empty() {
        let line = format!("{}\n", json!(argv));
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .and_then(|mut file| file.write_all(line.as_bytes()));
        if let Err(e) = written {
            return refuse("fake_log", &format!("cannot write {log}: {e}"), 2);
        }
    }
    let words = argv.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    if var("FAKE_HERDR_FAIL") == words {
        return refuse("fake_failure", &format!("{words} failed"), 1);
    }
    let path = var("FAKE_HERDR_STATE");
    let read = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()));
    let mut state = match read {
        Ok(Value::Object(state)) => state,
        Ok(_) => return refuse("fake_state", &format!("{path} holds no object"), 2),
        Err(e) => return refuse("fake_state", &format!("{path}: {e}"), 2),
    };
    for key in ["tabs", "panes"] {
        if !state.get(key).is_some_and(Value::is_array) {
            state.insert(key.into(), json!([]));
        }
    }
    let result = match answer(argv, &words, &mut state) {
        Answer::Result(result) => result,
        Answer::Text(text) => {
            println!("{text}");
            return 0;
        }
        Answer::Refused(code, message, exit) => return refuse(code, &message, exit),
    };
    if let Err(e) = save(&path, &state) {
        return refuse("fake_state", &e, 2);
    }
    if words == "agent start"
        && flag(argv, "--kind") == Some("codex")
        && !var("FAKE_HERDR_START_SCREEN").is_empty()
    {
        // Native startup waits for the pane to become interactive, not merely for process creation.
        // Reads/keys run beside this blocked call, just as with the real Herdr client.
        let started = std::time::Instant::now();
        loop {
            let current: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let pane = current["panes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|pane| pane["pane_id"].as_str() == flag(argv, "--pane"));
            if let Some(pane) = pane {
                if let Some(choice) = pane["confirmed_choice"].as_u64() {
                    if choice != 3 {
                        return refuse(
                            "unsafe_choice",
                            "startup trusted hooks or opened their review",
                            1,
                        );
                    }
                    break;
                }
            } else {
                return refuse("pane_not_found", "startup pane disappeared", 1);
            }
            let wait_ms = count("FAKE_HERDR_START_WAIT_MS");
            let wait_ms = if wait_ms == 0 { 10_000 } else { wait_ms };
            if started.elapsed() >= std::time::Duration::from_millis(wait_ms) {
                return refuse("timeout", "timed out waiting for agent startup", 1);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    println!("{}", json!({"id": "fake", "result": result}));
    0
}

/// Write the state whole, by a rename, so a call running beside this one never reads half of it.
fn save(path: &str, state: &Map<String, Value>) -> Result<(), String> {
    let tmp = format!("{path}.{}.tmp", std::process::id());
    std::fs::write(&tmp, Value::Object(state.clone()).to_string())
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("cannot write {path}: {e}"))
}

enum Answer {
    Result(Value),
    /// Printed as it is, the state left as it was.
    Text(String),
    Refused(&'static str, String, i32),
}

/// The value after `name` in `args`, if it is there.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).map(String::as_str)
}

fn panes(state: &mut Map<String, Value>) -> &mut Vec<Value> {
    match state.get_mut("panes") {
        Some(Value::Array(panes)) => panes,
        _ => unreachable!("run made panes a list"),
    }
}

fn pane_mut<'s>(state: &'s mut Map<String, Value>, id: &str) -> Option<&'s mut Map<String, Value>> {
    panes(state)
        .iter_mut()
        .filter_map(Value::as_object_mut)
        .find(|p| p.get("pane_id").and_then(Value::as_str) == Some(id))
}

fn no_pane(id: &str) -> Answer {
    Answer::Refused("pane_not_found", format!("no pane {id}"), 1)
}

/// A counter of the state, moved on by one: the new value.
fn bump(state: &mut Map<String, Value>, key: &str, from: u64) -> u64 {
    let next = state.get(key).and_then(Value::as_u64).unwrap_or(from) + 1;
    state.insert(key.into(), json!(next));
    next
}

fn answer(argv: &[String], words: &str, state: &mut Map<String, Value>) -> Answer {
    let arg = |n: usize| argv.get(n).map_or("", String::as_str);
    match words {
        "tab list" => {
            let all = panes(state).clone();
            let tabs: Vec<Value> = state["tabs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|tab| {
                    let mut view = tab.clone();
                    let inside = all
                        .iter()
                        .filter(|p| p.get("tab_id") == tab.get("tab_id"))
                        .count();
                    if let Some(view) = view.as_object_mut() {
                        view.insert("pane_count".into(), json!(inside));
                    }
                    view
                })
                .collect();
            Answer::Result(json!({"type": "tab_list", "tabs": tabs}))
        }
        "pane list" => Answer::Result(json!({"type": "pane_list", "panes": state["panes"]})),
        "pane layout" => {
            let id = flag(argv, "--pane").unwrap_or("");
            let Some(tab) = pane_mut(state, id).and_then(|p| p.get("tab_id").cloned()) else {
                return no_pane(id);
            };
            let inside: Vec<_> = panes(state)
                .iter()
                .filter(|pane| pane.get("tab_id") == Some(&tab))
                .map(|pane| {
                    json!({
                        "pane_id": pane["pane_id"],
                        "rect": pane.get("rect").cloned().unwrap_or_else(|| json!({
                            "x":0,"y":0,"width":200,"height":80
                        }))
                    })
                })
                .collect();
            Answer::Result(json!({"type":"pane_layout", "layout":{"panes":inside}}))
        }
        "agent list" => {
            let agents: Vec<Value> = panes(state)
                .iter()
                .filter(|p| p.get("agent").is_some_and(truthy))
                .cloned()
                .collect();
            Answer::Result(json!({"type": "agent_list", "agents": agents}))
        }
        "pane read" => {
            let Some(pane) = pane_mut(state, arg(2)) else {
                return no_pane(arg(2));
            };
            let mut shown = pane
                .get("screen")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let input = pane.get("input").and_then(Value::as_str).unwrap_or("");
            if !input.is_empty() {
                if !shown.is_empty() {
                    shown.push('\n');
                }
                shown.push_str("> ");
                shown.push_str(input);
            }
            Answer::Text(shown)
        }
        "tab create" => {
            let n = bump(state, "next", 100);
            let tab = json!({"tab_id": format!("w1:t{n}"), "label": flag(argv, "--label").unwrap_or(""), "workspace_id": "w1"});
            let pane = json!({"pane_id": format!("w1:p{n}"), "tab_id": format!("w1:t{n}"), "cwd": flag(argv, "--cwd").unwrap_or("/"), "agent_status": "unknown", "workspace_id": "w1"});
            if let Some(Value::Array(tabs)) = state.get_mut("tabs") {
                tabs.push(tab.clone());
            }
            panes(state).push(pane.clone());
            Answer::Result(json!({"type": "tab_created", "tab": tab, "root_pane": pane}))
        }
        "pane split" => {
            let target = if argv.len() > 2 && !arg(2).starts_with('-') {
                arg(2)
            } else {
                flag(argv, "--pane").unwrap_or("")
            };
            let Some(tab) =
                pane_mut(state, target).map(|p| p.get("tab_id").cloned().unwrap_or(Value::Null))
            else {
                return no_pane(target);
            };
            let n = bump(state, "next", 100);
            let pane = json!({"pane_id": format!("w1:p{n}"), "tab_id": tab, "cwd": flag(argv, "--cwd").unwrap_or("/"), "agent_status": "unknown", "workspace_id": "w1"});
            panes(state).push(pane.clone());
            Answer::Result(json!({"type": "pane_created", "pane": pane}))
        }
        "agent start" => {
            let busy_seen = state.get("busy_seen").and_then(Value::as_u64).unwrap_or(0);
            if count("FAKE_HERDR_BUSY") > busy_seen {
                // the new pane's shell is not ready yet
                bump(state, "busy_seen", 0);
                if let Err(e) = save(&var("FAKE_HERDR_STATE"), state) {
                    return Answer::Refused("fake_state", e, 2);
                }
                return Answer::Refused("agent_pane_busy", "pane is busy".into(), 1);
            }
            let rest: Vec<String> = match argv.iter().position(|a| a == "--") {
                Some(at) => argv[at + 1..].to_vec(),
                None => Vec::new(),
            };
            let session = flag(&rest, "--session-id").or_else(|| flag(&rest, "--resume"));
            let target = flag(argv, "--pane").unwrap_or("");
            let Some(pane) = pane_mut(state, target) else {
                return no_pane(target);
            };
            let screen = var("FAKE_HERDR_START_SCREEN");
            let chooser = !screen.is_empty() && flag(argv, "--kind") == Some("codex");
            pane.insert(
                "agent".into(),
                json!(if chooser { "codex" } else { "claude" }),
            );
            pane.insert("name".into(), json!(arg(2)));
            pane.insert("agent_status".into(), json!("idle"));
            pane.insert("agent_args".into(), json!(rest));
            pane.insert(
                "agent_session".into(),
                json!({"agent": "claude", "kind": "id", "value": session}),
            );
            if chooser {
                pane.insert("agent_status".into(), json!("unknown"));
                pane.insert("launch_pending".into(), json!(true));
                pane.insert(
                    "startup_selection".into(),
                    json!(count("FAKE_HERDR_START_SELECTED").max(1)),
                );
                pane.insert(
                    "screen".into(),
                    json!(std::fs::read_to_string(screen).unwrap()),
                );
                pane.insert(
                    "agent_session".into(),
                    json!({"agent":"codex", "kind":"id", "value":"startup-probe-session"}),
                );
                let pid_file = var("FAKE_HERDR_START_PID");
                if !pid_file.is_empty() {
                    std::fs::write(pid_file, std::process::id().to_string()).unwrap();
                }
            }
            Answer::Result(json!({"type": "agent_started"}))
        }
        "agent prompt" => {
            let (name, text) = (arg(2), arg(3));
            let prompts = state.entry("prompts").or_insert_with(|| json!([]));
            if !prompts.is_array() {
                *prompts = json!([]);
            }
            if let Some(prompts) = prompts.as_array_mut() {
                prompts.push(json!([name, text]));
            }
            let stuck_seen = state.get("stuck_seen").and_then(Value::as_u64).unwrap_or(0);
            let stuck = count("FAKE_HERDR_STUCK") > stuck_seen;
            if stuck {
                bump(state, "stuck_seen", 0);
            }
            let agent = panes(state)
                .iter_mut()
                .filter_map(Value::as_object_mut)
                .find(|p| {
                    p.get("agent").is_some_and(truthy)
                        && p.get("name").and_then(Value::as_str) == Some(name)
                });
            if let Some(pane) = agent {
                if stuck {
                    pane.insert("input".into(), json!(text));
                } else {
                    pane.insert("agent_status".into(), json!("working"));
                }
            }
            Answer::Result(json!({"type": "prompt_sent"}))
        }
        "pane send-keys" => {
            let keys: Vec<&str> = argv.iter().skip(3).map(String::as_str).collect();
            let Some(pane) = pane_mut(state, arg(2)) else {
                return no_pane(arg(2));
            };
            if pane.get("launch_pending").and_then(Value::as_bool) == Some(true) {
                let mut selection = pane["startup_selection"].as_u64().unwrap();
                for key in &keys {
                    match *key {
                        "down" => {
                            if count("FAKE_HERDR_DROP_DOWN") > 0
                                && pane.get("dropped_down").and_then(Value::as_bool) != Some(true)
                            {
                                pane.insert("dropped_down".into(), json!(true));
                            } else {
                                selection = selection % 3 + 1;
                            }
                        }
                        "up" => selection = (selection + 1) % 3 + 1,
                        "enter" => {
                            pane.insert("confirmed_choice".into(), json!(selection));
                            if selection == 3 {
                                pane.insert("screen".into(), json!("› Ask Codex to do anything"));
                                pane.insert("agent_status".into(), json!("idle"));
                                pane.insert("interactive_ready".into(), json!(true));
                                pane.insert("launch_pending".into(), json!(false));
                            }
                        }
                        _ => {
                            return Answer::Refused(
                                "unsupported",
                                format!("unknown startup key {key}"),
                                2,
                            );
                        }
                    }
                }
                pane.insert("startup_selection".into(), json!(selection));
                if pane.get("launch_pending").and_then(Value::as_bool) == Some(true) {
                    let screen = pane["screen"].as_str().unwrap_or("").replace('›', " ");
                    pane.insert(
                        "screen".into(),
                        json!(
                            screen.replace(&format!("  {selection}."), &format!("› {selection}."))
                        ),
                    );
                }
            }
            let typed = pane
                .get("input")
                .and_then(Value::as_str)
                .is_some_and(|i| !i.is_empty());
            if keys == ["enter"] && typed {
                pane.insert("input".into(), json!(""));
                pane.insert("agent_status".into(), json!("working"));
            }
            Answer::Result(json!({"type": "keys_sent"}))
        }
        "pane close" => {
            let id = arg(2).to_string();
            let all = panes(state);
            let before = all.len();
            all.retain(|p| p.get("pane_id").and_then(Value::as_str) != Some(id.as_str()));
            if all.len() == before {
                return no_pane(&id);
            }
            Answer::Result(json!({"type": "pane_closed"}))
        }
        "pane process-info" => {
            let target = flag(argv, "--pane").unwrap_or("").to_string();
            let Some(pane) = pane_mut(state, &target) else {
                return no_pane(&target);
            };
            let shell = pane.get("shell_pid").cloned().unwrap_or(json!(999_999));
            let foreground = pane.get("foreground").cloned().unwrap_or(json!([]));
            Answer::Result(json!({"type": "pane_process_info", "process_info": {
                "pane_id": target, "shell_pid": shell, "foreground_processes": foreground,
            }}))
        }
        _ => Answer::Refused("unsupported", format!("fake herdr has no {words}"), 2),
    }
}

/// Whether a JSON value is true as Python reads it.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
