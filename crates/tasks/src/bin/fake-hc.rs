//! A stand-in for `hc` (herdr-core) and `herdr-app`, for the tests of the launcher, the idle sweep
//! and the pane watch (`crates/tasks/tests/launch/`): never the live terminals. `HC_BIN`
//! names it; `HERDR_APP_BIN` names it with a first word `app`.
//!
//! State is a JSON file named by `$FAKE_HC_STATE`: `terminals` (each as `hc get` shows it, its
//! `status` the bare word), `layout` (the Herdr app's spaces and tabs, as `herdr-app ls` and the
//! core's `herdr-app` document show them), `screen` (what `herdr-app status` says the window shows)
//! and counters. Every call is appended, as one JSON list of
//! its arguments per line (`app` first for the window's), to `$FAKE_HC_LOG`. It speaks the slice of
//! hc the board uses, with hc's JSON shapes: `ls`, `get`, `run`, `read`, `keys`, `send`, `kill`,
//! `rm`, `connect` (a `doc.get` of the layout), and the window's `ls`, `status`, `do tab.rename`
//! / `space.rename` and `do agent.focus` / `pane.focus` (the terminal shown kept as `focused`). A
//! terminal `run` labels into a space and tab the layout lacks joins it, as the window adopts it.
//!
//! An agent `run` starts (its program, past `env` and its settings, is `claude` or `codex`) waits
//! at its prompt, `idle`, as hc runs one with no template: no `first_message`. `send` gives it its
//! next message as hc does: refused while it is `blocked` (unless `--force`), else it goes
//! `working` and the answer says `started` (null for a program that reports no status), `status`
//! (its word when the text went in) and `turn`. `FAKE_HC_SILENT=N` leaves the first N agents saying
//! nothing, `FAKE_HC_BUSY=N` `working` before any message, `FAKE_HC_BLOCKED=N` `blocked` on Claude's
//! folder-trust dialog (its status `msg` the question; with `FAKE_HC_BLOCKED_SCREEN=FILE`, on a
//! dialog showing FILE that hc names only `dialog open`), and `FAKE_HC_DEAF=N` never starting a turn
//! on what is sent (`started` false); `FAKE_HC_EXIT=N` ends the first N at once.
//! `FAKE_HC_START_SCREEN=FILE` makes a codex start show FILE as its hooks chooser, silent until a
//! `keys` press confirms a choice (`FAKE_HC_START_SELECTED` picks the highlighted one,
//! `FAKE_HC_DROP_DOWN=1` loses the first down).
//! `FAKE_HC_FAIL=WORDS[,WORDS]` makes those calls exit 1 with hc's error on stderr: a command's
//! first word (`get`, `run`), or `app` and its first word (`app ls`, `app do`).
//! A terminal whose state holds `ending: N` is still ending after its `kill`, as a program that
//! takes a while to exit is: live, so `rm` refuses it, until the Nth `get` after the kill.

use serde_json::{Map, Value, json};
use std::io::{BufRead, Write};

/// What Claude's folder-trust dialog asks.
const TRUST_QUESTION: &str = "Quick safety check: Is this a project you created or one you trust?";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}

/// An hc error, on stderr as hc prints one, with its exit code.
fn refuse(kind: &str, message: &str, exit: i32) -> i32 {
    eprintln!(
        "{}",
        json!({"error": {"kind": kind, "message": message, "hint": ""}})
    );
    exit
}

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn count(name: &str) -> u64 {
    var(name).trim().parse().unwrap_or(0)
}

