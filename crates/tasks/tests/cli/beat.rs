//! Bound-key folder contracts for tasks beat and the heartbeat hook's tasks-beat.
//! The hook checks this file before handing a full session's ordinary calls
//! to the binary; without it only board commands reach the beat path.
//! The beats cases pin binding behaviour and session-liveness duration.

use crate::{env, run};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use symphony_board::cli::Env;

/// A keys' folder of its own, not there yet.
fn keys(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beat-keys-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// One beat of `event`, as the hook hands it: quiet, exit 0.
fn beat(env: &Env, event: &Value) {
    let ran = run(env, &["beat"], &event.to_string());
    assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
}

fn ok(env: &Env, args: &[&str]) -> String {
    let ran = run(env, args, "");
    assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
    ran.out.trim().to_string()
}

/// The handle the board has bound to `agent` (an agent or session id), if any.
fn bound_to(env: &Env, agent: &str) -> Value {
    let call = json!({"method": "handle_for_agent", "args": {"agent": agent}});
    let answer: Value = serde_json::from_str(&run(env, &["api"], &call.to_string()).out).unwrap();
    answer["ok"].clone()
}

#[test]
fn binding_a_full_session_writes_its_key_and_the_key_goes_when_nobody_is_bound() {
    let folder = keys("bind");
    let mut env = env();
    env.vars
        .insert("TASKS_KEYS".into(), folder.to_string_lossy().into_owned());
    let h = ok(&env, &["register", "--kind", "worker"]);
    let command = format!("TASKS_SESSION={h} just tasks inbox");
    beat(
        &env,
        &json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
                "tool_input": {"command": command}, "session_id": "s-k", "cwd": "/somewhere"}),
    );
    assert!(folder.join("s-k").is_file(), "no key written for s-k");
    assert_eq!(bound_to(&env, "s-k"), json!(h));
    ok(&env, &["--as", &h, "leave"]);
    beat(
        &env,
        &json!({"hook_event_name": "PostToolUse", "tool_name": "Read", "session_id": "s-k"}),
    );
    assert!(!folder.join("s-k").exists(), "the key outlived its binding");
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_subagents_binding_writes_no_key() {
    // The hook never looks a subagent's key up: its calls always reach the binary.
    let folder = keys("agent");
    let mut env = env();
    env.vars
        .insert("TASKS_KEYS".into(), folder.to_string_lossy().into_owned());
    let h = ok(&env, &["register", "--kind", "worker"]);
    let command = format!("TASKS_SESSION={h} just tasks inbox");
    beat(
        &env,
        &json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
                "tool_input": {"command": command}, "agent_id": "agent-1", "session_id": "s-a"}),
    );
    assert_eq!(bound_to(&env, "agent-1"), json!(h));
    assert!(!folder.join("agent-1").exists() && !folder.join("s-a").exists());
    let _ = fs::remove_dir_all(&folder);
}

/// `env` as `just tasks` runs a command: under the mark of a recipe an agent runs unasked.
fn marked(env: &Env) -> Env {
    let mut marked = env.clone();
    marked
        .vars
        .insert(symphony_board::confine::MARKER.into(), "1".into());
    marked
}

#[test]
fn through_just_tasks_the_api_binds_no_agent_and_the_binary_by_itself_binds_any() {
    let env = env();
    let h = ok(&env, &["register", "--kind", "worker"]);
    let bind = |env: &Env| {
        let call = json!({"method": "bind_agent",
                          "args": {"agent": "dead-beef", "handle": h, "session": true}});
        let ran = run(env, &["api"], &call.to_string());
        assert_eq!((ran.code, ran.err.as_str()), (0, ""));
        serde_json::from_str::<Value>(&ran.out).unwrap()
    };
    assert_eq!(
        bind(&marked(&env)),
        json!({"refused": "no bind_agent through a recipe an agent runs unasked: the agent id a \
            handle is bound to is a conversation staffing may record for it, for a restore to \
            resume; a session's own heartbeat binds it (the hook runs tasks beat as the binary \
            itself), and the binary run by itself binds any"})
    );
    assert_eq!(bound_to(&env, "dead-beef"), Value::Null);
    assert_eq!(bind(&env), json!({"ok": true}));
    assert_eq!(bound_to(&env, "dead-beef"), json!(h));
}

