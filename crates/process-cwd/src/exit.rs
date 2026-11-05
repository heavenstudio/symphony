//! A child's end awaited, as tokio's `kill_on_drop` wait awaits it, for a child [`Command`] started
//! without a fork: tokio can wait only for std's child, whose spawn copies every descriptor not yet
//! close-on-exec into it (a connection a listener accepted a moment before, say).
//!
//! [`Exit`] is that wait as a future. A thread of its own blocks until the child has ended without
//! reaping it (`waitid` with `WNOWAIT`), then reaps it under the lock the future shares and wakes
//! the future's task. An `Exit` dropped first sends SIGKILL under that lock, and the thread reaps
//! the child as it ends: the drop never waits for it. The pid is signalled only while it is
//! unreaped, so still this child's, never a process that took its number after.
//!
//! [`Command`]: crate::spawn::Command

use crate::spawn::Child;
use std::future::Future;
use std::io;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

/// The end of a child, ready once it has ended and been reaped: its status, or the error its
/// wait answered. Dropped before then, it kills the child (SIGKILL), whose end is still reaped.
#[derive(Debug)]
pub struct Exit {
    watched: Arc<Mutex<Watched>>,
}

#[derive(Debug)]
struct Watched {
    child: Child,
    /// Set once the child is reaped, or once its end could not be asked: its pid is no longer
    /// known to be this child's, and nothing signals it after.
    over: bool,
    /// The answer, until the future takes it.
    outcome: Option<io::Result<ExitStatus>>,
    answered: bool,
    waker: Option<Waker>,
}

/// The shared state, poisoned or not. A poisoned state is still one the guard and the future may
/// act on: each holder's writes are single field assignments that cannot panic midway, and the
/// one that reaps sets `over` in the statement after its reap, before anything else, so no
/// state ever says unreaped of a reaped child. A panic elsewhere under the lock leaves at worst
/// an answer not yet stored, which a poll waits for and a drop never needs.
fn lock(watched: &Mutex<Watched>) -> MutexGuard<'_, Watched> {
    watched.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Exit {
    /// `child`'s end, its input closed first as std's `wait` closes it. Its output pipes, if it
    /// has any, are kept with it until it ends: take them first to read them. A child already
    /// waited for, or that has ended, answers its status at once, and its pid is never asked or
    /// signalled. Fails only when no thread can be started to wait on it; the child is then
    /// killed and reaped before the error.
    pub fn of(mut child: Child) -> io::Result<Self> {
        drop(child.stdin.take());
        let pid = child.id();
        // The child's own cached end first: once reaped, its pid may already be another's.
        let ended = child.try_wait();
        let over = !matches!(ended, Ok(None));
        let watched = Arc::new(Mutex::new(Watched {
            child,
            over,
            outcome: ended.transpose(),
            answered: false,
            waker: None,
        }));
        if over {
            return Ok(Self { watched });
        }
        let watching = Arc::clone(&watched);
        let started = std::thread::Builder::new()
            .name("child exit".into())
            .spawn(move || watch(&watching, pid));
        if let Err(error) = started {
            let mut held = lock(&watched);
            let _ = held.child.kill();
            let _ = held.child.wait();
            held.over = true;
            return Err(error);
        }
        Ok(Self { watched })
    }
}

