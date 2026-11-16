//! The public names of `symphony_server` that consuming projects' code calls (a host's binaries and
//! its bootstrap tests), each in the shape it calls it: a signature pinned by coercing the function
//! to its pointer type, a struct it builds by its fields built by them all. Nothing here runs; it is
//! compiled by the test build and by `just lint-rs` (clippy over every target), so a change that
//! would break a consumer fails a task's gate rather than a run later. A field it reads off a
//! value (a `repo::Checkout`'s, say) is not held here: the project meets that when it moves its
//! pin, its own gate building it on the new one. The other packages' guards are named in
//! `crates/symphony/tests/consumer_api.rs`.
//!
//! A consumer takes a release by moving its pin in its own landing, and nothing here changes its
//! code, so a consumer's code never changes with a release: a name or signature here changes
//! only with a consumer release plan (the consumer's code moved first, against a release that still
//! has the old name). Names were read from the consuming projects' code on 2026-11-12; add one when
//! a consumer starts calling it.

use std::path::Path;

use symphony_server::{binary, development, home, repo, sys};

#[test]
fn the_names_consumers_call_keep_their_shapes() {
    let _: fn(&'static [&'static dyn symphony_plugin::Plugin], &[String]) -> i32 = binary::run;
    // The retired client's dev server: a stub that refuses, until the consumer's binary goes.
    let _: fn(&[String]) -> Result<(), String> = development::main;
    let _: fn(&Path) -> Result<repo::Checkout, String> = repo::checkout;
    let _: fn(i64) -> bool = sys::pid_alive;
    let _: fn(i64, libc::c_int) = sys::signal;
    // Built by its fields: one added here breaks the consumer's fixture.
    let _ = |port: u16, pid: i64, root: String| home::Running { port, pid, root };
}
