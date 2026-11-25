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

use symphony_server::{binary, home, repo, sys};

#[test]
fn the_names_consumers_call_keep_their_shapes() {
    let _: fn(&'static [&'static dyn symphony_plugin::Plugin], &[String]) -> i32 = binary::run;
    let _: fn(&Path) -> Result<repo::Checkout, String> = repo::checkout;
    let _: fn(i64) -> bool = sys::pid_alive;
    let _: fn(i64, libc::c_int) = sys::signal;
    // A server's process taken by its pid, asked whether it is still that process, and printed in
    // a fixture's failure (read from its code on 2026-11-25).
    let _: fn(i64) -> sys::Taken = sys::Taken::of;
    let _: fn(&sys::Taken) -> bool = sys::Taken::alive;
    let _ = |taken: &sys::Taken| format!("{taken:?}");
    // Built by its fields: one added here breaks the consumer's fixture.
    let _ = |port: u16, pid: i64, root: String| home::Running { port, pid, root };
}

#[test]
fn hosted_admission_is_additive_and_explicit() {
    use std::sync::Arc;
    use symphony_server::{front, hosted};
    let _: fn(&str, &str, &str, &str) -> Result<hosted::Config, hosted::Refusal> =
        hosted::Config::new;
    type HostedRouter = fn(
        Arc<front::Shared>,
        hosted::Config,
        Arc<dyn hosted::Host>,
    ) -> Result<axum::Router, hosted::Refusal>;
    let _: HostedRouter = hosted::router;
}

#[test]
fn execution_providers_install_beside_the_plugins_by_configuration() {
    use std::sync::Arc;
    use symphony_board::execution::{Adapter, Capabilities};
    use symphony_server::execution::{self, Provider};
    type WithProviders =
        fn(&'static [&'static dyn symphony_plugin::Plugin], Vec<Provider>, &[String]) -> i32;
    let _: WithProviders = binary::run_with_providers;
    let _: fn(&str, Capabilities, Arc<dyn Adapter>) -> Provider = Provider::new;
    let _: fn(Provider, &str, u32) -> Provider = Provider::with_capacity;
    let _: fn(Vec<Provider>) -> Result<(), String> = execution::install;
}
