//! What the `symphony-server` binary links, of what it never runs: on macOS it exports no symbol, as
//! no other code loads it. The size it would add is the binary's budget's to hold (docs/workflow/CHECKS.md,
//! "Size budgets").

// The symbols a Mach-O binary exports, read from its export trie.
#[cfg(target_os = "macos")]
#[path = "../../../tools/test-support/macho_exports.rs"]
mod macho_exports;

fn server() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_symphony-server")).unwrap()
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
