//! How a thread of the server tells another that something happened: a channel of `()`, std's
//! [`mpsc`], made here alone ([`channel`], [`sync_channel`]).
//!
//! A std channel is compiled whole for each type it carries, 5 to 16 KB of a binary a type. Every
//! channel the server's own code makes carries `()`: a watching loop's wake
//! ([`crate::backstop::Wakes`]), the notice worker's, the ticks' stop, the first board's start and
//! the end of a pipe read, one copy of some 25 KB. Carried boxed, as symphony-app's and the board's
//! code carry theirs, they would be a copy of the server's own of that box in place of this one, as
//! a generic is compiled again in each crate that uses it, so nothing would be gained. They stay
//! std's of `()`, and these two constructors, of `()` alone, keep it the one type: the crate's
//! clippy.toml refuses std's `channel` and `sync_channel` anywhere else. A test makes std's of any
//! type through tests/common/test_channel.rs, as no test holds a size line.

use std::sync::mpsc;

/// An unbounded channel of `()`: its sending end, which clones, and its receiving end.
pub(crate) fn channel() -> (mpsc::Sender<()>, mpsc::Receiver<()>) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the server's one unbounded std channel, of () alone"
    )]
    mpsc::channel()
}

/// A channel of `()` holding `bound` wakes at most: its sending end and its receiving end.
pub(crate) fn sync_channel(bound: usize) -> (mpsc::SyncSender<()>, mpsc::Receiver<()>) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the server's one bounded std channel, of () alone"
    )]
    mpsc::sync_channel(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_unbounded_channel_takes_every_wake_and_ends_with_its_senders() {
        let (send, woken) = channel();
        let other = send.clone();
        send.send(()).unwrap();
        other.send(()).unwrap();
        assert_eq!(woken.try_iter().count(), 2);
        drop((send, other));
        assert_eq!(woken.recv(), Err(mpsc::RecvError));
    }

    #[test]
    fn a_bounded_channel_holds_its_bound_and_hands_back_the_wake_past_it() {
        let (send, woken) = sync_channel(1);
        send.try_send(()).unwrap();
        assert_eq!(send.try_send(()), Err(mpsc::TrySendError::Full(())));
        assert_eq!(woken.recv_timeout(Duration::ZERO), Ok(()));
        drop(woken);
        assert_eq!(send.try_send(()), Err(mpsc::TrySendError::Disconnected(())));
    }

    /// Clippy reads the nearest clippy.toml alone: the crate's, which refuses std's channel past
    /// [`channel`] and [`sync_channel`], must hold the workspace's settings whole before its own.
    /// symphony-app's lib tests hold every crate's to that (crates/symphony/src/channel_tests.rs);
    /// this holds the server's in the server's own run, which an edit to it alone is tested by.
    #[test]
    fn the_crates_clippy_settings_start_with_the_workspaces() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let ours = std::fs::read_to_string(manifest.join("clippy.toml")).unwrap();
        let workspace = std::fs::read_to_string(manifest.join("../../clippy.toml")).unwrap();
        assert!(
            ours.starts_with(&workspace),
            "apps/symphony-server/clippy.toml must start with the workspace's clippy.toml whole"
        );
    }
}
