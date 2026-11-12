//! The board's server: found where it says it is, started when it is not (on a reserved port, and on
//! another when that one was taken), and the git common dir it keeps its data in; what its store
//! takes on disk, and the reclaim that rebuilds it; the move of the SQLite board into it, in a
//! repository of its own; and, in that shape, a write's delivery by the detached dispatcher it starts.

#![expect(
    clippy::disallowed_methods,
    reason = "a test starts its fixtures and stand-ins as it likes; the board's children are sys's"
)]

mod common;
mod copy;
mod cutover;
mod dispatch;
#[path = "../../../../tools/test-support/script_process_cwd.rs"]
mod fixture_script;
mod git;
mod helper;
mod hold;
mod home;
mod home_fixture;
/// The recipients' Unix sockets, served by this process (the perf tests').
#[path = "../perf/listeners.rs"]
mod listeners;
mod own_board;
mod port;
mod reclaim;
mod sessionless;
mod snapshot;
#[path = "../store/sqlite.rs"]
mod sqlite;
/// The store's tests' helpers, for the one way the tests find the pinned `surreal`.
#[path = "../store/common.rs"]
#[allow(dead_code)]
mod store;
