//! A development child, and the `lsof` the board builds to ask whether it listens, run without the
//! session of the terminal the development front door was started from.

use super::*;

const TEST: &str = "development::session_tests::a_development_child_and_its_listen_probe_run_without_the_starters_session";
const DIR: &str = "SYMPHONY_DEV_SESSION_DIR";

/// A line naming what a process holds of the starter's session, appended to
/// `$SYMPHONY_DEV_SESSION_DIR/NAME.held`.
fn held(name: &str) -> String {
    format!(
        "printf '%s|%s|%s|%s|%s\\n' \"${{TASKS_SESSION-unset}}\" \"${{HC_TERMINAL_ID-unset}}\" \
         \"${{HC_TERMINAL_SOCKET-unset}}\" \"${{HC_SESSION-unset}}\" \"${{HC_SCOPE-unset}}\" \
         >> \"${DIR}/{name}.held\""
    )
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_development_child_and_its_listen_probe_run_without_the_starters_session() {
    if std::env::var_os(DIR).is_some() {
        let (child, _) = start_child(
            Instant::now() + Duration::from_secs(10),
            crate::sys::parent(),
            &AtomicBool::new(false),
            |_| {
                let mut command = Command::new("/bin/sh");
                command.args(["-c", &format!("{}; exec /bin/sleep 30", held("child"))]);
                command
            },
        )
        .unwrap();
        Children(vec![child]).stop().unwrap();
        return;
    }
    let scratch =
        Scratch(std::env::temp_dir().join(format!("symphony-dev-session-{}", std::process::id())));
    std::fs::create_dir_all(&scratch.0).unwrap();
    // The `lsof` first on the PATH: it says the child listens once the child has said what it
    // holds, and what it holds itself each time it is asked. It is the machine's copy of the
    // fixture script, already run, so the child's startup deadline covers no first exec of a new
    // file.
    let lsof = scratch.0.join("lsof");
    crate::fixture_script::write(
        &lsof,
        &format!(
            "#!/bin/sh\n{}\ntest -s \"${DIR}/child.held\" || exit 1\nprintf 'p%s\\n' \"$5\"\n",
            held("lsof")
        ),
        false,
    );
    crate::testing::assert_primed(&lsof);
    // The front door, as one an agent starts from its hc terminal.
    let started = [
        ("TASKS_SESSION", "fixture-session"),
        ("HC_TERMINAL_ID", "t7"),
        ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
        ("HC_SESSION", "fixture-core"),
        ("HC_SCOPE", "fixture-scope"),
    ];
    let done = sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(DIR, &scratch.0)
            .env("PATH", format!("{}:/usr/bin:/bin", scratch.0.display()))
            .envs(started),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    for name in ["child", "lsof"] {
        let held = std::fs::read_to_string(scratch.0.join(format!("{name}.held"))).unwrap();
        assert!(!held.is_empty(), "{name} never ran");
        for line in held.lines() {
            assert_eq!(line, "unset|unset|unset|unset|unset", "{name}");
        }
    }
}
