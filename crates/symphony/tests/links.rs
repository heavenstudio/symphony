//! What the `symphony` binary links, of what it never runs: it reads boards and runs no board
//! command (`symphony_board::cli::board` and `running_board`, never `cli::main`), and it holds no
//! plugin (it installs none: `symphony::plugins::install`). So it links none of the SQLite a
//! command needs to tell a board a move to SurrealDB froze (`symphony_board::cutover::frozen`), and
//! none of the loopback that measures a compiled plugin's calls through the protocol's JSON
//! (`symphony::plugins::measure`); and on macOS it exports no symbol, as no other code loads it.
//! The size each would add is the binary's budget's to hold (docs/CHECKS.md, "Size budgets").

use symphony::plugins::measure::READ_AS_ANOTHER;

// Whether a Mach-O binary exports a symbol, read from its export trie.
#[cfg(target_os = "macos")]
#[path = "../../../tools/test-support/macho_exports.rs"]
mod macho_exports;

/// The header every SQLite database file starts with: a string of SQLite's own C code, in any
/// binary that links it.
const SQLITE: &[u8] = b"SQLite format 3\0";

fn binary() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_symphony")).unwrap()
}

fn holds(binary: &[u8], text: &[u8]) -> bool {
    binary.windows(text.len()).any(|window| window == text)
}

#[test]
fn the_binary_links_no_sqlite() {
    assert!(
        !holds(&binary(), SQLITE),
        "the symphony binary links SQLite: something it runs reaches a board command's reach \
         (symphony_board::cli's `reach`, which names `not_here`)"
    );
}

#[test]
fn the_binary_links_no_plugin_loopback() {
    assert!(
        !holds(&binary(), READ_AS_ANOTHER.as_bytes()),
        "the symphony binary links the plugins' loopback: something it runs names \
         `symphony::plugins::measure::measured` other than through `install`"
    );
}

/// On macOS the binary links with `-no_exported_symbols` (crates/symphony/build.rs): no other code
/// loads it, so the export trie a link writes by default, a name for each global symbol, would
/// serve only a debugger reading a stripped build. This test's own executable, linked without it,
/// exports, and so shows the reading finds a trie where there is one.
#[test]
#[cfg(target_os = "macos")]
fn the_binary_exports_no_symbols() {
    let this = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    assert!(
        macho_exports::exports_any(&this),
        "this test's executable exports nothing: the reading no longer finds an export trie"
    );
    assert!(
        !macho_exports::exports_any(&binary()),
        "the symphony binary exports symbols: its link lost `-Wl,-no_exported_symbols` \
         (crates/symphony/build.rs)"
    );
}
