//! How a thread of the board hands another what it has: one channel, std's [`mpsc`], whatever it
//! carries ([`channel`]). Its items go boxed, as `dyn Any`, and come out as the type its two ends
//! were made for, so a [`Sender`] and a [`Receiver`] of any type are std's of one type alone.
//!
//! A std channel is compiled whole for each type it carries, its three kinds (the bounded, the
//! unbounded and the rendezvous) with their sends, receives and drops, as a receive picks the kind
//! at run time: 5 to 16 KB of a binary for every type. `tasks` linked ten (a child's output, a
//! recorder's sightings, a slot's lock, a proxy's lines, an attempt's calls and more), some 110 KB
//! of it. Carried boxed, every channel the board's own code makes shares one, and a type costs its
//! box and its unboxing alone: the crate's clippy.toml refuses std's `channel` and `sync_channel`
//! anywhere else. What remains is not the board's code: the two channels the published
//! `process_cwd` crate makes for itself, and a test's or a load example's, std's
//! (tests/support/test_channel.rs), as neither holds a size line.
//!
//! Each end is invariant in its type. Were a [`Receiver`] covariant or a [`Sender`]
//! contravariant, a channel of a type with a more general subtype, such as `fn(&'static str)` of
//! `for<'a> fn(&'a str)`, could be taken as a channel of the other, and a receive would find an item
//! of a type its end was not made for. Invariant, the compiler refuses that view (the tests compile
//! it to see).
//!
//! Everything else is std's: an item sent is received once, in the order sent; a receive waits
//! until an item comes, every sender has gone ([`RecvError`], [`RecvTimeoutError::Disconnected`])
//! or, with a deadline, it passes ([`RecvTimeoutError::Timeout`]); and a send fails only once the
//! receiver has gone, handing the item back.

use std::any::Any;
use std::marker::PhantomData;
use std::sync::mpsc;
use std::time::Duration;

pub(crate) use std::sync::mpsc::{RecvError, RecvTimeoutError};

/// What a channel made here carries, whatever the type of its ends.
type Boxed = Box<dyn Any + Send>;

/// A channel of `T`: its sending end, which clones, and its receiving end.
pub(crate) fn channel<T: Send + 'static>() -> (Sender<T>, Receiver<T>) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one std channel of the board's code, of Boxed alone, whatever its ends carry"
    )]
    let (sender, receiver) = mpsc::channel();
    (Sender(sender, PhantomData), Receiver(receiver, PhantomData))
}

/// The sending end of a channel of `T` ([`channel`]), invariant in `T` (the module's doc).
pub(crate) struct Sender<T>(mpsc::Sender<Boxed>, PhantomData<fn(T) -> T>);

impl<T: Send + 'static> Sender<T> {
    /// `item` sent, or handed back where the receiver has gone.
    pub(crate) fn send(&self, item: T) -> Result<(), T> {
        (self.0.send(Box::new(item))).map_err(|mpsc::SendError(item)| unboxed(item))
    }
}

impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sender { .. }")
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender(self.0.clone(), PhantomData)
    }
}

/// The receiving end of a channel of `T` ([`channel`]), invariant in `T` (the module's doc).
pub(crate) struct Receiver<T>(mpsc::Receiver<Boxed>, PhantomData<fn(T) -> T>);

impl<T: 'static> Receiver<T> {
    /// The next item, waited for until one comes or every sender has gone.
    pub(crate) fn recv(&self) -> Result<T, RecvError> {
        self.0.recv().map(unboxed)
    }

    /// The next item, waited for `within` at most.
    pub(crate) fn recv_timeout(&self, within: Duration) -> Result<T, RecvTimeoutError> {
        self.0.recv_timeout(within).map(unboxed)
    }
}

impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Receiver { .. }")
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

/// `item` as the `T` it was sent as: only a [`Sender<T>`] puts one on a channel of `T`.
fn unboxed<T: 'static>(item: Boxed) -> T {
    match item.downcast() {
        Ok(item) => *item,
        Err(_) => mistyped(),
    }
}

