//! What runs under a pane, read: `ps`'s table walked, and each pattern as Python's `re` reads it,
//! matched by hand (the protected work, `harmless`, `shell`, `helper` and `board`).

use symphony_board::launch::procs::{
    board, busy_reason, harmless, helper, land, named, reap_busy_reason, shell, symphony_app,
    test_run, under, verify,
};

fn all(commands: &[&str]) -> Vec<String> {
    commands.iter().map(|c| c.to_string()).collect()
}

#[test]
fn the_table_is_walked_from_the_pid_down_each_child_before_its_own() {
    let table = "  PID  PPID COMMAND\n 10 1 zsh\n 11 10 claude -n x\n 12 11 node mcp\n 13 10 tail -f   log  \n 14 13 cat\n 15 1 other\nbad line\n 16 x y\n";
    assert_eq!(
        under(table, 10),
        ["claude -n x", "tail -f   log  ", "cat", "node mcp"]
    );
    assert_eq!(under(table, 14), Vec::<String>::new());
    assert_eq!(under(" 20 1\n 21 20 \n", 20), Vec::<String>::new());
}

#[test]
fn a_verify_is_a_word_or_a_path_ending_in_one() {
    for yes in [
        "just verify",
        "verify",
        "tools/verify -x",
        "just verify-full",
        "a\tverify\n",
        "x/verify-full",
    ] {
        assert!(verify(yes), "{yes:?}");
    }
    for no in [
        "just verifying",
        "verify-fullx",
        "averify",
        "verify_full",
        "just verify-ful",
    ] {
        assert!(!verify(no), "{no:?}");
    }
}

#[test]
fn a_land_is_a_word_or_land_py() {
    for yes in [
        "just land tools/x",
        "python3 tools/land.py x",
        "land",
        "x/land.py",
    ] {
        assert!(land(yes), "{yes:?}");
    }
    for no in [
        "landing",
        "island",
        "land.pyc",
        "just landx",
        "land_test.py",
    ] {
        assert!(!land(no), "{no:?}");
    }
}

#[test]
fn a_test_run_is_named_by_its_runner_or_godot() {
    for yes in [
        "python3 tools/run_sim_tests.py",
        "run_tool_suites",
        "just test-sim SUITE=a",
        "test_render",
        "just test-tools",
        "godot --headless -s tests/run_sim_tests.gd",
        "/Applications/Godot.app/godot",
        "x tests/run_y",
        "godot",
    ] {
        assert!(test_run(yes), "{yes:?}");
    }
    for no in ["godotx", "xgodot", "go_dot", "_godot", "tests/runner"] {
        assert!(!test_run(no), "{no:?}");
    }
    // `test[-_]sim` has no end of its own
    assert!(test_run("test-simple"));
    assert!(test_run("godot-4"));
    assert!(test_run("élan godot."));
    assert!(!test_run("égodot"));
}

#[test]
fn the_symphony_is_its_recipe_or_native_server() {
    for yes in [
        "just symphony",
        "just symphony --port 1",
        "/usr/local/bin/symphony-server serve",
    ] {
        assert!(symphony_app(yes), "{yes:?}");
    }
    for no in ["just symphony-watch", "just  symphony", "Symphony server"] {
        assert!(!symphony_app(no), "{no:?}");
    }
}

#[test]
fn the_agent_and_its_helper_servers_are_harmless_whatever_their_case() {
    for yes in [
        "claude",
        "claude --model x",
        "/usr/local/bin/Claude -n y",
        "node /x/mcp-server.js",
        "npx @modelcontextprotocol/MCP",
        "uvx foo-mcp",
        "x_mcp.y",
        "mcp",
        "a mcp\tb",
    ] {
        assert!(harmless(yes), "{yes:?}");
    }
    for no in [
        "claudex",
        "xclaude",
        " claude",
        "node mcpserver",
        "amcp-x",
        "tail -f log",
        "my/claude-code",
    ] {
        assert!(!harmless(no), "{no:?}");
    }
}

#[test]
fn a_shell_is_named_by_its_first_word() {
    for yes in [
        "zsh",
        "/bin/zsh -c sleep 5",
        "bash x",
        "sh",
        "/usr/bin/dash",
        "fish -l",
    ] {
        assert!(shell(yes), "{yes:?}");
    }
    for no in ["zshx", "ksh", "tail -f bash", "flash", "/bin/sh-x"] {
        assert!(!shell(no), "{no:?}");
    }
}

#[test]
fn a_helper_and_a_board_command_are_named_as_they_run() {
    assert!(helper("/usr/bin/caffeinate -i -t 300"));
    assert!(helper("caffeinate"));
    assert!(!helper("caffeinated"));
    for yes in [
        "just tasks inbox",
        "just  tasks",
        "/x/just tasks-attach --kind worker",
        "python3 -B tools/tasks.py",
        "/repo/.git/sf-cargo/bin/tasks-0123456789abcdef attach --kind worker",
        "/repo/.target/sf-bin/tasks-0123456789abcdef",
        "sf-bin/tasks-0123456789abcdef",
        "/w/.target/bin/tasks beat",
    ] {
        assert!(board(yes), "{yes:?}");
    }
    for no in [
        "just tasksx",
        "justtasks",
        "just tasks-attachx",
        "tools/tasks.pyc",
        "/repo/.git/sf-cargo/bin/tasks-0123456789abcde",
        "/repo/.git/sf-cargo/bin/tasks-0123456789ABCDEF",
        "/repo/.git/sf-cargo/bin/tasks-0123456789abcdefg",
        "x.target/bin/tasks",
        "/w/.target/bin/tasks-x",
        "sf-cargo/bin/tasks-é123456789abcdef",
    ] {
        assert!(!board(no), "{no:?}");
    }
}

