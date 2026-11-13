//! What the `symphony-server` binary links, of what it never runs: it reads boards and runs no
//! board command (`symphony_board::cli::board` and `running_board`, never `cli::main`), and it
//! holds no plugin (`binary::run_holding_none`, which installs none). So it links none of the
//! SQLite a command needs to tell a board a move to SurrealDB froze
//! (`symphony_board::cutover::frozen`), and none of the loopback that measures a compiled plugin's
//! calls through the protocol's JSON (`symphony::plugins::measure`): the size each would add is
//! the binary's budget's to hold (docs/CHECKS.md, "Size budgets"). The shelf's server, which
//! installs a plugin, links the loopback, and so shows what the test looks for is there to find.

use symphony::plugins::measure::READ_AS_ANOTHER;

/// The header every SQLite database file starts with: a string of SQLite's own C code, in any
/// binary that links it.
const SQLITE: &[u8] = b"SQLite format 3\0";

fn server() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_symphony-server")).unwrap()
}

fn holds(binary: &[u8], text: &[u8]) -> bool {
    binary.windows(text.len()).any(|window| window == text)
}

#[test]
fn the_binary_links_no_sqlite() {
    assert!(
        !holds(&server(), SQLITE),
        "the symphony-server binary links SQLite: something it runs reaches a board command's \
         reach (symphony_board::cli's `reach`, which names `not_here`)"
    );
}

#[test]
fn the_binary_links_no_plugin_loopback() {
    let shelf = std::fs::read(env!("CARGO_BIN_EXE_symphony-shelf")).unwrap();
    assert!(
        holds(&shelf, READ_AS_ANOTHER.as_bytes()),
        "the shelf's server, which installs a plugin, links no loopback: the text this test looks \
         for is no longer the loopback's"
    );
    assert!(
        !holds(&server(), READ_AS_ANOTHER.as_bytes()),
        "the symphony-server binary links the plugins' loopback: something it runs names \
         `symphony::plugins::measure::measured` other than through `install`"
    );
}
