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
            let commands = hook_commands(&settings, event);
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
        assert!(!tree.path(".git/sf-tasks/server.json").exists());
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
                    let groups = settings["hooks"][event].as_array().unwrap();
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