#[test]
fn through_just_tasks_a_beat_binds_nothing_and_the_hooks_binds_its_session() {
    // An event `just tasks beat` reads is one an agent wrote, not its own tool call.
    let folder = keys("marked");
    let mut env = env();
    env.vars
        .insert("TASKS_KEYS".into(), folder.to_string_lossy().into_owned());
    let h = ok(&env, &["register", "--kind", "worker"]);
    let event = bash(&format!("just tasks inbox --as {h}"))
        .session("dead-beef")
        .0;
    beat(&marked(&env), &event);
    assert_eq!(bound_to(&env, "dead-beef"), Value::Null);
    assert!(!folder.join("dead-beef").exists());
    // The hook runs the binary itself, unmarked: the same call binds the session it came from.
    beat(&env, &event);
    assert_eq!(bound_to(&env, "dead-beef"), json!(h));
    assert!(folder.join("dead-beef").is_file());
    let _ = fs::remove_dir_all(&folder);
}

/// A hook event, as Claude Code sends it on stdin.
pub struct Event(pub Value);

/// A Bash call about to run (`PreToolUse`) in Claude Code session `s-1`, from the main agent.
pub fn bash(command: &str) -> Event {
    Event(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
                 "tool_input": {"command": command}, "session_id": "s-1", "cwd": "/somewhere"}))
}

/// Another tool's call that has returned, from `agent_id` `agent`.
pub fn tool(name: &str, agent: &str) -> Value {
    json!({"hook_event_name": "PostToolUse", "tool_name": name, "agent_id": agent})
}

impl Event {
    /// From subagent `id`.
    pub fn agent(mut self, id: &str) -> Event {
        self.0["agent_id"] = json!(id);
        self
    }

    /// Tool call `id`.
    pub fn call(mut self, id: &str) -> Event {
        self.0["tool_use_id"] = json!(id);
        self
    }

    /// In Claude Code session `key`.
    pub fn session(mut self, key: &str) -> Event {
        self.0["session_id"] = json!(key);
        self
    }

    /// The call has returned (`PostToolUse`).
    pub fn ended(mut self) -> Event {
        self.0["hook_event_name"] = json!("PostToolUse");
        self
    }

    /// With `key` set to `value` in the tool's input.
    pub fn with(mut self, key: &str, value: Value) -> Event {
        self.0["tool_input"][key] = value;
        self
    }
}

mod place;
mod usage;

/// Beat command health and session updates.
mod beats {
    use super::{bash, tool};
    use crate::common::NOW;
    use crate::{Case, Ran, repository, run, scratch};
    use serde_json::{Value, json};
    use std::cell::Cell;
    use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
    use symphony_board::{Beat, NewSession, Store};

    /// Three-second polls past the lost line.
    const GONE_POLLS: usize = ((LIVE_SECONDS + STALE_SECONDS) / 3.0) as usize + 3;

    /// The boot a [`Clocked`] case's hooks read the clock since boot in, and its reading at `NOW`.
    pub(super) const BOOT: (&str, f64) = ("boot-1", 1000.0);

    /// A case whose board clock a test moves (`TASKS_NOW`), from `NOW`, and whose hooks read the
    /// clock since boot (`TASKS_BOOT_CLOCK`) in [`BOOT`], from its reading.
    pub(super) struct Clocked {
        c: Case,
        now: Cell<f64>,
        since_boot: Cell<f64>,
    }

    impl Clocked {
        pub(super) fn new() -> Clocked {
            Clocked {
                c: Case::new(),
                now: Cell::new(NOW),
                since_boot: Cell::new(BOOT.1),
            }
        }

        /// `seconds` go by, on both clocks.
        pub(super) fn pass(&self, seconds: f64) {
            self.now.set(self.now.get() + seconds);
            self.since_boot.set(self.since_boot.get() + seconds);
        }

        /// The wall clock is set by `seconds` (as a time server or a person sets it): the board's
        /// clock moves, the clock since boot does not.
        pub(super) fn set_wall(&self, seconds: f64) {
            self.now.set(self.now.get() + seconds);
        }

        fn at(&self) -> Case {
            let mut env = self.c.env.clone();
            env.vars
                .insert("TASKS_NOW".into(), format!("{:?}", self.now.get()));
            let since_boot = format!("{} {:?}", BOOT.0, self.since_boot.get());
            env.vars.insert("TASKS_BOOT_CLOCK".into(), since_boot);
            Case { env }
        }