fn run(argv: &[String]) -> i32 {
    let log = var("FAKE_HC_LOG");
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
    let words = match argv.first().map(String::as_str) {
        Some("app") => argv.iter().take(2).cloned().collect::<Vec<_>>().join(" "),
        Some(first) => first.to_string(),
        None => String::new(),
    };
    if var("FAKE_HC_FAIL").split(',').any(|w| w.trim() == words) {
        if words.starts_with("app") {
            println!("{}", json!({"error": "the window did not answer"}));
            return 1;
        }
        return refuse("fake_failure", &format!("{words} failed"), 1);
    }
    let path = var("FAKE_HC_STATE");
    let mut state = match load(&path) {
        Ok(state) => state,
        Err(e) => return refuse("fake_state", &e, 2),
    };
    let answer = match argv.first().map(String::as_str) {
        Some("app") => app(&argv[1..], &mut state),
        Some("connect") => connect(&state),
        _ => answer(argv, &mut state),
    };
    match answer {
        Answer::Saved(text) => {
            if let Err(e) = save(&path, &state) {
                return refuse("fake_state", &e, 2);
            }
            println!("{text}");
            0
        }
        Answer::Kept(text) => {
            println!("{text}");
            0
        }
        Answer::Refused(kind, message, exit) => refuse(kind, &message, exit),
        Answer::Window(message) => {
            println!("{}", json!({"error": message}));
            1
        }
    }
}

/// The state `path` holds, with its `terminals` list and `layout`, empty when it has none.
fn load(path: &str) -> Result<Map<String, Value>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut state = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(state)) => state,
        Ok(_) => return Err(format!("{path} holds no object")),
        Err(e) => return Err(format!("{path}: {e}")),
    };
    if !state.get("terminals").is_some_and(Value::is_array) {
        state.insert("terminals".into(), json!([]));
    }
    if !state.get("layout").is_some_and(Value::is_object) {
        state.insert("layout".into(), json!({"spaces": []}));
    }
    Ok(state)
}

/// Write the state whole, by a rename, so a call running beside this one never reads half of it.
fn save(path: &str, state: &Map<String, Value>) -> Result<(), String> {
    let tmp = format!("{path}.{}.tmp", std::process::id());
    std::fs::write(&tmp, Value::Object(state.clone()).to_string())
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("cannot write {path}: {e}"))
}

enum Answer {
    /// Printed, the state saved.
    Saved(String),
    /// Printed, the state left as it was.
    Kept(String),
    Refused(&'static str, String, i32),
    /// The window's error: `{"error": ...}` on stdout, exit 1.
    Window(String),
}

fn terminals(state: &mut Map<String, Value>) -> &mut Vec<Value> {
    match state.get_mut("terminals") {
        Some(Value::Array(all)) => all,
        _ => unreachable!("load made terminals a list"),
    }
}

/// The terminal `id` names, by address or name, a live one first.
fn find<'s>(state: &'s mut Map<String, Value>, id: &str) -> Option<&'s mut Map<String, Value>> {
    let all = terminals(state);
    let at = all
        .iter()
        .position(|t| t["address"] == id || (t["name"] == id && t["state"] == "live"))
        .or_else(|| all.iter().position(|t| t["name"] == id))?;
    all[at].as_object_mut()
}

fn not_found(id: &str) -> Answer {
    Answer::Refused("not_found", format!("no terminal \"{id}\""), 3)
}

/// A counter of the state, moved on by one: the new value.
fn bump(state: &mut Map<String, Value>, key: &str, from: u64) -> u64 {
    let next = state.get(key).and_then(Value::as_u64).unwrap_or(from) + 1;
    state.insert(key.into(), json!(next));
    next
}

/// Whether the `name` knob still has a turn: its first N uses, counted in the state.
fn knob(state: &mut Map<String, Value>, name: &str) -> bool {
    let seen = format!("{name}_seen");
    let used = state.get(&seen).and_then(Value::as_u64).unwrap_or(0);
    if count(name) > used {
        bump(state, &seen, 0);
        return true;
    }
    false
}

/// The value after `name` in `args`, if it is there.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).map(String::as_str)
}

/// Every value given after `name`.
fn flags<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .collect()
}

/// The program an argv runs, past `env` and its `KEY=VALUE` settings.
fn program(argv: &[String]) -> &[String] {
    let mut at = 0;
    if argv.first().is_some_and(|a| a == "env") {
        at = 1;
        while argv.get(at).is_some_and(|a| a.contains('=')) {
            at += 1;
        }
    }
    &argv[at.min(argv.len())..]
}

fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// The terminal as `hc get` shows it: its status as `{app, state}`, its foreground filled in.
fn shown(t: &Map<String, Value>) -> Value {
    let mut view = t.clone();
    let agent = t.get("agent").cloned().unwrap_or(Value::Null);
    view.insert(
        "status".into(),
        match (t.get("status"), t.get("status_msg")) {
            (Some(Value::String(word)), Some(msg)) => {
                json!({"app": agent, "state": word, "msg": msg})
            }
            (Some(Value::String(word)), None) => json!({"app": agent, "state": word}),
            _ => Value::Null,
        },
    );
    view.remove("status_msg");
    if t["state"] == "live" && !t.contains_key("foreground") {
        let argv: Vec<String> = t["argv"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
        let run = program(&argv);
        view.insert(
            "foreground".into(),
            json!({"argv": run, "pid": t["pid"], "cwd": t["cwd"],
                   "name": run.first().map_or("", |p| base(p))}),
        );
    }
    view.remove("screen");
    view.remove("chooser");
    Value::Object(view)
}

/// The terminal as `hc ls` lists it: what is unknown is left out.
fn listed(t: &Map<String, Value>) -> Value {
    let mut view = Map::new();
    for key in ["address", "agent", "cwd", "name", "state", "status", "turn"] {
        if let Some(value) = t.get(key).filter(|v| !v.is_null()) {
            view.insert(key.into(), value.clone());
        }
    }
    Value::Object(view)
}

fn answer(argv: &[String], state: &mut Map<String, Value>) -> Answer {
    let arg = |n: usize| argv.get(n).map_or("", String::as_str);
    match arg(0) {
        // An owned fixture may put this binary on PATH as codex. The log is its native queue
        // receipt; accepting the message deliberately changes no terminal's status or sent text.
        "queue" => Answer::Kept(String::new()),
        "ls" => {
            let all = argv.iter().any(|a| a == "--all");
            let wanted = flags(argv, "--label");
            let found: Vec<Value> = terminals(state)
                .iter()
                .filter_map(Value::as_object)
                .filter(|t| all || t["state"] == "live")
                .filter(|t| {
                    wanted.iter().all(|kv| {
                        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                        t["labels"][k] == v
                    })
                })
                .map(listed)
                .collect();
            Answer::Kept(json!({"terminals": found}).to_string())
        }
        "get" => match find(state, arg(1)) {
            Some(t) if t.contains_key("killed") => {
                let left = t.get("ending").and_then(Value::as_u64).unwrap_or(0);
                t.insert("ending".into(), json!(left.saturating_sub(1)));
                if left <= 1 {
                    end(t);
                }
                Answer::Saved(shown(t).to_string())
            }
            Some(t) => Answer::Kept(shown(t).to_string()),
            None => not_found(arg(1)),
        },
        "read" => match find(state, arg(1)) {
            Some(t) => Answer::Kept(t.get("screen").and_then(Value::as_str).unwrap_or("").into()),
            None => not_found(arg(1)),
        },
        "run" => start(argv, state),
        "keys" => {
            let keys: Vec<String> = argv[2..]
                .iter()
                .filter(|k| !k.starts_with("--"))
                .cloned()
                .collect();
            let Some(t) = find(state, arg(1)) else {
                return not_found(arg(1));
            };
            let pressed = t.entry("keys").or_insert_with(|| json!([]));
            if let Some(pressed) = pressed.as_array_mut() {
                pressed.push(json!(keys));
            }
            if t.get("chooser").is_some_and(|c| c == true)
                && let Err(why) = press(t, &keys)
            {
                return Answer::Refused("unsupported", why, 2);
            }
            let said = json!({"status": t.get("status"), "turn": t["turn"]});
            Answer::Saved(said.to_string())
        }
        "send" => {
            let text = arg(2).to_string();
            let Some(t) = find(state, arg(1)) else {
                return not_found(arg(1));
            };
            if t["status"] == "blocked" && !argv.iter().any(|a| a == "--force") {
                return Answer::Refused("blocked", "it waits on a person".into(), 9);
            }
            let sent = t.entry("sent").or_insert_with(|| json!([]));
            if let Some(sent) = sent.as_array_mut() {
                sent.push(json!(text));
            }
            let before = t.get("status").cloned().unwrap_or(Value::Null);
            // hc's `started`: whether it went working; null when it says nothing of its status.
            let started = if before.is_null() {
                Value::Null
            } else if t.get("deaf").is_some_and(|d| d == true) {
                json!(false)
            } else {
                t.insert("status".into(), json!("working"));
                json!(true)
            };
            let said = json!({"started": started, "status": before, "turn": t["turn"]});
            Answer::Saved(said.to_string())
        }
        "kill" => {
            let Some(t) = find(state, arg(1)) else {
                return not_found(arg(1));
            };
            let ending = t.get("ending").and_then(Value::as_u64).unwrap_or(0);
            if t["state"] == "live" && ending > 0 {
                t.insert("killed".into(), json!(true));
            } else if t["state"] == "live" {
                end(t);
            }
            let said = shown(t);
            Answer::Saved(said.to_string())
        }
        "rm" => {
            let Some(t) = find(state, arg(1)) else {
                return not_found(arg(1));
            };
            if t["state"] == "live" {
                let address = t["address"].as_str().unwrap_or("").to_string();
                return Answer::Refused("live", format!("{address} is still running"), 1);
            }
            let address = t["address"].clone();
            terminals(state).retain(|t| t["address"] != address);
            Answer::Saved(json!({"removed": address}).to_string())
        }
        other => Answer::Refused("unsupported", format!("fake hc has no {other}"), 2),
    }
}

/// A killed terminal's program ends.
fn end(t: &mut Map<String, Value>) {
    t.insert("state".into(), json!("finished"));
    t.insert("reason".into(), json!("killed"));
    t.insert("status".into(), Value::Null);
    t.insert("exit_code".into(), json!(129));
    t.remove("killed");
}

/// `hc run --bg --json [--name N] [--cwd C] [--label K=V]... [flags] -- ARGV`.
fn start(argv: &[String], state: &mut Map<String, Value>) -> Answer {
    let (options, command) = match argv.iter().position(|a| a == "--") {
        Some(at) => (&argv[..at], argv[at + 1..].to_vec()),
        None => (argv, Vec::new()),
    };
    let name = flag(options, "--name").map(str::to_string);
    if let Some(name) = &name
        && terminals(state)
            .iter()
            .any(|t| t["name"] == name.as_str() && t["state"] == "live")
    {
        return Answer::Refused("name_taken", format!("{name} is taken"), 1);
    }
    let mut labels = Map::new();
    for kv in flags(options, "--label") {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        labels.insert(k.into(), json!(v));
    }
    let n = bump(state, "next", 100);
    let pid = bump(state, "next_pid", 7000);
    let run = program(&command);
    let agent = match run.first().map(|p| base(p)) {
        Some("claude") => "claude-code".to_string(),
        Some(other) => other.to_string(),
        None => "zsh".into(),
    };
    let is_agent = agent == "claude-code" || agent == "codex";
    // As hc: the terminal it is started from (`HC_TERMINAL_ID`) is its parent, unless named or none.
    let parent = if options.iter().any(|a| a == "--no-parent") {
        None
    } else {
        flag(options, "--parent")
            .map(str::to_string)
            .or_else(|| Some(var("HC_TERMINAL_ID")).filter(|t| !t.is_empty()))
    };
    let mut t = json!({
        "address": format!("t{n}"), "argv": command, "cwd": flag(options, "--cwd").unwrap_or("/"),
        "name": name, "labels": labels, "pid": pid, "state": "live", "status": Value::Null,
        "turn": 0, "agent": agent, "parent": parent, "first_message": Value::Null,
        "keep": options.iter().any(|a| a == "--keep"),
        "outlive_parent": options.iter().any(|a| a == "--outlive-parent"),
        "notify": !options.iter().any(|a| a == "--no-notify"),
    });
    let screen = var("FAKE_HC_START_SCREEN");
    let dialog = var("FAKE_HC_BLOCKED_SCREEN");
    if is_agent {
        if knob(state, "FAKE_HC_EXIT") {
            t["state"] = json!("finished");
            t["exit_code"] = json!(1);
            t["reason"] = json!("exited");
        } else if !screen.is_empty() && agent == "codex" {
            t["chooser"] = json!(true);
            t["selection"] = json!(count("FAKE_HC_START_SELECTED").max(1));
            t["screen"] = json!(std::fs::read_to_string(&screen).unwrap_or_default());
        } else if knob(state, "FAKE_HC_SILENT") {
        } else if knob(state, "FAKE_HC_BUSY") {
            t["status"] = json!("working");
        } else if !dialog.is_empty() && knob(state, "FAKE_HC_BLOCKED") {
            t["status"] = json!("blocked");
            t["status_msg"] = json!("dialog open");
            t["screen"] = json!(std::fs::read_to_string(&dialog).unwrap_or_default());
        } else if knob(state, "FAKE_HC_BLOCKED") {
            t["status"] = json!("blocked");
            t["status_msg"] = json!(TRUST_QUESTION);
            t["screen"] = json!(format!(
                "Accessing workspace:\n\n{}\n\n{TRUST_QUESTION}\n\n❯ 1. Yes, I trust this folder\n  \
                 2. No, exit\n\nEnter to confirm · Esc to cancel",
                flag(options, "--cwd").unwrap_or("/")
            ));
        } else {
            t["status"] = json!("idle");
            if knob(state, "FAKE_HC_DEAF") {
                t["deaf"] = json!(true);
            }
        }
    }
    if parent.is_none() {
        adopt(state, &labels); // the window shows a child under its parent, never in a tab
    }
    let said = shown(t.as_object().expect("an object"));
    terminals(state).push(t);
    Answer::Saved(said.to_string())
}

/// The window adopts a terminal labelled into a space or tab it does not have: an unnamed one.
fn adopt(state: &mut Map<String, Value>, labels: &Map<String, Value>) {
    let (Some(space), Some(tab)) = (
        labels.get("herdr.space").and_then(Value::as_str),
        labels.get("herdr.tab").and_then(Value::as_str),
    ) else {
        return;
    };
    let spaces = state["layout"]["spaces"].as_array_mut().expect("a list");
    if !spaces.iter().any(|s| s["id"] == space) {
        spaces.push(json!({"id": space, "name": "", "tabs": []}));
    }
    let at = spaces.iter().position(|s| s["id"] == space).expect("there");
    let tabs = spaces[at]["tabs"].as_array_mut().expect("a list");
    if !tabs.iter().any(|t| t["id"] == tab) {
        tabs.push(json!({"id": tab, "name": ""}));
    }
}

/// `keys` pressed on a codex start's hooks chooser: down and up move the selection, enter confirms
/// it, and confirming "Continue without trusting" lets the agent take its first message. An unknown
/// key is an error.
fn press(t: &mut Map<String, Value>, keys: &[String]) -> Result<(), String> {
    let mut selection = t["selection"].as_u64().unwrap_or(1);
    for key in keys {
        match key.as_str() {
            "down" => {
                if count("FAKE_HC_DROP_DOWN") > 0 && t.get("dropped_down").is_none() {
                    t.insert("dropped_down".into(), json!(true));
                } else {
                    selection = selection % 3 + 1;
                }
            }
            "up" => selection = (selection + 1) % 3 + 1,
            "enter" => {
                t.insert("confirmed_choice".into(), json!(selection));
                t.insert("chooser".into(), json!(false));
                if selection == 3 {
                    t.insert("screen".into(), json!("› Ask Codex to do anything"));
                    t.insert("status".into(), json!("idle"));
                }
                break;
            }
            key => return Err(format!("unknown startup key {key}")),
        }
    }
    t.insert("selection".into(), json!(selection));
    if t.get("chooser").is_some_and(|c| c == true) {
        let screen = t["screen"].as_str().unwrap_or("").replace('›', " ");
        t.insert(
            "screen".into(),
            json!(screen.replace(&format!("  {selection}."), &format!("› {selection}."))),
        );
    }
    Ok(())
}

/// `hc connect`: JSON-RPC lines on stdin, each answered on its own line; `doc.get` of the window's
/// document answers the layout.
fn connect(state: &Map<String, Value>) -> Answer {
    let mut out = Vec::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(call) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = call["id"].clone();
        let answer = match call["method"].as_str() {
            Some("hello") => json!({"id": id, "jsonrpc": "2.0", "result": {"generation": 1}}),
            Some("doc.get") if call["params"]["name"] == "herdr-app" => {
                json!({"id": id, "jsonrpc": "2.0", "result": {"value": state["layout"], "version": 1}})
            }
            Some("doc.get") => {
                json!({"id": id, "jsonrpc": "2.0", "result": {"value": Value::Null, "version": 0}})
            }
            _ => {
                json!({"id": id, "jsonrpc": "2.0", "error": {"code": -32601, "message": "no such method"}})
            }
        };
        out.push(answer.to_string());
    }
    Answer::Kept(out.join("\n"))
}

