//! Launch commands use a private board, a throwaway repository and the fake hc.

use crate::children::Child as _;
use crate::{Ran, env, run};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
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

pub(crate) fn git(dir: &PathBuf, args: &[&str]) {
    let done = symphony_board::sys::output(
        Command::of("git")
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
        // The agents' programs a launch looks for, stand-ins found first: no case passes or fails
        // by what this machine has installed.
        let agents = dir.join("agents-bin");
        crate::stand_ins::stand_ins(&agents, &["claude", "codex"]);
        let path = std::env::join_paths(std::iter::once(agents).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap()
        .into_string()
        .unwrap();
        let mut env = env();
        for (key, value) in [
            ("PATH", path),
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
    pub(crate) fn manifest(&self, text: &str) {
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
    // A reviewer's first message is its own skill's.
    assert_eq!(p.sent(), format!("/review-task sim/jump {} go", s.handle));
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
fn a_launch_takes_the_model_agents_models_names_for_its_role_and_flags_win_over_it() {
    let manifest = "[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n\n\
                    [agents.models]\nexecutor = \"sonnet\"\nreviewer = \"fable\"\n";
    for (name, flags, agent, model) in [
        ("executor", &[][..], "claude", "sonnet"),
        ("reviewer", &["--role", "reviewer"][..], "claude", "fable"),
        ("flag", &["--model", "haiku"][..], "claude", "haiku"),
        // A role's model is the project's agent's: another agent takes the board's own.
        ("codex", &["--kind", "codex"][..], "codex", "gpt-6.1-sol"),
    ] {
        let p = Terminals::new(&format!("role-model-{name}"));
        p.manifest(manifest);
        let orch = p.orchestrator();
        let (_, s) = p.launch_with(&orch, flags);
        let (started, started_model, _) = p.started_agent();
        assert_eq!(
            (started.as_str(), started_model.as_str()),
            (agent, model),
            "{name}"
        );
        assert_eq!(s.model, model, "{name}");
    }
}

#[test]
fn a_launch_takes_the_model_the_boards_store_holds_over_the_files_and_under_the_local_files() {
    let manifest = "[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n\n\
                    [agents.models]\nexecutor = \"sonnet\"\n";
    for (name, local, model) in [
        ("stored", None, "fable"),
        (
            "local",
            Some("[agents.models]\nexecutor = \"haiku\"\n"),
            "haiku",
        ),
    ] {
        let p = Terminals::new(&format!("stored-model-{name}"));
        p.manifest(manifest);
        if let Some(local) = local {
            fs::write(p.env.cwd.join("symphony.local.toml"), local).unwrap();
        }
        let orch = p.orchestrator();
        let set = p.as_(
            &orch,
            &["settings", "set", "agents.models.executor", "fable"],
        );
        assert_eq!(set.code, 0, "{}", set.err);
        let (_, s) = p.launch_with(&orch, &[]);
        let (started, started_model, _) = p.started_agent();
        assert_eq!(
            (started.as_str(), started_model.as_str()),
            ("claude", model),
            "{name}"
        );
        assert_eq!(s.model, model, "{name}");
    }
}

/// `count` live agent terminals of the machine, none of this board's (another project's, a
/// session started by hand), and the Herdr app's space `lamp-works`.
fn running_agents(p: &Terminals, count: usize) {
    let terminals: Vec<Value> = (0..count)
        .map(|n| {
            json!({
                "address": format!("x{n}"), "name": format!("elsewhere-{n}"), "state": "live",
                "status": "working", "agent": "claude-code", "pid": NO_PID, "cwd": "/elsewhere",
            })
        })
        .collect();
    p.set_state(json!({"terminals": terminals, "layout": {"spaces": [
        {"id": "sp-lamp", "name": "lamp-works", "tabs": []},
    ]}}));
}

#[test]
fn a_launch_at_the_machines_agent_limit_is_refused_and_starts_nothing_until_the_limit_is_raised() {
    let p = Terminals::new("machine-agents");
    p.manifest("[limits]\nmachine_agents = 2\n");
    // One agent's terminal, and the orchestrator's session, live with no terminal of its own.
    let orch = p.orchestrator();
    running_agents(&p, 1);
    let ran = p.as_(&orch, &["launch", "sim/jump"]);
    let path = p.env.cwd.join("symphony.toml").canonicalize().unwrap();
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (
            1,
            "",
            format!(
                "✖ no launch: this machine runs 2 agents, at its limit of 2 ([limits] \
                 machine_agents in {})\n",
                path.display()
            )
        )
    );
    assert!(p.calls_of("run").is_empty(), "{:?}", p.calls());
    assert!(p.board().task_sessions("sim/jump").unwrap().is_empty());
    // Raised in the file, the next launch goes ahead.
    p.manifest("[limits]\nmachine_agents = 3\n");
    let (ran, s) = p.launch_with(&orch, &[]);
    assert!(
        ran.out
            .starts_with(&format!("launched {} on sim/jump", s.handle)),
        "{}",
        ran.out
    );
}

#[test]
fn next_past_the_machines_agent_limit_warns_and_still_names_the_task() {
    let mut p = Terminals::new("machine-agents-next");
    p.orchestrator();
    let worker = p
        .board()
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    // One agent's terminal, and the orchestrator's and the worker's sessions, live with none.
    running_agents(&p, 1);
    // No limit written: nothing is counted, nothing said.
    assert_eq!(
        (p.as_(&worker, &["next"]).out, p.as_(&worker, &["next"]).err),
        ("sim/jump  p2  Jump\n".to_string(), String::new())
    );
    p.manifest("[limits]\nmachine_agents = 3\n");
    let at = p.as_(&worker, &["next"]);
    assert_eq!(
        (at.code, at.out.as_str(), at.err.as_str()),
        (0, "sim/jump  p2  Jump\n", "")
    );
    p.manifest("[limits]\nmachine_agents = 2\n");
    let past = p.as_(&worker, &["next"]);
    let path = p.env.cwd.join("symphony.toml").canonicalize().unwrap();
    assert_eq!(
        (past.code, past.out.as_str(), past.err),
        (
            0,
            "sim/jump  p2  Jump\n",
            format!(
                "⚠ this machine runs 3 agents, past its limit of 2 ([limits] machine_agents in \
                 {}): no more start until some finish\n",
                path.display()
            )
        )
    );
    // hc not there: the count cannot be made, which is said, and the task still named.
    p.set("HC_BIN", "/nonexistent/hc");
    let unknown = p.as_(&worker, &["next"]);
    assert_eq!(
        (unknown.code, unknown.out.as_str()),
        (0, "sim/jump  p2  Jump\n")
    );
    assert!(
        unknown.err.starts_with("⚠ hc cannot be asked, so the agents this machine runs cannot be counted: [limits] machine_agents in "),
        "{}",
        unknown.err
    );
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
                "--model MODEL         default the model the project's [agents.models] names"
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
fn a_reviewer_launch_says_it_resumed_the_tasks_recorded_reviewer() {
    let p = Terminals::new("resumed");
    let orch = p.orchestrator();
    let given = p.dir.display().to_string();
    let review = [
        "launch", "sim/jump", "--role", "reviewer", "--kind", "claude",
    ];
    let launched = p.as_(&orch, &[&review[..], &["--worktree", &given]].concat());
    assert_eq!((launched.code, launched.err.as_str()), (0, ""));
    let board = p.board();
    let [s] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    board
        .close_task_session("sim/jump", &s.handle, "idle", "r")
        .unwrap();
    p.set_state({
        let mut state = p.state();
        state["terminals"] = json!([]);
        state
    });
    let ran = p.as_(&orch, &review);
    let [again] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (ran.code, ran.out, ran.err.as_str()),
        (
            0,
            format!(
                "resumed {}, sim/jump's recorded reviewer: pane {}, agent session {}, worktree \
                 {given}\n",
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

// -- through `just tasks`, which an agent runs unasked -----------------------------------------

impl Terminals {
    /// What a command run unasked in the repository says of a folder it will not start an agent
    /// in: `place` names it.
    fn outside(&self, place: &str) -> String {
        format!(
            "{place} not a folder a recipe an agent runs unasked starts an agent in: only the \
             checkout at {}, none of its git folders, or a folder under the project's worktree \
             base {}; the binary run by itself starts one anywhere",
            self.env.cwd.display(),
            self.dir.join("wt").display()
        )
    }
}

#[test]
fn through_just_tasks_a_launch_starts_its_agent_only_in_the_project_and_with_no_prompt() {
    let mut p = Terminals::new("unasked-launch");
    let orch = p.orchestrator();
    let elsewhere = p.dir.join("given");
    let inside = p.dir.join("wt/given");
    for folder in [&elsewhere, &inside] {
        fs::create_dir_all(folder).unwrap();
    }
    p.set(symphony_board::confine::MARKER, "1");
    let ran = p.as_(
        &orch,
        &[
            "launch",
            "sim/jump",
            "--worktree",
            &elsewhere.display().to_string(),
        ],
    );
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (
            1,
            "",
            format!(
                "✖ {}\n",
                p.outside(&format!("--worktree {} is", elsewhere.display()))
            )
        )
    );
    let ran = p.as_(&orch, &["launch", "sim/jump", "--prompt", "go"]);
    assert_eq!((ran.code, ran.out.as_str()), (1, ""));
    assert!(
        ran.err.starts_with("✖ no --prompt through a recipe"),
        "{}",
        ran.err
    );
    assert_eq!(p.calls_of("run"), Vec::<Vec<String>>::new());
    assert_eq!(p.board().task_sessions("sim/jump").unwrap(), vec![]);
    // A folder under the worktree base, named from where the command runs.
    let ran = p.as_(&orch, &["launch", "sim/jump", "--worktree", "../wt/given"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(s.worktree, inside.display().to_string());
    assert_eq!(p.sent(), format!("$work sim/jump {}", s.handle));
}

#[test]
fn through_just_tasks_a_worktree_whose_git_file_names_another_repository_has_no_worktree_base() {
    let mut p = Terminals::new("unasked-renamed");
    let orch = p.orchestrator();
    p.board()
        .create(&NewTask::new("sim/dock", "Dock", &orch))
        .unwrap();
    // The base beside the main checkout, `lamp-works-wt`: no `SYMPHONY_WT_DIR` names one.
    p.env.vars.remove("SYMPHONY_WT_DIR");
    let (tree, other) = (p.dir.join("tree"), p.dir.join("other"));
    git(
        &p.env.cwd,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "tree",
            &tree.display().to_string(),
        ],
    );
    fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    git(&other, &["commit", "-q", "--allow-empty", "-m", "x"]);
    let (theirs, ours) = (
        p.dir.join("other-wt/given"),
        p.dir.join("lamp-works-wt/given"),
    );
    for folder in [&theirs, &ours] {
        fs::create_dir_all(folder).unwrap();
    }
    p.env.cwd = tree.clone();
    p.set(symphony_board::confine::MARKER, "1");
    let listed = fs::read_to_string(tree.join(".git")).unwrap();
    // The worktree's `.git` file, untracked, rewritten to name the other repository, which does
    // not list this worktree: no base beside either main checkout, and no worktree made in it.
    fs::write(
        tree.join(".git"),
        format!("gitdir: {}\n", other.join(".git").display()),
    )
    .unwrap();
    for folder in [&theirs, &ours] {
        let given = folder.display().to_string();
        let ran = p.as_(&orch, &["launch", "sim/jump", "--worktree", &given]);
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err),
            (1, "", no_base(&format!("--worktree {given}"), &tree))
        );
    }
    let ran = p.as_(&orch, &["launch", "sim/jump"]);
    let made = p.dir.join("other-wt/sim-jump");
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (
            1,
            "",
            no_base(&format!("its worktree {}", made.display()), &tree)
        )
    );
    assert!(!made.exists());
    assert_eq!(p.calls_of("run"), Vec::<Vec<String>>::new());
    assert_eq!(p.board().task_sessions("sim/jump").unwrap(), vec![]);
    // A base `SYMPHONY_WT_DIR` names is the setting's, taken as it is.
    let mut named = p.env.clone();
    named.vars.insert(
        "SYMPHONY_WT_DIR".into(),
        p.dir.join("other-wt").display().to_string(),
    );
    named.vars.insert("TASKS_SESSION".into(), orch.clone());
    let given = theirs.display().to_string();
    let ran = run(&named, &["launch", "sim/jump", "--worktree", &given], "");
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    // Listed by its repository again, the base beside its main checkout is the worktree's.
    fs::write(tree.join(".git"), listed).unwrap();
    let given = ours.display().to_string();
    let ran = p.as_(&orch, &["launch", "sim/dock", "--worktree", &given]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/dock").unwrap()).unwrap();
    assert_eq!(s.worktree, given);
}

/// What a command run unasked in the checkout at `checkout`, which has no worktree base, says of a
/// folder it will not start an agent in: `place` names it.
fn no_base(place: &str, checkout: &Path) -> String {
    format!(
        "✖ {place} is not a folder a recipe an agent runs unasked starts an agent in: only the \
         checkout at {}, none of its git folders, and no worktree base, which is taken only beside \
         a main checkout that lists this checkout as its own, or where SYMPHONY_WT_DIR names one \
         (a submodule, a --separate-git-dir checkout or a bare repository's worktree has no main \
         checkout); the binary run by itself starts one anywhere\n",
        checkout.display()
    )
}

/// A checkout whose repository has no main checkout, its common dir being no `.git` folder.
#[derive(Clone, Copy)]
enum NoMain {
    /// `lamp-works/sub`, a submodule of `lamp-works`, its common dir `lamp-works/.git/modules/sub`.
    Submodule,
    /// `work`, which `git init --separate-git-dir store.git` made.
    SeparateGitDir,
    /// `bwt`, a linked worktree of the bare repository `bare.git`.
    BareWorktree,
}

impl Terminals {
    /// A checkout of `layout` with one commit on `main`: its folder and its repository's common dir.
    fn no_main(&self, layout: NoMain) -> (PathBuf, PathBuf) {
        let dir = &self.dir;
        match layout {
            NoMain::Submodule => {
                let lib = dir.join("lib");
                fs::create_dir_all(&lib).unwrap();
                git(&lib, &["init", "-q", "-b", "main"]);
                git(&lib, &["commit", "-q", "--allow-empty", "-m", "x"]);
                let lib = lib.display().to_string();
                let allow = "protocol.file.allow=always";
                git(
                    &self.env.cwd,
                    &["-c", allow, "submodule", "add", "-q", &lib, "sub"],
                );
                (
                    self.env.cwd.join("sub"),
                    self.env.cwd.join(".git/modules/sub"),
                )
            }
            NoMain::SeparateGitDir => {
                let (work, store) = (dir.join("work"), dir.join("store.git"));
                let (work_dir, store_dir) =
                    (work.display().to_string(), store.display().to_string());
                git(
                    dir,
                    &[
                        "init",
                        "-q",
                        "-b",
                        "main",
                        "--separate-git-dir",
                        &store_dir,
                        &work_dir,
                    ],
                );
                git(&work, &["commit", "-q", "--allow-empty", "-m", "x"]);
                (work, store)
            }
            NoMain::BareWorktree => {
                let bare = dir.join("bare.git");
                let bare_dir = bare.display().to_string();
                git(dir, &["init", "-q", "--bare", "-b", "main", &bare_dir]);
                git(&self.env.cwd, &["push", "-q", &bare_dir, "main"]);
                git(&bare, &["worktree", "add", "-q", "../bwt", "main"]);
                (dir.join("bwt"), bare)
            }
        }
    }
}

/// Through `just tasks` in a checkout of `layout`, which has no main checkout: no worktree base
/// beside it, a folder in it taken as `--worktree`, and under `SYMPHONY_WT_DIR` a worktree made of
/// the checkout's own repository, never of `lamp-works` beside it (a submodule's superproject).
fn through_just_tasks_a_checkout_without_a_main_checkout(name: &str, layout: NoMain) {
    let mut p = Terminals::new(name);
    let orch = p.orchestrator();
    p.board()
        .create(&NewTask::new("sim/dock", "Dock", &orch))
        .unwrap();
    p.env.vars.remove("SYMPHONY_WT_DIR");
    let (checkout, common) = p.no_main(layout);
    let superproject = p.env.cwd.join(".git/worktrees");
    p.env.cwd = checkout.clone();
    p.set(symphony_board::confine::MARKER, "1");
    // No repository lists the checkout back as its main checkout: no base beside it.
    let beside = checkout
        .with_file_name(format!(
            "{}-wt",
            checkout.file_name().unwrap().to_string_lossy()
        ))
        .join("sim-jump");
    let ran = p.as_(&orch, &["launch", "sim/jump"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (
            1,
            "",
            no_base(&format!("its worktree {}", beside.display()), &checkout)
        )
    );
    assert!(!beside.exists());
    assert_eq!(p.calls_of("run"), Vec::<Vec<String>>::new());
    assert_eq!(p.board().task_sessions("sim/jump").unwrap(), vec![]);
    // A folder in the checkout is the checkout's own.
    let given = checkout.join("given");
    fs::create_dir_all(&given).unwrap();
    let given = given.display().to_string();
    let ran = p.as_(&orch, &["launch", "sim/jump", "--worktree", &given]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(s.worktree, given);
    // The base `SYMPHONY_WT_DIR` names: the worktree a launch makes there is of the checkout's own
    // repository, its git run in the checkout, not in the folder holding the common dir.
    p.set("SYMPHONY_WT_DIR", &p.dir.join("wt").display().to_string());
    let ran = p.as_(&orch, &["launch", "sim/dock"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let made = p.dir.join("wt/sim-dock");
    assert_eq!(
        symphony_board::git::checkout_common_dir(&made),
        Some(common.canonicalize().unwrap())
    );
    assert!(!superproject.exists());
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/dock").unwrap()).unwrap();
    assert_eq!(s.worktree, made.display().to_string());
}

#[test]
fn through_just_tasks_a_submodule_has_a_worktree_base_only_where_symphony_wt_dir_names_one() {
    through_just_tasks_a_checkout_without_a_main_checkout("unasked-submodule", NoMain::Submodule);
}

#[test]
fn through_just_tasks_a_separate_git_dir_checkout_has_a_worktree_base_only_where_symphony_wt_dir_names_one()
 {
    through_just_tasks_a_checkout_without_a_main_checkout(
        "unasked-separate",
        NoMain::SeparateGitDir,
    );
}

#[test]
fn through_just_tasks_a_bare_repositorys_worktree_has_a_worktree_base_only_where_symphony_wt_dir_names_one()
 {
    through_just_tasks_a_checkout_without_a_main_checkout("unasked-bare", NoMain::BareWorktree);
}

#[test]
fn through_just_tasks_the_api_records_no_launch_outside_the_project_nor_a_conversation() {
    let mut p = Terminals::new("unasked-record");
    p.orchestrator();
    let worker = p
        .board()
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    let (elsewhere, inside) = (p.dir.join("elsewhere"), p.dir.join("wt/kept"));
    let record = |env: &Env, worktree: &PathBuf, conversation: &str| {
        let call = json!({"method": "record_launch", "args": {
            "slug": "sim/jump", "handle": worker, "role": "executor",
            "worktree": worktree.display().to_string(), "claude_session": conversation,
        }});
        let ran = run(env, &["api"], &call.to_string());
        assert_eq!((ran.code, ran.err.as_str()), (0, ""));
        serde_json::from_str::<Value>(&ran.out).unwrap()
    };
    p.set(symphony_board::confine::MARKER, "1");
    assert_eq!(
        record(&p.env, &elsewhere, ""),
        json!({"refused": p.outside(&format!("the record's worktree {} is", elsewhere.display()))})
    );
    assert_eq!(
        record(&p.env, &inside, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"),
        json!({"refused": "a record written through a recipe an agent runs unasked names no \
            conversation, not aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee: the board's launcher records \
            one, and a Codex thread comes from its agent's own callback; the binary run by \
            itself records any"})
    );
    assert_eq!(p.board().task_sessions("sim/jump").unwrap(), vec![]);
    // In the project, naming no conversation, it is recorded.
    assert_eq!(
        record(&p.env, &inside, "")["ok"]["worktree"],
        json!(inside.display().to_string())
    );
    // The binary run by itself records any, and a restore through `just tasks` starts nothing in
    // the folder such a record names.
    let mut unmarked = p.env.clone();
    unmarked.vars.remove(symphony_board::confine::MARKER);
    let conversation = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    assert_eq!(
        record(&unmarked, &elsewhere, conversation)["ok"]["claude_session"],
        json!(conversation)
    );
    p.board()
        .close_task_session("sim/jump", &worker, "idle", "r")
        .unwrap();
    let ran = p.run(&["restore", "sim/jump"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (
            1,
            "",
            format!(
                "✖ {}\n",
                p.outside(&format!(
                    "{worker}'s recorded worktree {} is",
                    elsewhere.display()
                ))
            )
        )
    );
    assert!(!elsewhere.exists());
    assert_eq!(p.calls_of("run"), Vec::<Vec<String>>::new());
}

#[test]
fn through_just_tasks_a_callback_gives_a_launch_record_only_its_callers_own_codex_thread() {
    let mut p = Terminals::new("unasked-thread");
    let orch = p.orchestrator();
    let inside = p.dir.join("wt/given");
    fs::create_dir_all(&inside).unwrap();
    p.set(symphony_board::confine::MARKER, "1");
    let ran = p.as_(
        &orch,
        &[
            "launch",
            "sim/jump",
            "--kind",
            "codex",
            "--worktree",
            &inside.display().to_string(),
        ],
    );
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let record = |p: &Terminals| {
        let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
        s
    };
    let handle = record(&p).handle;
    assert_eq!(record(&p).claude_session, "codex:");
    let not_own = |thread: &str| {
        format!(
            "✖ codex:{thread} is not this session's own Codex thread (CODEX_THREAD_ID, which a \
             callback with no address detects): through a recipe an agent runs unasked a callback \
             registers only its caller's, as the board takes it into the handle's launch record \
             for a restore to resume; the binary run by itself registers any\n"
        )
    };
    // A thread named that is not the caller's: refused with no thread of its own, or another.
    let ran = p.as_(&handle, &["callback", "codex:dead-beef"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (1, "", not_own("dead-beef"))
    );
    p.set("CODEX_THREAD_ID", "0abc-12");
    let ran = p.as_(&handle, &["callback", "codex:dead-beef"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (1, "", not_own("dead-beef"))
    );
    assert_eq!(record(&p).claude_session, "codex:");
    assert!(!p.board().callbacks().unwrap().contains_key(&handle));
    // Its own thread, detected or named: registered, and the record takes it.
    let ran = p.as_(&handle, &["callback"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    assert_eq!(record(&p).claude_session, "codex:0abc-12");
    let ran = p.as_(&handle, &["callback", "codex:0abc-12"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    // A restore through `just tasks` resumes that thread, in the worktree the record names.
    p.board()
        .close_task_session("sim/jump", &handle, "idle", "r")
        .unwrap();
    p.set_state({
        let mut state = p.state();
        state["terminals"] = json!([]);
        state
    });
    let ran = p.run(&["restore", "sim/jump"]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
    let [_, resumed] = <[_; 2]>::try_from(p.calls_of("run")).unwrap();
    assert_eq!(resumed[resumed.len() - 2..], ["resume", "0abc-12"]);
    assert!(
        resumed
            .windows(2)
            .any(|pair| pair[0] == "--cwd" && pair[1] == inside.display().to_string())
    );
    // The binary run by itself registers any.
    let mut unmarked = p.env.clone();
    unmarked.vars.remove(symphony_board::confine::MARKER);
    unmarked.vars.insert("TASKS_SESSION".into(), handle.clone());
    let ran = run(&unmarked, &["callback", "codex:dead-beef"], "");
    assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
}

#[test]
fn a_board_it_cannot_reach_is_exit_2() {
    let mut p = Terminals::new("unreached");
    p.set("TASKS_SERVER", crate::UNREACHED_SERVER);
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
            Command::of(TASKS)
                .args(args)
                .cleared()
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
    pub(crate) fn staffed(&self) -> String {
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
