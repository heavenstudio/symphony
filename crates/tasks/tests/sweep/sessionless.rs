//! The sweep's process scans (`Options::machine`'s `cwds` and `occupied`) run `lsof` without the
//! session of the process that sweeps (`symphony_board::sys::SESSION_VARS`): a sweep an agent starts
//! from its hc terminal holds its board handle and hc's terminal, socket, core and scope, and the
//! `lsof` it asks holds none of them. A stand-in is the only `lsof` the helper finds first on its
//! `PATH`; no scan reads this machine's processes.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use symphony_board::sweep::Options;
use symphony_board::sys::{self, Command};

/// The session the helper below is started in, as an agent's hc terminal starts a command.
const SESSION: [(&str, &str); 5] = [
    ("TASKS_SESSION", "fixture-session"),
    ("HC_TERMINAL_ID", "t7"),
    ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
    ("HC_SESSION", "fixture-core"),
    ("HC_SCOPE", "fixture-scope"),
];

/// The folder the helper is handed, where the stand-in writes.
const DIR: &str = "SF_TEST_SWEEP_SESSION_DIR";

/// A stand-in `lsof`: a line of what it holds of [`SESSION`] per run, `|` between, then one working
/// directory for the listing (`-Fn`) and one process for a search.
const LSOF: &str = r#"#!/bin/sh
printf '%s|%s|%s|%s|%s\n' "${TASKS_SESSION-unset}" "${HC_TERMINAL_ID-unset}" "${HC_TERMINAL_SOCKET-unset}" "${HC_SESSION-unset}" "${HC_SCOPE-unset}" >> "$SF_TEST_SWEEP_SESSION_DIR/lsof-saw"
case "$1" in
    -Fn) printf 'p1\nn/fixture/cwd\n' ;;
    *) printf 'p7\n' ;;
esac
"#;

/// Run as a process of its own by the next test, started in [`SESSION`]: what this process holds of
/// it, then the machine sweep's two scans, the listing and a search, which the stand-in answers.
#[test]
#[ignore = "a helper the next test runs as a process"]
fn helper_scans_of_a_session() {
    let Some(dir) = std::env::var_os(DIR).map(PathBuf::from) else {
        return;
    };
    let held: Vec<String> = SESSION
        .iter()
        .map(|(name, _)| std::env::var(name).unwrap_or_else(|_| "unset".into()))
        .collect();
    fs::write(dir.join("helper-saw"), format!("{}\n", held.join("|"))).unwrap();
    let vars: BTreeMap<String, String> = std::env::vars().collect();
    let machine = Options::machine(&vars);
    assert_eq!((machine.cwds)(), Some(vec![PathBuf::from("/fixture/cwd")]));
    assert_eq!((machine.occupied)(Path::new("/fixture/cwd")), Some(true));
}

#[test]
fn the_sweeps_process_scans_run_without_the_sweepers_session() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sweep-{}-sessionless", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let lsof = dir.join("lsof");
    fs::write(&lsof, LSOF).unwrap();
    fs::set_permissions(&lsof, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let done = sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "sessionless::helper_scans_of_a_session",
                "--test-threads=1",
            ])
            .env(DIR, &dir)
            .env("PATH", path)
            .envs(SESSION),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!(
        read("helper-saw"),
        "fixture-session|t7|/fixture/hc/t7.sock|fixture-core|fixture-scope\n",
        "the sweeper's own"
    );
    assert_eq!(
        read("lsof-saw"),
        "unset|unset|unset|unset|unset\nunset|unset|unset|unset|unset\n",
        "the listing, then the search"
    );
    let _ = fs::remove_dir_all(&dir);
}
