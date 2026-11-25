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

/// A read of the whole table, as a server's board lookup asks for it.
const TABLE: [&str; 3] = ["-ww", "-axo", "pid=,command="];

/// A server starts its ps stand-in without a fork and passes it nothing (here: a forkless spawn
/// with no endpoint), and may kill that shell alone. What the stand-in holds then keeps cleanup
/// waiting: the real ps stand-in held on its FIFO, and the child of a killed shell that took the
/// lifetime with ps.sh's own lines, until each one's release.
#[test]
fn cleanup_waits_for_stand_ins_a_server_started_without_an_endpoint() {
    let repo = Repo::new("held-ps-stand-in");
    let hold = fifo(&repo.base.join("ps-hold"));
    let mut table = stand_in(&repo, "table", &repo.bin.join("ps"), TABLE);
    until("the ps stand-in reading its hold", || {
        repo.base.join("ps-held").exists().then_some(())
    });
    cleanup_waits_for_release("a held ps stand-in", repo, (), hold);
    // Released, it ran to its end.
    assert!(table.wait().unwrap().success());

    let repo = Repo::new("killed-stand-in");
    // The ps stand-in's own interpreter: its children inherit what `exec` opens.
    let mut shell = stand_in(
        &repo,
        "killed",
        Path::new("/bin/bash"),
        [
            "-c",
            &format!(
                "{}; exec 3<&0; sh -c 'printf ready; read -r line; printf finished > \"$1\"' fixture \"$1\" <&3 & wait",
                registration().join("; ")
            ),
            "fixture",
            repo.base.join("late-write").to_str().unwrap(),
        ],
    );
    let input = shell.stdin.take().unwrap();
    until("the killed stand-in's child readiness", || {
        (fs::read_to_string(repo.base.join("killed.out")).ok()? == "ready").then_some(())
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
    let mut table = stand_in(&repo, "table", &repo.bin.join("ps"), TABLE);
    assert!(!table.wait().unwrap().success());
    let said = fs::read_to_string(repo.base.join("table.err")).unwrap();
    assert!(said.contains(STAND_IN_LIFETIME), "{said}");
    assert_wrote_nothing(&repo);
    assert!(!repo.base.join(STAND_IN_LIFETIME).parent().unwrap().exists());
}

/// The close withdraws the lifetime, then waits, then removes its folder. A stand-in starting in
/// that wait fails to open it and creates nothing in the folder still there; one that opened it
/// before the withdrawal and goes on after it finds it gone. Neither writes into scratch.
#[test]
fn stand_ins_meeting_a_withdrawal_in_progress_write_and_create_nothing() {
    let repo = Repo::new("withdrawn-stand-ins");
    let [open, check] = registration();
    let late = repo.base.join("late-write");
    // Held between ps.sh's two lines: past its open, before its check.
    let mut gated = stand_in(
        &repo,
        "gated",
        Path::new("/bin/bash"),
        [
            "-c",
            &format!("set -e; {open}; printf ready; read -r line; {check}; printf late > \"$1\""),
            "fixture",
            late.to_str().unwrap(),
        ],
    );
    let mut input = gated.stdin.take().unwrap();
    until("the gated stand-in's open", || {
        (fs::read_to_string(repo.base.join("gated.out")).ok()? == "ready").then_some(())
    });
    let lifetime = repo.base.join(STAND_IN_LIFETIME);
    let folder = lifetime.parent().unwrap();
    std::thread::scope(|scope| {
        let closing = scope.spawn(|| repo.close_stand_ins());
        until("the lifetime's withdrawal", || {
            (!lifetime.exists()).then_some(())
        });
        // The gated stand-in holds the close in its wait, between the withdrawal and the
        // folder's removal.
        let mut table = stand_in(&repo, "table", &repo.bin.join("ps"), TABLE);
        assert!(!table.wait().unwrap().success());
        assert!(folder.is_dir(), "the close went past its wait");
        assert!(
            fs::symlink_metadata(&lifetime).is_err(),
            "a stand-in created the withdrawn lifetime"
        );
        writeln!(input, "go on").unwrap();
        closing.join().unwrap();
    });
    assert!(!gated.wait().unwrap().success());
    assert!(
        !late.exists(),
        "a stand-in past its open wrote after the withdrawal"
    );
    assert_wrote_nothing(&repo);
    assert!(!folder.exists());
}

/// ps.sh's own registration: its open of the lifetime and its check that the case had not
/// withdrawn it, the first two commands after its options.
fn registration() -> [String; 2] {
    let script = fs::read_to_string(script_source("ps.sh")).unwrap();
    let mut commands = script
        .lines()
        .filter(|line| !line.starts_with('#'))
        .skip_while(|line| *line != "set -euo pipefail")
        .skip(1);
    let lines = [commands.next(), commands.next()].map(|line| line.unwrap().to_owned());
    assert!(
        lines[0].starts_with("exec 9<") && lines[0].contains(STAND_IN_LIFETIME),
        "{lines:?}"
    );
    lines
}

/// No ps stand-in this test started wrote its observation into scratch.
fn assert_wrote_nothing(repo: &Repo) {
    let observed = repo.base.join(format!("env.{}", std::process::id()));
    assert!(
        !observed.exists(),
        "a stand-in wrote {} after its case withdrew the lifetime",
        observed.display()
    );
}

/// `program` started as a server starts its ps: forkless and with no endpoint of the case's,
/// in the fixture's environment. Its input is piped, its output in `NAME.out` and `NAME.err` in
/// scratch.
fn stand_in<'a>(
    repo: &Repo,
    name: &str,
    program: &Path,
    args: impl IntoIterator<Item = &'a str>,
) -> symphony_board::sys::Child {
    Command::new(program)
        .args(args)
        .current_dir(&repo.main)
        .env_clear()
        .envs(repo.environment())
        .stdin(Stdio::piped())
        .stdout(File::create(repo.base.join(format!("{name}.out"))).unwrap())
        .stderr(File::create(repo.base.join(format!("{name}.err"))).unwrap())
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
    let (finished, completion) = crate::test_channel::channel();
    let cleanup = std::thread::spawn(move || {
        let began = Instant::now();
        drop(owner);
        drop(repo);
        finished.send((began.elapsed(), Instant::now())).unwrap();
    });
    // A bounded negative observation, not cleanup's synchronization. The line releases the
    // holder; the end of the lifetime it holds is what releases cleanup.
    let early = completion.recv_timeout(Duration::from_millis(50));
    if let Ok((waited, _)) = early {
        panic!("cleanup ({what}) ended after {waited:?}, before its holder's final write");
    }
    let scratch_present = base.is_dir();
    let released = Instant::now();
    writeln!(release, "finish").unwrap();
    let ended = completion.recv_timeout(BACKSTOP);
    // A cleanup still running at the backstop, which only a stall reaches, fails here: joining it
    // would wait unbounded.
    if matches!(ended, Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
        panic!(
            "{}",
            stalled(
                &format!("cleanup ({what}) to end after its release"),
                BACKSTOP
            )
        );
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
