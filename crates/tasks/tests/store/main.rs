//! The store over a real SurrealDB server, one test binary so they share one server.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

/// Tests written once and run on each database the board can be in: each `fn NAME(backend:
/// Backend)` becomes the module `NAME` of two tests, `NAME::surrealdb` and `NAME::postgresql`,
/// each handing its body the database it names ([`common::Backend`]). The parent module's items
/// are in scope in the body.
macro_rules! on_both {
    ($($(#[$meta:meta])* fn $name:ident($backend:ident: Backend) $body:block)+) => {$(
        mod $name {
            #[allow(unused_imports, reason = "the body names what it uses of its module")]
            use super::*;

            fn run($backend: Backend) $body

            #[test]
            $(#[$meta])*
            fn surrealdb() {
                run(crate::common::Backend::SurrealDb);
            }

            #[test]
            $(#[$meta])*
            fn postgresql() {
                run(crate::common::Backend::PostgreSql);
            }
        }
    )+};
}

mod amend;
mod artifacts;
mod asks;
mod attempt_reads;
mod bindings;
mod blocked_panes;
mod blocks;
mod board;
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
    reason = "the fixture scripts every crate's tests share start their children as a test likes, and wait by no timer"
)]
mod fixture_script;
mod gate;
mod gate_decisions;
mod hand_over_outage;
mod health;
mod held_settle;
mod held_stream;
mod keyed;
mod launches;
mod lost_observers;
mod low_volume;
mod marker;
mod marker_retirement;
mod messages;
mod mixed_dispatchers;
mod move_from;
mod needs;
mod next;
mod open_read;
mod orchestrator_given;
mod orchestrator_limit;
mod orchestrators;
mod ordered_maps;
mod owner_word;
mod pending_pull;
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
mod relay;
mod reports;
mod reserve;
mod resume;
mod roles;
mod round_trip;
mod schema;
mod schema_copy;
mod search;
mod session_marker;
mod session_urls;
mod sessions;
mod shown_sessions;
mod slots;
mod slow_builds;
mod stamp;
mod start;
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
