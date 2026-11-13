//! What `Process::success` reports for a child that fails: its pid, command, status and output,
//! and for a silent signal its executable's identity at spawn and at failure.

use super::*;

#[test]
fn unsuccessful_children_report_their_pid_command_status_and_output() {
    let scratch = Scratch::new("unsuccessful-child");
    for (index, (script, status, output)) in [
        ("exit 7", "exit status: 7", ""),
        (
            "printf 'child stdout'; printf 'child stderr' >&2; exit 9",
            "exit status: 9",
            "child stdoutchild stderr",
        ),
        ("kill -TERM $$", "signal: 15", ""),
        (
            "printf 'signal stdout'; kill -TERM $$",
            "signal: 15",
            "signal stdout",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        let mut process = Process::spawn(command, &scratch.path.join(index.to_string()));
        let pid = process.child.id();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process.success();
        }));
        let failure = result.expect_err("unsuccessful child must fail success()");
        let message = failure.downcast_ref::<String>().unwrap();
        assert!(message.starts_with(&format!("child {pid} (\"/bin/sh\" [\"-c\", ")));
        assert!(message.contains(script), "{message}");
        assert!(message.contains(status), "{message}");
        let kept = scratch.path.join(format!("{index}.killed"));
        if status.starts_with("signal") {
            // The shell signalled itself, which its Process did not: the failure ends with what
            // was known of its pid then, kept beside its logs.
            let capture = fs::read_to_string(&kept).unwrap();
            assert!(
                capture.starts_with(&format!(
                    "child {pid} (\"/bin/sh\" [\"-c\", \"{script}\"]) ended by SIGTERM (signal 15), \
                     which its Process did not send;"
                )),
                "{capture}"
            );
            assert!(
                message.ends_with(&format!(": {output}\n{capture}")),
                "{message}"
            );
        } else {
            assert!(message.ends_with(&format!(": {output}")), "{message}");
            assert!(!kept.exists(), "an exit captured {}", kept.display());
        }
        if status == "signal: 15" && output.is_empty() {
            let meta = fs::metadata("/bin/sh").unwrap();
            let identity = format!(
                "inode={} mtime={}.{:09}",
                meta.ino(),
                meta.mtime(),
                meta.mtime_nsec()
            );
            assert!(
                message.contains(&format!("at spawn: {identity}; at failure: {identity}")),
                "{message}"
            );
        } else {
            assert!(!message.contains("at spawn:"), "{message}");
        }
    }
}

#[test]
fn a_silent_signal_reports_the_original_and_current_executable_identity() {
    use std::os::unix::fs::PermissionsExt;
    let script = "#!/bin/sh\nprintf ready > \"$1\"\nread -r release\nkill -TERM $$\n";
    for removed in [false, true] {
        let scratch = Scratch::new("signal-image");
        let executable = scratch.path.join("child");
        let entered = scratch.path.join("entered");
        write(&executable, script);
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        File::open(&executable)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1000))
            .unwrap();
        let before = fs::metadata(&executable).unwrap();
        // A new file's first exec waits for macOS to assess it, one file at a time machine-wide.
        // That is setup, as for the server image (prepare_app): run this file once, with no input
        // and a scratch argument, to its own end, before the timed wait starts it again. The
        // at-spawn identity asserted below must still be `before`'s.
        let first_run = scratch.path.join("first-run");
        let prepared =
            symphony_board::sys::output(Command::new(&executable).arg(&first_run).env_clear())
                .unwrap();
        assert_eq!(
            fs::read_to_string(&first_run).ok().as_deref(),
            Some("ready"),
            "the first run must reach the script's body: {}",
            said(&prepared)
        );
        let mut command = Command::new(&executable);
        command.arg(&entered).stdin(Stdio::piped());
        let mut child = Process::spawn(command, &scratch.path.join("process"));
        until("silent child entered its input wait", || {
            (fs::read_to_string(&entered).ok().as_deref() == Some("ready")).then_some(())
        });
        // Replace/unlink the path, never the running image's bytes. Input releases the owned
        // child only after the new path state is established; no startup timing is assumed.
        let after = if removed {
            fs::remove_file(&executable).unwrap();
            None
        } else {
            let replacement = scratch.path.join("replacement");
            write(&replacement, script);
            fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();
            File::open(&replacement)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(2000))
                .unwrap();
            fs::rename(&replacement, &executable).unwrap();
            Some(fs::metadata(&executable).unwrap())
        };
        writeln!(child.child.stdin.as_mut().unwrap(), "release").unwrap();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child.success()))
            .expect_err("a signalled child must still fail success()");
        let message = failure.downcast_ref::<String>().unwrap();
        assert!(message.contains("signal: 15"), "{message}");
        assert!(
            message.contains(&format!("executable {executable:?}")),
            "{message}"
        );
        assert!(
            message.contains(&format!(
                "at spawn: inode={} mtime=1000.000000000",
                before.ino()
            )),
            "{message}"
        );
        if let Some(after) = after {
            assert_ne!(before.ino(), after.ino());
            assert!(
                message.contains(&format!(
                    "at failure: inode={} mtime=2000.000000000",
                    after.ino()
                )),
                "{message}"
            );
        } else {
            assert!(message.contains("at failure: unavailable:"), "{message}");
        }
        assert!(child.output().is_empty());
    }
}

