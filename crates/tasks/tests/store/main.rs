//! The store over a real PostgreSQL server, one test binary so they share one server.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

mod amend;
mod artifacts;
mod asks;
mod attempt_reads;
mod bindings;
mod blocked_panes;
mod blocks;
mod board;
mod board_people;
mod callbacks;
mod cancel;
mod carried;
mod catalogue;
mod claim;
mod closing;
mod common;
mod compatible_ahead;
mod contract;
mod copy;
mod copy_marker;
mod covered;
mod created;
mod depends;
mod dispatch_commit;
mod dispatch_holder;
mod dispatch_later;
mod done;
mod edit;
mod execution;
mod fast_fix;
mod fields;
// The adapter contract tests run once in `server`; this adapter uses the same spawn lock.
#[path = "../../../../tools/test-support/script_board.rs"]
#[expect(
    clippy::disallowed_methods,
    reason = "the fixture scripts every crate's tests share start their children as a test likes, wait by no timer, and recover machine_copy.rs's poisoned lock by std's PoisonError::into_inner, as a crate without the board's lock::Recovered must"
)]
mod fixture_script;
mod gate_decisions;
mod hand_over_outage;
mod health;
mod held_settle;
mod keyed;
mod kind;
mod launches;
mod lost_observers;
mod low_volume;
mod marker;
mod messages;
mod mixed_dispatchers;
mod move_from;
mod needs;
mod next;
mod orchestrator_given;
mod orchestrator_limit;
mod orchestrators;
mod ordered_maps;
mod owner_word;
mod pass_reads;
mod pending_pull;
mod people;
mod pg_client;
mod pg_reading;
mod priority;
mod pull_acknowledgements;
mod pushes;
mod quiet_callbacks;
mod raw;
mod realistic;
mod recovery;
mod red;
mod reports;
mod reserve;
mod resume;
mod roles;
mod round_trip;
mod schema;
mod search;
mod session_marker;
mod session_urls;
mod sessions;
mod shown_sessions;
mod slots;
mod slow_builds;
mod stamp;
mod stored_settings;
mod subtask_roles;
mod thread_window;
mod tree;
mod two_writers;
mod unlanded;
mod usage;
mod verdict;
mod waits;
mod writers;
