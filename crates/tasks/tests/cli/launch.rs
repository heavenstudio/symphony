//! Launch commands use a private board, a throwaway repository and the fake hc.

use crate::{Ran, env, run};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use symphony_board::cli::{Env, connect};
use symphony_board::launch::hc::board_of;
use symphony_board::sys::Command;
use symphony_board::{NewLaunch, NewSession, NewTask, Store};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-hc");
const TASKS: &str = env!("CARGO_BIN_EXE_tasks");

/// A pid no process has (macOS's run to 99998, Linux's to 4194304).
pub(crate) const NO_PID: i64 = 99_999_999;

/// A folder of its own outside any repository (the target dir is inside this one), emptied first: a
/// scratch folder of this process's, gone with the binary's exit.
fn outside(name: &str) -> PathBuf {
    crate::scratch(&format!("launch-{name}"))
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

/// A board of its own, a repository `lamp-works` with `main` to run in, its worktrees in a folder
/// of their own, and the fake hc's state (the Herdr app showing the space `lamp-works`) and log.
pub(crate) struct Terminals {
    pub(crate) dir: PathBuf,
    pub(crate) env: Env,
}

impl Terminals {
    pub(crate) fn new(name: &str) -> Terminals {
        let dir = outside(name);
        let repo = dir.join("lamp-works");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let mut env = env();
        for (key, value) in [
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("HC_BIN", FAKE.to_string()),
            ("HERDR_APP_BIN", format!("'{FAKE}' app")),
            ("FAKE_HC_STATE", dir.join("hc.json").display().to_string()),
            ("FAKE_HC_LOG", dir.join("hc.log").display().to_string()),
            ("SYMPHONY_WT_DIR", dir.join("wt").display().to_string()),
            ("TASKS_PS_TABLE", dir.join("ps.txt").display().to_string()),
        ] {
            env.vars.insert(key.into(), value);
        }
        env.cwd = repo;
        // The process table the commands read: never this machine's `ps`, whose transient failures
        // under load would keep a terminal these cases close. NO_PID is a shell with nothing under
        // it.
        fs::write(
            dir.join("ps.txt"),
            format!("    1     0 /sbin/launchd\n{NO_PID}     1 -zsh\n"),
        )
        .unwrap();
        let terminals = Terminals { dir, env };
        terminals.set_state(json!({"terminals": [], "layout": {"spaces": [
            {"id": "sp-lamp", "name": "lamp-works", "tabs": []},
        ]}}));
        terminals
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

    pub(crate) fn set_state(&self, state: Value) {
        fs::write(self.dir.join("hc.json"), state.to_string()).unwrap();
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.dir.join("hc.json")).unwrap()).unwrap()
    }

    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.dir.join("hc.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// The arguments of the calls that start with `word`.
    fn calls_of(&self, word: &str) -> Vec<Vec<String>> {
        self.calls().into_iter().filter(|c| c[0] == word).collect()
    }

    /// What the one `hc run` started: the program, past `env` and its settings.
    fn program(&self) -> Vec<String> {
        let [run] = <[_; 1]>::try_from(self.calls_of("run")).unwrap();
        let argv = run[run.iter().position(|a| a == "--").unwrap() + 1..].to_vec();
        symphony_board::launch::hc::program(&argv).to_vec()
    }

    /// The first message the one `hc send` gave.
    fn sent(&self) -> String {
        let [send] = <[_; 1]>::try_from(self.calls_of("send")).unwrap();
        send[2].clone()
    }

    pub(crate) fn addresses(&self) -> Vec<String> {
        self.state()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["address"].as_str().unwrap().to_string())
            .collect()
    }

    pub(crate) fn edit_terminals(&self, edit: impl Fn(&mut serde_json::Map<String, Value>)) {
        let mut state = self.state();
        for t in state["terminals"].as_array_mut().unwrap() {
            edit(t.as_object_mut().unwrap());
        }
        self.set_state(state);
    }

    /// An orchestrator and a task `sim/jump` it made: the orchestrator's handle.
    pub(crate) fn orchestrator(&self) -> String {
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

    /// An idle claude terminal `t1` of this board, its conversation `c`, nothing running under
    /// it.
    pub(crate) fn idle_terminal(&self, handle: &str) {
        self.set_state(json!({"terminals": [{
            "address": "t1", "name": handle, "state": "live", "status": "idle", "agent": "claude-code",
            "pid": NO_PID, "cwd": "/somewhere", "turn": 3,
            "argv": ["env", "TASKS_SESSION=x", "claude", "--session-id", "c", "-n", handle, "go"],
            "labels": {"symphony.board": board_of(&self.env), "symphony.handle": handle},
        }], "layout": {"spaces": []}}));
    }
}

#[test]
fn a_launch_cases_folder_is_scratch_of_this_runs_own_and_goes_with_it() {
    // The run's own folder goes at its exit, or at the next run after a kill (`scratch_folder`).
    let p = Terminals::new("own");
    assert_eq!(p.dir.parent(), Some(crate::own_scratch()));
    assert!(p.dir.join("lamp-works/.git").is_dir());
}

// -- launch -----------------------------------------------------------------------------

impl Terminals {
    /// The repository's `symphony.toml`, which the launch reads in its main checkout.
    fn manifest(&self, text: &str) {
        fs::write(self.env.cwd.join("symphony.toml"), text).unwrap();
    }

    /// `launch sim/jump FLAGS` by the orchestrator, in a worktree given so nothing is checked out:
    /// what it said, and the one session it recorded.
    fn launch_with(
        &self,
        orch: &str,
        flags: &[&str],
    ) -> (Ran, symphony_board::rows::TaskSessionRow) {
        let given = self.dir.join("given");
        fs::create_dir_all(&given).unwrap();
        let given = given.display().to_string();
        let mut args = vec!["launch", "sim/jump", "--worktree", &given];
        args.extend_from_slice(flags);
        let ran = self.as_(orch, &args);
        assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{flags:?}");
        let [s] = <[_; 1]>::try_from(self.board().task_sessions("sim/jump").unwrap()).unwrap();
        (ran, s)
    }

    /// The one `hc run`'s agent, model and effort: Claude's `--model` and `--effort`, Codex's `-m`
    /// and `model_reasoning_effort`.
    fn started_agent(&self) -> (String, String, Option<String>) {
        let program = self.program();
        let after = |flag: &str| {
            program
                .windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1].clone())
        };
        if program[0] == "codex" {
            let effort = program
                .iter()
                .find_map(|a| a.strip_prefix("model_reasoning_effort="))
                .map(|e| e.trim_matches('"').to_string());
            (program[0].clone(), after("-m").unwrap(), effort)
        } else {
            (
                program[0].clone(),
                after("--model").unwrap(),
                after("--effort"),
            )
        }
    }
}

