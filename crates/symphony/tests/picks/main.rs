//! Untasked owner verdicts and the review binary the verdict filing runs.

#[path = "../common/mod.rs"]
mod common;
#[path = "../../../../../tools/gate/tests/verify/script_board.rs"]
mod fixture_script;

/// The task store's test server, as its own tests run on it: a database of its own per test.
#[path = "../../../tasks/tests/store/common.rs"]
#[allow(dead_code)]
mod board;

mod verdicts;

use symphony::register::{Decision, parse_decisions};

/// The cards in `lines`.
pub fn cards(lines: &[&str]) -> Vec<Decision> {
    parse_decisions(lines)
}

/// The first card in `lines`.
pub fn card(lines: &[&str]) -> Decision {
    cards(lines).into_iter().next().expect("a card")
}

fn review_binary_for(root: &std::path::Path, prepared: bool) -> Result<std::path::PathBuf, String> {
    if prepared {
        common::prebuilt_binary(root, "symphony")
    } else {
        Ok(env!("CARGO_BIN_EXE_symphony").into())
    }
}

fn review_binary() -> &'static std::path::Path {
    static BINARY: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    BINARY.get_or_init(|| {
        review_binary_for(
            &common::repo(),
            std::env::var("SF_TEST_SYMPHONY_PREBUILT").as_deref() == Ok("1"),
        )
        .unwrap_or_else(|why| panic!("{why}"))
    })
}

#[test]
fn review_prebuild_is_resolved_from_this_tree_and_plain_cargo_keeps_its_binary() {
    use std::os::unix::fs::PermissionsExt as _;
    let tree = common::Tree::new();
    std::fs::create_dir_all(tree.path("tools")).unwrap();
    fixture_script::write(
        &tree.path("tools/rs-bin.sh"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > calls\nprintf '%s/candidate\\n' \"$PWD\"\n",
        false,
    );
    tree.write("candidate", "fixture image");
    assert_eq!(
        review_binary_for(&tree.root, false).unwrap(),
        std::path::Path::new(env!("CARGO_BIN_EXE_symphony"))
    );
    assert!(
        !tree.path("calls").exists(),
        "plain cargo needs no source-cache lookup"
    );
    assert!(
        review_binary_for(&tree.root, true)
            .unwrap_err()
            .contains("no setup-built executable")
    );
    std::fs::set_permissions(
        tree.path("candidate"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert_eq!(
        review_binary_for(&tree.root, true).unwrap(),
        tree.path("candidate").canonicalize().unwrap()
    );
    assert_eq!(tree.read("calls"), "--where symphony\n");
    std::fs::remove_file(tree.path("candidate")).unwrap();
    assert!(
        review_binary_for(&tree.root, true)
            .unwrap_err()
            .contains("no setup-built executable for these sources")
    );
}
