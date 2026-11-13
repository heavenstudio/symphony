//! A child keeps none of its starter's session but what its caller sets after.

use super::{board_env, command, output, sessionless};
use symphony_board::sys::Command;

/// The five variables as the child sees them, `|` between.
const SAW: &str = "printf '%s|%s|%s|%s|%s' \"${TASKS_SESSION-unset}\" \"${HC_TERMINAL_ID-unset}\" \
                   \"${HC_TERMINAL_SOCKET-unset}\" \"${HC_SESSION-unset}\" \"${HC_SCOPE-unset}\"";

fn saw(command: &mut Command) -> String {
    let done = symphony_board::sys::output(command.args(["-c", SAW])).unwrap();
    assert!(done.status.success(), "{done:?}");
    String::from_utf8(done.stdout).unwrap()
}

#[test]
fn a_child_runs_without_the_session_and_terminal_of_its_starter_but_with_its_actor() {
    // The variables are this test's own process's, set when it is started: set here, they would
    // reach every other test's children too.
    const CASE: &str = "SYMPHONY_CHILDREN_CASE";
    if std::env::var_os(CASE).is_some() {
        let all = "fixture-session|t7|/fixture/hc/t7.sock|fixture-core|fixture-scope";
        assert_eq!(saw(&mut Command::new("/bin/sh")), all, "the starter's own");
        assert_eq!(
            saw(&mut command("/bin/sh")),
            "unset|unset|unset|unset|unset"
        );
        assert_eq!(
            saw(command("/bin/sh").env("TASKS_SESSION", "an-actor")),
            "an-actor|unset|unset|unset|unset"
        );
        // A launcher handed a command drops even what its caller set.
        let mut handed = Command::new("/bin/sh");
        handed.env("HC_TERMINAL_ID", "t8");
        assert_eq!(
            saw(sessionless(&mut handed)),
            "unset|unset|unset|unset|unset"
        );
        // So does the runner of a command the board builds and hands back.
        let ran = output(Command::new("/bin/sh").args(["-c", SAW])).unwrap();
        assert_eq!(ran.stdout, b"unset|unset|unset|unset|unset");
        // The board's environment keeps all of this process's but the session.
        let env = board_env();
        for (name, _) in STARTED {
            assert!(!env.vars.contains_key(name), "{name}");
        }
        assert_eq!(env.vars.get(CASE).map(String::as_str), Some("1"));
        return;
    }
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "children::tests::a_child_runs_without_the_session_and_terminal_of_its_starter_but_with_its_actor",
            ])
            .env(CASE, "1")
            .envs(STARTED),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// The session this test's own process is started in.
const STARTED: [(&str, &str); 5] = [
    ("TASKS_SESSION", "fixture-session"),
    ("HC_TERMINAL_ID", "t7"),
    ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
    ("HC_SESSION", "fixture-core"),
    ("HC_SCOPE", "fixture-scope"),
];
