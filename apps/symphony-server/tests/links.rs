//! What the `symphony-server` binary links, of what it never runs: it reads boards and runs no
//! board command (`symphony_board::cli::board` and `running_board`, never `cli::main`), and it
//! holds no plugin (`binary::run_holding_none`, which installs none). So it links none of the
//! loopback that measures a compiled plugin's calls through the protocol's JSON
//! (`symphony::plugins::measure`); and on macOS it exports no symbol, as no other code loads it.
//! The size each would add is the binary's budget's to hold
//! (docs/CHECKS.md, "Size budgets"). The shelf's server, which installs a plugin, links the
//! loopback, and so shows what the test looks for is there to find.

use symphony::plugins::measure::READ_AS_ANOTHER;

// The symbols a Mach-O binary exports, read from its export trie.
#[cfg(target_os = "macos")]
#[path = "../../../tools/test-support/macho_exports.rs"]
mod macho_exports;

fn server() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_symphony-server")).unwrap()
}

fn holds(binary: &[u8], text: &[u8]) -> bool {
    binary.windows(text.len()).any(|window| window == text)
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

/// On macOS the server links with `-no_exported_symbols` (build.rs): no other code loads it, so
/// the export trie a link writes by default, a name for each global symbol, would serve only a
/// debugger reading a stripped build. This test's own executable, linked without it, exports, and
/// so shows the reading finds a trie where there is one.
#[test]
#[cfg(target_os = "macos")]
fn the_binary_exports_no_symbols() {
    let this = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    assert!(
        !macho_exports::exports(&this).is_empty(),
        "this test's executable exports nothing: the reading no longer finds an export trie"
    );
    let exports = macho_exports::exports(&server());
    assert!(
        exports.is_empty(),
        "the symphony-server binary exports symbols: its link lost `-Wl,-no_exported_symbols` \
         (apps/symphony-server/build.rs): {}",
        macho_exports::said(&exports)
    );
}
