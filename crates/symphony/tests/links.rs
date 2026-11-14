//! What the `symphony` binary links, of what it never runs: it reads boards and runs no board
//! command (`symphony_board::cli::board` and `running_board`, never `cli::main`), and it holds no
//! plugin (it installs none: `symphony::plugins::install`). So it links none of the loopback that
//! measures a compiled plugin's calls through the protocol's JSON (`symphony::plugins::measure`).
//! It checks no JSON Schema (`serve` refuses `validate`: a schema check is a library call,
//! `symphony::schema::validate`), so it links none of the validator, which would add a fifth to
//! its size. The commands only this repository's own build and release run use answer from
//! `symphony-build` alone, so it links none of them either. And on macOS it exports no symbol, as
//! no other code loads it. The size each would add is the binary's budget's to hold
//! (docs/CHECKS.md, "Size budgets").

use serde_json::json;
use symphony::plugins::measure::READ_AS_ANOTHER;

// Whether a Mach-O binary exports a symbol, read from its export trie.
#[cfg(target_os = "macos")]
#[path = "../../../tools/test-support/macho_exports.rs"]
mod macho_exports;

fn binary() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_symphony")).unwrap()
}

fn holds(binary: &[u8], text: &[u8]) -> bool {
    binary.windows(text.len()).any(|window| window == text)
}

#[test]
fn the_binary_links_no_plugin_loopback() {
    assert!(
        !holds(&binary(), READ_AS_ANOTHER.as_bytes()),
        "the symphony binary links the plugins' loopback: something it runs names \
         `symphony::plugins::measure::measured` other than through `install`"
    );
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

/// `publish-client`, `release-inputs`, `size-budget`, `split-repair` and `release` are
/// `symphony-build`'s (`symphony::cli::build_main`), which no project is given (`init::BUILDS`), so
/// neither this binary nor a consuming project's host, which runs `symphony::cli::main`, links
/// them. Each is looked for by the prefix of its refusals, a text of its own module; the
/// `symphony-build` binary, which runs them, holds each, and so shows the text is there to find
/// where the command is linked.
#[test]
fn the_binary_links_none_of_the_build_only_commands() {
    let build = std::fs::read(env!("CARGO_BIN_EXE_symphony-build")).unwrap();
    let shipped = binary();
    for command in [
        "publish-client",
        "release-inputs",
        "size-budget",
        "split-repair",
        "release",
    ] {
        let said = format!("symphony-build {command}: ");
        assert!(
            holds(&build, said.as_bytes()),
            "symphony-build holds no {said:?}: the text this test looks for is no longer \
             `{command}`'s"
        );
        assert!(
            !holds(&shipped, said.as_bytes()),
            "the symphony binary links `{command}`: something it runs reaches it, which only \
             `symphony::cli::build_main` should"
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
        macho_exports::exports_any(&this),
        "this test's executable exports nothing: the reading no longer finds an export trie"
    );
    assert!(
        !macho_exports::exports_any(&binary()),
        "the symphony binary exports symbols: its link lost `-Wl,-no_exported_symbols` \
         (crates/symphony/build.rs)"
    );
}
