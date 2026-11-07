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
        assert!(message.ends_with(&format!(": {output}")), "{message}");
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
