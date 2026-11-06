//! Launch commands use a private board and throwaway repository.

use crate::{Ran, env, run};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use symphony_board::cli::{Env, connect};
use symphony_board::sys::Command;
use symphony_board::{NewSession, NewTask, Store};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-herdr");
const TASKS: &str = env!("CARGO_BIN_EXE_tasks");

/// A pid no process has (macOS's run to 99998, Linux's to 4194304).
const NO_PID: i64 = 99_999_999;

/// A folder of its own outside any repository (the target dir is inside this one), emptied first.
fn outside(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("launch-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn git(dir: &PathBuf, args: &[&str]) {
    let done = symphony_board::sys::output(
        Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
            .arg(dir)
            .args(args),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
}

/// A board of its own, a repository with `main` to run in, its worktrees in a folder of their own,
/// and the fake herdr's state and log.
pub(crate) struct Panes {
    dir: PathBuf,
    env: Env,
}

impl Panes {
    pub(crate) fn new(name: &str) -> Panes {
        let dir = outside(name);
        let repo = dir.join("lamp-works");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let mut env = env();
        for (key, value) in [
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("HERDR_BIN", FAKE.to_string()),
            (
                "FAKE_HERDR_STATE",
                dir.join("herdr.json").display().to_string(),
            ),
            (
                "FAKE_HERDR_LOG",
                dir.join("herdr.log").display().to_string(),
            ),
            ("SF_WT_DIR", dir.join("wt").display().to_string()),
            ("TASKS_PS_TABLE", dir.join("ps.txt").display().to_string()),
        ] {
            env.vars.insert(key.into(), value);
        }
        env.cwd = repo;
        // The process table the commands read: never this machine's `ps`, whose transient failures
        // under load would keep a pane these cases close. NO_PID is a shell with nothing under it.
        fs::write(
            dir.join("ps.txt"),
            format!("    1     0 /sbin/launchd\n{NO_PID}     1 -zsh\n"),
        )
        .unwrap();
        let panes = Panes { dir, env };
        panes.set_state(json!({"tabs": [], "panes": [], "next": 100}));
        panes
    }

    /// The process table the commands read in place of `ps` (`TASKS_PS_TABLE`).
    pub(crate) fn ps_table(&self) -> PathBuf {
        self.dir.join("ps.txt")
    }

    pub(crate) fn set(&mut self, key: &str, value: &str) {
        self.env.vars.insert(key.into(), value.into());
    }

    pub(crate) fn run(&self, args: &[&str]) -> Ran {
        run(&self.env, args, "")
    }

    pub(crate) fn as_(&self, who: &str, args: &[&str]) -> Ran {
        let mut env = self.env.clone();
        env.vars.insert("TASKS_SESSION".into(), who.into());
        run(&env, args, "")
    }

    pub(crate) fn board(&self) -> Store {
        connect(&self.env).unwrap()
    }

    fn set_state(&self, state: Value) {
        fs::write(self.dir.join("herdr.json"), state.to_string()).unwrap();
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.dir.join("herdr.json")).unwrap()).unwrap()
    }

    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.dir.join("herdr.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    pub(crate) fn pane_ids(&self) -> Vec<String> {
        self.state()["panes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["pane_id"].as_str().unwrap().to_string())
            .collect()
    }

    /// An orchestrator and a task `sim/jump` it made: the orchestrator's handle.
    fn orchestrator(&self) -> String {
        let board = self.board();
        let orch = board
            .register(&NewSession::new("orchestrator"))
            .unwrap()
            .row
            .handle;
        board
            .create(&NewTask::new("sim/jump", "Jump", &orch))
            .unwrap();
        orch
    }

    /// An idle claude pane `w1:p1`, its session `c`, nothing running under it.
    pub(crate) fn idle_pane(&self, handle: &str) {
        self.set_state(json!({"next": 100, "tabs": [{"tab_id": "w1:t1", "label": "code", "workspace_id": "w1"}],
            "panes": [{
                "pane_id": "w1:p1", "tab_id": "w1:t1", "agent": "claude", "agent_status": "idle",
                "shell_pid": NO_PID, "agent_session": {"agent": "claude", "kind": "id", "value": "c"},
                "foreground": [{"argv": ["claude", "-n", handle], "cmdline": format!("claude -n {handle}"),
                                "name": "claude", "pid": NO_PID}],
            }]}));
    }
}

// -- launch -----------------------------------------------------------------------------

#[test]
fn a_launch_says_who_it_started_where() {
    for (name, flags, agent, model) in [
        ("default", &[][..], "codex", "gpt-6.1-sol"),
        (
            "claude-default",
            &["--kind", "claude"][..],
            "claude",
            "sonnet",
        ),
        (
            "explicit-model",
            &["--model", "sonnet"][..],
            "codex",
            "sonnet",
        ),
    ] {
        let p = Panes::new(&format!("says-{name}"));
        let orch = p.orchestrator();
        let given = p.dir.join("given");
        fs::create_dir_all(&given).unwrap();
        let given_arg = given.display().to_string();
        let mut args = vec!["launch", "sim/jump", "--worktree", &given_arg];
        args.extend_from_slice(flags);
        let ran = p.as_(&orch, &args);
        assert_eq!((ran.code, ran.err.as_str()), (0, ""));
        let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!(
            ran.out,
            format!(
                "launched {} on sim/jump as executor: pane {}, agent session {}, worktree {given_arg}\n",
                s.handle, s.pane, s.claude_session
            )
        );
        assert_eq!(s.model, model, "{name}");
        let start = p
            .calls()
            .into_iter()
            .find(|call| call.starts_with(&["agent".into(), "start".into()]))
            .unwrap();
        assert_eq!(start[4], agent, "{start:?}");
        let model_flag = if agent == "codex" { "-m" } else { "--model" };
        assert!(
            start.windows(2).any(|pair| pair == [model_flag, model]),
            "{start:?}"
        );
        let work = if agent == "codex" { "$work" } else { "/work" };
        assert_eq!(
            p.state()["prompts"],
            json!([[s.handle, format!("{work} sim/jump {}", s.handle)]])
        );
    }
}

#[test]
fn a_launch_hands_each_flag_to_the_launcher() {
    let p = Panes::new("flags");
    let orch = p.orchestrator();
    let ran = p.as_(
        &orch,
        &[
            "launch",
            "sim/jump",
            "--model",
            "opus",
            "--kind",
            "reviewer",
            "--role",
            "reviewer",
            "--detached",
            "--tab",
            "t",
            "--prompt",
            "go",
        ],
    );
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (s.role.as_str(), s.model.as_str(), s.branch.as_str()),
        ("reviewer", "opus", "")
    );
    assert_eq!(p.board().session(&s.handle).unwrap().row.kind, "reviewer");
    let state = p.state();
    assert_eq!(state["tabs"][0]["label"], "t");
    let prompt = format!("/work sim/jump {} go", s.handle);
    assert_eq!(state["prompts"], json!([[s.handle, prompt]]));
}

