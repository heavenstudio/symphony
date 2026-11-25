//! Where this binary's scratch goes ([`crate::scratch`]): one folder of the process's own in the temp
//! folder's [`SCRATCH_HARNESS`] folder, gone when the binary exits, and swept by the next run when a
//! kill left it behind.

use crate::children::Child as _;
use crate::{SCRATCH_HARNESS, own_scratch, scratch};
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use symphony_board::sys::{Command, Stdio};

/// Set on a run of this binary that plays [`a_run_of_this_binary`]: `made`, or `held`.
const ROLE: &str = "SYMPHONY_TASKS_CLI_SCRATCH_ROLE";
/// What a held run prints once its scratch is made.
const MADE: &str = "scratch made";

/// A run of this binary that makes a scratch folder and ends, or, `held`, then waits for its input's
/// end. Run with no role, it has nothing to do.
#[test]
fn a_run_of_this_binary() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    fs::write(scratch("made").join("file"), "a run's scratch").unwrap();
    if role == "held" {
        println!("{MADE}");
        // The input ends once the test that started this run has reaped it, or with that test.
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
    }
}

/// This binary as `role`, with `temp` its temp folder.
fn run(role: &str, temp: &Path) -> Command {
    let mut command = Command::of(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "scratch_folder::a_run_of_this_binary",
            "--nocapture",
        ])
        .env(ROLE, role)
        .env("TMPDIR", temp);
    command
}

/// A run of this binary as `made`, which must pass.
fn made(temp: &Path) {
    let done = symphony_board::sys::output(&mut run("made", temp)).unwrap();
    assert!(done.status.success(), "{done:?}");
    assert!(
        String::from_utf8_lossy(&done.stdout).contains("1 passed"),
        "{done:?}"
    );
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    symphony_board::order::sort_by(&mut names, &Ord::cmp);
    names
}

#[test]
fn a_scratch_folder_is_in_this_processs_own_folder_in_the_harness_folder() {
    let dir = scratch("where");
    let own = own_scratch();
    assert_eq!(dir.parent(), Some(own));
    let root = std::env::temp_dir()
        .join(SCRATCH_HARNESS)
        .canonicalize()
        .unwrap();
    assert_eq!(own.parent(), Some(root.as_path()));
    let name = own.file_name().unwrap().to_str().unwrap();
    let (pid, n) = name.split_once('-').unwrap();
    assert_eq!(pid, std::process::id().to_string());
    assert!(n.parse::<usize>().is_ok(), "{name}");
}

#[test]
fn a_run_leaves_only_the_empty_harness_folder_in_the_temp_folder() {
    let temp = scratch("temp-of-a-run");
    made(&temp);
    assert_eq!(names(&temp), [SCRATCH_HARNESS]);
    assert!(names(&temp.join(SCRATCH_HARNESS)).is_empty());
}

#[test]
fn a_killed_runs_scratch_goes_at_the_next_run() {
    let temp = scratch("temp-of-a-killed-run");
    let mut held = run("held", &temp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Kept until the held run is reaped: `wait` closes a child's input before it waits, and on
    // macOS a killed process's thread blocked on that input can wake to its end and remove the
    // scratch at the exit before the kill has ended the process.
    let input = held.stdin.take();
    let mut out = BufReader::new(held.stdout.take().unwrap());
    let mut said = String::new();
    while !said.lines().any(|line| line == MADE) {
        assert_ne!(
            out.read_line(&mut said).unwrap(),
            0,
            "the held run ended before its scratch was made: {said}"
        );
    }
    held.kill().unwrap();
    assert_eq!(held.wait().unwrap().signal(), Some(9));
    drop(input);
    let left = format!("{}-0", held.id());
    assert_eq!(names(&temp.join(SCRATCH_HARNESS)), [left.as_str()]);
    let file = temp.join(SCRATCH_HARNESS).join(&left).join("made/file");
    assert!(file.is_file(), "the kill left no scratch, proving nothing");
    made(&temp);
    assert!(
        names(&temp.join(SCRATCH_HARNESS)).is_empty(),
        "the killed run's scratch outlived the next run"
    );
}