/// The waiting thread: the child's end, then its reap and answer under the lock.
fn watch(watched: &Mutex<Watched>, pid: u32) {
    let ended = ended(pid);
    let waker = {
        let mut held = lock(watched);
        let reaped = ended.and_then(|()| held.child.try_wait());
        held.over = true;
        held.outcome = Some(reaped.and_then(|status| {
            status.ok_or_else(|| {
                io::Error::other(format!("child {pid} ended but could not be reaped"))
            })
        }));
        held.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// Blocks until child `pid` has ended, leaving it waitable: its pid stays this child's.
fn ended(pid: u32) -> io::Result<()> {
    loop {
        let mut info = MaybeUninit::<libc::siginfo_t>::uninit();
        // SAFETY: waits on this process's own unreaped child only; waitid writes the siginfo it is
        // given, which is never read, and WNOWAIT leaves the child to be reaped by its owner.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if waited == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

impl Future for Exit {
    type Output = io::Result<ExitStatus>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut held = lock(&self.watched);
        if held.answered {
            return Poll::Ready(Err(io::Error::other(
                "the child's end was already answered",
            )));
        }
        match held.outcome.take() {
            Some(outcome) => {
                held.answered = true;
                Poll::Ready(outcome)
            }
            None => {
                held.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for Exit {
    fn drop(&mut self) {
        let mut held = lock(&self.watched);
        if !held.over {
            // Unreaped, so still this child's pid; the waiting thread reaps it as it ends.
            let _ = held.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn::{Command, Stdio, output, pipe};
    use std::io::{Read, Write};
    use std::os::unix::process::ExitStatusExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;
    use std::thread::Thread;
    use std::time::{Duration, Instant};

    /// A waker that counts its wakes and unparks the thread that polls.
    struct Unpark {
        thread: Thread,
        wakes: AtomicUsize,
    }

    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.wakes.fetch_add(1, Ordering::SeqCst);
            self.thread.unpark();
        }
    }

    fn unpark() -> Arc<Unpark> {
        Arc::new(Unpark {
            thread: std::thread::current(),
            wakes: AtomicUsize::new(0),
        })
    }

    fn poll(exit: &mut Exit, waker: &Arc<Unpark>) -> Poll<io::Result<ExitStatus>> {
        let waker = Waker::from(Arc::clone(waker));
        Pin::new(exit).poll(&mut Context::from_waker(&waker))
    }

    /// `exit`'s answer, polled again only once its waker is woken, within a bound that only a wake
    /// that never comes reaches.
    fn answer(exit: &mut Exit) -> io::Result<ExitStatus> {
        let waker = unpark();
        let began = Instant::now();
        loop {
            let woken = waker.wakes.load(Ordering::SeqCst);
            if let Poll::Ready(answer) = poll(exit, &waker) {
                return answer;
            }
            while waker.wakes.load(Ordering::SeqCst) == woken {
                assert!(began.elapsed() < Duration::from_secs(30), "never woken");
                std::thread::park_timeout(Duration::from_secs(1));
            }
        }
    }

    /// Whether process `pid` is in the table at all: a zombie still is, a reaped child is not.
    fn listed(pid: u32) -> bool {
        let said =
            output(Command::new("/bin/ps").args(["-o", "pid=", "-p", &pid.to_string()])).unwrap();
        !String::from_utf8_lossy(&said.stdout).trim().is_empty()
    }

    fn until_unlisted(pid: u32) {
        let began = Instant::now();
        while listed(pid) {
            assert!(
                began.elapsed() < Duration::from_secs(30),
                "child {pid} was never reaped"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn an_exit_answers_its_childs_status_once_and_reaps_it() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut exit = Exit::of(child).unwrap();
        assert_eq!(answer(&mut exit).unwrap().code(), Some(7));
        assert!(!listed(pid), "answered before it was reaped");
        let again = poll(&mut exit, &unpark());
        assert!(
            matches!(&again, Poll::Ready(Err(error)) if error.to_string().contains("already")),
            "{again:?}"
        );
        // Answered, so over: the drop sends nothing to a pid that is no longer this child's.
        assert!(lock(&exit.watched).over);
    }

    /// A child whose end its owner already has answers that end on the first poll, from its cache:
    /// its pid, perhaps another process's by then, is never waited on or signalled.
    #[test]
    fn a_child_already_waited_for_answers_its_cached_status_at_once() {
        let waited = |code: i32| {
            Command::new("/bin/sh")
                .args(["-c", &format!("exit {code}")])
                .spawn()
                .unwrap()
        };
        let mut by_wait = waited(7);
        assert_eq!(by_wait.wait().unwrap().code(), Some(7));
        let mut by_try = waited(9);
        let began = Instant::now();
        while by_try.try_wait().unwrap().is_none() {
            assert!(began.elapsed() < Duration::from_secs(30), "never ended");
            std::thread::sleep(Duration::from_millis(10));
        }
        for (child, code) in [(by_wait, 7), (by_try, 9)] {
            let mut exit = Exit::of(child).unwrap();
            assert!(
                lock(&exit.watched).over,
                "{code}: its pid is not this child's"
            );
            let answered = poll(&mut exit, &unpark());
            assert!(
                matches!(&answered, Poll::Ready(Ok(status)) if status.code() == Some(code)),
                "{code}: {answered:?}"
            );
        }
    }

    #[test]
    fn an_exit_is_pending_until_its_child_ends_and_is_woken_then() {
        let (reader, mut writer) = pipe().unwrap();
        let child = Command::new("/bin/sh")
            .args(["-c", "read -r line; kill -TERM $$"])
            .stdin(reader)
            .spawn()
            .unwrap();
        let mut exit = Exit::of(child).unwrap();
        let waker = unpark();
        assert!(
            poll(&mut exit, &waker).is_pending(),
            "ended before its line"
        );
        writer.write_all(b"end\n").unwrap();
        let status = answer(&mut exit).unwrap();
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }

    #[test]
    fn an_exit_closes_its_childs_input_first() {
        let child = Command::new("cat").stdin(Stdio::piped()).spawn().unwrap();
        assert!(answer(&mut Exit::of(child).unwrap()).unwrap().success());
    }

    #[test]
    fn dropping_a_pending_exit_kills_its_hung_child_and_the_child_is_still_reaped() {
        let (mut reader, writer) = pipe().unwrap();
        let child = Command::new("/bin/sleep")
            .arg("60")
            .stdout(writer)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut exit = Exit::of(child).unwrap();
        assert!(poll(&mut exit, &unpark()).is_pending());
        let dropped = Instant::now();
        drop(exit);
        assert!(
            dropped.elapsed() < Duration::from_secs(5),
            "the drop waited for its child"
        );
        // The child held the writer: its end reads as EOF only once it has been killed.
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert!(rest.is_empty());
        until_unlisted(pid);
    }

    #[test]
    fn dropping_an_exit_never_polled_kills_its_child_too() {
        let child = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        drop(Exit::of(child).unwrap());
        until_unlisted(pid);
    }

    #[test]
    fn exits_of_many_children_each_answer_their_own() {
        let mut exits: Vec<(i32, Exit)> = (0..16)
            .map(|code| {
                let child = Command::new("/bin/sh")
                    .args(["-c", &format!("exit {code}")])
                    .spawn()
                    .unwrap();
                (code, Exit::of(child).unwrap())
            })
            .collect();
        for (code, exit) in &mut exits {
            assert_eq!(answer(exit).unwrap().code(), Some(*code));
        }
    }
}
