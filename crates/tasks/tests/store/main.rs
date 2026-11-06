//! The store over a real SurrealDB server, one test binary so they share one server.

mod blocked_panes;
mod board;
mod callbacks;
mod claim;
mod common;
mod copy;
mod copy_marker;
mod cutover;
mod depends;
mod dispatch_later;
mod done;
mod embeddings;
mod fields;
#[path = "../../../../../tools/gate/tests/verify/script_process_cwd.rs"]
mod fixture_script;
mod hand_over_outage;
mod health;
mod keyed;
mod launches;
mod messages;
mod mixed_dispatchers;
mod next;
mod orchestrators;
mod pending_pull;
mod priority;
mod pull_acknowledgements;
mod quiet_callbacks;
mod reserve;
mod roles;
mod sessions;
mod sqlite;
mod start;
mod tree;
mod verdict;
