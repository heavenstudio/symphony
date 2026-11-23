//! Agent launch lifecycle on private board and terminal fixtures: hc and the Herdr app are the
//! crate's `fake-hc`.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

#[path = "../store/common.rs"]
#[allow(dead_code)]
mod common;

mod attempts;
mod cleanup;
mod cleanup_kept;
mod close;
mod close_fences;
mod close_holds;
mod close_races;
mod feature_branch;
mod finish;
mod fixtures;
mod go;
mod hc;
mod last_look;
mod launch;
mod off;
mod procs;
mod reap;
mod reap_unread;
mod restore;
mod review_resume;
mod screen;
mod thread;

use crate::children::Child as _;
use common::Fixture;
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use symphony_board::cli::Env;
use symphony_board::launch::hc::{Binary, ENDING_VAR, Hc};
use symphony_board::launch::{Ask, Bounds, LaunchError, Machine, Started};
use symphony_board::sys::Command;
use symphony_board::{Error, NewTask, Store};

pub const FAKE: &str = env!("CARGO_BIN_EXE_fake-hc");

static FOLDERS: AtomicUsize = AtomicUsize::new(0);
/// A folder of its own in the temp dir, outside any repository, emptied first and removed after.
pub struct Folder(pub PathBuf);

impl Folder {
    pub fn new() -> Folder {
        let n = FOLDERS.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tasks-launch-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Folder(dir.canonicalize().unwrap())
    }
}

impl Default for Folder {
    fn default() -> Folder {
        Folder::new()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `git ARGS` in `dir`, which must succeed: its output, trimmed.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let done = symphony_board::sys::output(
        Command::of("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
            .arg(dir)
            .args(args),
    )
    .unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}

/// What a hook answers in place of the fake for a call, if anything.
pub type Instead<'a> = Box<dyn Fn(&[&str]) -> Option<Result<String, LaunchError>> + 'a>;

/// What a hook does after a call the fake answered.
pub type After<'a> = Box<dyn Fn(&[&str]) + 'a>;

/// The fake hc with hooks: `instead` may answer an hc call in its place (the call is then not in the
/// fake's log), and `after` runs after each hc call the fake answered. The window's calls go to the
/// fake as they are.
pub struct Hooked<'a> {
    pub inner: Binary,
    pub instead: Instead<'a>,
    pub after: After<'a>,
}

impl<'a> Hooked<'a> {
    pub fn new(world: &World) -> Hooked<'a> {
        Hooked {
            inner: world.hc(),
            instead: Box::new(|_| None),
            after: Box::new(|_| {}),
        }
    }
}

impl Hc for Hooked<'_> {
    fn queue_codex(&self, thread: &str, text: &str) -> Result<(), LaunchError> {
        self.inner.queue_codex(thread, text)
    }

    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        if let Some(answer) = (self.instead)(args) {
            return answer;
        }
        let answer = self.inner.run(args);
        (self.after)(args);
        answer
    }

    fn feed(&self, args: &[&str], input: &str) -> Result<String, LaunchError> {
        self.inner.feed(args, input)
    }

    fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
        self.inner.app(args)
    }

    fn caller(&self) -> Option<String> {
        self.inner.caller()
    }

    fn board(&self) -> String {
        self.inner.board()
    }

    fn project(&self) -> String {
        self.inner.project()
    }

    fn ending(&self) -> std::time::Duration {
        self.inner.ending()
    }
}

/// How long a world's hc gives a killed terminal to end: the backstop of hc's own ten seconds,
/// so a loaded machine's slow looks never run it out before the terminal ends by its count of them.
const ENDING: std::time::Duration = test_time::backstop(std::time::Duration::from_secs(10));

/// The folder of a world's agent programs, the search path its launches look for them on.
pub const PROGRAMS: &str = "agents-bin";

