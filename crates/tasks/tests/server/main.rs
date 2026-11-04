//! The board's server: found where it says it is, started when it is not (on a reserved port, and on
//! another when that one was taken), and the git common dir it keeps its data in; and the move of the
//! SQLite board into it, in a repository of its own.

mod common;
mod copy;
mod cutover;
#[path = "../../../../../tools/gate/tests/verify/script.rs"]
mod fixture_script;
mod git;
mod home;
mod home_fixture;
mod port;
#[path = "../store/sqlite.rs"]
mod sqlite;
/// The store's tests' helpers, for the one way the tests find the pinned `surreal`.
#[path = "../store/common.rs"]
#[allow(dead_code)]
mod store;