#[cold]
fn mistyped() -> ! {
    unreachable!("a channel of the board carries only the type its ends were made for")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_come_in_the_order_sent_from_every_clone_and_end_with_their_senders() {
        let (send, receive) = channel::<(u8, String)>();
        assert_eq!(
            format!("{send:?} {receive:?}"),
            "Sender { .. } Receiver { .. }"
        );
        let other = send.clone();
        send.send((1, "a".into())).unwrap();
        other.send((2, "b".into())).unwrap();
        send.send((3, "c".into())).unwrap();
        assert_eq!(receive.recv(), Ok((1, "a".into())));
        drop((send, other));
        let rest: Vec<(u8, String)> = (&receive).into_iter().collect();
        assert_eq!(rest, [(2, "b".into()), (3, "c".into())]);
        assert_eq!(receive.recv(), Err(RecvError));
    }

    #[test]
    fn a_send_to_a_receiver_that_went_hands_its_item_back() {
        let (send, receive) = channel::<Vec<u8>>();
        drop(receive);
        assert_eq!(send.send(vec![1, 2]), Err(vec![1, 2]));
    }

    #[test]
    fn a_receive_with_a_deadline_says_whether_it_passed_or_the_senders_went() {
        let (send, receive) = channel::<()>();
        assert_eq!(
            receive.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Timeout)
        );
        send.send(()).unwrap();
        assert_eq!(receive.recv_timeout(Duration::ZERO), Ok(()));
        drop(send);
        assert_eq!(
            receive.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn a_receiver_taken_whole_iterates_each_item_and_so_does_one_lent() {
        let (send, receive) = channel::<u32>();
        let sender = std::thread::spawn(move || {
            for n in 0..5 {
                send.send(n).unwrap();
            }
        });
        let mut lent = Vec::new();
        for n in &receive {
            lent.push(n);
            if n == 1 {
                break;
            }
        }
        sender.join().unwrap();
        let whole: Vec<u32> = receive.into_iter().collect();
        assert_eq!((lent, whole), (vec![0, 1], vec![2, 3, 4]));
    }

    #[test]
    #[should_panic(expected = "carries only the type its ends were made for")]
    fn an_item_of_another_type_is_refused_as_a_mistake() {
        let _: u8 = unboxed(Box::new("not a byte"));
    }

    /// This module compiled by the build's own rustc as a crate of its own, after it a receiver of
    /// `General` (a fn type) returned as one of `received` (line 6) and a sender of `Static` (a fn
    /// type `General` is a subtype of) as one of `sent` (line 7), in `ends.rs`: whether it
    /// compiled, and each error the compiler pointed at a line, by its code, that line's file
    /// (`ends.rs`, or the whole path of a file outside the scratch folder) and the line. The
    /// scratch folder goes whether rustc started or not.
    fn compiled(name: &str, received: &str, sent: &str) -> (bool, Vec<(String, String, u64)>) {
        use crate::children::Child as _;
        let dir =
            std::env::temp_dir().join(format!("symphony-channel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let module = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/channel.rs");
        let ends = dir.join("ends.rs");
        let source = format!(
            "#[path = {module:?}]\nmod channel;\nuse channel::{{Receiver, Sender}};\n\
             type Static = fn(&'static str);\ntype General = for<'a> fn(&'a str);\n\
             fn received(end: Receiver<General>) -> Receiver<{received}> {{ end }}\n\
             fn sent(end: Sender<Static>) -> Sender<{sent}> {{ end }}\n"
        );
        std::fs::write(&ends, source).unwrap();
        let rustc = std::path::Path::new(env!("CARGO")).with_file_name("rustc");
        let output = std::process::Command::of(&rustc)
            .args([
                "--edition",
                "2024",
                "--crate-type",
                "lib",
                "--crate-name",
                "ends",
            ])
            .args([
                "--emit",
                "metadata",
                "--error-format",
                "json",
                "--cap-lints",
                "allow",
            ])
            .arg("--out-dir")
            .args([&dir, &ends])
            .output();
        std::fs::remove_dir_all(&dir).unwrap();
        let output = output.unwrap_or_else(|error| panic!("{}: {error}", rustc.display()));
        let errors = String::from_utf8_lossy(&output.stderr)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|said| said["level"] == "error")
            .filter_map(|said| {
                let spans = said["spans"].as_array()?;
                let primary = spans.iter().find(|span| span["is_primary"] == true)?;
                let code = said["code"]["code"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let file = std::path::Path::new(primary["file_name"].as_str()?);
                let file = file
                    .strip_prefix(&dir)
                    .unwrap_or(file)
                    .display()
                    .to_string();
                Some((code, file, primary["line_start"].as_u64()?))
            })
            .collect();
        (output.status.success(), errors)
    }

    #[test]
    fn neither_end_is_taken_as_a_channel_of_another_type() {
        // Each end at its own type compiles: the module is read whole, so what is refused below is
        // the other type alone.
        assert_eq!(compiled("own", "General", "Static"), (true, vec![]));
        let mismatched = |line| ("E0308".to_string(), "ends.rs".to_string(), line);
        assert_eq!(
            compiled("other", "Static", "General"),
            (false, vec![mismatched(6), mismatched(7)]),
            "a receiver taken as one of a supertype, a sender as one of a subtype"
        );
    }
}
