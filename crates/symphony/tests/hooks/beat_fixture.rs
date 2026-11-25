//! `symphony hook tasks-beat`: a subagent's tool calls are its heartbeat on the task board. The
//! hook hands an event with a key, whole, to the `beat` of the board's binary
//! (`crates/tasks/src/cli/beat.rs`, tested with the board), a build it never makes, and skips a
//! full session's call whose key the bound keys' folder says is not bound. The binary here is a
//! recorder that writes down how it was run.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::common::{BIN, Done, TempDir, spawn};

/// What the hook reads from its environment: each run starts with none of it but what the test
/// sets, since this process may be an agent's.
pub(crate) const READS: [&str; 8] = [
    "TASKS_BIN",
    "TASKS_KEYS",
    "TASKS_SERVER",
    symphony_env::HOOK_ROOT.name,
    "SYMPHONY_HOOK_BIN",
    "CLAUDE_PROJECT_DIR",
    symphony_env::GIT_COMMON_DIR.name,
    CLOCK,
];

/// The beat's clock: every run here names a file in its tree that no case writes, so the beat's limit
/// never passes and each beat ends when its binary does, however long a loaded machine takes to start
/// it. A case's recorder is then never stopped before it records, and the case decides from what it
/// recorded.
pub(crate) const CLOCK: &str = symphony_env::HOOK_BEAT_CLOCK.name;

/// A hook event for one Bash call, as Claude Code sends it on stdin.
pub(crate) fn bash(command: &str, agent: Option<&str>, session: Option<&str>) -> Value {
    let mut data = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
                          "tool_input": {"command": command}, "cwd": "/somewhere"});
    if let Some(session) = session {
        data["session_id"] = Value::from(session);
    }
    if let Some(agent) = agent {
        data["agent_id"] = Value::from(agent);
    }
    data
}

/// A full session's Read call, from wherever the hook runs.
pub(crate) fn reading(session: &str) -> Value {
    json!({"hook_event_name": "PostToolUse", "tool_name": "Read", "session_id": session})
}

pub(crate) fn executable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The runs of the recorder by the name `name`: its arguments, one line a run.
pub(crate) fn runs(name: &Path) -> Vec<String> {
    let mut log = name.as_os_str().to_owned();
    log.push(".argv");
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// What the recorder by the name `name` read on its stdin, every run's after the last.
pub(crate) fn handed(name: &Path) -> String {
    let mut log = name.as_os_str().to_owned();
    log.push(".stdin");
    std::fs::read_to_string(log).unwrap_or_default()
}

/// The names in `folder`, sorted; none when it is not there.
pub(crate) fn names(folder: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    symphony_board::order::sort_by(&mut found, &Ord::cmp);
    found
}

pub(crate) fn git(cwd: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// A tree of the project's, whose hook finds the board's binary at `.target/bin/tasks`, and a recorder
/// to stand in for that binary: it writes down each run beside the name it was run by.
pub(crate) struct Tree {
    dir: TempDir,
}

/// A recorder at `path`: each run appends its arguments to `NAME.argv` and its stdin to `NAME.stdin`,
/// NAME being the name it was run by.
pub(crate) const RECORDER: &str =
    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.argv\"\ncat >> \"$0.stdin\"\n";

pub(crate) fn recorder_at(path: &Path) {
    crate::fixture_script::write(path, RECORDER, false);
}

impl Tree {
    pub(crate) fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        recorder_at(&dir.path().join("tasks-recorder"));
        Self { dir }
    }

    pub(crate) fn root(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn recorder(&self) -> PathBuf {
        self.root().join("tasks-recorder")
    }

    pub(crate) fn link(&self) -> PathBuf {
        self.root().join(".target/bin/tasks")
    }

    /// The file this tree's beats read as their clock ([`CLOCK`]), never written.
    pub(crate) fn clock(&self) -> PathBuf {
        self.root().join("beat-clock")
    }

    /// `.target/bin/tasks` linked to `to`, as `tools/rs-bin.sh` links the build a tree last ran.
    pub(crate) fn link_to(&self, to: &Path) {
        std::fs::create_dir_all(self.link().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(to, self.link()).unwrap();
    }

    /// The hook on `event`, run from this tree, with `env` set and nothing else it reads.
    pub(crate) fn beat(&self, event: &Value, env: &[(&str, &str)]) -> Done {
        self.beat_raw(&event.to_string(), env)
    }

    pub(crate) fn beat_raw(&self, raw: &str, env: &[(&str, &str)]) -> Done {
        let mut command = Command::new(BIN);
        command
            .args(["hook", "tasks-beat"])
            .current_dir(self.root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for key in READS {
            command.env_remove(key);
        }
        // A search path holding no build of the board's, unless the case names one, and a machine
        // folder of the tree's own, never the one this process inherits: no case runs a release
        // installed on this machine.
        command.env("PATH", "/usr/bin:/bin");
        command.env(
            symphony::machine::DIR_ENV,
            self.root().join("machine-folder"),
        );
        command.env(CLOCK, self.clock());
        for (key, value) in env {
            command.env(key, value);
        }
        spawn(command, raw)
    }
}

pub(crate) fn quiet(done: &Done) {
    assert_eq!(
        (done.code, done.stdout.as_str(), done.stderr.as_str()),
        (0, "", "")
    );
}

/// The main checkout's link to `build`, its build of the board's binary, as a project's build setup
/// links it: `.target/bin/tasks` in the folder holding `common`, a repository's `.git` common dir.
pub(crate) fn link_main(common: &Path, build: &Path) -> PathBuf {
    assert_eq!(
        common.file_name().unwrap(),
        ".git",
        "{common:?} is no main checkout's common dir"
    );
    let link = common.parent().unwrap().join(".target/bin/tasks");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(build, &link).unwrap();
    link
}

/// A repository at `TREE/repo` with one commit, and a worktree of it at `TREE/linked`, as git makes
/// them: the worktree's `.git` is a file naming its folder in the repository's common dir.
pub(crate) fn repo_with_worktree(tree: &Tree) -> (PathBuf, PathBuf) {
    let repo = tree.root().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ],
    );
    let linked = tree.root().join("linked");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    (repo, linked)
}
