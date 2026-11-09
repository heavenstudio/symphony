//! The tests' server start ([`launch`]): it comes up whatever is left of the folder its log goes in,
//! and a server that ends before it answers is reported with its exit status and what it and its
//! shell said.

use crate::common::{launch, surreal};
use std::path::PathBuf;
use symphony_board::rpc;

/// A folder of this test's own, empty.
fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("start-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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
