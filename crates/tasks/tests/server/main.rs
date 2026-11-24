//! The board's server: the board's PostgreSQL started and stopped on scratch boards, each holding
//! one of the machine's slots for its life (a test's own here), read and
//! exported as it runs, a snapshot put back over its board under the switch's journal, and its
//! dead space reclaimed; a board still on SurrealDB refused by every command that would start
//! it; the git common dir its board is kept in; in a repository of its own, a write's delivery by
//! the detached dispatcher it starts; and a child started on a port of its own.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

mod common;
mod dispatch;
mod export;
#[path = "../../../../tools/test-support/script_process_cwd.rs"]
#[expect(
    clippy::disallowed_methods,
    reason = "the fixture scripts every crate's tests share start their children as a test likes, wait by no timer, and recover machine_copy.rs's and script_tests.rs's poisoned locks by std's PoisonError::into_inner, as a crate without the board's lock::Recovered must"
)]
mod fixture_script;
mod git;
mod helper;
/// The recipients' Unix sockets, served by this process (the perf tests').
#[path = "../perf/listeners.rs"]
#[expect(
    dead_code,
    reason = "the perf target waits on what a socket hears; these tests read it in a wait of their own"
)]
mod listeners;
mod moving;
mod on_surrealdb;
mod own_board;
mod port;
mod postgres;
mod reclaim;
mod sessionless;
mod slots;
