//! Codex's helpers under its terminal, each by its exact path: the configured REPL helper
//! (`CODEX_REPL`) and the ChatGPT app's computer-use REPL (`CODEX_CUA_NODE` running
//! `CODEX_CUA_REPL`) are not work, any other node program beside them is, and a finished Codex
//! with only its helpers under it closes.

use super::all;
use crate::World;
use symphony_board::launch::procs::{
    CODEX_CUA_NODE, CODEX_CUA_REPL, CODEX_REPL, busy_reason, harmless, reap_busy_reason,
};
use symphony_board::launch::reap::Sweep;

#[test]
fn codexs_configured_repl_helper_is_harmless_but_any_other_node_program_is_work() {
    let codex = "codex --no-daemon -s workspace-write -m gpt-6.1-sol";
    for idle in [
        all(&[codex, CODEX_REPL]),
        all(&[codex, &format!("{CODEX_REPL} --stdio")]),
    ] {
        assert_eq!(busy_reason(&idle), None, "{idle:?}");
        assert_eq!(reap_busy_reason(&idle), None, "{idle:?}");
    }
    let node = CODEX_REPL.strip_suffix("_repl").unwrap();
    for work in [
        "node script.js".to_string(),
        "/usr/local/bin/node x.js".into(),
        format!("{node} x.js"),
        "node_repl".into(),
        "/opt/cua_node/bin/node_repl".into(),
        CODEX_REPL.to_lowercase(),
        format!("x{CODEX_REPL}"),
        format!("{CODEX_REPL}x"),
        format!("/bin/zsh -c {CODEX_REPL}"),
    ] {
        assert!(!harmless(&work), "{work}");
        let expected = Some(format!(
            "runs {}",
            work.chars().take(60).collect::<String>()
        ));
        assert_eq!(busy_reason(&all(&[codex, CODEX_REPL, &work])), expected);
        assert_eq!(
            reap_busy_reason(&all(&[codex, CODEX_REPL, &work])),
            expected
        );
    }
    // A real job beside the helper is still work; a protected one is named as such.
    assert_eq!(
        reap_busy_reason(&all(&[codex, CODEX_REPL, "cargo test -p x"])),
        Some("runs cargo test -p x".into())
    );
    assert_eq!(
        busy_reason(&all(&[CODEX_REPL, "just verify"])),
        Some("runs a verify (just verify)".into())
    );
}

/// The ChatGPT app's computer-use REPL as `ps` prints it under every Codex session.
fn cua_repl() -> String {
    format!("{CODEX_CUA_NODE} {CODEX_CUA_REPL}")
}

#[test]
fn codexs_computer_use_helper_is_harmless_only_as_its_node_running_its_module() {
    let codex = "codex --no-daemon -s workspace-write -m gpt-6.1-sol";
    let cua = cua_repl();
    // As observed: the helper is a child of codex, and starts its own node_repl.
    for idle in [
        all(&[codex, &cua, CODEX_REPL]),
        all(&[codex, &cua]),
        all(&[codex, &format!("{cua} --stdio"), CODEX_REPL]),
    ] {
        assert!(harmless(&idle[1]), "{idle:?}");
        assert_eq!(busy_reason(&idle), None, "{idle:?}");
        assert_eq!(reap_busy_reason(&idle), None, "{idle:?}");
    }
    let modules = CODEX_CUA_REPL.strip_suffix("cua-repl.mjs").unwrap();
    let elsewhere = CODEX_CUA_NODE.replace("/Applications/", "/opt/");
    for work in [
        // Another script, or none, under the same node.
        format!("{CODEX_CUA_NODE} build.js"),
        format!("{CODEX_CUA_NODE} {modules}other.mjs"),
        format!("{CODEX_CUA_NODE} {CODEX_CUA_REPL}x"),
        format!("{CODEX_CUA_NODE} x{CODEX_CUA_REPL}"),
        format!("{CODEX_CUA_NODE} --inspect {CODEX_CUA_REPL}"),
        format!("{CODEX_CUA_NODE} {}", CODEX_CUA_REPL.to_lowercase()),
        CODEX_CUA_NODE.to_string(),
        // The same module under a node at another path, or by itself.
        format!("node {CODEX_CUA_REPL}"),
        format!("/usr/local/bin/node {CODEX_CUA_REPL}"),
        format!("{elsewhere} {CODEX_CUA_REPL}"),
        format!("{CODEX_CUA_NODE}x {CODEX_CUA_REPL}"),
        format!("x{CODEX_CUA_NODE} {CODEX_CUA_REPL}"),
        CODEX_CUA_REPL.to_string(),
        // A shell that starts it is a shell's job, as any is.
        format!("/bin/zsh -c {cua}"),
    ] {
        assert!(!harmless(&work), "{work}");
        let expected = Some(format!(
            "runs {}",
            work.chars().take(60).collect::<String>()
        ));
        let beside = all(&[codex, &cua, CODEX_REPL, &work]);
        assert_eq!(busy_reason(&beside), expected, "{work}");
        assert_eq!(reap_busy_reason(&beside), expected, "{work}");
    }
    // Protected work beside the helpers is named as such.
    assert_eq!(
        reap_busy_reason(&all(&[codex, &cua, CODEX_REPL, "just test-sim SUITE=jump"])),
        Some("runs a test run (just test-sim SUITE=jump)".into())
    );
}

#[test]
fn a_finished_codex_with_only_its_repl_helper_under_it_closes_and_another_node_keeps_it() {
    let cua = cua_repl();
    let sets: [&[&str]; 2] = [&[CODEX_REPL], &[&cua, CODEX_REPL]];
    for helpers in sets {
        for sweep in [false, true] {
            let w = World::new();
            let (handle, terminal) = w.worker("sim/jump", Some("done"));
            assert_eq!(w.program(&terminal)[0], "codex");
            w.run_under(w.pid_of(&terminal), helpers);
            if sweep {
                assert_eq!(w.sweep_through(&w.hc()).idle, std::slice::from_ref(&handle));
                assert_eq!(w.sweep_through(&w.hc()).closed.len(), 1);
            } else {
                let [closed] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
                assert!(closed.closed, "{closed:?}");
            }
            assert!(
                !w.has_terminal(&terminal),
                "{terminal} stayed, sweep {sweep}, {helpers:?}"
            );
        }
    }
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.run_under(w.pid_of(&terminal), &[CODEX_REPL, "node build.js"]);
    let closures = w.clean(false, &[]);
    assert!(closures.is_empty(), "{closures:?}");
    for _ in 0..3 {
        assert_eq!(w.sweep_through(&w.hc()), Sweep::default());
    }
    assert!(w.has_terminal(&terminal), "{terminal} was closed");
}