#[test]
fn a_launch_says_who_it_started_where() {
    for (name, flags, agent, model, said) in [
        (
            "default",
            &[][..],
            "codex",
            "gpt-6.1-sol",
            "codex on gpt-6.1-sol (effort xhigh)",
        ),
        (
            "claude-default",
            &["--kind", "claude"][..],
            "claude",
            "sonnet",
            "claude on sonnet",
        ),
        (
            "explicit-model",
            &["--model", "sonnet"][..],
            "codex",
            "sonnet",
            "codex on sonnet (effort xhigh)",
        ),
    ] {
        let p = Terminals::new(&format!("says-{name}"));
        let orch = p.orchestrator();
        let given_arg = p.dir.join("given").display().to_string();
        let (ran, s) = p.launch_with(&orch, flags);
        assert_eq!(
            ran.out,
            format!(
                "launched {} on sim/jump as executor: {said}, pane {}, agent session {}, worktree {given_arg}\n",
                s.handle, s.pane, s.claude_session
            )
        );
        assert_eq!(s.model, model, "{name}");
        let program = p.program();
        assert_eq!(program[0], agent, "{program:?}");
        let model_flag = if agent == "codex" { "-m" } else { "--model" };
        assert!(
            program.windows(2).any(|pair| pair == [model_flag, model]),
            "{program:?}"
        );
        let work = if agent == "codex" { "$work" } else { "/work" };
        assert_eq!(p.sent(), format!("{work} sim/jump {}", s.handle));
        // The agent reaches the board its launcher does.
        let [run] = <[_; 1]>::try_from(p.calls_of("run")).unwrap();
        let database = format!("TASKS_DATABASE={}", p.env.vars["TASKS_DATABASE"]);
        assert!(run.contains(&database), "{run:?}");
        assert!(
            run.contains(&format!("symphony.board={}", board_of(&p.env))),
            "{run:?}"
        );
    }
}

