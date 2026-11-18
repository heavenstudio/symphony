//! A hosted board (`symphony_board::pg::hosted`, the `hosted-postgresql` feature) against real
//! PostgreSQL 16 and 18 servers of this test binary's own: each a scratch cluster under the
//! temporary folder, listening on loopback with TLS and SCRAM, ended with the binary. Every test
//! provisions what a platform would: a login that can make nothing, and a database of its own
//! that the login owns. A machine without PostgreSQL 16 fails these tests, naming what to
//! install; none is skipped ([`cluster`]).

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

mod analyzer;
mod board;
mod capacity;
mod cluster;
mod refusals;
/// The store's tests' helpers: the workspace's root, a child that ends with this process.
#[path = "../store/common.rs"]
#[allow(dead_code)]
mod store;
mod transactions;