        fn run(&self, args: &[&str], input: &str) -> Ran {
            run(&self.at().env, args, input)
        }

        fn ok(&self, who: &str, args: &[&str]) -> String {
            self.at().ok(who, args)
        }

        pub(super) fn register(&self, kind: &str) -> String {
            self.at().register(kind)
        }

        pub(super) fn board(&self) -> Store {
            self.at().store()
        }

        pub(super) fn beat(&self, event: &Value) {
            let ran = self.run(&["beat"], &event.to_string());
            assert_eq!(
                (ran.code, ran.out.as_str(), ran.err.as_str()),
                (0, "", ""),
                "{event}"
            );
        }

        fn health(&self, handle: &str) -> &'static str {
            self.board().session(handle).unwrap().health.as_str()
        }

        /// A session whose monitor is this process, on this host: alive.
        fn monitored(&self) -> String {
            let new = NewSession {
                pid: Some(i64::from(std::process::id())),
                host: symphony_board::sys::hostname(),
                ..NewSession::new("orchestrator")
            };
            self.board().register(&new).unwrap().row.handle
        }

        /// A beat of a Bash board command naming `handle` (else a `Read`) in Claude Code session `key`.
        fn session_call(&self, handle: &str, key: &str, board_command: bool) {
            if board_command {
                let command = format!("TASKS_SESSION={handle} just tasks show sim/a");
                self.beat(&bash(&command).session(key).0);
            } else {
                self.beat(&json!({"hook_event_name": "PostToolUse", "tool_name": "Read", "session_id": key}));
            }
        }