#[test]
fn a_launch_hands_each_flag_to_the_launcher() {
    let p = Terminals::new("flags");
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
    assert_eq!(p.state()["layout"]["spaces"][0]["tabs"][0]["name"], "t");
    assert_eq!(p.sent(), format!("/work sim/jump {} go", s.handle));
}

#[test]
fn a_bare_launch_starts_the_projects_agent_model_and_effort_and_flags_win_over_them() {
    let claude =
        "[agents]\nlaunch = { default = \"claude\", model = \"opus\", effort = \"high\" }\n";
    let effortless =
        "[agents]\npanes = \"hc\"\nlaunch = { default = \"claude\", model = \"opus\" }\n";
    for (name, manifest, flags, agent, model, effort) in [
        ("bare", claude, &[][..], "claude", "opus", Some("high")),
        // This repository's own: Claude on Opus at Claude's own effort.
        ("ours", effortless, &[][..], "claude", "opus", None),
        (
            "model",
            claude,
            &["--model", "sonnet"][..],
            "claude",
            "sonnet",
            Some("high"),
        ),
        (
            "reviewer-kind",
            claude,
            &["--kind", "reviewer"][..],
            "claude",
            "opus",
            Some("high"),
        ),
        // The project's model and effort are its agent's: another agent takes the board's own.
        (
            "codex",
            claude,
            &["--kind", "codex"][..],
            "codex",
            "gpt-6.1-sol",
            Some("xhigh"),
        ),
        (
            "codex-model",
            claude,
            &["--kind", "codex", "--model", "m"][..],
            "codex",
            "m",
            Some("xhigh"),
        ),
        (
            "codex-project",
            "[agents.launch]\ndefault = \"codex\"\nmodel = \"gpt-x\"\neffort = \"medium\"\n",
            &[][..],
            "codex",
            "gpt-x",
            Some("medium"),
        ),
        (
            "no-table",
            "[project]\nname = \"Lamp\"\n",
            &[][..],
            "codex",
            "gpt-6.1-sol",
            Some("xhigh"),
        ),
    ] {
        let p = Terminals::new(&format!("project-{name}"));
        p.manifest(manifest);
        let orch = p.orchestrator();
        let (ran, s) = p.launch_with(&orch, flags);
        assert_eq!(
            p.started_agent(),
            (
                agent.to_string(),
                model.to_string(),
                effort.map(str::to_string)
            ),
            "{name}"
        );
        assert_eq!(s.model, model, "{name}");
        let said = match effort {
            Some(effort) => format!("{agent} on {model} (effort {effort})"),
            None => format!("{agent} on {model}"),
        };
        assert!(
            ran.out.contains(&format!(" as executor: {said}, pane ")),
            "{name}: {}",
            ran.out
        );
        let work = if agent == "codex" { "$work" } else { "/work" };
        assert_eq!(p.sent(), format!("{work} sim/jump {}", s.handle), "{name}");
    }
}

#[test]
fn a_project_launch_table_it_cannot_read_refuses_the_launch_and_starts_nothing() {
    let p = Terminals::new("project-broken");
    p.manifest("[agents.launch]\ndefault = \"gemini\"\n");
    let orch = p.orchestrator();
    let ran = p.as_(&orch, &["launch", "sim/jump", "--kind", "claude"]);
    let path = p.env.cwd.join("symphony.toml");
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (
            1,
            "",
            format!(
                "✖ {}: [agents] launch default \"gemini\" is not one of codex, claude\n",
                path.display()
            )
        )
    );
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
    assert!(p.board().task_sessions("sim/jump").unwrap().is_empty());
}