/// A child that waits on its input until something ends it, said ready once it reads, and the
/// file a capture of its end is kept in.
fn waiting_child(scratch: &Scratch, name: &str) -> (Process, PathBuf) {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "printf ready; read -r line"])
        .stdin(Stdio::piped());
    let log = scratch.path.join(name);
    let process = Process::spawn(command, &log);
    until("the child reads its input", || {
        (process.output() == "ready").then_some(())
    });
    (process, log.with_extension("killed"))
}

/// What `process.success()` panics with.
fn failure_of(process: &mut Process) -> String {
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process.success()))
        .expect_err("a signalled child must fail success()");
    failure.downcast_ref::<String>().unwrap().clone()
}

/// A process other than the test kills the child, as a kill by a pid the kernel gave the child
/// after its first owner ended would: the failure says its Process did not send it, and what was
/// known of the pid then, as the scratch folder keeps it.
#[test]
fn a_child_another_process_kills_names_what_was_known_of_its_pid() {
    let scratch = Scratch::new("killed-by-a-helper");
    let (mut process, kept) = waiting_child(&scratch, "victim");
    let pid = process.child.id();
    let helper =
        symphony_board::sys::output(Command::new("/bin/kill").args(["-KILL", &pid.to_string()]))
            .unwrap();
    assert!(helper.status.success(), "{}", said(&helper));
    let message = failure_of(&mut process);
    let capture = fs::read_to_string(&kept).unwrap();
    assert!(
        message.ends_with(&format!(": ready\n{capture}")),
        "{message}"
    );
    assert!(
        capture.starts_with(&format!(
            "child {pid} (\"/bin/sh\" [\"-c\", \"printf ready; read -r line\"]) ended by SIGKILL \
             (signal 9), which its Process did not send; what was known of pid {pid} at "
        )),
        "{capture}"
    );
    assert!(
        capture.contains(&format!(
            " ms since Unix epoch, kept in {}:\n",
            kept.display()
        )),
        "{capture}"
    );
    assert!(
        capture.contains(&format!(
            "\n· signals this test process sent to pid {pid}: none\n"
        )),
        "{capture}"
    );
    assert!(
        capture.contains(&format!(
            "\n· lines naming pid {pid} in this test process's scratch logs"
        )),
        "{capture}"
    );
    if cfg!(target_os = "macos") {
        let memory = "\n· memory, swap and load (sysctl, vm_stat):\n";
        assert!(capture.contains(memory), "{capture}");
        for said in [
            "  vm.swapusage: total = ",
            "  kern.memorystatus_level: ",
            "  Pages free: ",
        ] {
            assert!(capture.contains(said), "{said}: {capture}");
        }
        let alive = format!(
            "\n· this test's processes alive (descendants of pid {}, ",
            std::process::id()
        );
        assert!(capture.contains(&alive), "{capture}");
        // This process's own row is among them, and the child's pid has none: it was reaped.
        assert!(
            capture
                .lines()
                .any(|line| line.starts_with(&format!("  {} ", std::process::id()))),
            "{capture}"
        );
        assert!(
            capture.contains(&format!("\n  pid {pid}: no row (ended and reaped)\n")),
            "{capture}"
        );
        assert!(
            capture.contains(&format!("\n· the system log for pid {pid} from ")),
            "{capture}"
        );
    } else {
        assert!(
            capture.ends_with("nothing captured, it is read on macOS only\n"),
            "{capture}"
        );
    }
}

/// A signal this test process sent the pid by its number alone, as one case's kill of a pid
/// another case's child was given would be, is the capture's first line of evidence.
#[test]
fn a_child_this_test_process_signalled_by_its_pid_alone_names_that_signal() {
    let scratch = Scratch::new("killed-by-pid");
    let (mut process, kept) = waiting_child(&scratch, "victim");
    let pid = process.child.id();
    killed::send(i64::from(pid), libc::SIGTERM, "another case's kill by pid");
    let message = failure_of(&mut process);
    let capture = fs::read_to_string(&kept).unwrap();
    assert!(
        message.ends_with(&format!(": ready\n{capture}")),
        "{message}"
    );
    assert!(
        capture.contains(&format!(
            "\n· signals this test process sent to pid {pid}:\n  at "
        )),
        "{capture}"
    );
    assert!(
        capture.contains(&format!(
            ": SIGTERM (signal 15) to pid {pid}: another case's kill by pid\n"
        )),
        "{capture}"
    );
}

/// A signal the child's own Process sent captures nothing: the case meant it.
#[test]
fn a_child_its_process_signals_captures_nothing() {
    let scratch = Scratch::new("killed-by-its-process");
    let (mut process, kept) = waiting_child(&scratch, "victim");
    process.signal(libc::SIGKILL);
    assert_eq!(process.wait().signal(), Some(libc::SIGKILL));
    let message = failure_of(&mut process);
    assert!(message.ends_with(": ready"), "{message}");
    assert!(!kept.exists(), "{}", kept.display());
}
