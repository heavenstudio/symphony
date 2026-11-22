//! The tests' server start ([`launch`]): it comes up whatever is left of the folder its log goes in,
//! a server that ends before it answers is reported with its exit status and what it and its shell
//! said, it keeps its data on the engine it is handed ([`launch_on`], [`store_held_in`]), and it
//! reaches this machine alone, for the gates a statement waits at. A test's own server on the
//! board's engine ([`BoardServer`]) holds its store in a folder of its own, which goes with it, or
//! with the next run when its process was killed.

use crate::common::{
    BoardServer, client, launch, launch_on, remove_ended_board_folders, store_held_in, surreal,
};
use crate::gate::Gate;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use symphony_board::rpc::{self, Client};

/// A folder of this test's own, empty.
fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("start-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn the_server_comes_up_with_its_log_in_a_folder_that_was_not_there() {
    let dir = folder("absent");
    let log = dir.join("tmp/surreal.log");
    let (addr, mut shell) = launch(&surreal(), &log).unwrap_or_else(|why| panic!("{why}"));
    assert!(rpc::healthy(&addr), "the server answers on {addr}");
    assert!(log.is_file(), "the log is at {}", log.display());
    shell.shutdown().unwrap();
    assert!(!rpc::healthy(&addr), "the server ends with its shell");
    std::fs::remove_dir_all(&dir).unwrap();
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_server_that_ends_at_once_is_reported_with_its_status_and_what_it_said() {
    let dir = folder("ends");
    let binary = dir.join("surreal");
    crate::fixture_script::write(
        &binary,
        "#!/bin/sh\necho 'the datastore cannot start'\nexit 3\n",
        false,
    );
    let why = launch(&binary, &dir.join("surreal.log")).unwrap_err();
    assert_eq!(
        why.to_string(),
        "it ended before it answered: exit status: 3; it said: the datastore cannot start\n"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_server_binary_that_is_not_there_is_reported_with_what_the_shell_said() {
    let dir = folder("missing");
    let binary = dir.join("surreal");
    let why = launch(&binary, &dir.join("surreal.log"))
        .unwrap_err()
        .to_string();
    assert!(
        why.starts_with("it ended before it answered: exit status: 127; it said: "),
        "{why}"
    );
    assert!(
        why.ends_with(&format!(
            "{}: No such file or directory\n",
            binary.display()
        )),
        "{why}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_server_whose_port_is_taken_on_each_start_is_started_on_another_until_the_bound() {
    let dir = folder("taken");
    let binary = dir.join("surreal");
    crate::fixture_script::write(
        &binary,
        "#!/bin/sh\nprintf '%s\\n' \"$4\" >> \"$0.starts\"\necho 'Address already in use (os error 48)'\nexit 1\n",
        false,
    );
    let why = launch(&binary, &dir.join("surreal.log")).unwrap_err();
    assert_eq!(
        why.to_string(),
        "its port was taken on each of 3 starts; the last one said: exit status: 1; it said: \
         Address already in use (os error 48)\n"
    );
    let starts = std::fs::read_to_string(dir.join("surreal.starts")).unwrap();
    let addresses: std::collections::BTreeSet<&str> = starts.lines().collect();
    assert_eq!(
        (starts.lines().count(), addresses.len()),
        (3, 3),
        "{starts}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_server_on_a_surrealkv_engine_keeps_its_rows_in_that_folder_for_the_next_one() {
    let dir = folder("surrealkv");
    let engine = format!("surrealkv://{}", dir.join("data").display());
    let log = dir.join("surreal.log");
    let up = || launch_on(&surreal(), &log, &engine).unwrap_or_else(|why| panic!("{why}"));
    let (addr, mut shell) = up();
    assert_eq!(store_held_in(&engine), Ok(()));
    Client::new(&addr, "sf", "kept")
        .define_then("CREATE kept:row SET n = 1 RETURN NONE;", json!({}))
        .unwrap();
    shell.shutdown().unwrap();
    let ended = store_held_in(&engine).unwrap_err();
    assert!(ended.contains("nothing holds its lock"), "{ended}");
    let (addr, mut shell) = up();
    let kept = Client::new(&addr, "sf", "kept")
        .query("RETURN (SELECT VALUE n FROM ONLY kept:row);", json!({}))
        .unwrap();
    assert_eq!(
        kept,
        vec![json!(1)],
        "the next server on the folder reads the row"
    );
    shell.shutdown().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_server_up_on_another_engine_than_the_one_named_fails_its_start_with_why() {
    let dir = folder("elsewhere");
    let binary = dir.join("surreal");
    // The real server, on its address (the fourth word), on the in-memory engine whatever it is
    // handed.
    crate::fixture_script::write(
        &binary,
        &format!(
            "#!/bin/sh\nexec '{}' start --unauthenticated --bind \"$4\" --log error memory\n",
            surreal().display()
        ),
        false,
    );
    let data = dir.join("data");
    let engine = format!("surrealkv://{}", data.display());
    let log = dir.join("surreal.log");
    let refused = std::panic::catch_unwind(|| launch_on(&binary, &log, &engine))
        .expect_err("a server on another engine than the one named is refused");
    let why = refused
        .downcast_ref::<String>()
        .expect("the refusal says why");
    assert!(
        why.starts_with("the SurrealDB server on 127.0.0.1:")
            && why.contains(&format!(
                ": it holds no store in {}: its lock {} cannot be opened (",
                data.display(),
                data.join("LOCK").display()
            ))
            && why.contains(&format!(
                "so it runs on an engine other than {engine}; its log ({}):\n",
                log.display()
            )),
        "{why}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_store_is_held_where_its_engine_names_only_while_its_lock_is_taken() {
    let dir = folder("held");
    let engine = format!("surrealkv://{}", dir.display());
    let lock = dir.join("LOCK");
    let elsewhere = format!("so it runs on an engine other than {engine}");
    let unopened = store_held_in(&engine).unwrap_err();
    assert!(
        unopened.starts_with(&format!(
            "it holds no store in {}: its lock {} cannot be opened (",
            dir.display(),
            lock.display()
        )) && unopened.ends_with(&format!("), {elsewhere}")),
        "{unopened}"
    );
    std::fs::write(&lock, "").unwrap();
    let free = Err(format!(
        "it holds no store in {}: nothing holds its lock {}, {elsewhere}",
        dir.display(),
        lock.display()
    ));
    assert_eq!(store_held_in(&engine), free);
    // The lock taken through another opening of the file, as the server's store takes it.
    let holder = std::fs::File::open(&lock).unwrap();
    holder.lock().unwrap();
    assert_eq!(store_held_in(&engine), Ok(()));
    assert_eq!(
        store_held_in(&engine),
        Ok(()),
        "a look takes nothing from the holder"
    );
    drop(holder);
    assert_eq!(store_held_in(&engine), free, "a lock let go holds nothing");
    assert_eq!(
        store_held_in("memory"),
        Ok(()),
        "an engine naming no folder"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A lock the system will not take on that file at all is no answer either way: said as such, not
/// as a store held or free. macOS refuses `flock` on a FIFO (`EOPNOTSUPP`).
#[cfg(target_os = "macos")]
// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_store_lock_that_cannot_be_tried_is_said_so() {
    let dir = folder("untried");
    let lock = dir.join("LOCK");
    let path = std::ffi::CString::new(lock.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: a NUL-terminated path, alive through the call.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    // A writer of its own, so the look's opening of it for reading does not wait for one.
    let _writer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
    let engine = format!("surrealkv://{}", dir.display());
    let why = store_held_in(&engine).unwrap_err();
    assert!(
        why.starts_with(&format!(
            "its store's lock {} cannot be tried: ",
            lock.display()
        )),
        "{why}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn the_tests_server_reaches_a_gate_on_this_machine_and_no_other_host() {
    let db = client();
    let gate = Gate::new();
    gate.open();
    let passed = db
        .define_then(&format!("{} RETURN 'passed';", gate.wait_here()), json!({}))
        .unwrap();
    assert_eq!(passed.last(), Some(&Value::from("passed")));
    gate.reached();
    let elsewhere = db
        .define_then("RETURN http::get('http://192.0.2.1/');", json!({}))
        .unwrap_err()
        .to_string();
    assert!(
        elsewhere.contains("Access to network target '192.0.2.1:80' is not allowed"),
        "{elsewhere}"
    );
}

// SurrealDB alone: exercises the SurrealDB process launcher, its engine flags, locks and HTTP gates.
#[test]
fn a_board_server_holds_its_store_in_a_folder_of_its_own_that_goes_with_it() {
    let (one, two) = (BoardServer::start(), BoardServer::start());
    let folder = one.folder().to_path_buf();
    assert_ne!(folder, two.folder(), "each server has a folder of its own");
    let engine = |folder: &Path| format!("surrealkv://{}", folder.join("data").display());
    assert_eq!(store_held_in(&engine(&folder)), Ok(()));
    assert_eq!(store_held_in(&engine(two.folder())), Ok(()));
    let (a, b) = (one.client(), one.client());
    assert_ne!(a.database(), b.database(), "a database each");
    assert_eq!(a.addr(), b.addr());
    assert_ne!(
        a.addr(),
        two.client().addr(),
        "each server on a port of its own"
    );
    drop(one);
    assert!(!folder.exists(), "the folder went with its server");
    assert_eq!(
        store_held_in(&engine(two.folder())),
        Ok(()),
        "the other one is still up"
    );
}

/// A folder a board server's run left, its process killed before the server dropped, goes with
/// the next run's first server; the folders of a run still going and every other name stay.
#[test]
fn a_board_servers_folder_left_by_a_run_that_ended_is_removed_and_no_other() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("board-folders-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let ended = i64::from(i32::MAX); // past any system's last pid, so no process has it
    let own = std::process::id();
    let folder = |name: String| {
        let folder = root.join(name);
        std::fs::create_dir_all(folder.join("data")).unwrap();
        std::fs::write(folder.join("data/LOCK"), "").unwrap();
        folder
    };
    let left = [
        folder(format!("board-surrealkv-{ended}-0")),
        folder(format!("board-surrealkv-{ended}-7")),
    ];
    let kept = [
        folder(format!("board-surrealkv-{own}-0")),
        folder("board-surrealkv-notapid-0".into()),
        folder(format!("board-surrealkv-{ended}")),
        folder(format!("server-{ended}-0")),
    ];
    remove_ended_board_folders(&root);
    for gone in &left {
        assert!(!gone.exists(), "{} stayed", gone.display());
    }
    for stays in &kept {
        assert!(stays.join("data/LOCK").exists(), "{} went", stays.display());
    }
    remove_ended_board_folders(&root.join("absent"));
    std::fs::remove_dir_all(&root).unwrap();
}
