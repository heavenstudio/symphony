//! What the `symphony` binary links, of what it never runs: it reads boards and runs no board
//! command (`symphony_board::cli::board` and `running_board`, never `cli::main`), and it holds no
//! plugin (it installs none: `symphony::plugins::install`). So it links none of the loopback that
//! measures a compiled plugin's calls through the protocol's JSON (`symphony::plugins::measure`):
//! the size it would add is the binary's budget's to hold (docs/CHECKS.md, "Size budgets").

use symphony::plugins::measure::READ_AS_ANOTHER;

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
