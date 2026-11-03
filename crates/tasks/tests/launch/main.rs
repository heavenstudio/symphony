//! Agent launch lifecycle on private board and pane fixtures.

#[path = "../store/common.rs"]
#[allow(dead_code)]
mod common;

mod cleanup;
mod finish;
mod herdr;
mod launch;
mod procs;
mod reap;
mod restore;

use common::Fixture;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tasks::cli::Env;
use tasks::launch::herdr::{Binary, Herdr};
use tasks::launch::{Ask, LaunchError, Machine, Started};
use tasks::sys::Command;
use tasks::{Error, NewTask, Store};

pub const FAKE: &str = env!("CARGO_BIN_EXE_fake-herdr");

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
    let done = tasks::sys::output(
        Command::new("git")
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

/// The fake herdr with hooks: `instead` may answer a call in its place (the call is then not in the
/// fake's log), and `after` runs after each call the fake answered.
pub struct Hooked<'a> {
    pub inner: Binary,
    pub instead: Instead<'a>,
    pub after: After<'a>,
}

impl<'a> Hooked<'a> {
    pub fn new(world: &World) -> Hooked<'a> {
        Hooked {
            inner: world.herdr(),
            instead: Box::new(|_| None),
            after: Box::new(|_| {}),
        }
    }
}

impl Herdr for Hooked<'_> {
    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        if let Some(answer) = (self.instead)(args) {
            return answer;
        }
        let answer = self.inner.run(args);
        (self.after)(args);
        answer
    }

    fn workspace(&self) -> Option<String> {
        self.inner.workspace()
    }
}

/// A board, a repository with `main` and the fake herdr's state, with an orchestrator and a task
/// `sim/jump` the owner made.
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
        let repo = folder.0.join("star-foundry");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let wt = folder.0.join("star-foundry-wt");
        let f = Fixture::empty();
        let orch = f.session("orchestrator");
        f.task("sim/jump");
        let mut env = Env {
            vars: Default::default(),
            cwd: folder.0.clone(),
        };
        for (key, value) in [
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("HERDR_BIN", FAKE.to_string()),
            (
                "FAKE_HERDR_STATE",
                folder.0.join("herdr.json").display().to_string(),
            ),
            (
                "FAKE_HERDR_LOG",
                folder.0.join("herdr.log").display().to_string(),
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
        world.set_state(json!({"tabs": [], "panes": [], "next": 100}));
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

    pub fn herdr(&self) -> Binary {
        Binary::of(&self.env)
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
        }
    }

    pub fn ask(&self, slug: &str) -> Ask {
        Ask::new(slug, &self.orch, "sonnet")
    }

    pub fn try_launch(&self, ask: &Ask) -> Result<Started, Error> {
        tasks::launch::launch(self.store(), &self.herdr(), &self.machine(), ask)
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

    // -- the fake herdr's state and log ---------------------------------------------------

    pub fn state(&self) -> Value {
        let text = fs::read_to_string(&self.env.vars["FAKE_HERDR_STATE"]).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    pub fn set_state(&self, state: Value) {
        fs::write(&self.env.vars["FAKE_HERDR_STATE"], state.to_string()).unwrap();
    }

    /// Change the state's panes.
    pub fn edit_panes(&self, edit: impl FnOnce(&mut Vec<Value>)) {
        let mut state = self.state();
        edit(state["panes"].as_array_mut().unwrap());
        self.set_state(state);
    }

    /// Change one pane, by id.
    pub fn edit_pane(&self, id: &str, edit: impl FnOnce(&mut serde_json::Map<String, Value>)) {
        self.edit_panes(|panes| {
            let pane = panes
                .iter_mut()
                .find(|p| p["pane_id"] == id)
                .expect("the pane is there");
            edit(pane.as_object_mut().unwrap());
        });
    }

    pub fn pane_ids(&self) -> Vec<String> {
        self.state()["panes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["pane_id"].as_str().unwrap().to_string())
            .collect()
    }

    pub fn has_pane(&self, id: &str) -> bool {
        self.pane_ids().iter().any(|p| p == id)
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        let Ok(text) = fs::read_to_string(&self.env.vars["FAKE_HERDR_LOG"]) else {
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

    pub fn add_tab(&self, tab: &str, label: &str, panes: usize) {
        let mut state = self.state();
        state["tabs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tab_id": tab, "label": label, "workspace_id": "w1"}));
        for n in 0..panes {
            state["panes"].as_array_mut().unwrap().push(json!({
                "pane_id": format!("{tab}-p{n}"), "tab_id": tab, "cwd": "/", "agent_status": "unknown",
            }));
        }
        self.set_state(state);
    }

    /// A pane of its own, an agent's (`claude -n HANDLE` when a handle is given) unless `agent` is
    /// false, idle, with a shell whose pid the ps table knows: its id.
    pub fn add_pane(&self, agent: bool, handle: Option<&str>, extra: Value) -> String {
        let pid = self.next_pid.get() + 1;
        self.next_pid.set(pid);
        let id = format!("w1:p{pid}");
        let mut pane = json!({
            "pane_id": id, "tab_id": "w1:t1", "cwd": "/somewhere", "agent_status": "idle",
            "shell_pid": pid,
        });
        if agent {
            let cmdline = match handle {
                Some(handle) => format!("claude --model sonnet -n {handle}"),
                None => "claude".into(),
            };
            pane["agent"] = json!("claude");
            pane["agent_session"] =
                json!({"agent": "claude", "kind": "id", "value": format!("sess-{id}")});
            pane["foreground"] = json!([{"cmdline": cmdline, "argv0": "claude"}]);
        }
        for (key, value) in extra.as_object().into_iter().flatten() {
            pane[key] = value.clone();
        }
        let mut state = self.state();
        if state["tabs"].as_array().unwrap().is_empty() {
            state["tabs"] = json!([{"tab_id": "w1:t1", "label": "x", "workspace_id": "w1"}]);
        }
        state["panes"].as_array_mut().unwrap().push(pane);
        self.set_state(state);
        self.procs.borrow_mut().insert(pid, Vec::new());
        id
    }

    /// A shell pid of its own for a pane, with nothing under it yet.
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

    pub fn shell_pid(&self, pane: &str) -> i64 {
        self.state()["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["pane_id"] == pane)
            .and_then(|p| p["shell_pid"].as_i64())
            .unwrap()
    }
}