#[test]
fn launch_help_says_what_a_bare_launch_starts_in_the_project_it_runs_in() {
    let p = Terminals::new("help");
    let path = p.env.cwd.join("symphony.toml").display().to_string();
    let help = |p: &Terminals| {
        let ran = p.run(&["launch", "--help"]);
        assert_eq!((ran.code, ran.err.as_str()), (0, ""));
        assert!(
            ran.out.contains(
                "--model MODEL         default the model of the project's [agents] launch for"
            ),
            "{}",
            ran.out
        );
        ran.out.lines().last().unwrap().to_string()
    };
    assert_eq!(
        help(&p),
        format!(
            "here a bare launch starts codex on gpt-6.1-sol (effort xhigh), the board's own: no \
             [agents] launch in {path}"
        )
    );
    p.manifest("[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n");
    assert_eq!(
        help(&p),
        format!("here a bare launch starts claude on opus, from [agents] launch in {path}")
    );
    // From a worktree too: the main checkout's file is the one read.
    let wt = p.dir.join("wt-help");
    git(
        &p.env.cwd,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &wt.display().to_string(),
        ],
    );
    let mut there = Terminals {
        dir: p.dir.clone(),
        env: p.env.clone(),
    };
    there.env.cwd = wt;
    assert_eq!(
        help(&there),
        format!("here a bare launch starts claude on opus, from [agents] launch in {path}")
    );
    // Other commands' help says nothing of it.
    let ran = p.run(&["restore", "--help"]);
    assert!(!ran.out.contains("bare launch"), "{}", ran.out);
}

#[test]
fn a_launch_blocked_on_the_folder_trust_dialog_exits_one_and_leaves_nothing() {
    let mut p = Terminals::new("unsent");
    let orch = p.orchestrator();
    p.set("FAKE_HC_BLOCKED", "1");
    let ran = p.as_(
        &orch,
        &[
            "launch", "sim/jump", "--kind", "claude", "--model", "sonnet",
        ],
    );
    let [removed] = <[_; 1]>::try_from(p.calls_of("rm")).unwrap();
    let terminal = &removed[1];
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (
            1,
            "",
            format!(
                "✖ the agent in terminal {terminal} waits on a person before it took its first \
                 message: Quick safety check: Is this a project you created or one you trust?\n"
            )
            .as_str()
        )
    );
    assert_eq!(p.calls_of("send"), Vec::<Vec<String>>::new());
    assert_eq!(p.calls_of("kill").len(), 1);
    assert!(p.board().task_sessions("sim/jump").unwrap().is_empty());
    assert!(p.addresses().is_empty());
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
    let p = Terminals::new("nobody");
    p.orchestrator();
    let ran = p.run(&["launch", "sim/jump", "--model", "m"]);
    assert_eq!(ran.code, 2);
    assert!(ran.err.contains("TASKS_SESSION"), "{}", ran.err);
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
}

#[test]
fn a_launch_the_board_refuses_is_its_reason_and_exit_1() {
    let p = Terminals::new("refused");
    let orch = p.orchestrator();
    let ran = p.as_(&orch, &["launch", "sim/none", "--model", "m"]);
    assert_eq!((ran.code, ran.out.as_str()), (1, ""));
    assert!(ran.err.starts_with("✖ no task 'sim/none'"), "{}", ran.err);
}

