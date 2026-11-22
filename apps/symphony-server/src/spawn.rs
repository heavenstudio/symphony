//! The server's work handed to tokio, every call through one task type.
//!
//! Tokio compiles its task machinery (the task's cell, its poll, its cancel, its join handle's
//! reads) once for each type of work it is handed, about 4 KB, and a spawned future's twice, once
//! for each of the runtime's schedulers: a closure handed to [`tokio::task::spawn_blocking`], or a
//! future to [`tokio::spawn`], costs that again at every call site. [`blocking`] hands tokio one
//! type for every call, a boxed closure that answers nothing, and keeps the work's answer beside the
//! task until the task ends; [`task`] boxes a future that answers nothing. What each call compiles
//! is then its own closure or future and that answer's hand-over alone (docs/workflow/CHECKS.md, "Size
//! budgets").

use crate::lock::Held as _;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use tokio::task::{JoinError, JoinHandle};

/// `work` run on tokio's blocking pool, as [`tokio::task::spawn_blocking`] runs it: its answer
/// once it returns, or the [`JoinError`] of its panic. Dropped before then, the work still runs to
/// its end, and its answer is dropped with it.
pub(crate) fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Blocking<T> {
    let answer = Arc::new(Mutex::new(None));
    let put = Arc::clone(&answer);
    let work: Box<dyn FnOnce() + Send> = Box::new(move || {
        let made = work();
        *put.lock().held() = Some(made);
    });
    Blocking {
        task: tokio::task::spawn_blocking(work),
        answer,
    }
}

/// [`blocking`]'s work as it runs: the task, and where the work leaves its answer.
pub(crate) struct Blocking<T> {
    task: JoinHandle<()>,
    answer: Arc<Mutex<Option<T>>>,
}

impl<T> Future for Blocking<T> {
    type Output = Result<T, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ended = ready!(Pin::new(&mut self.task).poll(cx));
        Poll::Ready(ended.map(|()| {
            self.answer
                .lock()
                .held()
                .take()
                .expect("a blocking task that returned left its answer")
        }))
    }
}

/// `future` spawned on the runtime, as [`tokio::spawn`] spawns it.
pub(crate) fn task(future: impl Future<Output = ()> + Send + 'static) -> JoinHandle<()> {
    let future: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(future);
    tokio::spawn(future)
}

#[cfg(test)]
mod tests;
