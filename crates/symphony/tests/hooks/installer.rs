//! The installed commands' ownership and whole-event forwarding through the real CLI.

use super::{common::*, support::Tree};
use serde_json::json;
use std::process::{Command, Stdio};

#[test]
fn installed_commands_forward_whole_events_and_skip_missing_builds_without_a_lookup() {
    let tree = Tree::new();
    let mut init = Command::new(BIN);
    init.args(["init", "--root"])
        .arg(&tree.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let done = spawn(init, "");
    assert_eq!(done.code, 0, "{done:?}");
    let settings: serde_json::Value =
        serde_json::from_str(&tree.read(".claude/settings.json")).unwrap();
    let recorder = tree.path("recorder");
    crate::fixture_script::write(
        &recorder,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.argv\"\ncat >> \"$0.stdin\"\n",
        false,
    );
    let lookup = tree.write(
        "tools/rs-bin.sh",
        "#!/bin/sh\necho called > \"$0.called\"\nexit 99\n",
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&lookup, std::fs::Permissions::from_mode(0o755)).unwrap();
    let raw = format!(
        "  {}\n",
        json!({"agent_id":"child", "cwd":tree.root,
        "tool_input":{"command":"just tasks inbox"}, "padding":"x".repeat(1 << 20)})
    );
    for linked in [false, true] {
        if linked {
            std::fs::create_dir_all(tree.path(".target/bin")).unwrap();
            std::os::unix::fs::symlink(BIN, tree.path(".target/bin/symphony")).unwrap();
        }
        for event in ["PreToolUse", "PostToolUse"] {
            let mut commands = hook_commands(&settings, event);
            commands.retain(|(_, command)| command.contains("tasks-beat"));
            assert_eq!(commands.len(), 1);
            let mut shell = Command::new("sh");
            shell
                .args(["-c", &commands[0].1])
                .current_dir(&tree.root)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("CLAUDE_PROJECT_DIR", &tree.root)
                .env("TASKS_BIN", &recorder)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            retain_coverage_env(&mut shell);
            let done = spawn(shell, &raw);
            assert_eq!(
                (done.code, done.stdout.as_str(), done.stderr.as_str()),
                (0, "", "")
            );
        }
        assert_eq!(
            std::fs::read_to_string(tree.path("recorder.argv")).unwrap_or_default(),
            if linked { "beat\nbeat\n" } else { "" }
        );
        assert_eq!(
            std::fs::read_to_string(tree.path("recorder.stdin")).unwrap_or_default(),
            if linked { raw.repeat(2) } else { String::new() }
        );
        assert!(!tree.path("tools/rs-bin.sh.called").exists());
        assert!(!tree.path(".git/symphony/tasks/server.json").exists());
    }
}

#[test]
fn real_init_preserves_unstamped_hooks_and_updates_stamped_paths_and_quoting() {
    let tree = Tree::new();
    for command in [
        "\"$CLAUDE_PROJECT_DIR/.target/bin/symphony\" hook tasks-beat",
        "'/old path/symphony' hook tasks-beat",
        "/old/symphony hook tasks-beat",
    ] {
        for stamped in [false, true] {
            let command = if stamped {
                format!("{command} # symphony-stamp: heartbeat-v0")
            } else {
                command.to_string()
            };
            let own = json!({"matcher":"Read", "hooks":[{
                "type":"command", "command":command, "async":true, "timeout":4
            }]});
            tree.write(
                ".claude/settings.json",
                &json!({"hooks":{
                    "PreToolUse":[own.clone()], "PostToolUse":[own.clone()]
                }})
                .to_string(),
            );
            for _ in 0..2 {
                let done = Command::new(BIN)
                    .args(["init", "--root"])
                    .arg(&tree.root)
                    .output()
                    .unwrap();
                assert!(done.status.success(), "{done:?}");
                let settings: serde_json::Value =
                    serde_json::from_str(&tree.read(".claude/settings.json")).unwrap();
                for event in ["PreToolUse", "PostToolUse"] {
                    let mut groups = settings["hooks"][event].as_array().unwrap().clone();
                    // The memory guard's group sits beside the heartbeat's before a tool call.
                    groups.retain(|group| !group.to_string().contains("memory-guard"));
                    assert_eq!(groups.len(), 1, "{event}: {groups:?}");
                    if stamped {
                        assert_eq!(
                            groups[0]["matcher"],
                            if event == "PreToolUse" { "Bash" } else { "" }
                        );
                        let hook = &groups[0]["hooks"][0];
                        assert_eq!(hook["timeout"], 10);
                        assert_eq!(hook["async"], serde_json::Value::Null);
                        assert!(
                            hook["command"]
                                .as_str()
                                .unwrap()
                                .ends_with("# symphony-stamp: heartbeat-v1"),
                            "{hook}"
                        );
                        assert!(
                            !hook["command"].as_str().unwrap().contains("/old"),
                            "{hook}"
                        );
                    } else {
                        assert_eq!(groups[0], own, "unstamped override: {command}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_installed_guard_passes_on_its_own_refusal_alone() {
    let tree = Tree::new();
    let done = Command::new(BIN)
        .args(["init", "--root"])
        .arg(&tree.root)
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
    let settings: serde_json::Value =
        serde_json::from_str(&tree.read(".claude/settings.json")).unwrap();
    let mut guard = hook_commands(&settings, "PreToolUse");
    guard.retain(|(_, command)| command.contains("memory-guard"));
    assert_eq!(guard.len(), 1, "{guard:?}");
    let (matcher, guard) = &guard[0];
    assert_eq!(matcher, "Write|Edit|MultiEdit|NotebookEdit|Bash");

    let memory = "/Users/someone/.claude/projects/-Users-someone-repo/memory";
    let bash = |command: String| {
        json!({"tool_name": "Bash", "tool_input": {"command": command}, "cwd": tree.root})
            .to_string()
    };
    let write = bash(format!("echo rule >> {memory}/MEMORY.md"));
    let read = bash(format!("cat {memory}/MEMORY.md"));
    let piped = || Stdio::piped();
    let installed = |raw: &str| {
        let mut shell = Command::new("sh");
        shell
            .args(["-c", guard])
            .current_dir(&tree.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("CLAUDE_PROJECT_DIR", &tree.root)
            .stdin(piped())
            .stdout(piped())
            .stderr(piped());
        retain_coverage_env(&mut shell);
        let done = spawn(shell, raw);
        (done.code, done.stdout, done.stderr)
    };
    let quiet = (0, String::new(), String::new());
    let link = tree.path(".target/bin/symphony");
    let link_to = |binary: &std::path::Path| {
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(binary, &link).unwrap();
    };

    // No binary linked: the call goes through, quietly.
    assert_eq!(installed(&write), quiet);
    // A binary built before the guard refuses the hook's name with its usage, exit 2 as well: the
    // call still goes through.
    let older = tree.path("older-symphony");
    crate::fixture_script::write(
        &older,
        "#!/bin/sh\necho 'usage: symphony hook tasks-beat' >&2\nexit 2\n",
        false,
    );
    link_to(&older);
    assert_eq!(installed(&write), quiet);
    // The guard's own refusal is passed on whole: its exit and its one line.
    link_to(std::path::Path::new(BIN));
    let mut direct = Command::new(BIN);
    direct
        .args(["hook", "memory-guard"])
        .stdin(piped())
        .stdout(piped())
        .stderr(piped());
    retain_coverage_env(&mut direct);
    let direct = spawn(direct, &write);
    assert_eq!(direct.code, 2, "{direct:?}");
    assert!(direct.stderr.starts_with("Blocked: "), "{direct:?}");
    assert_eq!(direct.stderr.lines().count(), 1, "{direct:?}");
    assert_eq!(installed(&write), (2, String::new(), direct.stderr));
    assert_eq!(installed(&read), quiet);
}