/// `folder` made, holding a runnable stand-in for each program of `names`: what a launch finds on
/// its search path (no fake hc runs it).
pub fn programs(folder: &Path, names: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(folder).unwrap();
    for name in names {
        let program = folder.join(name);
        fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The id of the project's space in a new world's Herdr app.
pub const SPACE: &str = "sp-lamp";

/// A board, a repository `lamp-works` with `main` and the fake hc's state (the Herdr app showing
/// the space `lamp-works`, with no tabs), with an orchestrator and a task `sim/jump` the owner made.
pub struct World {
    pub folder: Folder,
    pub repo: PathBuf,
    pub wt: PathBuf,
    pub f: Fixture,
    pub orch: String,
    pub env: Env,
    /// pid -> the command lines under it, as `ps` would say.
    pub procs: Rc<RefCell<HashMap<i64, Vec<String>>>>,
    /// Each wait the launcher asked for.
    pub sleeps: Rc<RefCell<Vec<f64>>>,
    ids: Rc<Cell<u32>>,
    next_pid: Cell<i64>,
}

impl Default for World {
    fn default() -> World {
        World::new()
    }
}

impl World {
    pub fn new() -> World {
        let folder = Folder::new();
        let repo = folder.0.join("lamp-works");
        fixtures::repository(&repo);
        // The agents' programs a launch looks for on its search path ([`World::machine`]).
        programs(&folder.0.join(PROGRAMS), &["claude", "codex"]);
        let wt = folder.0.join("lamp-works-wt");
        let (f, orch) = fixtures::board();
        let mut env = Env {
            vars: Default::default(),
            cwd: repo.clone(),
        };
        for (key, value) in [
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("HC_BIN", FAKE.to_string()),
            ("HERDR_APP_BIN", format!("'{FAKE}' app")),
            (
                "FAKE_HC_STATE",
                folder.0.join("hc.json").display().to_string(),
            ),
            ("FAKE_HC_LOG", folder.0.join("hc.log").display().to_string()),
            (ENDING_VAR, ENDING.as_secs().to_string()),
            (
                symphony_board::UNANSWERED_WAIT_VAR,
                common::LOOKUP_BACKSTOP.to_string(),
            ),
        ] {
            env.vars.insert(key.into(), value);
        }
        let world = World {
            folder,
            repo,
            wt,
            f,
            orch,
            env,
            procs: Rc::default(),
            sleeps: Rc::default(),
            ids: Rc::default(),
            next_pid: Cell::new(5000),
        };
        world.set_state(json!({
            "terminals": [],
            "layout": {"spaces": [{"id": SPACE, "name": "lamp-works", "tabs": []}]},
        }));
        world
    }

    pub fn store(&self) -> &Store {
        &self.f.store
    }

    pub fn dir(&self) -> &Path {
        &self.folder.0
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.env.vars.insert(key.into(), value.into());
    }

    pub fn hc(&self) -> Binary {
        Binary::of(&self.env)
    }

    /// The `symphony.board` label of this world's terminals.
    pub fn board(&self) -> String {
        self.hc().board()
    }

    /// The launcher's machine: this world's repository, worktree folder, ps table and waits, and the
    /// session ids `00000001-…`, `00000002-…` in turn.
    pub fn machine(&self) -> Machine {
        let (ids, procs, sleeps) = (
            Rc::clone(&self.ids),
            Rc::clone(&self.procs),
            Rc::clone(&self.sleeps),
        );
        Machine {
            root: self.repo.clone(),
            wt_base: self.wt.clone(),
            new_id: Box::new(move || {
                ids.set(ids.get() + 1);
                format!("{:08}-0000-4000-8000-000000000000", ids.get())
            }),
            sleep: Box::new(move |s| sleeps.borrow_mut().push(s)),
            processes: Box::new(move |pid| {
                Ok(procs.borrow().get(&pid).cloned().unwrap_or_default())
            }),
            mode: "auto".into(),
            board_env: Vec::new(),
            bounds: None,
            path: Some(self.folder.0.join(PROGRAMS).into_os_string()),
        }
    }

    /// The launcher's machine for a command run in the repository through a recipe an agent runs
    /// unasked (`just tasks`, which sets `SYMPHONY_UNASKED`): the agents it starts stay in the
    /// project ([`Bounds`]).
    pub fn unasked(&self) -> Machine {
        self.unasked_in(&self.repo)
    }

    /// [`World::unasked`] for a command run in `cwd`.
    pub fn unasked_in(&self, cwd: &Path) -> Machine {
        let mut env = self.env.clone();
        env.cwd = cwd.to_path_buf();
        env.vars
            .insert(symphony_board::confine::MARKER.into(), "1".into());
        Machine {
            bounds: Bounds::of(&env, &self.repo, &self.wt),
            ..self.machine()
        }
    }

    /// What a launch or a restore run unasked in the repository says of a folder it will not start
    /// an agent in: `place` names it (`--worktree X is`, `… leads to Y, which is`).
    pub fn outside(&self, place: &str) -> String {
        format!(
            "{place} not a folder a recipe an agent runs unasked starts an agent in: only the \
             checkout at {}, none of its git folders, or a folder under the project's worktree \
             base {}; the binary run by itself starts one anywhere",
            self.repo.display(),
            self.wt.display()
        )
    }

    pub fn ask(&self, slug: &str) -> Ask {
        Ask::new(slug, &self.orch, "sonnet")
    }

    pub fn try_launch(&self, ask: &Ask) -> Result<Started, Error> {
        symphony_board::launch::launch(self.store(), &self.hc(), &self.machine(), ask)
    }

    /// An executor launched on `slug`.
    pub fn launch(&self, slug: &str) -> Started {
        self.try_launch(&self.ask(slug)).expect("the launch goes")
    }

    /// A reviewer launched on `slug`.
    pub fn review(&self, slug: &str) -> Started {
        let ask = Ask {
            role: "reviewer".into(),
            kind: Some("reviewer".into()),
            ..self.ask(slug)
        };
        self.try_launch(&ask).expect("the launch goes")
    }

    pub fn task(&self, slug: &str, parent: Option<&str>) {
        self.store()
            .create(&NewTask {
                parent: parent.map(str::to_string),
                instruction: parent.is_some(),
                ..NewTask::new(slug, "Jump", "owner")
            })
            .unwrap();
    }

    // -- the fake hc's state and log ------------------------------------------------------

    pub fn state(&self) -> Value {
        let text = fs::read_to_string(&self.env.vars["FAKE_HC_STATE"]).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    pub fn set_state(&self, state: Value) {
        fs::write(&self.env.vars["FAKE_HC_STATE"], state.to_string()).unwrap();
    }

    /// Change the state's terminals.
    pub fn edit_terminals(&self, edit: impl FnOnce(&mut Vec<Value>)) {
        let mut state = self.state();
        edit(state["terminals"].as_array_mut().unwrap());
        self.set_state(state);
    }

    /// Change one terminal, by address.
    pub fn edit_terminal(&self, address: &str, edit: impl FnOnce(&mut Map<String, Value>)) {
        self.edit_terminals(|all| {
            let terminal = all
                .iter_mut()
                .find(|t| t["address"] == address)
                .expect("the terminal is there");
            edit(terminal.as_object_mut().unwrap());
        });
    }

    /// One terminal, by address, as the state holds it.
    pub fn terminal(&self, address: &str) -> Value {
        self.state()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["address"] == address)
            .cloned()
            .expect("the terminal is there")
    }

    /// Every terminal hc knows, live or finished.
    pub fn addresses(&self) -> Vec<String> {
        self.state()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["address"].as_str().unwrap().to_string())
            .collect()
    }

    /// The live terminals.
    pub fn live(&self) -> Vec<String> {
        self.state()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["state"] == "live")
            .map(|t| t["address"].as_str().unwrap().to_string())
            .collect()
    }

    pub fn has_terminal(&self, address: &str) -> bool {
        self.addresses().iter().any(|a| a == address)
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        let Ok(text) = fs::read_to_string(&self.env.vars["FAKE_HC_LOG"]) else {
            return Vec::new();
        };
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// The calls that start with `words`.
    pub fn calls_of(&self, words: &[&str]) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|c| c.len() >= words.len() && c[..words.len()] == *words)
            .collect()
    }

    /// The calls that would change a terminal or the window: what a test of a read-only path
    /// expects none of.
    pub fn changes(&self) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|c| {
                ["run", "kill", "rm", "keys", "send", "do"]
                    .iter()
                    .any(|word| c.iter().take(2).any(|w| w == word))
            })
            .collect()
    }

    /// A tab `name` in the project's space with `terminals` live shells in it: its id.
    pub fn add_tab(&self, name: &str, terminals: usize) -> String {
        let mut state = self.state();
        let id = format!("tb-{name}");
        state["layout"]["spaces"][0]["tabs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": id, "name": name}));
        self.set_state(state);
        for _ in 0..terminals {
            self.add_terminal(
                false,
                None,
                json!({"labels": {"herdr.space": SPACE, "herdr.tab": id}}),
            );
        }
        id
    }

    /// A live terminal of this board, an idle Claude agent's (`claude -n HANDLE` when a handle is
    /// given, labelled with it) unless `agent` is false (a shell), whose pid the ps table knows
    /// with nothing under it; `extra` replaces its fields: its address.
    pub fn add_terminal(&self, agent: bool, handle: Option<&str>, extra: Value) -> String {
        let pid = self.new_pid();
        let address = format!("t{pid}");
        let mut labels = json!({"symphony.board": self.board()});
        let mut terminal = json!({
            "address": address, "cwd": "/somewhere", "name": handle, "pid": pid, "state": "live",
            "status": null, "turn": 0, "agent": "zsh", "argv": ["zsh"],
        });
        if agent {
            let mut argv = vec!["env".to_string(), "TASKS_SESSION=x".into(), "claude".into()];
            argv.extend(["--session-id".into(), format!("sess-{address}")]);
            if let Some(handle) = handle {
                argv.extend(["-n".into(), handle.to_string()]);
                labels["symphony.handle"] = json!(handle);
            }
            argv.push("/work sim/jump x".into());
            terminal["agent"] = json!("claude-code");
            terminal["status"] = json!("idle");
            terminal["argv"] = json!(argv);
        }
        terminal["labels"] = labels;
        for (key, value) in extra.as_object().into_iter().flatten() {
            if key == "labels" {
                for (k, v) in value.as_object().into_iter().flatten() {
                    terminal["labels"][k] = v.clone();
                }
            } else {
                terminal[key] = value.clone();
            }
        }
        self.edit_terminals(|all| all.push(terminal));
        address
    }

    /// A pid of its own, with nothing under it yet.
    pub fn new_pid(&self) -> i64 {
        let pid = self.next_pid.get() + 1;
        self.next_pid.set(pid);
        self.procs.borrow_mut().insert(pid, Vec::new());
        pid
    }

    pub fn run_under(&self, pid: i64, commands: &[&str]) {
        self.procs
            .borrow_mut()
            .insert(pid, commands.iter().map(|c| c.to_string()).collect());
    }

    pub fn pid_of(&self, address: &str) -> i64 {
        self.terminal(address)["pid"].as_i64().unwrap()
    }

    /// The program a launch started in its terminal: its argv past `env` and its settings.
    pub fn program(&self, address: &str) -> Vec<String> {
        let argv: Vec<String> = self.terminal(address)["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        symphony_board::launch::hc::program(&argv).to_vec()
    }

    /// Every message `hc send` gave the terminal, in order.
    pub fn sent(&self, address: &str) -> Vec<String> {
        let sent = self.terminal(address)["sent"].clone();
        serde_json::from_value(sent).unwrap_or_default()
    }

    /// The name of the Herdr app tab holding `tab`, and its space's.
    pub fn tab_name(&self, tab: &str) -> (String, String) {
        let state = self.state();
        for space in state["layout"]["spaces"].as_array().unwrap() {
            for t in space["tabs"].as_array().unwrap() {
                if t["id"] == tab {
                    return (
                        space["name"].as_str().unwrap().to_string(),
                        t["name"].as_str().unwrap().to_string(),
                    );
                }
            }
        }
        panic!("no tab {tab}")
    }
}
