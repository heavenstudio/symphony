//! The store over a real SurrealDB server, one test binary so they share one server.

mod blocked_panes;
mod board;
mod callbacks;
mod claim;
mod common;
mod copy;
mod cutover;
mod depends;
mod done;
mod embeddings;
mod fields;
#[path = "../../../../../tools/gate/tests/verify/script.rs"]
mod fixture_script;
mod health;
mod launches;
mod messages;
mod next;
mod priority;
mod quiet_callbacks;
mod reserve;
mod roles;
mod schema;
mod sessions;
mod sqlite;
mod start;
mod tree;
mod verdict;
