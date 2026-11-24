//! Symphony's hook and the board's real beat, on a private board and checkout.
//! Binding, clocks, build lookup and beats all use their production entrypoints.

use crate::board::{Fixture, GONE};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use symphony_board::Health;

/// A checkout of a private repository: its main checkout, which links the board's binary at
/// `.target/bin/tasks` as a project's build setup does, or (not `linked`) a worktree of it, which
/// links none and beats on its main checkout's link.
struct Checkout {
    scratch: PathBuf,
    top: PathBuf,
}

fn git(cwd: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
}

impl Checkout {
    fn new(linked: bool) -> Self {
        let scratch = crate::scratch(if linked {
            "beat-linked"
        } else {
            "beat-unlinked"
        });
        let warm = Command::new(env!("CARGO_BIN_EXE_symphony"))
            .args(["init", "--help"])
            .output()
            .unwrap();
        assert!(warm.status.success(), "{warm:?}");
        let tasks = crate::support::tasks_binary();
        let main = scratch.join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        std::fs::create_dir_all(main.join(".target/bin")).unwrap();
        std::os::unix::fs::symlink(tasks, main.join(".target/bin/tasks")).unwrap();
        let top = if linked {
            main
        } else {
            let worktree = scratch.join("worktree");
            git(
                &main,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    worktree.to_str().unwrap(),
                ],
            );
            worktree
        };
        Self { scratch, top }
    }

    fn beat(&self, board: &Fixture, event: &Value) {
        let client = board.other_database();
        let mut child = Command::new(env!("CARGO_BIN_EXE_symphony"))
            .args(["hook", "tasks-beat"])
            .current_dir(&self.top)
            .env_clear()
            .env("CLAUDE_PROJECT_DIR", &self.top)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("TASKS_SERVER", client.addr())
            .env("TASKS_DATABASE", client.database())
            .env("TASKS_NOW", board.now().to_string())
            // A clock that never passes: the beat ends when the board's binary does, never at the
            // hook's limit on a loaded machine's clock.
            .env(
                "SYMPHONY_HOOK_BEAT_CLOCK",
                self.scratch.join("clock-never-passes"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stdout.is_empty() && output.stderr.is_empty(),
            "{output:?}"
        );
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

#[test]
fn worktree_tool_calls_keep_the_bound_child_live_with_or_without_a_tasks_pointer() {
    for linked in [false, true] {
        let checkout = Checkout::new(linked);
        let board = Fixture::empty();
        let child = board.session("worker");
        let parent = board.session("orchestrator");
        board.task("tools/example");
        board.store.claim("tools/example", &child).unwrap();
        board.task("tools/parent");
        board.store.claim("tools/parent", &parent).unwrap();
        board
            .store
            .bind_agent("parent-session", &parent, true)
            .unwrap();
        let command = format!(
            "TASKS_SESSION={child} just --justfile /main/justfile tasks show tools/example"
        );
        let binding = json!({
            "hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_input": {"command": command}, "tool_use_id": "bind",
            "session_id": "parent-session", "agent_id": "child-agent", "cwd": checkout.top
        });
        checkout.beat(&board, &binding);
        assert_eq!(
            board.store.handle_for_agent("child-agent").unwrap(),
            Some(child.clone())
        );
        let own = checkout.top.join(".target/bin/tasks");
        assert_eq!(own.exists(), linked, "a beat makes no link");

        for (tool, phase) in [
            ("Read", "PostToolUse"),
            ("Bash", "PreToolUse"),
            ("Bash", "PostToolUse"),
            ("Edit", "PostToolUse"),
        ] {
            board.advance(GONE);
            assert_eq!(board.health(&child), Health::Lost);
            let event = json!({
                "hook_event_name": phase, "tool_name": tool, "tool_input": {"command": "ls"},
                "tool_use_id": "work", "session_id": "parent-session",
                "agent_id": "child-agent", "cwd": checkout.top
            });
            checkout.beat(&board, &event);
            assert_eq!(own.exists(), linked, "a beat makes no link");
            let session = board.store.session(&child).unwrap();
            assert_eq!(
                session.row.last_seen,
                board.now(),
                "linked={linked}, {tool}, {phase}"
            );
            assert_eq!(session.health, Health::Live);
            assert_eq!(board.health(&parent), Health::Lost);
        }
    }
}
