//! Immutable SurrealKV stores made once per binary, copied into each case's own scratch board.
//! No server opens the seed after publication; a case replays or changes only its own copy. Seeds
//! live under the same pid-named folders that `Scratch` prunes after their binary has ended.

use crate::children::Child as _;
use crate::common::Scratch;
use std::path::{Path, PathBuf};
use symphony_board::sys::{self, Command};

/// Copy the whole store, copy-on-write where supported, never opening its source in a server.
pub fn clone_data(from: &Path, into: &Path) {
    assert!(
        !into.exists(),
        "a case starts without a store: {}",
        into.display()
    );
    let done = sys::output(
        Command::of("cp")
            .args(if cfg!(target_os = "macos") {
                &["-c", "-R"][..]
            } else {
                &["-R", "--reflink=auto"][..]
            })
            .arg(from)
            .arg(into),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
}

/// Publish a seed outside its setup's scratch folder, so that setup's guard and children are
/// reaped normally. Only the immutable store remains until the next binary's scratch pruning.
pub fn keep(s: &Scratch, name: &str) -> PathBuf {
    assert_eq!(s.home().serving(), None, "no server opens a published seed");
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("server-{}-{name}", std::process::id()));
    // As for Scratch, a previous binary may have used this pid before it was recycled.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let data = dir.join("data");
    clone_data(&s.dir.join("data"), &data);
    data
}

#[test]
fn clones_keep_the_source_and_each_other_unchanged() {
    let s = Scratch::new("store-clones");
    let source = s.dir.join("source");
    std::fs::create_dir_all(source.join("wal")).unwrap();
    std::fs::write(source.join("wal/segment"), b"original").unwrap();
    for name in ["first", "second"] {
        clone_data(&source, &s.dir.join(name));
    }
    std::fs::write(s.dir.join("first/wal/segment"), b"changed").unwrap();
    std::fs::remove_file(s.dir.join("first/wal/segment")).unwrap();
    for path in [source, s.dir.join("second")] {
        assert_eq!(
            std::fs::read(path.join("wal/segment")).unwrap(),
            b"original"
        );
    }
}

#[test]
fn an_existing_destination_is_refused_without_changing_it() {
    let s = Scratch::new("store-clone-existing");
    let source = s.dir.join("source");
    let into = s.dir.join("into");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&into).unwrap();
    std::fs::write(into.join("held"), b"kept").unwrap();
    assert!(std::panic::catch_unwind(|| clone_data(&source, &into)).is_err());
    assert_eq!(std::fs::read(into.join("held")).unwrap(), b"kept");
    assert!(!into.join("source").exists());
}

#[test]
fn a_missing_source_fails_without_publishing_a_destination() {
    let s = Scratch::new("store-clone-missing");
    let into = s.dir.join("into");
    assert!(std::panic::catch_unwind(|| clone_data(&s.dir.join("missing"), &into)).is_err());
    assert!(!into.exists());
}
