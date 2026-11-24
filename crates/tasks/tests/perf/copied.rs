//! The suite's board as [`board`] makes it: the generated rows copied whole, in the copy's one
//! transaction, onto a PostgreSQL of the test binary's own whose data is in the run's folder. Every
//! other test of the target times or plans what runs on that board; this one says the board is
//! there, on the condition of its rows and its server, never a time.

use crate::board::board;
use crate::generated::dump;
use serde_json::json;

#[test]
fn the_generated_board_is_copied_whole_onto_a_server_in_the_runs_folder() {
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
        }),
        "each table holds the generated rows"
    );
    let serving = (board.server.current())
        .unwrap_or_else(|why| panic!("the board's server: {why}"))
        .expect("the board's server serves");
    assert_eq!(
        (serving.pid, &serving.socket),
        (board.pid, &board.socket),
        "the board's server is the one the suite started"
    );
    assert!(
        board.server.data().starts_with(&board.dir),
        "the board's data {} is in the run's folder {}",
        board.server.data().display(),
        board.dir.display()
    );
}