#[test]
fn a_launch_without_prompt_confirmation_exits_one_and_records_no_live_session() {
    let mut p = Panes::new("stuck");
    let orch = p.orchestrator();
    p.set("FAKE_HERDR_STUCK", "1");
    p.set("FAKE_HERDR_FAIL", "pane send-keys");
    let ran = p.as_(
        &orch,
        &[
            "launch", "sim/jump", "--kind", "claude", "--model", "sonnet",
        ],
    );
    let start = p
        .calls()
        .into_iter()
        .find(|call| call.starts_with(&["agent".into(), "start".into()]))
        .unwrap();
    let pane = &start[start.iter().position(|arg| arg == "--pane").unwrap() + 1];
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (
            1,
            "",
            format!("✖ could not confirm that the agent in pane {pane} took its prompt\n").as_str()
        )
    );
    assert!(p.board().task_sessions("sim/jump").unwrap().is_empty());
    assert!(p.pane_ids().is_empty());
    assert!(!p.dir.join("wt/sim-jump").exists());
    let worker_health: Vec<String> = p
        .board()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|session| session.row.kind == "worker")
        .map(|session| session.health.as_str().to_string())
        .collect();
    assert_eq!(worker_health, ["left"]);
}
#[test]
fn a_launch_without_a_session_runs_nothing() {
    let p = Panes::new("nobody");
    p.orchestrator();
    let ran = p.run(&["launch", "sim/jump", "--model", "m"]);
    assert_eq!(ran.code, 2);
    assert!(ran.err.contains("TASKS_SESSION"), "{}", ran.err);
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
}

