//! The fixture's own cases: its server log tail, its timeout reports and its traced starts.

use super::*;

#[test]
fn server_log_tail_bounds_a_large_log_and_keeps_its_latest_progress() {
    let scratch = Scratch::new("large-server-log-tail");
    let path = scratch.path.join("server.log");
    let latest = "\nlatest detached server progress\n";
    let mut bytes = vec![b'x'; 128 * 1024];
    bytes.extend_from_slice(latest.as_bytes());
    fs::write(&path, bytes).unwrap();
    let tail = server_log_tail(&path).unwrap();
    let label = "[showing last 16384 bytes]\n";
    assert_eq!(tail.len(), label.len() + 16384);
    assert_eq!(
        tail,
        format!("{label}{}{latest}", "x".repeat(16384 - latest.len()))
    );
}

#[test]
fn server_log_tail_preserves_progress_on_invalid_and_split_utf8() {
    let scratch = Scratch::new("server-log-tail-utf8");
    let path = scratch.path.join("server.log");
    let mut split = vec![b'x'; 16384];
    split.extend_from_slice("é".as_bytes());
    split.extend(std::iter::repeat_n(b'y', 16383));
    for (bytes, expected) in [
        (
            b"early \xff progress\nlatest progress\n".to_vec(),
            "early � progress\nlatest progress\n".to_owned(),
        ),
        (
            split,
            format!("[showing last 16384 bytes]\n�{}", "y".repeat(16383)),
        ),
    ] {
        fs::write(&path, bytes).unwrap();
        assert_eq!(server_log_tail(&path).unwrap(), expected);
    }
}

#[test]
fn server_log_tail_refuses_nonregular_files_without_a_writer() {
    let scratch = Scratch::new("nonregular-server-log-tail");
    let directory = scratch.path.join("directory");
    fs::create_dir(&directory).unwrap();
    let fifo = scratch.path.join("fifo");
    mkfifo(&fifo);
    for path in [&directory, &fifo] {
        let error = server_log_tail(path).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "not a regular file");
    }

    let regular = scratch.path.join("regular");
    write(&regular, "owned progress\n");
    assert_eq!(server_log_tail(&regular).unwrap(), "owned progress\n");
    let linked = scratch.path.join("linked");
    symlink(&regular, &linked).unwrap();
    assert_eq!(
        server_log_tail(&linked).unwrap_err().raw_os_error(),
        Some(libc::ELOOP)
    );
}

#[test]
fn timeout_diagnostics_distinguish_an_unwritten_server_log_from_an_empty_one() {
    let scratch = Scratch::new("server-log-diagnostics");
    let stdout = scratch.path.join("stdout");
    let stderr = scratch.path.join("stderr");
    let log = scratch.path.join("server.log");
    write(&stdout, "starter stdout\n");
    write(&stderr, "starter stderr\n");
    let streams = "starter stdout\nstarter stderr\n";
    assert_eq!(diagnostic_output(&stdout, &stderr, None), streams);
    let label = format!("\ndetached server log {}:\n", log.display());
    assert!(
        diagnostic_output(&stdout, &stderr, Some(&log))
            .starts_with(&format!("{streams}{label}unavailable: "))
    );
    write(&log, "");
    assert_eq!(
        diagnostic_output(&stdout, &stderr, Some(&log)),
        format!("{streams}{label}")
    );
}

#[test]
fn a_traced_start_names_its_detached_server_waits() {
    let repo = Repo::new("traced-detached-waits");
    let starter = repo.start(&repo.main, false);
    let text = starter.output();
    let mut previous = 0;
    for stage in [
        "begin spawn detached server",
        "end spawn detached server:",
        "begin detached server port announcement",
        "end detached server port announcement:",
        "begin detached server HTTP readiness",
        "end detached server HTTP readiness:",
    ] {
        let position = text
            .find(stage)
            .unwrap_or_else(|| panic!("missing {stage}: {text}"));
        assert!(position >= previous, "startup stages out of order: {text}");
        previous = position;
    }
}

/// A child std forks keeps every end this process holds until its exec, a sibling case's
/// readiness pipe or lifetime endpoint among them, and no wait can shorten that.
#[cfg(target_os = "macos")]
#[test]
fn no_fixture_child_forks() {
    use std::cell::Cell;
    thread_local! {
        /// The forks this thread made, counted by the handler `fork` runs before it forks.
        static FORKS: Cell<u32> = const { Cell::new(0) };
    }
    extern "C" fn prepare() {
        FORKS.set(FORKS.get() + 1);
    }
    static HANDLER: Once = Once::new();
    HANDLER.call_once(|| {
        // SAFETY: the handler only bumps the forking thread's count; it is registered once.
        let registered = unsafe { libc::pthread_atfork(Some(prepare), None, None) };
        assert_eq!(registered, 0);
    });
    {
        // The repository's setup, a recipe's start (a bare `just` in a cleared environment),
        // a piped child, a terminal's, and the drop's stop and descendant waits.
        let repo = Repo::new("forkless");
        repo.start(&repo.tree, true);
        let mut command = Command::new("/bin/cat");
        command.stdin(Stdio::piped());
        let mut piped = repo.run(command);
        piped
            .child
            .stdin
            .take()
            .unwrap()
            .write_all(b"echo")
            .unwrap();
        piped.success();
        assert_eq!(piped.output(), "echo");
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "test -t 1"]);
        let (mut terminal, _master) = repo.run_terminal(command);
        terminal.success();
    }
    assert_eq!(FORKS.get(), 0, "a fixture child forked");
}

