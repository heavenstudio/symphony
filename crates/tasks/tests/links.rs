//! What the board's shipped binaries, `tasks` and `agent-send`, link of what they never run: on
//! macOS no symbol is exported, as no other code loads them. The size it would add is their
//! budgets' to hold (docs/workflow/CHECKS.md, "Size budgets").

// The symbols a Mach-O binary exports, read from its export trie.
#[cfg(target_os = "macos")]
#[path = "../../../tools/test-support/macho_exports.rs"]
mod macho_exports;

/// On macOS every binary of the board links with `-no_exported_symbols` (build.rs): no other code
/// loads it, so the export trie a link writes by default, a name for each global symbol, would
/// serve only a debugger reading a stripped build. This test's own executable, linked without it,
/// exports, and so shows the reading finds a trie where there is one.
#[test]
#[cfg(target_os = "macos")]
fn the_shipped_binaries_export_no_symbols() {
    let this = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    assert!(
        !macho_exports::exports(&this).is_empty(),
        "this test's executable exports nothing: the reading no longer finds an export trie"
    );
    for (name, path) in [
        ("tasks", env!("CARGO_BIN_EXE_tasks")),
        ("agent-send", env!("CARGO_BIN_EXE_agent-send")),
    ] {
        let exports = macho_exports::exports(&std::fs::read(path).unwrap());
        assert!(
            exports.is_empty(),
            "the {name} binary exports symbols: its link lost `-Wl,-no_exported_symbols` \
             (crates/tasks/build.rs): {}",
            macho_exports::said(&exports)
        );
    }
}
