//! A child a test starts with a cleared environment holds no `CARGO_MANIFEST_DIR`, while the process
//! it runs under was started by Cargo, or by nextest, in the repository: the child is refused the
//! live board as that process is, before its server is started, by every entry, from any folder.

use super::{COPY, Held, Live, TASKS, before, refused, this};
use crate::Ran;
use crate::children::Child as _;
use std::path::{Path, PathBuf};

/// Set, this test binary is the stand-in for Cargo ([`cargo_stand_in`]): the file it names holds
/// the folder to start in, then the program and its arguments, a line each.
const STAND_IN: &str = "TASKS_TEST_CARGO_STAND_IN";

/// What a test does in the shell Cargo started, run as `sh CALLER PID DIR COMMAND...`: write its pid
/// to the file PID, then start COMMAND in DIR without `CARGO_MANIFEST_DIR` and wait for it, so
/// COMMAND runs under the shell, never in its place, and the shell stays in the folder Cargo
/// started it in.
const CALLER: &str = r#"echo $$ > "$1"
dir=$2
shift 2
(cd "$dir" && exec env -u CARGO_MANIFEST_DIR "$@")
exit $?
"#;

/// The stand-in for Cargo, which this test binary is when it runs from a file named `cargo` (or
/// `cargo-nextest`) with [`STAND_IN`] set, so the kernel names the process as it names Cargo (or
/// nextest): it starts the program
/// [`STAND_IN`]'s file names, in the folder it names, as Cargo starts a test in its package's,
/// waits for it, and ends as it ended. Without [`STAND_IN`] it does nothing.
#[test]
fn cargo_stand_in() {
    let Some(file) = std::env::var_os(STAND_IN) else {
        return;
    };
    let text = std::fs::read_to_string(file).unwrap();
    let mut lines = text.lines();
    let (dir, program) = (lines.next().unwrap(), lines.next().unwrap());
    let status = std::process::Command::of(program)
        .args(lines)
        .current_dir(dir)
        .env_remove(STAND_IN)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

/// This test binary as a file named `name` in `bin`: a hard link, else a copy.
fn as_program(bin: &Path, name: &str) -> PathBuf {
    let this = std::env::current_exe().unwrap();
    let program = bin.join(name);
    if std::fs::hard_link(&this, &program).is_err() {
        std::fs::copy(&this, &program).unwrap();
    }
    program
}

#[test]
fn a_child_started_without_the_cargo_variable_is_refused_as_the_process_cargo_or_nextest_started() {
    let live = Live::new("main-only-cleared", Held::Own);
    let package = live.wt.join("crates/tasks");
    std::fs::create_dir_all(&package).unwrap();
    let caller = live.base.join("caller.sh");
    std::fs::write(&caller, CALLER).unwrap();
    let bin = live.base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // The stand-in by Cargo's name, by nextest's, and by another.
    let cargo = as_program(&bin, "cargo");
    let nextest = as_program(&bin, "cargo-nextest");
    let other = as_program(&bin, "starter");
    let pid = live.base.join("caller.pid");
    // `starter` starts the caller in the package's folder, holding the package's folder as Cargo's
    // children do; the caller runs `command` in `cwd`. What it said, and the caller's pid.
    let run = |starter: &Path, cwd: &Path, command: &[&str], input: &str| -> (Ran, String) {
        let mut lines = vec![
            package.to_str().unwrap(),
            "/bin/sh",
            caller.to_str().unwrap(),
            pid.to_str().unwrap(),
            cwd.to_str().unwrap(),
        ];
        lines.extend_from_slice(command);
        let told = live.base.join("stand-in");
        std::fs::write(&told, lines.join("\n")).unwrap();
        let vars = [
            (STAND_IN, told.to_str().unwrap()),
            ("CARGO_MANIFEST_DIR", package.to_str().unwrap()),
        ];
        let harness = [
            "main_only::cleared::cargo_stand_in",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ];
        let ran = live.run(starter, &live.base, &vars, &harness, input);
        let caller = std::fs::read_to_string(&pid).unwrap();
        (ran, caller.trim().to_string())
    };
    // The refusal names the caller Cargo (or nextest, `by`) started, and the package's folder it
    // works in.
    let why_by = |caller: &str, by: &str| {
        let own = live.under_cargo(&package);
        let from = format!("(CARGO_MANIFEST_DIR={}: ", package.display());
        assert!(own.contains(&from), "{own}");
        own.replacen(
            &from,
            &format!(
                "(process {caller}, which {by} started in {} and this process runs under: ",
                package.display()
            ),
            1,
        )
    };
    let why = |caller: &str| why_by(caller, "Cargo");

    // Refused before its server is started.
    live.home().stop_board().unwrap();
    let (ran, caller) = run(&cargo, &live.wt, &[TASKS, "list"], "");
    refused(&ran, &why(&caller));
    assert!(!live.serving());

    live.start();
    live.seed("tasks");
    let addr = live.addr();
    let (ran, caller) = run(&cargo, &live.wt, &[TASKS, "list"], "");
    refused(&ran, &why(&caller));
    // By the server's address, from outside the repository: the caller's folder names the board.
    let (ran, caller) = run(&cargo, &live.outside, &[COPY, &addr], "{}");
    assert_eq!(
        (ran.code, ran.err),
        (2, format!("❌ tasks-copy: {}\n", why(&caller)))
    );
    // Under `cargo nextest run`, which starts each test itself.
    let (ran, caller) = run(&nextest, &live.wt, &[TASKS, "list"], "");
    refused(&ran, &why_by(&caller, "nextest"));
    assert_eq!(live.version("tasks"), Some(before()));

    // The same caller, started by a process of another name: the child is main's own build, and
    // moves the board.
    let (ran, _) = run(&other, &live.wt, &[TASKS, "list"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(live.version("tasks"), Some(this()));
}
