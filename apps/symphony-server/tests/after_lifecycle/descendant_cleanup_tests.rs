//! An orphaned fixture descendant holds cleanup until its last write, whichever owner reaps it,
//! and so does a stand-in a server starts itself, which holds none of the case's endpoints.

use super::*;

#[test]
fn cleanup_waits_for_orphaned_fixture_descendants_before_removing_scratch() {
    for repository in [false, true] {
        let repo = Repo::new("descendant-cleanup");
        let mut command = Command::new("/bin/sh");
        // The launcher exits while its child holds the lifetime endpoint and waits for the
        // test's input. The child's final write must finish before scratch can disappear.
        // The launcher reassigns every single-digit descriptor first, as a script may: the
        // endpoint must reach the child under a number none of them replaces.
        command
            .args([
                "-c",
                "exec 3<&0 4<&0 5<&0 6<&0 7<&0 8<&0 9<&0; sh -c 'printf ready; read -r line; printf finished > \"$1\"' fixture \"$1\" <&3 & exit",
                "fixture",
            ])
            .arg(repo.base.join("late-write"))
            .stdin(Stdio::piped());
        let mut process = if repository {
            repo.run(command)
        } else {
            Process::spawn(command, &repo.log("orphan"))
        };
        let input = process.child.stdin.take().unwrap();
        process.success();
        until("orphaned fixture readiness", || {
            (process.output() == "ready").then_some(())
        });
        cleanup_waits_for_release(&format!("repository {repository}"), repo, process, input);
    }
}

/// A server starts its census stand-in without a fork and passes it nothing (here: a forkless
/// spawn with no endpoint), and its stop kills that shell alone. What the stand-in holds then
/// keeps cleanup waiting: the real ps stand-in held on its census FIFO, and the child of a
/// killed shell that took the lifetime as ps.sh does, until each one's release.
#[test]
fn cleanup_waits_for_stand_ins_a_server_started_without_an_endpoint() {
    let repo = Repo::new("held-ps-stand-in");
    let hold = fifo(&repo.base.join("census-hold"));
    let mut census = stand_in(
        &repo,
        &repo.bin.join("ps"),
        ["-axww", "-o", "pid=,ppid=,command="],
    );
    until("the census stand-in reading its hold", || {
        repo.base.join("census-held").exists().then_some(())
    });
    cleanup_waits_for_release("a held ps stand-in", repo, (), hold);
    // Released, it ran to its end.
    assert!(census.wait().unwrap().success());

    let repo = Repo::new("killed-stand-in");
    // The ps stand-in's own interpreter: its children inherit what `exec` opens.
    let mut shell = stand_in(
        &repo,
        Path::new("/bin/bash"),
        [
            "-c",
            &format!(
                "exec 9>\"$HOME/{STAND_IN_LIFETIME}\" 3<&0; sh -c 'printf ready; read -r line; printf finished > \"$1\"' fixture \"$1\" <&3 & wait"
            ),
            "fixture",
            repo.base.join("late-write").to_str().unwrap(),
        ],
    );
    let input = shell.stdin.take().unwrap();
    until("the killed stand-in's child readiness", || {
        (fs::read_to_string(repo.base.join("stand-in.out")).ok()? == "ready").then_some(())
    });
    // The server's stop: SIGKILL to the shell alone, its end reaped.
    shell.kill().unwrap();
    assert_eq!(shell.wait().unwrap().signal(), Some(libc::SIGKILL));
    cleanup_waits_for_release("a killed stand-in's child", repo, (), input);
}

/// A stand-in that starts once its case has closed the lifetime, an orphan of a killed server
/// say, cannot open (or create) it and writes nothing into scratch.
#[test]
fn a_stand_in_starting_after_its_case_closed_the_lifetime_writes_nothing() {
    let repo = Repo::new("closed-stand-ins");
    repo.close_stand_ins();
    let mut census = stand_in(
        &repo,
        &repo.bin.join("ps"),
        ["-axww", "-o", "pid=,ppid=,command="],
    );
    assert!(!census.wait().unwrap().success());
    let said = fs::read_to_string(repo.base.join("stand-in.err")).unwrap();
    assert!(said.contains(STAND_IN_LIFETIME), "{said}");
    let observed = repo.base.join(format!("env.{}", std::process::id()));
    assert!(
        !observed.exists(),
        "a closed case's stand-in wrote {}",
        observed.display()
    );
    assert!(!repo.base.join(STAND_IN_LIFETIME).parent().unwrap().exists());
}

/// `program` started as a server starts its census: forkless and with no endpoint of the case's,
/// in the fixture's environment. Its input is piped, its output in `stand-in.out` and
/// `stand-in.err` in scratch.
fn stand_in<'a>(
    repo: &Repo,
    program: &Path,
    args: impl IntoIterator<Item = &'a str>,
) -> symphony_board::sys::Child {
    Command::new(program)
        .args(args)
        .current_dir(&repo.main)
        .env_clear()
        .envs(repo.environment())
        .stdin(Stdio::piped())
        .stdout(File::create(repo.base.join("stand-in.out")).unwrap())
        .stderr(File::create(repo.base.join("stand-in.err")).unwrap())
        .spawn()
        .unwrap()
}

/// Drop `owner`, then `repo`, on a thread of their own, and check that cleanup leaves scratch in
/// place until `release` lets the last holder go (a line), and ends after it.
fn cleanup_waits_for_release(
    what: &str,
    repo: Repo,
    owner: impl Send + 'static,
    mut release: impl Write,
) {
    let base = repo.base.clone();
    let (finished, completion) = std::sync::mpsc::channel();
    let cleanup = std::thread::spawn(move || {
        let began = Instant::now();
        drop(owner);
        drop(repo);
        finished.send((began.elapsed(), Instant::now())).unwrap();
    });
    // A bounded negative observation, not cleanup's synchronization. The line releases the
    // holder; EOF on the lifetime it holds is what releases cleanup.
    let early = completion.recv_timeout(Duration::from_millis(50));
    if let Ok((waited, _)) = early {
        panic!("cleanup ({what}) ended after {waited:?}, before its holder's final write");
    }
    let scratch_present = base.is_dir();
    let released = Instant::now();
    writeln!(release, "finish").unwrap();
    let ended = completion.recv_timeout(PATIENCE);
    // A cleanup still running at the deadline fails here: joining it would wait unbounded.
    if matches!(ended, Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
        panic!("cleanup ({what}) did not end within {PATIENCE:?} of release");
    }
    // It sent or ended without sending: the join returns at once and carries its panic.
    if let Err(failure) = cleanup.join() {
        std::panic::resume_unwind(failure);
    }
    let (waited, end) = ended
        .unwrap_or_else(|error| panic!("cleanup ({what}) ended without its report: {error:?}"));
    // The holder's write follows the release, so a cleanup that waited for it ends later.
    assert!(
        end > released,
        "cleanup ({what}) ended {:?} before its holder was released",
        released.duration_since(end)
    );
    println!(
        "cleanup ({what}) waited {waited:?}, {:?} of it after release",
        end.duration_since(released)
    );
    assert!(scratch_present, "scratch removed while {what} could write");
    assert!(!base.exists());
}