#[test]
fn a_busy_reason_names_the_first_protected_command_before_any_other_work() {
    assert_eq!(
        busy_reason(&all(&["tail -f log", "just verify"])),
        Some("runs a verify (just verify)".into())
    );
    assert_eq!(
        busy_reason(&all(&["claude", "tail -f log", "sleep 9"])),
        Some("runs tail -f log".into())
    );
    assert_eq!(
        busy_reason(&all(&["claude --model x", "node mcp-server"])),
        None
    );
    assert_eq!(
        busy_reason(&all(&["/bin/zsh -c claude"])),
        Some("runs /bin/zsh -c claude".into())
    );
    assert_eq!(busy_reason(&[]), None);
    let long = format!("just verify {}", "x".repeat(80));
    assert_eq!(
        busy_reason(std::slice::from_ref(&long)),
        Some(format!("runs a verify ({})", &long[..60]))
    );
    assert_eq!(
        busy_reason(&all(&["/app/bin/symphony-server serve"])),
        Some("runs the Symphony (/app/bin/symphony-server serve)".into())
    );
}

#[test]
fn interactive_codex_is_harmless_but_nested_exec_is_work() {
    for command in [
        "codex",
        "/opt/bin/codex -m model",
        "codex resume thread",
        "codex --profile e",
        "codex resume THREAD 'a prompt containing exec'",
        "codex --profile=exec",
        "codex -pe",
        "codex --search -m e resume THREAD 'exec'",
        "codex --config 'key = \"e exec\"' resume THREAD",
        "codex -",
        "codex --",
        "codex -- exec",
        "codex -- e",
        "codex -p",
    ] {
        assert!(harmless(command), "{command}");
        assert_eq!(busy_reason(&all(&[command])), None);
        assert_eq!(reap_busy_reason(&all(&[command])), None);
    }
    for command in [
        "codex exec draw",
        "codex e draw",
        "/opt/bin/codex -m model exec draw",
        "CODEX e draw",
        "codex exec 'draw an mcp-lamp'",
        "CODEX e 'run just tasks inbox'",
        "codex --profile e exec draw",
        "codex --profile=e e draw",
        "codex -pe e draw",
        "codex --search --model e exec draw",
        "codex --config 'key = \"e exec\"' exec draw",
        "codex exec 'unfinished",
    ] {
        assert!(!harmless(command), "{command}");
        let expected = Some(format!("runs {command}"));
        assert_eq!(busy_reason(&all(&[command])), expected);
        assert_eq!(reap_busy_reason(&all(&[command])), expected);
    }
    for option in [
        "-c",
        "--config",
        "-p",
        "--profile",
        "-m",
        "--model",
        "-C",
        "--cd",
        "-s",
        "--sandbox",
        "-a",
        "--ask-for-approval",
        "-i",
        "--image",
        "--enable",
        "--disable",
        "--remote",
        "--remote-auth-token-env",
        "--local-provider",
        "--add-dir",
    ] {
        let idle = format!("codex {option} e");
        let work = format!("{idle} exec draw");
        assert!(harmless(&idle), "{idle}");
        assert_eq!(busy_reason(&all(&[&idle])), None, "{idle}");
        assert_eq!(reap_busy_reason(&all(&[&idle])), None, "{idle}");
        assert!(!harmless(&work), "{work}");
        let expected = Some(format!("runs {work}"));
        assert_eq!(busy_reason(&all(&[&work])), expected, "{work}");
        assert_eq!(reap_busy_reason(&all(&[&work])), expected, "{work}");
    }
}

#[test]
fn under_an_agent_the_sweep_also_lets_helpers_and_board_commands_be() {
    let monitor = all(&[
        "caffeinate -i -t 300",
        "just tasks-attach --kind worker",
        "node x/mcp.js",
    ]);
    assert_eq!(
        busy_reason(&monitor),
        Some("runs caffeinate -i -t 300".into())
    );
    assert_eq!(reap_busy_reason(&monitor), None);
    assert_eq!(
        reap_busy_reason(&all(&["just tasks inbox", "sleep 5"])),
        Some("runs sleep 5".into())
    );
    assert_eq!(
        reap_busy_reason(&all(&["caffeinate", "just land x"])),
        Some("runs a land (just land x)".into())
    );
}

#[test]
fn the_name_flag_is_the_first_one_named() {
    assert_eq!(
        named("claude --model sonnet -n amber-heron"),
        Some("amber-heron")
    );
    assert_eq!(named("claude --name\t slate-owl x"), Some("slate-owl"));
    assert_eq!(named("-n first -n second"), Some("first"));
    assert_eq!(named("claude -nx y"), None);
    assert_eq!(named("claude -n"), None);
    assert_eq!(named("claude -n   "), None);
    assert_eq!(named("claude x-n y"), None);
    assert_eq!(named("claude -name y"), None);
}
