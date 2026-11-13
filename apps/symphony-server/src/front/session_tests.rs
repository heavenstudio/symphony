//! The server's maintenance children, the board's idle-session reap and its embedding catch-up,
//! run without the session of the terminal the server was started from.

use super::sweep_tests::SweepFixture;
use super::*;

const TEST: &str = "front::session_tests::the_maintenance_children_run_without_the_session_of_the_servers_terminal";
const CHILD: &str = "SYMPHONY_SESSIONLESS_CHILD";

/// A board command's stand-in: what it holds of the starter's session, written whole.
const HELD: &str = "printf '%s|%s|%s|%s|%s' \"${TASKS_SESSION-unset}\" \"${HC_TERMINAL_ID-unset}\" \
                    \"${HC_TERMINAL_SOCKET-unset}\" \"${HC_SESSION-unset}\" \"${HC_SCOPE-unset}\" \
                    > held.part; mv held.part held; exec sleep 30";

#[test]
fn the_maintenance_children_run_without_the_session_of_the_servers_terminal() {
    if std::env::var_os(CHILD).is_none() {
        // The server, as one an agent starts from its hc terminal: the board handle it acts as,
        // its terminal, that terminal's socket, the core and the scope.
        let started = [
            ("TASKS_SESSION", "fixture-session"),
            ("HC_TERMINAL_ID", "t7"),
            ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
            ("HC_SESSION", "fixture-core"),
            ("HC_SCOPE", "fixture-scope"),
        ];
        let mut server = Command::new(std::env::current_exe().unwrap());
        server
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, "1")
            .envs(started);
        let done = symphony_board::sys::output(&mut server).unwrap();
        let said = String::from_utf8_lossy(&done.stdout);
        let errors = String::from_utf8_lossy(&done.stderr);
        assert!(done.status.success(), "{said}\n{errors}");
        assert!(
            said.contains("1 passed"),
            "the child must run the case: {said}"
        );
        return;
    }
    assert_eq!(
        std::env::var("HC_TERMINAL_SOCKET").unwrap(),
        "/fixture/hc/t7.sock"
    );
    let mut reap = SweepFixture::new(HELD);
    reap.start(session_maintenance(&reap.shared, Some(&reap.root), Stamp::now).unwrap());
    let mut embed = SweepFixture::with_embedding_launcher(HELD);
    embed.start(embeddings::start(&embed.shared, Some(&embed.root), Stamp::now).unwrap());
    for fixture in [&reap, &embed] {
        fixture.wait_for("held", "unset|unset|unset|unset|unset");
    }
}
