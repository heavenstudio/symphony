//! The store over a real SurrealDB server, one test binary so they share one server.

#![expect(
    clippy::disallowed_methods,
    reason = "a test starts its fixtures and stand-ins as it likes; the board's children are sys's"
)]

mod blocked_panes;
mod board;
mod callbacks;
mod claim;
mod closing;
mod common;
mod copy;
mod copy_marker;
mod cutover;
mod depends;
mod dispatch_commit;
mod dispatch_holder;
mod dispatch_later;
mod done;
mod embeddings;
mod fields;
#[path = "../../../../../tools/gate/tests/verify/script_process_cwd.rs"]
mod fixture_script;
mod hand_over_outage;
mod health;
mod held_stream;
mod keyed;
mod launches;
mod messages;
mod mixed_dispatchers;
mod move_from;
mod next;
mod open_read;
mod orchestrators;
mod pending_pull;
mod priority;
mod pull_acknowledgements;
mod quiet_callbacks;
mod relay;
mod reserve;
mod roles;
mod sessions;
mod slots;
mod sqlite;
mod start;
mod tree;
mod verdict;
