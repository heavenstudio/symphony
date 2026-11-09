//! Symphony's hook and the board's real beat, on a private board and checkout.
//! Binding, clocks, build lookup and beats all use their production entrypoints.

use crate::board::{Fixture, GONE};
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use symphony_board::Health;

struct Checkout(PathBuf);

impl Checkout {
    fn new(linked: bool) -> Self {
        let root = crate::scratch(if linked {
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
        let init = Command::new("git")
            .args(["init", "-q"])
            .arg(&root)
            .status()
            .unwrap();
        assert!(init.success());
        std::fs::create_dir_all(root.join(".target/bin")).unwrap();
        std::os::unix::fs::symlink(
            env!("CARGO_BIN_EXE_symphony"),
            root.join(".target/bin/symphony"),
        )
        .unwrap();
        if linked {
            std::os::unix::fs::symlink(tasks, root.join(".target/bin/tasks")).unwrap();
        }
        let records = root.join(".git/sf-cargo/tasks-main");
        std::fs::create_dir_all(&records).unwrap();
        std::fs::write(records.join("current"), format!("{tasks}\n")).unwrap();
        Self(root)
    }

    fn beat(&self, board: &Fixture, event: &Value) {
        let client = board.other_client();
        let mut child = Command::new(self.0.join(".target/bin/symphony"))
            .args(["hook", "tasks-beat"])
            .current_dir(&self.0)
            .env_clear()
            .env("CLAUDE_PROJECT_DIR", &self.0)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("TASKS_SERVER", client.addr())
            .env("TASKS_DATABASE", client.database())
            .env("TASKS_NOW", board.now().to_string())
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
        let _ = std::fs::remove_dir_all(&self.0);
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
            "session_id": "parent-session", "agent_id": "child-agent", "cwd": checkout.0
        });
        checkout.beat(&board, &binding);
        assert_eq!(
            board.store.handle_for_agent("child-agent").unwrap(),
            Some(child.clone())
        );
        assert!(checkout.0.join(".target/bin/tasks").exists());

        for (tool, phase) in [
            ("Read", "PostToolUse"),
            ("Bash", "PreToolUse"),
            ("Bash", "PostToolUse"),
            ("Edit", "PostToolUse"),
        ] {
            if !linked {
                std::fs::remove_file(checkout.0.join(".target/bin/tasks")).unwrap();
            }
            board.advance(GONE);
            assert_eq!(board.health(&child), Health::Lost);
            let event = json!({
                "hook_event_name": phase, "tool_name": tool, "tool_input": {"command": "ls"},
                "tool_use_id": "work", "session_id": "parent-session",
                "agent_id": "child-agent", "cwd": checkout.0
            });
            checkout.beat(&board, &event);
            assert!(checkout.0.join(".target/bin/tasks").exists());
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