#[test]
fn the_recipe_observer_uses_the_interpreter_from_the_original_path() {
    let repo = Repo::new("recipe-interpreter");
    let original = repo.base.join("original-path");
    fs::create_dir(&original).unwrap();
    // An immutable executable distinguishes PATH selection from a hard-coded /bin/bash:
    // false exits 1, whereas bash -c true exits 0.
    symlink("/usr/bin/false", original.join("bash")).unwrap();
    let mut command = Command::new(repo.bin.join("bash"));
    command
        .env_clear()
        .env(
            "PATH",
            format!(
                "{}:{}:/usr/bin:/bin",
                repo.bin.display(),
                original.display()
            ),
        )
        .args(["-c", "true"]);
    let mut observer = repo.run(command);
    assert_eq!(observer.wait().code(), Some(1), "{}", observer.output());
}

#[test]
fn a_recipe_timeout_reports_runner_bash_and_the_pending_build_command() {
    use std::os::unix::ffi::OsStrExt;

    struct ReleaseLog(PathBuf);
    impl Drop for ReleaseLog {
        fn drop(&mut self) {
            // An earlier assertion may unwind before the normal release. Unblock any
            // opening writer before Repo waits on descendants, including on that path.
            let _ = fs::OpenOptions::new().read(true).write(true).open(&self.0);
        }
    }

    let repo = Repo::new("recipe-stage-timeout");
    // The build's real log is a FIFO with no reader yet. Its opening writer waits here,
    // giving the report a stable pending recipe stage without spending the wall deadline.
    let name = std::ffi::CString::new(repo.builds.as_os_str().as_bytes()).unwrap();
    // SAFETY: a terminated path in this test's owned scratch folder.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let _release_on_failure = ReleaseLog(repo.builds.clone());
    let mut starter = repo.run(repo.recipe(&repo.tree));
    until("recipe's pending UI build", || {
        let said = starter.output();
        (said.contains("tools/fixture-build ui ")
            && !said.contains("---> symphony-ui-dist completed"))
        .then_some(())
    });
    starter.wait_elapsed = Box::new(|_| PATIENCE);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| starter.wait()));
    // Release the real writer, then let the same start finish and own its server normally.
    let _release = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&repo.builds)
        .unwrap();
    starter.wait_elapsed = Box::new(Instant::elapsed);
    starter.success();
    repo.remember();
    repo.check_server();

    let failure = failure.expect_err("the pending build must fail the exit wait");
    let message = failure.downcast_ref::<String>().unwrap();
    assert!(message.contains(&starter.launch), "{message}");
    assert!(
        message.contains("script executable=no"),
        "bash must read the recipe without executing a fresh temporary image: {message}"
    );
    for stage in [
        "===> running recipe `symphony`...",
        "· symphony recipe bash entered at ",
        "+ symphony recipe ",
        "_symphony-build",
        "===> running recipe `symphony-ui-dist`...",
        "tools/fixture-build ui ",
    ] {
        assert!(
            message.contains(stage),
            "missing recipe stage {stage}: {message}"
        );
    }
    assert!(
        !message.contains("---> symphony-ui-dist completed"),
        "{message}"
    );
    assert!(!message.contains("start command entered at"), "{message}");
}

#[test]
fn ordinary_starts_and_traced_stop_and_serve_commands_have_no_startup_trace() {
    let repo = Repo::new("quiet-startup");
    let mut command = repo.command(&repo.main);
    command.env_remove("SF_SYMPHONY_STARTUP_TRACE");
    let mut starter = repo.run(command);
    starter.success();
    repo.remember();
    repo.check_server();
    assert_eq!(fs::read_to_string(&starter.stderr).unwrap(), "");
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert!(!log.contains("· Symphony startup "), "{log}");

    let mut command = repo.command(&repo.main);
    command.arg("--stop");
    let mut stop = repo.run(command);
    stop.success();
    assert_eq!(fs::read_to_string(&stop.stderr).unwrap(), "");

    // Pass the trace variable to serve directly, rather than relying on Home's filter.
    let mut command = repo.command(&repo.main);
    command.args([
        "serve",
        "--root",
        repo.main.to_str().unwrap(),
        "--port",
        "0",
        "--host",
        "127.0.0.1",
    ]);
    let mut server = repo.run(command);
    server.ready();
    let said = server.output();
    assert!(!said.contains("· Symphony startup "), "{said}");
}

#[test]
fn a_starter_timeout_keeps_the_requested_pending_stage() {
    let repo = Repo::new("traced-startup-timeout");
    fs::create_dir_all(&repo.home).unwrap();
    let lock = File::create(repo.home.join("lock")).unwrap();
    lock.lock().unwrap();
    let mut starter = repo.run(repo.command(&repo.main));
    until("starter waiting on its startup lock", || {
        starter
            .output()
            .contains("begin startup lock")
            .then_some(())
    });
    starter.wait_elapsed = Box::new(|_| PATIENCE);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| starter.wait()));
    let launch = starter.launch.clone();
    drop(starter);
    let failure = failure.expect_err("the held startup lock keeps the starter pending");
    let said = failure.downcast_ref::<String>().unwrap();
    assert!(said.contains(&launch), "{said}");
    assert!(said.contains("end checkout discovery:"), "{said}");
    assert!(said.contains("begin startup lock"), "{said}");
    assert!(!said.contains("end startup lock:"), "{said}");
    assert!(!said.contains("begin spawn detached server"), "{said}");
}
