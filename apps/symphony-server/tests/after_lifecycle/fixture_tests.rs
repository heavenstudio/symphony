//! The fixture's own cases: its server log tail, its timeout reports and its traced starts.

use super::*;

#[test]
fn generic_fixture_supplies_only_its_owned_expensive_build_standin() {
    let repo = Repo::new("generic-build-input");
    assert_eq!(
        fs::read_to_string(repo.main.join("justfile")).unwrap(),
        "\nsymphony-ui-dist OUT=\"build/symphony-ui\":\n    @tools/fixture-build ui \"{{OUT}}\"\n"
    );
}

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
fn native_fixture_children_use_the_owned_spawn_boundary() {
    forkless::check(false);
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
fn ordinary_starts_and_traced_stop_and_serve_commands_have_no_startup_trace() {
    let repo = Repo::new("quiet-startup");
    let mut command = repo.command(&repo.main);
    command.env_remove("SYMPHONY_STARTUP_TRACE");
    let mut starter = repo.run(command);
    starter.success();
    repo.remember();
    let running = repo.check_server();
    assert_eq!(fs::read_to_string(&starter.stderr).unwrap(), "");
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert!(!log.contains("· Symphony startup "), "{log}");

    let mut command = repo.command(&repo.main);
    command.arg("--stop");
    let mut stop = repo.run(command);
    stop.success();
    // The stop says the one signal it sends, and nothing of a startup.
    assert_eq!(
        fs::read_to_string(&stop.stderr).unwrap(),
        format!(
            "· Symphony: SIGTERM to the server, pid {}: asked to stop (`--stop`)\n",
            running.pid
        )
    );

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