        fn agent_beat(&self, agent: &str) -> Option<String> {
            self.board().agent_beat(agent, &Beat::default()).unwrap()
        }
    }

    pub(super) fn inbox(handle: &str) -> String {
        format!("TASKS_SESSION={handle} just tasks inbox")
    }

    #[test]
    fn a_board_command_binds_the_subagent_and_its_later_tools_beat() {
        let b = Clocked::new();
        let h = b.register("worker");
        b.beat(
            &bash(&format!("TASKS_SESSION={h} just tasks claim sim/a"))
                .agent("agent-1")
                .0,
        );
        for _ in 0..GONE_POLLS / 10 + 2 {
            b.pass(30.0); // 30 s apart, past the lost line
            b.beat(&tool("Read", "agent-1"));
        }
        assert_eq!(b.health(&h), "live");
    }

    #[test]
    fn the_as_flag_binds_too() {
        for handle_last in [false, true] {
            let b = Clocked::new();
            let h = b.register("worker");
            let command = if handle_last {
                format!("just tasks show sim/a --as {h}")
            } else {
                format!("just tasks --as {h} say sim/a hi")
            };
            b.beat(&bash(&command).agent("agent-1").0);
            b.pass(LIVE_SECONDS + 1.0);
            b.beat(&tool("Grep", "agent-1"));
            assert_eq!(b.health(&h), "live", "{command}");
        }
    }

    #[test]
    fn a_main_session_with_no_monitor_is_bound_by_its_session_id() {
        // A full session started in an hc terminal has no agent_id and no monitor: its session id is its key.
        let b = Clocked::new();
        let h = b.register("worker");
        b.beat(&bash(&inbox(&h)).0);
        b.pass(LIVE_SECONDS + 1.0);
        b.beat(
            &json!({"hook_event_name": "PostToolUse", "tool_name": "Read", "session_id": "s-1"}),
        );
        assert_eq!(b.health(&h), "live");
    }

    #[test]
    fn a_main_session_with_a_monitor_is_never_bound() {
        let b = Clocked::new();
        let h = b.monitored();
        b.beat(&bash(&inbox(&h)).0);
        assert!(!b.board().bound_agents().unwrap().contains_key(&h));
    }

    #[test]
    fn a_monitored_session_naming_a_childs_handle_never_keeps_the_child_alive() {
        let b = Clocked::new();
        let orch = b.monitored();
        let child = b.register("worker");
        // Its own handle: monitored, so nothing binds, and the key is barred.
        b.session_call(&orch, "s-orch", true);
        b.session_call(&child, "s-orch", true);
        b.pass(LIVE_SECONDS + 1.0);
        b.session_call(&child, "s-orch", false); // the orchestrator's later calls
        assert_eq!(b.health(&child), "stale");
    }

    #[test]
    fn the_bar_holds_when_the_child_is_named_first() {
        let b = Clocked::new();
        let orch = b.monitored();
        let child = b.register("worker");
        b.session_call(&child, "s-orch", true);
        b.session_call(&orch, "s-orch", true); // proves the session monitored: it lets go of the child
        b.pass(LIVE_SECONDS + 1.0);
        b.session_call(&child, "s-orch", false);
        assert_eq!(b.health(&child), "stale");
    }

    #[test]
    fn a_full_session_keeps_the_first_handle_it_names() {
        let b = Clocked::new();
        let (mine, theirs) = (b.register("worker"), b.register("worker"));
        b.session_call(&mine, "s-pane", true);
        b.session_call(&theirs, "s-pane", true); // registering or relaying for a child
        b.pass(LIVE_SECONDS + 1.0);
        b.session_call(&mine, "s-pane", false);
        assert_eq!((b.health(&mine), b.health(&theirs)), ("live", "stale"));
    }

    #[test]
    fn a_subagent_still_follows_the_last_handle_it_names() {
        let b = Clocked::new();
        let (first, last) = (b.register("worker"), b.register("worker"));
        b.beat(&bash(&inbox(&first)).agent("agent-1").0);
        b.beat(&bash(&inbox(&last)).agent("agent-1").0);
        b.pass(LIVE_SECONDS + 1.0);
        b.beat(&tool("Read", "agent-1"));
        assert_eq!((b.health(&first), b.health(&last)), ("stale", "live"));
    }

    #[test]
    fn an_event_with_neither_agent_nor_session_beats_nothing() {
        let b = Clocked::new();
        let h = b.register("worker");
        let mut keyless = bash(&inbox(&h)).0;
        keyless.as_object_mut().unwrap().remove("session_id");
        b.beat(&keyless); // names the handle, but has no key to bind it by
        b.pass(LIVE_SECONDS + 1.0);
        b.beat(&json!({"hook_event_name": "PostToolUse", "tool_name": "Read"}));
        assert_eq!(b.health(&h), "stale");
    }

    #[test]
    fn a_foreground_bash_call_is_busy_for_its_timeout() {
        let b = Clocked::new();
        let h = b.register("worker");
        b.beat(&bash(&inbox(&h)).agent("agent-1").0);
        let long = || {
            bash("just verify-full")
                .agent("agent-1")
                .with("timeout", json!(600000))
        };
        b.beat(&long().0);
        b.pass(590.0);
        assert_eq!(b.health(&h), "live");
        b.beat(&long().ended().0);
        b.pass(LIVE_SECONDS + 1.0);
        assert_eq!(b.health(&h), "stale");
    }

    #[test]
    fn a_bash_call_without_a_timeout_is_busy_for_the_default_two_minutes() {
        let b = Clocked::new();
        let h = b.register("worker");
        b.beat(&bash(&inbox(&h)).agent("agent-1").call("bind").0);
        b.beat(&bash("").agent("agent-1").call("bind").ended().0);
        b.beat(&bash("just check").agent("agent-1").call("check").0);
        b.pass(115.0);
        assert_eq!(b.health(&h), "live");
        b.pass(LIVE_SECONDS);
        assert_eq!(b.health(&h), "stale");
    }

    #[test]
    fn a_background_bash_call_is_not_busy() {
        let b = Clocked::new();
        let h = b.register("worker");
        b.beat(&bash(&inbox(&h)).agent("agent-1").call("bind").0);
        b.beat(&bash("").agent("agent-1").call("bind").ended().0);
        let background = bash("just verify-full").agent("agent-1");
        b.beat(&background.with("run_in_background", json!(true)).0);
        b.pass(LIVE_SECONDS + 1.0);
        assert_eq!(b.health(&h), "stale");
    }

    #[test]
    fn a_grep_for_a_dead_handle_does_not_rebind_the_agent() {
        let b = Clocked::new();
        let (me, dead) = (b.register("reviewer"), b.register("worker"));
        b.ok(&dead, &["create", "sim/a", "A"]);
        b.ok(&dead, &["claim", "sim/a"]);
        b.pass(2000.0); // the executor is long gone
        b.beat(&bash(&inbox(&me)).agent("agent-1").0);
        let grep = format!("grep 'TASKS_SESSION={dead}' artifacts/x.log");
        b.beat(&bash(&grep).agent("agent-1").0);
        b.beat(&tool("Read", "agent-1"));
        assert_eq!((b.health(&me), b.health(&dead)), ("live", "lost"));
    }

    #[test]
    fn a_flag_quoted_inside_a_message_does_not_bind() {
        let b = Clocked::new();
        let (me, other) = (b.register("worker"), b.register("worker"));
        let command = format!("just tasks say sim/a \"run it --as {other} next\" --as {me}");
        b.beat(&bash(&command).agent("agent-1").0);
        b.pass(LIVE_SECONDS + 1.0);
        assert_eq!(b.agent_beat("agent-1"), Some(me));
        assert_eq!(b.health(&other), "stale");
    }

    #[test]
    fn a_non_board_command_run_as_another_session_does_not_bind() {
        let b = Clocked::new();
        let (me, orch) = (b.register("worker"), b.register("orchestrator"));
        b.beat(&bash(&inbox(&me)).agent("agent-1").0);
        let land = format!("TASKS_SESSION={orch} just land tools/x");
        b.beat(&bash(&land).agent("agent-1").0);
        b.pass(LIVE_SECONDS + 1.0);
        assert_eq!(b.agent_beat("agent-1"), Some(me));
        assert_eq!(b.health(&orch), "stale");
    }

    #[test]
    fn a_session_with_a_live_monitor_is_never_bound() {
        let b = Clocked::new();
        let orch = b.monitored();
        b.beat(&bash(&inbox(&orch)).agent("agent-1").0);
        assert_eq!(b.agent_beat("agent-1"), None);
    }

    #[test]
    fn parallel_bash_calls_keep_the_latest_busy_mark() {
        let b = Clocked::new();
        let h = b.register("worker");
        b.beat(&bash(&inbox(&h)).agent("agent-1").0);
        let long = || {
            bash("just verify-full")
                .agent("agent-1")
                .call("long")
                .with("timeout", json!(600000))
        };
        b.beat(&long().0);
        let short = || bash("ls").agent("agent-1").call("short");
        b.beat(&short().with("timeout", json!(10000)).0);
        b.beat(&short().ended().0);
        b.pass(500.0);
        assert_eq!(b.health(&h), "live"); // the short call ending left the long one's mark
        b.beat(&long().ended().0);
        b.pass(LIVE_SECONDS + 1.0);
        assert_eq!(b.health(&h), "stale");
    }

    #[test]
    fn the_beat_command_reads_the_event_from_stdin_and_prints_nothing() {
        let b = Clocked::new();
        let h = b.register("worker");
        let event = bash(&inbox(&h)).agent("agent-1").0.to_string();
        let ran = b.run(&["beat"], &event);
        assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
        b.pass(LIVE_SECONDS + 1.0);
        assert_eq!(b.agent_beat("agent-1"), Some(h));
    }

    #[test]
    fn the_beat_never_fails_the_tool_call() {
        let mut b = Clocked::new();
        let ghost = bash("TASKS_SESSION=ghost-owl just tasks inbox")
            .agent("a")
            .0
            .to_string();
        let inputs = [
            "",
            "not json",
            "[]",
            "{}",
            &json!({"agent_id": 5}).to_string(),
            &ghost,
        ];
        for input in inputs {
            let ran = b.run(&["beat"], input);
            assert_eq!(
                (ran.code, ran.out.as_str(), ran.err.as_str()),
                (0, "", ""),
                "{input}"
            );
        }
        b.c.env
            .vars
            .insert("TASKS_SERVER".into(), crate::UNREACHED_SERVER.into());
        let ran = b.run(&["beat"], &bash("ls").agent("a").0.to_string());
        assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
    }

    #[test]
    fn a_beat_never_starts_the_boards_server() {
        let b = Clocked::new();
        let repo = repository(&scratch("beat-no-server").join("repo"));
        let mut env = b.c.env.clone();
        env.vars.remove("TASKS_SERVER");
        env.vars.remove("TASKS_DATABASE");
        let mut event = bash("TASKS_SESSION=amber-heron just tasks inbox")
            .agent("a")
            .0;
        event["cwd"] = json!(repo.to_str().unwrap());
        let ran = run(&env, &["beat"], &event.to_string());
        assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
        assert!(!repo.join(".git").join("symphony/tasks").exists());
    }
}