#[test]
fn show_has_a_sessions_block_once_a_session_is_launched_open_then_closed() {
    let p = Terminals::new("show");
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
    let p = Terminals::new("open");
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
    let p = Terminals::new("reopened");
    let orch = p.orchestrator();
    let given = p.dir.display().to_string();
    // Claude's conversation id is the launch's own; a Codex thread is known once it registers.
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
    // The board's record closes and its terminal goes, as cleanup does.
    board
        .close_task_session("sim/jump", &s.handle, "idle", "r")
        .unwrap();
    p.set_state({
        let mut state = p.state();
        state["terminals"] = json!([]);
        state
    });
    let ran = p.run(&["restore", "sim/jump", &s.handle]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""));
    let [again] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    assert_ne!(again.pane, s.pane);
    assert_eq!(again.claude_session, s.claude_session);
    assert_eq!(again.worktree, s.worktree);
    assert!(again.closed.is_none());
    assert_eq!(p.addresses(), [again.pane.as_str()]);
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
fn a_herdr_pane_record_is_closed_by_cleanup_and_restored_into_an_hc_terminal() {
    let p = Terminals::new("herdr-record");
    let orch = p.orchestrator();
    let board = p.board();
    let worker = board
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    board
        .record_launch(&NewLaunch {
            slug: "sim/jump".into(),
            handle: worker.clone(),
            role: "executor".into(),
            claude_session: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            worktree: p.dir.display().to_string(),
            pane: "w1:p3".into(),
            model: "sonnet".into(),
            ..NewLaunch::default()
        })
        .unwrap();
    let restore = format!("just tasks restore sim/jump {worker}");
    let ran = p.as_(&orch, &["cleanup"]);
    assert_eq!(
        (ran.code, ran.out, ran.err.as_str()),
        (
            0,
            format!("closed pane w1:p3 of {worker} (its pane is gone); restore: {restore}\n"),
            ""
        )
    );
    assert_eq!(p.calls_of("kill"), Vec::<Vec<String>>::new());
    let ran = p.run(&["restore", "sim/jump"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""));
    let [again] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    assert!(again.closed.is_none());
    assert_eq!(p.addresses(), [again.pane.as_str()]);
    assert_eq!(
        p.program()[..3],
        [
            "claude".to_string(),
            "--resume".into(),
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into()
        ]
    );
}

