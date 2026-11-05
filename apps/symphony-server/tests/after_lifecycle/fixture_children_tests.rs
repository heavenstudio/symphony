//! The fixture's own cases: its shared preparation, its descendants and its children's reports.

use super::*;

#[test]
fn fixture_repositories_share_one_completed_app_and_command_preparation() {
    let first = Repo::new("prepared-first");
    let pid = *PREPARED_APP
        .get()
        .expect("Repo::new prepares the real image");
    assert!(pid > 0);
    let second = Repo::new("prepared-second");
    assert_eq!(prepare_app(&first.log("unused-preparation")), pid);
    assert_eq!(prepare_app(&second.log("unused-preparation")), pid);
    let commands = *PREPARED_COMMANDS
        .get()
        .expect("Repo::new prepares every fixture command image");
    assert!(commands.iter().all(|pid| *pid > 0));
    // A second preparation would create its owned process logs at one of these paths.
    for repo in [&first, &second] {
        let unused = repo.base.join("unused-command-preparation");
        assert_eq!(prepare_commands(repo, &unused), commands);
        assert!(
            !unused.exists(),
            "prepared command images must not launch again"
        );
        assert!(
            !repo.base.join("unused-preparation-0.out").exists(),
            "a prepared image must not be launched again by later fixtures"
        );
    }
}

#[test]
fn a_readiness_timeout_names_its_child_and_reports_late_open_stage() {
    let scratch = Scratch::new("readiness-timeout");
    let mut command = Command::new("/bin/sh");
    command
            .args([
                "-c",
                "read -r line; printf 'startup stdout'; printf '\\n· review timing start: hub branch trees\\n' >&2; read -r line",
            ])
            .stdin(Stdio::piped());
    let mut process = Process::spawn(command, &scratch.path.join("startup"));
    let pid = process.child.id();
    let stdout = process.stdout.clone();
    let stderr = process.stderr.clone();
    let mut input = process.child.stdin.take().unwrap();
    process.wait_elapsed = Box::new(move |_| {
        writeln!(input, "report progress").unwrap();
        until("startup output", || {
            (output(&stdout, &stderr)
                == "startup stdout\n· review timing start: hub branch trees\n")
                .then_some(())
        });
        PATIENCE
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process.ready()));
    drop(process);
    let failure = failure.expect_err("a child without a readiness event must time out");
    assert_eq!(
        failure.downcast_ref::<String>().unwrap(),
        &format!(
            "timed out: native server readiness for child {pid}: startup stdout\n· review timing start: hub branch trees\n"
        )
    );
}

#[test]
fn a_descendant_that_keeps_its_endpoint_fails_with_its_launch_identity() {
    let scratch = Scratch::new("descendant-timeout");
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "printf ready; read -r release"])
        .stdin(Stdio::piped());
    let mut process = Process::spawn(command, &scratch.path.join("held"));
    until("descriptor holder readiness", || {
        (process.output() == "ready").then_some(())
    });
    let pid = process.child.id();
    process.descendants.as_mut().unwrap().wait_elapsed = Box::new(|_| PATIENCE);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process.descendants.as_mut().unwrap().wait();
    }));
    // The clock reaches the unchanged deadline while this exact child still holds its end.
    // Drop kills the child and waits on these same Descendants: give them the real clock back,
    // so an EOF trailing the reap (a concurrent fork's copy of the endpoint) is waited for.
    process.descendants.as_mut().unwrap().wait_elapsed = Box::new(Instant::elapsed);
    drop(process);
    let failure = failure.expect_err("a live holder must fail the bounded EOF wait");
    assert_eq!(
        failure.downcast_ref::<String>().unwrap(),
        &format!(
            "timed out: descendants of child {pid} (\"/bin/sh\" [\"-c\", \"printf ready; read -r release\"]) to exit before scratch removal"
        )
    );
}

