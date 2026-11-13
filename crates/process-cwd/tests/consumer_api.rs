//! The public names of `process_cwd` that consuming projects' code calls, each in the shape it calls
//! it: a signature pinned by coercing the function to its pointer type, a trait method pinned on the
//! type it is called on, the `Command` and `Child` methods a consumer calls called. Nothing here
//! runs; it is compiled by the test build and by `just lint-rs` (clippy over every target), so a
//! change that would break a consumer fails a task's gate rather than a run later. What it calls
//! that is not named here the release run's conciliation
//! (`crates/symphony/src/release/conciliate.rs`) alone catches. The other packages' guards are
//! named in `crates/symphony/tests/consumer_api.rs`.
//!
//! A release's refresh moves a consumer's pin in a commit that touches only its `Cargo.toml` and
//! `Cargo.lock`, so a consumer's code never changes with a release: a name or signature here changes
//! only with a consumer release plan (the consumer's code moved first, against a release that still
//! has the old name). Names were read from the consuming projects' code on 2026-11-12; add one when
//! a consumer starts calling it.

use std::fs::OpenOptions;
use std::io::{self, PipeReader, PipeWriter};
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::sync::MutexGuard;

use process_cwd::spawn::{self, Child, Spawning, Stdio};
use process_cwd::{Command, close_on_fork, occupied, open_file_pids, process_cwds};

#[test]
fn the_names_consumers_call_keep_their_shapes() {
    let _: fn() -> Option<Vec<PathBuf>> = process_cwds;
    let _: fn(&Path) -> Option<bool> = occupied;
    let _: fn(&mut Command) -> Option<Vec<i64>> = open_file_pids;
    let _: fn(&mut OpenOptions) -> &mut OpenOptions = close_on_fork;

    let _: fn() -> MutexGuard<'static, ()> = spawn::spawning;
    let _: fn() -> io::Result<(PipeReader, PipeWriter)> = spawn::pipe;
    let _: fn(&mut Command) -> io::Result<Child> = spawn::spawn_output;
    let _: fn(&mut Command) -> io::Result<Child> = spawn::spawn_piped;
    let _: fn(Child) -> io::Result<Output> = spawn::wait_output;
    let _: fn(&mut Command) -> io::Result<Output> = spawn::output;
    let _: [fn() -> Stdio; 3] = [Stdio::null, Stdio::inherit, Stdio::piped];
    let _: fn(&mut std::process::Command) -> io::Result<std::process::Child> =
        <std::process::Command as Spawning>::spawn_locked;
    let _: fn(&mut std::process::Command) -> io::Result<ExitStatus> =
        <std::process::Command as Spawning>::status_locked;
    let _: fn(&mut std::process::Command) -> io::Result<Output> =
        <std::process::Command as Spawning>::output_locked;

    // A command built, started and waited on as a consumer does.
    let _ = |dir: &Path, fd: RawFd| -> io::Result<Child> {
        let mut command = Command::new("program");
        command
            .arg("a")
            .args(["b"])
            .env("K", "V")
            .envs([("K", "V")])
            .env_remove("K")
            .env_clear()
            .current_dir(dir)
            .process_group(0)
            .new_session()
            .pass_fd(fd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let _ = (
            command.get_program(),
            command.get_args().len(),
            command.get_current_dir(),
        );
        command.spawn()
    };
    let _ = |mut child: Child| -> io::Result<ExitStatus> {
        let _ = (
            child.id(),
            child.stdin.take(),
            child.stdout.take(),
            child.stderr.take(),
        );
        child.kill()?;
        let _ = child.try_wait()?;
        child.wait()
    };
}