#[test]
fn a_restore_of_a_task_never_launched_is_refused_with_exit_1() {
    let p = Terminals::new("never");
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
    let mut p = Terminals::new("unreached");
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
fn a_cleanup_with_no_terminals_says_there_is_nothing_to_clean_up() {
    let p = Terminals::new("empty");
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
    let p = Terminals::new("closes");
    let orch = p.orchestrator();
    let given = p.dir.display().to_string();
    p.as_(
        &orch,
        &["launch", "sim/jump", "--model", "m", "--worktree", &given],
    );
    let board = p.board();
    let [s] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    board.leave(&s.handle).unwrap();
    p.edit_terminals(|t| {
        t.insert("status".into(), json!("idle"));
        t.insert("pid".into(), json!(NO_PID));
    });
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
    assert!(p.addresses().contains(&s.pane));
    let ran = p.run(&["cleanup"]);
    assert_eq!(
        (ran.code, ran.out, ran.err.as_str()),
        (0, line("closed"), "")
    );
    assert!(!p.addresses().contains(&s.pane));
}

#[test]
fn the_callers_own_terminal_is_never_cleaned_up() {
    let mut p = Terminals::new("caller");
    let worker = p
        .board()
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    p.board()
        .create(&NewTask::new("sim/jump", "Jump", &worker))
        .unwrap();
    p.board().claim("sim/jump", &worker).unwrap();
    p.board().move_to("sim/jump", "cancelled", &worker).unwrap();
    p.idle_terminal(&worker);
    p.set("HC_TERMINAL_ID", "t1");
    let ran = p.run(&["cleanup"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (0, "nothing to clean up\n", "")
    );
    assert_eq!(p.addresses(), ["t1"]);
}

#[test]
fn an_hc_failure_is_exit_1() {
    let mut p = Terminals::new("hc-fails");
    p.set("FAKE_HC_FAIL", "ls");
    let ran = p.run(&["cleanup"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (1, "", "✖ hc ls: fake_failure: ls failed\n")
    );
}

#[test]
fn a_launch_hc_fails_is_a_line_naming_the_call_and_exit_1() {
    let mut p = Terminals::new("launch-hc-fails");
    let orch = p.orchestrator();
    p.set("FAKE_HC_FAIL", "run");
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
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (1, "", "✖ hc run: fake_failure: run failed\n")
    );
}

// -- reap -------------------------------------------------------------------------------

impl Terminals {
    /// `tasks ARGS` as its own process, at `TASKS_NOW` `now`: the binary sweeps on one thread, so
    /// no process this test binary's other threads spawn holds a copy of its lock.
    pub(crate) fn binary(&self, now: f64, args: &[&str]) -> Ran {
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
    let p = Terminals::new("disabled-staff");
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
fn a_staffing_pass_launches_codex_for_ready_work_into_the_code_tab_and_says_so_on_its_thread() {
    let p = Terminals::new("staff");
    let orch = p.staffed();
    let ran = p.binary(crate::common::NOW, &["staff"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let sessions = p.board().task_sessions("sim/jump").unwrap();
    let [s] = <[_; 1]>::try_from(sessions).unwrap_or_else(|s| panic!("{s:?}: {}", ran.out));
    let line = format!(
        "staff sim/jump: launched {} as executor: codex on gpt-6.1-sol (effort xhigh), pane {}, \
         worktree {}, prompt $work sim/jump {}",
        s.handle, s.pane, s.worktree, s.handle
    );
    assert_eq!(ran.out, format!("{line}\n"));
    assert_eq!(
        (s.role.as_str(), s.branch.as_str()),
        ("executor", "sim/jump")
    );
    // One hc run: codex, without its daemon, into a `code` tab of the project's space.
    assert_eq!(p.program()[..2], ["codex", "--no-daemon"]);
    let [run] = <[_; 1]>::try_from(p.calls_of("run")).unwrap();
    assert!(run.contains(&"herdr.space=sp-lamp".to_string()), "{run:?}");
    let tabs = &p.state()["layout"]["spaces"][0]["tabs"];
    assert_eq!(tabs[0]["name"], "code", "{tabs}");
    assert!(
        run.contains(&format!("herdr.tab={}", tabs[0]["id"].as_str().unwrap())),
        "{run:?}"
    );
    assert_eq!(p.addresses(), [s.pane.as_str()]);
    let thread = p.board().thread("sim/jump").unwrap();
    assert!(
        thread.iter().any(|m| m.sender == orch && m.text == line),
        "{thread:?}"
    );
}

impl Terminals {
    /// An orchestrator holding `sim/jump`, ready, with staffing enabled in the checkout.
    fn staffed(&self) -> String {
        let orch = self.orchestrator();
        self.board()
            .take_role("sim/jump", &orch, "orchestrator")
            .unwrap();
        let tools = self.env.cwd.join("tools");
        fs::create_dir_all(&tools).unwrap();
        fs::write(
            tools.join("task_staff.json"),
            r#"{"enabled": true, "min_free_memory_gib": 0}"#,
        )
        .unwrap();
        orch
    }
}

#[test]
fn a_staffing_pass_launches_the_projects_agent_and_a_dry_run_names_it() {
    let p = Terminals::new("staff-project");
    p.manifest("[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n");
    p.staffed();
    let dry = p.binary(crate::common::NOW, &["staff", "--dry-run"]);
    assert_eq!(
        (dry.code, dry.out.as_str(), dry.err.as_str()),
        (
            0,
            "staff sim/jump: would launch claude on opus as executor\n",
            ""
        )
    );
    let ran = p.binary(crate::common::NOW, &["staff"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        ran.out,
        format!(
            "staff sim/jump: launched {} as executor: claude on opus, pane {}, worktree {}, \
             prompt /work sim/jump {}\n",
            s.handle, s.pane, s.worktree, s.handle
        )
    );
    assert_eq!(s.model, "opus");
    assert_eq!(
        p.started_agent(),
        ("claude".to_string(), "opus".to_string(), None)
    );
    assert_eq!(p.sent(), format!("/work sim/jump {}", s.handle));
}

#[test]
fn a_staffing_pass_refuses_a_project_launch_table_it_cannot_read() {
    let p = Terminals::new("staff-broken");
    p.manifest("[agents.launch]\nmodel = 4\n");
    p.staffed();
    let ran = p.binary(crate::common::NOW, &["staff", "--dry-run"]);
    assert_eq!((ran.code, ran.out.as_str()), (1, ""));
    assert!(
        ran.err.contains("symphony.toml: [agents] launch: "),
        "{}",
        ran.err
    );
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
    assert!(p.board().task_sessions("sim/jump").unwrap().is_empty());
}