#[test]
fn cleanup_waits_for_orphaned_fixture_descendants_before_removing_scratch() {
    for repository in [false, true] {
        let repo = Repo::new("descendant-cleanup");
        let base = repo.base.clone();
        let mut command = Command::new("/bin/sh");
        // The launcher exits while its child holds the lifetime endpoint and waits for the
        // test's input. The child's final write must finish before scratch can disappear.
        command
                .args([
                    "-c",
                    "exec 3<&0; sh -c 'printf ready; read -r line; printf finished > \"$1\"' fixture \"$1\" <&3 & exit",
                    "fixture",
                ])
                .arg(base.join("late-write"))
                .stdin(Stdio::piped());
        let mut process = if repository {
            repo.run(command)
        } else {
            Process::spawn(command, &repo.log("orphan"))
        };
        let mut input = process.child.stdin.take().unwrap();
        process.success();
        until("orphaned fixture readiness", || {
            (process.output() == "ready").then_some(())
        });
        let (finished, completion) = std::sync::mpsc::channel();
        let cleanup = std::thread::spawn(move || {
            drop(process);
            drop(repo);
            finished.send(()).unwrap();
        });
        // A bounded negative observation, not cleanup's synchronization. Input releases the
        // descendant; EOF on its inherited endpoint is what releases cleanup.
        let early = completion.recv_timeout(Duration::from_millis(50));
        let scratch_present = base.is_dir();
        writeln!(input, "finish").unwrap();
        completion.recv_timeout(PATIENCE).unwrap();
        cleanup.join().unwrap();
        assert_eq!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout));
        assert!(
            scratch_present,
            "scratch removed while a descendant could write"
        );
        assert!(!base.exists());
    }
}

/// Another case's fixture command can be spawned while this one makes its endpoint. Racing
/// children live on, so a writer one took would hold back that endpoint's EOF until cleanup.
#[test]
fn a_spawn_racing_endpoint_creation_does_not_inherit_the_endpoint() {
    const RACERS: usize = 16;
    let scratch = Scratch::new("racing-spawn");
    let start = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let spawner = scope.spawn(|| {
            start.wait();
            (0..RACERS)
                .map(|index| {
                    // cat waits on its input until its Process is dropped.
                    let mut command = Command::new("/bin/cat");
                    command.stdin(Stdio::piped());
                    Process::spawn(command, &scratch.path.join(format!("racer-{index}")))
                })
                .collect::<Vec<_>>()
        });
        start.wait();
        for made in 0.. {
            let (mut endpoint, writer) = Descendants::endpoint(
                &scratch.path.join(format!("endpoint-{made}")),
                format!("racing endpoint {made}"),
            );
            drop(writer);
            // A fork between its own fork and exec holds the writer only until that exec.
            endpoint.wait();
            if spawner.is_finished() {
                break;
            }
        }
        for racer in &mut spawner.join().unwrap() {
            assert!(
                racer.child.try_wait().unwrap().is_none(),
                "racer {} ended before every endpoint was observed",
                racer.child.id()
            );
        }
    });
}

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

#[test]
fn timed_out_wait_reports_output_written_after_polling_begins() {
    assert_eq!(
        poll(
            Duration::ZERO,
            || panic!("successful waits need no diagnostic"),
            || Some(7),
            Instant::elapsed,
        ),
        7
    );
    let scratch = Scratch::new("wait-timeout");
    let logs = scratch.path.join("timeout");
    let mut command = Command::new("/bin/sh");
    // The first input releases both writes; the second read keeps this exact child pending.
    command
        .args([
            "-c",
            r"read -r line; printf 'late stdout\n'; printf 'late stderr\n' >&2; read -r line",
        ])
        .stdin(Stdio::piped());
    let mut process = Process::spawn(command, &logs);
    let server_log = scratch.path.join("detached-server.log");
    process.server_log = Some(server_log.clone());
    let id = process.child.id();
    assert!(process.child.try_wait().unwrap().is_none());
    assert!(process.output().is_empty());
    let stdout = process.stdout.clone();
    let stderr = process.stderr.clone();
    let mut input = process.child.stdin.take().unwrap();
    let detached = server_log.clone();
    process.wait_elapsed = Box::new(move |_: &Instant| {
        // The clock is read after Process::wait has begun and observed its pending child.
        // Release and observe real child output, then reach the unchanged wait deadline.
        writeln!(input, "emit").unwrap();
        until("late child stdout and stderr", || {
            (output(&stdout, &stderr) == "late stdout\nlate stderr\n").then_some(())
        });
        write(&detached, "late detached server progress\n");
        PATIENCE
    });
    // The expected timeout leaves Process owning its child, which is cleaned up below.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process.wait()));
    let launch = process.launch.clone();
    drop(process);
    let timeout = result.unwrap_err();
    let message = timeout.downcast_ref::<String>().unwrap();
    assert_eq!(
        message,
        &format!(
            "timed out: child {id} to exit; {launch}: late stdout\nlate stderr\n\ndetached server log {}:\nlate detached server progress\n",
            server_log.display()
        )
    );
}
