//! The board's server: found where it says it is, started when it is not (on a reserved port, and on
//! another when that one was taken), and the git common dir it keeps its data in; what its store
//! takes on disk, and the reclaim that rebuilds it; in a repository of its own, a write's
//! delivery by the detached dispatcher it starts; the board's PostgreSQL beside it, started
//! and stopped on scratch boards; and the lock `tools/surreal.sh` fetches its SurrealDB under.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

mod common;
mod copy;
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
mod hold;
mod home;
mod home_fixture;
mod left_replay;
/// The recipients' Unix sockets, served by this process (the perf tests').
#[path = "../perf/listeners.rs"]
#[expect(
    dead_code,
    reason = "the perf target waits on what a socket hears; these tests read it in a wait of their own"
)]
mod listeners;
mod moving;
mod no_lsof;
mod own_board;
mod port;
mod postgres;
mod reclaim;
mod sessionless;
mod snapshot;
/// The store's tests' helpers, for the one way the tests find the pinned `surreal`.
#[path = "../store/common.rs"]
#[allow(dead_code)]
mod store;
mod stores;
mod surreal_lock;
