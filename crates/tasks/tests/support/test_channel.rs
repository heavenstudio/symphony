//! How a test or a load example makes a channel: std's own, past the board's rule that a channel of
//! the board's code is `crate::channel`'s (the crate's clippy.toml). That rule keeps a copy of std's
//! channel per item type out of the board's binaries, and no test or example holds a size line, so
//! they keep std's, made here alone under an expectation of its own: clippy reads every test
//! against the rest of what the file lists.
//!
//! The library's tests, each test target that makes a channel and the board_load example take it as
//! the module `test_channel` at their root.

/// A channel of `T`, std's.
#[expect(
    clippy::disallowed_methods,
    reason = "a test's or an example's channel is std's: neither holds a size line"
)]
pub fn channel<T>() -> (std::sync::mpsc::Sender<T>, std::sync::mpsc::Receiver<T>) {
    std::sync::mpsc::channel()
}
