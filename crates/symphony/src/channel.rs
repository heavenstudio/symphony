//! How a thread of this crate hands another what it has: one channel, std's [`mpsc`], whatever it
//! carries ([`channel`], [`sync_channel`]). Its items go boxed, as `dyn Any`, and come out as the
//! type its ends were made for, so a [`Sender`], a [`SyncSender`] and a [`Receiver`] of any type
//! are std's of one type alone.
//!
//! A std channel is compiled whole for each type it carries, its three kinds (the bounded, the
//! unbounded and the rendezvous) with their sends, receives and drops, as a receive picks the kind
//! at run time: 5 to 16 KB of a binary for every type, in every crate that makes one, as a release
//! build compiles a generic in each crate that uses it. This crate's own code made seven (a
//! plugin process's lines, the end of its stderr and its replies, the committer's queued jobs and
//! its wakes, a described setting's program output, an authority's answer), some 74 KB of the
//! server. Carried boxed, every channel this crate makes shares one, and a type costs its box and
//! its unboxing alone. The board's code does the same with a module of its own
//! (`symphony_board`'s `crate::channel`), as a module one crate shares with another is still
//! compiled once in each.
//!
//! Each end is invariant in its type. Were a [`Receiver`] covariant or a sender contravariant, a
//! channel of a type with a more general subtype, such as `fn(&'static str)` of
//! `for<'a> fn(&'a str)`, could be taken as a channel of the other, and a receive would find an item
//! of a type its end was not made for. Invariant, the compiler refuses that view (the tests compile
//! it to see).
//!
//! Everything else is std's, its errors too: an item sent is received once, in the order sent; a
//! receive waits until an item comes, every sender has gone ([`mpsc::RecvError`],
//! [`RecvTimeoutError::Disconnected`]) or, with a deadline, it passes
//! ([`RecvTimeoutError::Timeout`]); a send fails only once the receiver has gone, handing the item
//! back; and a bounded channel's `try_send` hands it back too where the channel is full.

use std::any::Any;
use std::marker::PhantomData;
use std::sync::mpsc;
use std::time::Duration;

pub(crate) use std::sync::mpsc::{RecvTimeoutError, TrySendError};

/// What a channel made here carries, whatever the type of its ends.
type Boxed = Box<dyn Any + Send>;

/// An unbounded channel of `T`: its sending end, which clones, and its receiving end.
pub(crate) fn channel<T: Send + 'static>() -> (Sender<T>, Receiver<T>) {
    let (sender, receiver) = mpsc::channel();
    (Sender(sender, PhantomData), Receiver(receiver, PhantomData))
}

/// A channel of `T` holding `bound` items at most: its sending end and its receiving end.
pub(crate) fn sync_channel<T: Send + 'static>(bound: usize) -> (SyncSender<T>, Receiver<T>) {
    let (sender, receiver) = mpsc::sync_channel(bound);
    (
        SyncSender(sender, PhantomData),
        Receiver(receiver, PhantomData),
    )
}

/// The sending end of an unbounded channel of `T` ([`channel`]), invariant in `T` (the module's
/// doc).
pub(crate) struct Sender<T>(mpsc::Sender<Boxed>, PhantomData<fn(T) -> T>);

impl<T: Send + 'static> Sender<T> {
    /// `item` sent, or handed back where the receiver has gone.
    pub(crate) fn send(&self, item: T) -> Result<(), mpsc::SendError<T>> {
        (self.0.send(Box::new(item)))
            .map_err(|mpsc::SendError(item)| mpsc::SendError(unboxed(item)))
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender(self.0.clone(), PhantomData)
    }
}

/// The sending end of a bounded channel of `T` ([`sync_channel`]), invariant in `T` (the module's
/// doc).
pub(crate) struct SyncSender<T>(mpsc::SyncSender<Boxed>, PhantomData<fn(T) -> T>);

impl<T: Send + 'static> SyncSender<T> {
    /// `item` sent without waiting, or handed back where the channel is full or the receiver has
    /// gone.
    pub(crate) fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        (self.0.try_send(Box::new(item))).map_err(|error| match error {
            TrySendError::Full(item) => TrySendError::Full(unboxed(item)),
            TrySendError::Disconnected(item) => TrySendError::Disconnected(unboxed(item)),
        })
    }
}

/// The receiving end of a channel of `T` ([`channel`], [`sync_channel`]), invariant in `T` (the
/// module's doc).
pub(crate) struct Receiver<T>(mpsc::Receiver<Boxed>, PhantomData<fn(T) -> T>);

impl<T: 'static> Receiver<T> {
    /// The next item, waited for until one comes or every sender has gone.
    pub(crate) fn recv(&self) -> Result<T, mpsc::RecvError> {
        self.0.recv().map(unboxed)
    }

    /// The next item, waited for `within` at most.
    pub(crate) fn recv_timeout(&self, within: Duration) -> Result<T, RecvTimeoutError> {
        self.0.recv_timeout(within).map(unboxed)
    }

    /// The items already sent, taken without waiting.
    pub(crate) fn try_iter(&self) -> impl Iterator<Item = T> + '_ {
        self.0.try_iter().map(unboxed)
    }
}

impl<'a, T: 'static> IntoIterator for &'a Receiver<T> {
    type Item = T;
    type IntoIter = std::iter::Map<mpsc::Iter<'a, Boxed>, fn(Boxed) -> T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().map(unboxed as fn(Boxed) -> T)
    }
}

impl<T: 'static> IntoIterator for Receiver<T> {
    type Item = T;
    type IntoIter = std::iter::Map<mpsc::IntoIter<Boxed>, fn(Boxed) -> T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().map(unboxed as fn(Boxed) -> T)
    }
}

/// `item` as the `T` it was sent as: only a sender of `T` puts one on a channel of `T`.
fn unboxed<T: 'static>(item: Boxed) -> T {
    match item.downcast() {
        Ok(item) => *item,
        Err(_) => mistyped(),
    }
}

#[cold]
fn mistyped() -> ! {
    unreachable!("a channel of this crate carries only the type its ends were made for")
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