#[test]
fn a_launch_the_board_refuses_is_its_reason_and_exit_1() {
    let p = Panes::new("refused");
    let orch = p.orchestrator();
    let ran = p.as_(&orch, &["launch", "sim/none", "--model", "m"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (1, "", "✖ no task 'sim/none'\n")
    );
}

#[test]
fn show_has_a_sessions_block_once_a_session_is_launched_open_then_closed() {
    let p = Panes::new("show");
    let orch = p.orchestrator();
    let show = || {
        let ran = p.as_(&orch, &["show", "sim/jump"]);
        assert_eq!(ran.code, 0, "{}", ran.err);
        ran.out
    };
    assert!(!show().contains("Sessions:"));
    let given = p.dir.join("given");
    fs::create_dir_all(&given).unwrap();
    let given = given.display().to_string();
    let ran = p.as_(
        &orch,
        &[
            "launch",
            "sim/jump",
            "--model",
            "sonnet",
            "--worktree",
            &given,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
    let shown = show();
    let block = shown.split("Sessions:").nth(1).unwrap_or_default();
    for field in [
        &s.handle,
        "executor",
        &s.claude_session,
        &given,
        &s.pane,
        "open",
    ] {
        assert!(block.contains(field), "{field:?} not in {shown}");
    }
    let restore = format!("just tasks restore sim/jump {}", s.handle);
    p.board()
        .close_task_session("sim/jump", &s.handle, "idle", &restore)
        .unwrap();
    assert!(
        show()
            .split("Sessions:")
            .nth(1)
            .unwrap_or_default()
            .contains("closed")
    );
}

// -- restore ----------------------------------------------------------------------------

#[test]
fn a_restore_with_every_session_open_says_there_is_nothing_to_restore() {
    let p = Panes::new("open");
    let orch = p.orchestrator();
    let given = p.dir.display().to_string();
    let launched = p.as_(
        &orch,
        &["launch", "sim/jump", "--model", "m", "--worktree", &given],
    );
    assert_eq!(launched.code, 0, "{}", launched.err);
    let ran = p.run(&["restore", "sim/jump"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "nothing to restore: every recorded session of sim/jump is open\n",
            ""
        )
    );
}

#[test]
fn a_restore_says_each_session_it_reopened() {
    let p = Panes::new("reopened");
    let orch = p.orchestrator();
    let given = p.dir.display().to_string();
    // The fake supplies Claude's requested conversation id; it cannot discover a new Codex id.
    let launched = p.as_(
        &orch,
        &[
            "launch",
            "sim/jump",
            "--kind",
            "claude",
            "--model",
            "m",
            "--worktree",
            &given,
        ],
    );
    assert_eq!((launched.code, launched.err.as_str()), (0, ""));
    let board = p.board();
    let [s] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    board
        .close_task_session("sim/jump", &s.handle, "idle", "r")
        .unwrap();
    // A closed session represents a pane that is gone. Keeping the old fake pane would leave
    // its duplicate agent name receiving prompts intended for the restored pane.
    let mut state = p.state();
    state["panes"]
        .as_array_mut()
        .unwrap()
        .retain(|pane| pane["pane_id"] != s.pane);
    p.set_state(state);
    let ran = p.run(&["restore", "sim/jump", &s.handle]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""));
    let [again] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    assert_ne!(again.pane, s.pane);
    assert_eq!(again.claude_session, s.claude_session);
    assert_eq!(again.worktree, s.worktree);
    assert!(again.closed.is_none());
    let state = p.state();
    let pane = state["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["pane_id"] == again.pane)
        .unwrap();
    assert_eq!(pane["agent_status"], "working");
    assert_eq!(
        (ran.code, ran.out, ran.err.as_str()),
        (
            0,
            format!(
                "restored {} on sim/jump: pane {}, claude session {}\n",
                s.handle, again.pane, s.claude_session
            ),
            ""
        )
    );
}

#[test]
fn a_restore_of_a_task_never_launched_is_refused_with_exit_1() {
    let p = Panes::new("never");
    p.orchestrator();
    let ran = p.run(&["restore", "sim/jump"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (
            1,
            "",
            "✖ sim/jump has no recorded sessions: it was not launched with `just tasks launch`\n"
        )
    );
}

#[test]
fn a_board_it_cannot_reach_is_exit_2() {
    let mut p = Panes::new("unreached");
    p.set("TASKS_SERVER", "127.0.0.1:1");
    let ran = p.run(&["restore", "sim/jump"]);
    assert_eq!((ran.code, ran.out.as_str()), (2, ""));
    assert!(
        ran.err.starts_with("✖ ") && ran.err.ends_with('\n') && ran.err.matches('\n').count() == 1,
        "{}",
        ran.err
    );
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
}

// -- cleanup ----------------------------------------------------------------------------

#[test]
fn a_cleanup_with_no_panes_says_there_is_nothing_to_clean_up() {
    let p = Panes::new("empty");
    for args in [&["cleanup", "--dry-run"][..], &["cleanup"]] {
        let ran = p.run(args);
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err.as_str()),
            (0, "nothing to clean up\n", ""),
            "{args:?}"
        );
    }
}

#[test]
fn a_cleanup_says_what_it_would_close_and_what_it_closed_with_its_restore_line() {
    let p = Panes::new("closes");
    let orch = p.orchestrator();
    let given = p.dir.display().to_string();
    p.as_(
        &orch,
        &["launch", "sim/jump", "--model", "m", "--worktree", &given],
    );
    let board = p.board();
    let [s] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    board.leave(&s.handle).unwrap();
    let mut state = p.state();
    for pane in state["panes"].as_array_mut().unwrap() {
        pane["agent_status"] = json!("idle");
        pane["shell_pid"] = json!(NO_PID);
    }
    p.set_state(state);
    let line = |verb: &str| {
        format!(
            "{verb} pane {} of {} (it holds nothing live); restore: just tasks restore sim/jump {}\n",
            s.pane, s.handle, s.handle
        )
    };
    let dry = p.run(&["cleanup", "--dry-run"]);
    assert_eq!(
        (dry.code, dry.out, dry.err.as_str()),
        (0, line("would close"), "")
    );
    assert!(p.pane_ids().contains(&s.pane));
    let ran = p.run(&["cleanup"]);
    assert_eq!(
        (ran.code, ran.out, ran.err.as_str()),
        (0, line("closed"), "")
    );
    assert!(!p.pane_ids().contains(&s.pane));
}

#[test]
fn a_herdr_failure_is_exit_1() {
    let mut p = Panes::new("herdr-fails");
    p.set("FAKE_HERDR_FAIL", "pane list");
    let ran = p.run(&["cleanup"]);
    assert_eq!((ran.code, ran.out.as_str()), (1, ""));
    assert!(
        ran.err.starts_with("✖ herdr pane list failed: "),
        "{}",
        ran.err
    );
}

#[test]
fn a_launch_herdr_fails_is_a_line_naming_the_call_and_exit_1() {
    let mut p = Panes::new("launch-herdr-fails");
    let orch = p.orchestrator();
    p.set("FAKE_HERDR_FAIL", "tab create");
    let given = p.dir.join("given");
    fs::create_dir_all(&given).unwrap();
    let given = given.display().to_string();
    let ran = p.as_(
        &orch,
        &[
            "launch",
            "sim/jump",
            "--model",
            "sonnet",
            "--worktree",
            &given,
        ],
    );
    assert_eq!((ran.code, ran.out.as_str()), (1, ""));
    assert!(
        ran.err.starts_with("✖ ")
            && ran.err.contains("herdr tab create")
            && ran.err.matches('\n').count() == 1,
        "{}",
        ran.err
    );
}

// -- reap -------------------------------------------------------------------------------

impl Panes {
    /// `tasks ARGS` as its own process, at `TASKS_NOW` `now`: the binary sweeps on one thread, so
    /// no process this test binary's other threads spawn holds a copy of its lock.
    fn binary(&self, now: f64, args: &[&str]) -> Ran {
        let done = symphony_board::sys::output(
            Command::new(TASKS)
                .args(args)
                .env_clear()
                .envs(&self.env.vars)
                .env("TASKS_NOW", now.to_string())
                .current_dir(&self.env.cwd),
        )
        .unwrap();
        Ran {
            code: done.status.code().unwrap_or(-1),
            out: String::from_utf8(done.stdout).unwrap(),
            err: String::from_utf8(done.stderr).unwrap(),
        }
    }
}

#[test]
fn idle_manual_staffing_reports_disabled_configuration() {
    let p = Panes::new("disabled-staff");
    let ran = p.binary(crate::common::NOW, &["staff"]);
    assert_eq!(
        (ran.code, ran.out, ran.err),
        (
            0,
            "staff disabled (tools/task_staff.json)\n".into(),
            String::new()
        )
    );
}

#[test]
fn a_reap_closes_an_agent_idle_for_two_sweeps_where_tasks_reap_state_names_its_state() {
    let mut p = Panes::new("reap");
    let state = p.dir.join("server-reap.json");
    p.set("TASKS_REAP_STATE", &state.display().to_string());
    let worker = p
        .board()
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    p.idle_pane(&worker);
    let now = crate::common::NOW;
    let said = |ran: Ran| (ran.code, ran.out, ran.err);
    assert_eq!(
        said(p.binary(now, &["reap", "--dry-run"])),
        (
            0,
            format!("idle once, closed if still idle next sweep: {worker}\n"),
            String::new()
        )
    );
    assert!(!state.exists(), "a dry run keeps no state");
    assert_eq!(
        said(p.binary(now, &["reap"])),
        (
            0,
            format!("idle once, closed if still idle next sweep: {worker}\n"),
            String::new()
        )
    );
    assert!(state.exists());
    let later = now + symphony_board::launch::reap::REAP_SECONDS;
    assert_eq!(
        said(p.binary(later, &["reap", "--dry-run"])),
        (0, format!("would close w1:p1 ({worker})\n"), String::new())
    );
    assert_eq!(
        said(p.binary(later, &["reap", "--dry-run", "--as", &worker])),
        (0, "nothing to close\n".into(), String::new()),
        "never the caller's own"
    );
    assert_eq!(
        said(p.binary(later, &["reap"])),
        (0, format!("closed w1:p1 ({worker})\n"), String::new())
    );
    assert_eq!(p.pane_ids(), Vec::<String>::new());
}

#[test]
fn a_reap_while_another_sweep_holds_the_lock_says_so() {
    let mut p = Panes::new("reap-held");
    let state = p.dir.join("reap.json");
    p.set("TASKS_REAP_STATE", &state.display().to_string());
    let held = fs::File::create(symphony_board::launch::reap::lock_path(&state)).unwrap();
    held.try_lock().unwrap();
    let ran = p.binary(crate::common::NOW, &["reap"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "another sweep is running (the Symphony's timer): nothing done\n",
            ""
        )
    );
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
}
