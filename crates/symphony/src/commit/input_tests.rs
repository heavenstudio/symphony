//! [`super::run`] with input, of a git that ends without reading it.

use super::{checked, run};

/// Each git here ends without reading its input, and an input larger than any pipe holds (64 KiB
/// on macOS, at most 1 MiB on Linux) is written whole only once it is read: the write fails in
/// every order the two run in. What git said is the error all the same; where it said nothing, or
/// succeeded, the write's error is said beside its end.
#[test]
fn git_ending_before_it_reads_its_input_is_heard_before_the_write() {
    let tree = crate::test_tree::Tree::new();
    let input = "x".repeat(4 << 20);
    let ran = |args: &[&str]| run(&tree.root, args, Some(&input), None).and_then(checked);
    let refused = ran(&["hash-object", "--sf-unknown", "--stdin"]).unwrap_err();
    assert!(
        refused.starts_with("error: unknown option `sf-unknown'\nusage: git hash-object"),
        "{refused}"
    );
    let unwritten = "its input was not written: Broken pipe (os error 32)";
    assert_eq!(
        ran(&["config", "--get", "sf-input-tests.never-set"]),
        Err(format!("git exited exit status: 1; {unwritten}"))
    );
    assert_eq!(
        ran(&["version"]),
        Err(format!("git answered, but {unwritten}"))
    );
}
