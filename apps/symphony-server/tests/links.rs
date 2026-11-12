//! What the `symphony-server` binary links, of what it never runs: it reads boards and runs no
//! board command (`symphony_board::cli::board` and `running_board`, never `cli::main`), and it
//! holds no plugin (`binary::run_holding_none`, which installs none). So it links none of the
//! SQLite a command needs to tell a board a move to SurrealDB froze
//! (`symphony_board::cutover::frozen`), and none of what measures a call into a plugin through the
//! protocol's JSON (`symphony::plugins::measure`): the size each would add is the binary's
//! budget's to hold (docs/CHECKS.md, "Size budgets").

use symphony_plugin::protocol::{Ask, Declare};

/// The header every SQLite database file starts with: a string of SQLite's own C code, in any
/// binary that links it.
const SQLITE: &[u8] = b"SQLite format 3\0";

fn binary() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_symphony-server")).unwrap()
}

fn holds(binary: &[u8], text: &[u8]) -> bool {
    binary.windows(text.len()).any(|window| window == text)
}

#[test]
fn the_binary_links_no_sqlite() {
    assert!(
        !holds(&binary(), SQLITE),
        "the symphony-server binary links SQLite: something it runs reaches a board command's \
         reach (symphony_board::cli's `reach`, which names `not_here`)"
    );
}

#[test]
fn the_binary_links_no_plugin_protocol() {
    assert!(
        !holds(&binary(), Declare::TAG.as_bytes()),
        "the symphony-server binary links the plugin protocol: something it runs names \
         `symphony::plugins::measure` other than through `install`"
    );
}
