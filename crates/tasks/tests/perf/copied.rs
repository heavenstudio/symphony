//! The suite's board as [`board`] makes it: the generated rows copied whole, in the copy's one
//! transaction, onto a server the store's fixture starts as every test target starts it
//! (`store::launch_on`), which keeps them in `surrealkv://` in the run's folder, the live board's
//! engine. Every other test of the target times or plans what runs on that board; this one says
//! the board is there, on the condition of its rows and its store, never a time.

use crate::board::board;
use crate::generated::dump;
use crate::store;
use serde_json::json;

#[test]
fn the_generated_board_is_copied_whole_onto_a_store_in_the_runs_folder() {
    let board = board();
    let dump = dump();
    assert_eq!(
        board.copied,
        json!({
            "task": dump.tasks.len(),
            "link": dump.task_links.len(),
            "session": dump.sessions.len(),
            "role": dump.roles.len(),
            "message": dump.messages.len(),
            "delivery": dump.deliveries.len(),
            "barred_key": dump.barred_keys.len(),
            "task_session": dump.task_sessions.len(),
            "delivery_key": dump.delivery_keys.len(),
            "ask": dump.asks.len(),
            "staging": [],
        }),
        "each table holds the generated rows, and the copy left no staging"
    );
    assert!(
        board.engine.starts_with("surrealkv://"),
        "the board's engine is {}",
        board.engine
    );
    store::store_held_in(&board.engine).unwrap_or_else(|why| panic!("the board's server: {why}"));
}
