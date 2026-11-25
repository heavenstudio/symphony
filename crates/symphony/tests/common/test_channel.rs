//! How a test makes a channel: std's own, past the crate's rule that a channel of its code is
//! `crate::channel`'s (the crate's clippy.toml). That rule keeps a copy of std's channel per item
//! type out of the binaries, and no test holds a size line, so a test keeps std's, made here alone
//! under an expectation of its own: clippy reads every test against the rest of what the file lists.
//!
//! The library's tests and each test target that makes a channel take it as the module
//! `test_channel` at their root.

/// A channel of `T`, std's.
#[expect(
    clippy::disallowed_methods,
    reason = "a test's channel is std's: no test holds a size line"
)]
pub fn channel<T>() -> (std::sync::mpsc::Sender<T>, std::sync::mpsc::Receiver<T>) {
    std::sync::mpsc::channel()
}
