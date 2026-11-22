//! What the `symphony` binary links, of what it never runs: it reads boards and runs no board
//! command (`symphony_board::cli::board` and `running_board`, never `cli::main`). It checks no JSON
//! Schema (`serve` refuses `validate`: a schema check is a library call,
//! `symphony::schema::validate`), so it links none of the validator, which would add a fifth to
//! its size. It opens boards only to read (`Store::read_as`, `Store::reading`), so it links none of
//! the migration that makes or moves one, the two databases' schema texts among it. And on macOS
//! it exports no symbol, as no other code loads it. The size each would add
//! is the binary's budget's to hold (docs/CHECKS.md, "Size budgets"). The commands only this
//! repository's own build and release run use are a package of their own (crates/symphony-build),
//! which this library never compiles, so there is nothing of theirs to look for here.

use serde_json::json;

// The symbols a Mach-O binary exports, read from its export trie.
#[cfg(target_os = "macos")]
#[path = "../../../tools/test-support/macho_exports.rs"]
mod macho_exports;

fn binary() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_symphony")).unwrap()
}

fn holds(binary: &[u8], text: &[u8]) -> bool {
    binary.windows(text.len()).any(|window| window == text)
}

/// The text looked for is a refusal the validator writes (`unsupported keyword(s): `), taken from
/// its answer at run time rather than written here, so this test's own executable, which calls the
/// validator, holds it only by linking it, and so shows the text is there to find where it is.
#[test]
fn the_binary_links_no_schema_validator() {
    let refused = symphony::schema::validate(&json!(1), &json!({"contentSchema": {}}))
        .expect_err("the validator refuses contentSchema");
    let text = refused
        .0
        .split("contentSchema")
        .next()
        .unwrap_or_default()
        .to_owned();
    assert!(
        text.len() >= 16,
        "the validator's refusal no longer opens with a text of its own: {refused:?}"
    );
    let this = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    assert!(
        holds(&this, text.as_bytes()),
        "this test's executable, which calls the validator, does not hold {text:?}: the text this \
         test looks for is no longer the validator's"
    );
    assert!(
        !holds(&binary(), text.as_bytes()),
        "the symphony binary links the JSON Schema validator: something it runs reaches \
         `symphony::schema` (`serve` refuses `validate`; a schema check is a library call)"
    );
}

/// The texts looked for are one statement of each database's schema, which only a migration
/// runs. This test's own executable names a writer's opening (`Store::open`), so it links the
/// migration and holds both, and so shows each text is there to find where it is.
#[test]
fn the_binary_links_no_board_migration() {
    let writer: fn(symphony_board::rpc::Client) -> _ = symphony_board::Store::open;
    std::hint::black_box(writer);
    let this = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let binary = binary();
    for text in [
        "DEFINE TABLE IF NOT EXISTS writer SCHEMALESS;",
        "CREATE OR REPLACE FUNCTION board_terms",
    ] {
        assert!(
            holds(&this, text.as_bytes()),
            "this test's executable, which names a writer's opening, does not hold {text:?}: the \
             text this test looks for is no longer the migration's"
        );
        assert!(
            !holds(&binary, text.as_bytes()),
            "the symphony binary links the board's schema migration ({text:?}): something it \
             runs opens a board other than by `Store::read_as` or `Store::reading` \
             (crates/symphony/src/taskview/connect.rs), or reads `Schema::steps`"
        );
    }
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
        !macho_exports::exports(&this).is_empty(),
        "this test's executable exports nothing: the reading no longer finds an export trie"
    );
    let exports = macho_exports::exports(&binary());
    assert!(
        exports.is_empty(),
        "the symphony binary exports symbols: its link lost `-Wl,-no_exported_symbols` \
         (crates/symphony/build.rs): {}",
        macho_exports::said(&exports)
    );
}