/// `herdr-app ls`, `herdr-app status` (the state's `screen`, else a window showing no tab),
/// `herdr-app do space.rename|tab.rename --id ID --name NAME`, and `herdr-app do agent.focus|pane.focus
/// --terminal T`, which keeps the live terminal it showed as the state's `focused` (`agent.focus`
/// refuses one that runs no agent, as the window shows only an agent by it).
fn app(argv: &[String], state: &mut Map<String, Value>) -> Answer {
    match argv.first().map(String::as_str) {
        Some("ls") => Answer::Kept(state["layout"].to_string()),
        Some("status") => Answer::Kept(
            state
                .get("screen")
                .cloned()
                .unwrap_or_else(|| {
                    json!({"tab": null, "agent": null, "overlays": [], "floats": {}, "errors": []})
                })
                .to_string(),
        ),
        Some("do") if matches!(argv.get(1).map(String::as_str), Some("agent.focus" | "pane.focus")) => {
            let command = argv[1].as_str();
            let Some(id) = flag(argv, "--terminal") else {
                return Answer::Window(format!("{command}: a terminal is needed"));
            };
            let Some(found) = find(state, id).filter(|t| t["state"] == "live") else {
                return Answer::Window(format!("no terminal \"{id}\""));
            };
            let agent = found["agent"].as_str().unwrap_or("");
            if command == "agent.focus" && !(agent.contains("claude") || agent.contains("codex")) {
                return Answer::Window(format!("terminal \"{id}\" runs no agent"));
            }
            let address = found["address"].clone();
            state.insert("focused".into(), address);
            Answer::Saved(json!({"ok": true}).to_string())
        }
        Some("do") => {
            let (id, name) = (flag(argv, "--id").unwrap_or(""), flag(argv, "--name"));
            let Some(name) = name else {
                return Answer::Window("a name is needed".into());
            };
            let spaces = state["layout"]["spaces"].as_array_mut().expect("a list");
            let found = match argv.get(1).map(String::as_str) {
                Some("space.rename") => spaces.iter_mut().find(|s| s["id"] == id),
                Some("tab.rename") => spaces
                    .iter_mut()
                    .flat_map(|s| s["tabs"].as_array_mut().into_iter().flatten())
                    .find(|t| t["id"] == id),
                other => return Answer::Window(format!("fake herdr-app cannot {other:?}")),
            };
            let Some(found) = found else {
                return Answer::Window(format!("no tab \"{id}\""));
            };
            found["name"] = json!(name);
            Answer::Saved(json!({"ok": true}).to_string())
        }
        other => Answer::Window(format!("fake herdr-app has no {other:?}")),
    }
}
