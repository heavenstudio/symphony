//! A process's connections to its hosted board: at most `max` at once, counting those being made
//! and those being closed, lent one at a time to one borrower each, and a bounded line of
//! borrowers served first come, first served.
//!
//! A borrower takes an idle connection, or makes one while fewer than `max` exist, or waits for a
//! return until its own deadline. It never makes one past `max`: a full line refuses at once and
//! a deadline refuses where it stands, both as [`Refused`], which is capacity and no reason to
//! try anywhere else. A connection is returned only by a borrower that says it is clean
//! ([`Lease::keep`]); any other is closed, and counts until its closing ends. A pool made in one
//! process lends nothing in its fork, and a thread that holds a lease is refused a second, which
//! it would wait for behind itself.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::lock::Recovered as _;

/// How a pool makes a connection, tells whether an idle one may be lent again, and closes one.
pub(super) trait Connector: Send + Sync {
    type Connection: Send;
    type Error;

    /// A new connection, made within `within`.
    fn connect(&self, within: Duration) -> Result<Self::Connection, Self::Error>;

    /// Whether an idle connection may be lent again: still open, and of the current credentials.
    fn usable(&self, connection: &Self::Connection) -> bool;

    /// `connection` closed, its server session ended as far as the client can tell.
    fn close(&self, connection: Self::Connection);
}

/// Why a borrower got no connection.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refused<E> {
    /// As many borrowers wait as the line holds.
    LineFull,
    /// The borrower's deadline came while it waited.
    Deadline,
    /// The pool is shut down.
    Closed,
    /// This process is a fork of the one that made the pool.
    Forked,
    /// This thread already holds a connection of the pool.
    Nested,
    /// The connection could not be made.
    Connect(E),
}

/// What a pool holds now, for its health.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Lent to a borrower.
    pub active: usize,
    /// Open and waiting to be lent.
    pub idle: usize,
    /// Being made, or being closed.
    pub connecting: usize,
    /// Borrowers in the line.
    pub waiting: usize,
}

struct State<C> {
    idle: VecDeque<C>,
    active: usize,
    connecting: usize,
    line: VecDeque<u64>,
    next: u64,
    closed: bool,
}

pub(super) struct Pool<K: Connector> {
    connector: K,
    max: usize,
    line_max: usize,
    pid: u32,
    state: Mutex<State<K::Connection>>,
    changed: Condvar,
}

thread_local! {
    /// Whether this thread holds a lease of any pool.
    static HOLDING: Cell<bool> = const { Cell::new(false) };
}

/// One connection lent to one borrower: closed when dropped, unless kept.
pub(super) struct Lease<'a, K: Connector> {
    pool: &'a Pool<K>,
    connection: Option<K::Connection>,
}

impl<K: Connector> Pool<K> {
    pub(super) fn new(connector: K, max: usize, line_max: usize) -> Pool<K> {
        Pool {
            connector,
            max,
            line_max,
            pid: std::process::id(),
            state: Mutex::new(State {
                idle: VecDeque::new(),
                active: 0,
                connecting: 0,
                line: VecDeque::new(),
                next: 0,
                closed: false,
            }),
            changed: Condvar::new(),
        }
    }

    pub(super) fn connector(&self) -> &K {
        &self.connector
    }

    pub(super) fn counts(&self) -> Counts {
        let state = self.locked();
        Counts {
            active: state.active,
            idle: state.idle.len(),
            connecting: state.connecting,
            waiting: state.line.len(),
        }
    }

    fn locked(&self) -> MutexGuard<'_, State<K::Connection>> {
        self.state.lock().recover()
    }

    /// A connection for this borrower alone, waited for at most `within`, and made within
    /// `connecting` when it is made.
    pub(super) fn acquire(
        &self,
        within: Duration,
        connecting: Duration,
    ) -> Result<Lease<'_, K>, Refused<K::Error>> {
        if std::process::id() != self.pid {
            return Err(Refused::Forked);
        }
        if HOLDING.get() {
            return Err(Refused::Nested);
        }
        let deadline = Instant::now() + within;
        let mut state = self.locked();
        if state.closed {
            return Err(Refused::Closed);
        }
        // A borrower that would not wait takes no place in the line, so a line of none still
        // lends what is free.
        let free = state.line.is_empty()
            && (!state.idle.is_empty() || state.active + state.connecting < self.max);
        if !free && state.line.len() >= self.line_max {
            return Err(Refused::LineFull);
        }
        let ticket = state.next;
        state.next += 1;
        state.line.push_back(ticket);
        loop {
            if state.closed {
                state.line.retain(|waiting| *waiting != ticket);
                return Err(Refused::Closed);
            }
            if state.line.front() == Some(&ticket) {
                // Idle connections no longer usable are closed on the way, each counted until
                // its closing ends.
                while let Some(connection) = state.idle.pop_front() {
                    if self.connector.usable(&connection) {
                        state.line.pop_front();
                        state.active += 1;
                        drop(state);
                        self.changed.notify_all();
                        return Ok(self.lease(connection));
                    }
                    state.connecting += 1;
                    drop(state);
                    self.connector.close(connection);
                    state = self.locked();
                    state.connecting -= 1;
                }
                if state.active + state.connecting < self.max {
                    state.line.pop_front();
                    state.connecting += 1;
                    drop(state);
                    self.changed.notify_all();
                    let made = self.connector.connect(connecting);
                    let mut state = self.locked();
                    state.connecting -= 1;
                    return match made {
                        Ok(connection) => {
                            state.active += 1;
                            drop(state);
                            Ok(self.lease(connection))
                        }
                        Err(why) => {
                            drop(state);
                            self.changed.notify_all();
                            Err(Refused::Connect(why))
                        }
                    };
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                state.line.retain(|waiting| *waiting != ticket);
                drop(state);
                self.changed.notify_all();
                return Err(Refused::Deadline);
            }
            state = self.changed.wait_timeout(state, left).recover().0;
        }
    }

    fn lease(&self, connection: K::Connection) -> Lease<'_, K> {
        HOLDING.set(true);
        Lease {
            pool: self,
            connection: Some(connection),
        }
    }

    /// No more lent: every borrower in the line refused, every idle connection closed. A
    /// connection out on a lease is closed when its borrower ends.
    pub(super) fn shut_down(&self) {
        let mut state = self.locked();
        state.closed = true;
        let idle: Vec<K::Connection> = state.idle.drain(..).collect();
        state.connecting += idle.len();
        drop(state);
        self.changed.notify_all();
        let closed = idle.len();
        for connection in idle {
            self.connector.close(connection);
        }
        self.locked().connecting -= closed;
        self.changed.notify_all();
    }

    /// Waits until no connection is lent or being made, at most `within`: whether it came to that.
    pub(super) fn drained(&self, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        let mut state = self.locked();
        while state.active + state.connecting > 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self.changed.wait_timeout(state, left).recover().0;
        }
        true
    }
}

impl<K: Connector> Lease<'_, K> {
    pub(super) fn connection(&mut self) -> &mut K::Connection {
        self.connection.as_mut().expect("held until dropped")
    }

    /// The connection returned to be lent again: its borrower saw its transaction end and
    /// nothing of its own left on the session.
    pub(super) fn keep(mut self) {
        let connection = self.connection.take().expect("held until dropped");
        HOLDING.set(false);
        let mut state = self.pool.locked();
        state.active -= 1;
        if state.closed {
            state.connecting += 1;
            drop(state);
            self.pool.connector.close(connection);
            self.pool.locked().connecting -= 1;
        } else {
            state.idle.push_back(connection);
            drop(state);
        }
        self.pool.changed.notify_all();
    }
}

impl<K: Connector> Drop for Lease<'_, K> {
    /// A connection not kept is closed, counted until its closing ends.
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        HOLDING.set(false);
        {
            let mut state = self.pool.locked();
            state.active -= 1;
            state.connecting += 1;
        }
        self.pool.connector.close(connection);
        self.pool.locked().connecting -= 1;
        self.pool.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};

    const LONG: Duration = test_time::backstop(Duration::from_secs(10));
    const NONE: Duration = Duration::ZERO;

    /// Connections that are numbers, each made, closed and refused as the test says.
    #[derive(Default)]
    struct Numbers {
        made: AtomicUsize,
        closed: Mutex<Vec<usize>>,
        stale: Mutex<Vec<usize>>,
        failing: AtomicBool,
        /// Held while a connection is made, by a test that holds it.
        making: Mutex<()>,
        /// The most that existed at once: made and not yet closed.
        open: AtomicUsize,
        most: AtomicUsize,
    }

    impl Connector for Arc<Numbers> {
        type Connection = usize;
        type Error = &'static str;

        fn connect(&self, _within: Duration) -> Result<usize, &'static str> {
            let _making = self.making.lock().unwrap();
            if self.failing.load(Ordering::SeqCst) {
                return Err("unreachable");
            }
            let open = self.open.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(open, Ordering::SeqCst);
            Ok(self.made.fetch_add(1, Ordering::SeqCst))
        }

        fn usable(&self, connection: &usize) -> bool {
            !self.stale.lock().unwrap().contains(connection)
        }

        fn close(&self, connection: usize) {
            self.open.fetch_sub(1, Ordering::SeqCst);
            self.closed.lock().unwrap().push(connection);
        }
    }

    fn pool(max: usize, line: usize) -> (Arc<Numbers>, Arc<Pool<Arc<Numbers>>>) {
        let numbers = Arc::new(Numbers::default());
        (
            Arc::clone(&numbers),
            Arc::new(Pool::new(numbers, max, line)),
        )
    }

    /// What a borrower got: its connection's number, or its refusal.
    type Got = mpsc::Receiver<Result<usize, Refused<&'static str>>>;

    /// A borrower on a thread of its own: it sends what it got, and keeps a lease until told.
    fn borrower(
        pool: &Arc<Pool<Arc<Numbers>>>,
        within: Duration,
    ) -> (Got, mpsc::Sender<bool>, std::thread::JoinHandle<()>) {
        let (got, gotten) = crate::test_channel::channel();
        let (release, released) = crate::test_channel::channel::<bool>();
        let pool = Arc::clone(pool);
        let thread = std::thread::spawn(move || match pool.acquire(within, LONG) {
            Ok(mut lease) => {
                got.send(Ok(*lease.connection())).unwrap();
                if released.recv().unwrap_or(false) {
                    lease.keep();
                }
            }
            Err(refused) => got.send(Err(refused)).unwrap(),
        });
        (gotten, release, thread)
    }

    /// Waits for the line to hold `waiting` borrowers.
    fn lined_up(pool: &Pool<Arc<Numbers>>, waiting: usize) {
        test_time::wait::until_true("the borrowers to line up", LONG, || {
            pool.counts().waiting == waiting
        })
        .unwrap();
    }

    #[test]
    fn a_kept_connection_is_lent_again_and_one_not_kept_is_closed() {
        let (numbers, pool) = pool(2, 4);
        let mut first = pool.acquire(NONE, LONG).ok().unwrap();
        assert_eq!(*first.connection(), 0);
        assert_eq!(
            pool.counts(),
            Counts {
                active: 1,
                ..Counts::default()
            }
        );
        first.keep();
        assert_eq!(
            pool.counts(),
            Counts {
                idle: 1,
                ..Counts::default()
            }
        );
        let mut again = pool.acquire(NONE, LONG).ok().unwrap();
        assert_eq!(*again.connection(), 0, "the idle one, and none made");
        drop(again);
        assert_eq!(*numbers.closed.lock().unwrap(), [0]);
        assert_eq!(pool.counts(), Counts::default());
        let mut made = pool.acquire(NONE, LONG).ok().unwrap();
        assert_eq!(*made.connection(), 1);
    }

    #[test]
    fn an_idle_connection_no_longer_usable_is_closed_and_another_made() {
        let (numbers, pool) = pool(1, 0);
        pool.acquire(NONE, LONG).ok().unwrap().keep();
        numbers.stale.lock().unwrap().push(0);
        let mut lease = pool.acquire(NONE, LONG).ok().unwrap();
        assert_eq!(*lease.connection(), 1);
        assert_eq!(*numbers.closed.lock().unwrap(), [0]);
        assert_eq!(numbers.most.load(Ordering::SeqCst), 1, "never two at once");
    }

    #[test]
    fn a_thread_holding_a_lease_is_refused_a_second_and_a_failed_connect_frees_its_place() {
        let (numbers, pool) = pool(2, 4);
        let lease = pool.acquire(NONE, LONG).ok().unwrap();
        assert!(matches!(pool.acquire(NONE, LONG), Err(Refused::Nested)));
        lease.keep();
        numbers.stale.lock().unwrap().push(0);
        numbers.failing.store(true, Ordering::SeqCst);
        assert!(matches!(
            pool.acquire(NONE, LONG),
            Err(Refused::Connect("unreachable"))
        ));
        assert_eq!(pool.counts(), Counts::default());
        numbers.failing.store(false, Ordering::SeqCst);
        assert!(
            pool.acquire(NONE, LONG).is_ok(),
            "the thread holds none now"
        );
    }

    #[test]
    fn borrowers_past_the_most_wait_in_line_and_are_served_in_their_order() {
        let (numbers, pool) = pool(1, 2);
        let (first, release_first, t1) = borrower(&pool, LONG);
        assert_eq!(first.recv().unwrap(), Ok(0));
        let (second, release_second, t2) = borrower(&pool, LONG);
        lined_up(&pool, 1);
        let (third, release_third, t3) = borrower(&pool, LONG);
        lined_up(&pool, 2);
        // The line is full: one more is refused at once, and makes no connection.
        assert!(matches!(pool.acquire(LONG, LONG), Err(Refused::LineFull)));
        release_first.send(true).unwrap();
        assert_eq!(second.recv().unwrap(), Ok(0), "the first in line");
        assert!(third.try_recv().is_err(), "the second still waits");
        release_second.send(false).unwrap();
        assert_eq!(third.recv().unwrap(), Ok(1), "a new one, the other closed");
        release_third.send(true).unwrap();
        for thread in [t1, t2, t3] {
            thread.join().unwrap();
        }
        assert_eq!(
            numbers.most.load(Ordering::SeqCst),
            1,
            "never past the most"
        );
        assert_eq!(numbers.made.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_borrower_whose_deadline_comes_leaves_the_line_and_the_next_is_served() {
        let (_numbers, pool) = pool(1, 2);
        let (first, release_first, t1) = borrower(&pool, LONG);
        assert_eq!(first.recv().unwrap(), Ok(0));
        let (hasty, _release, t2) = borrower(&pool, Duration::from_millis(20));
        assert_eq!(hasty.recv().unwrap(), Err(Refused::Deadline));
        t2.join().unwrap();
        assert_eq!(pool.counts().waiting, 0);
        let (patient, release_patient, t3) = borrower(&pool, LONG);
        lined_up(&pool, 1);
        release_first.send(true).unwrap();
        assert_eq!(patient.recv().unwrap(), Ok(0));
        release_patient.send(true).unwrap();
        t1.join().unwrap();
        t3.join().unwrap();
    }

    #[test]
    fn a_connection_being_made_counts_toward_the_most() {
        let (numbers, pool) = pool(1, 1);
        let making = numbers.making.lock().unwrap();
        let (first, release_first, t1) = borrower(&pool, LONG);
        test_time::wait::until_true("the connection to be under way", LONG, || {
            pool.counts().connecting == 1
        })
        .unwrap();
        let (second, release_second, t2) = borrower(&pool, LONG);
        lined_up(&pool, 1);
        assert_eq!(numbers.made.load(Ordering::SeqCst), 0);
        drop(making);
        assert_eq!(first.recv().unwrap(), Ok(0));
        assert!(
            second.try_recv().is_err(),
            "it waits for the one being made"
        );
        release_first.send(true).unwrap();
        assert_eq!(second.recv().unwrap(), Ok(0));
        release_second.send(true).unwrap();
        t1.join().unwrap();
        t2.join().unwrap();
        assert_eq!(numbers.most.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shutting_down_refuses_the_line_closes_the_idle_and_waits_for_the_lent() {
        let (numbers, pool) = pool(2, 2);
        pool.acquire(NONE, LONG).ok().unwrap().keep();
        let (first, release_first, t1) = borrower(&pool, LONG);
        assert_eq!(first.recv().unwrap(), Ok(0));
        let (second, release_second, t2) = borrower(&pool, LONG);
        assert_eq!(second.recv().unwrap(), Ok(1));
        let (waiting, _release, t3) = borrower(&pool, LONG);
        lined_up(&pool, 1);
        pool.shut_down();
        assert_eq!(waiting.recv().unwrap(), Err(Refused::Closed));
        assert!(matches!(pool.acquire(NONE, LONG), Err(Refused::Closed)));
        assert!(
            !pool.drained(Duration::from_millis(10)),
            "two are still lent"
        );
        release_first.send(true).unwrap();
        release_second.send(false).unwrap();
        assert!(pool.drained(LONG));
        for thread in [t1, t2, t3] {
            thread.join().unwrap();
        }
        let mut closed = numbers.closed.lock().unwrap().clone();
        closed.sort_unstable();
        assert_eq!(
            closed,
            [0, 1],
            "kept or not, a lease ends closed after shutdown"
        );
        assert_eq!(pool.counts(), Counts::default());
    }

    #[test]
    fn a_fork_of_the_pools_process_is_lent_nothing() {
        let (_numbers, pool) = pool(1, 1);
        let other = Pool {
            connector: Arc::clone(pool.connector()),
            max: 1,
            line_max: 1,
            pid: std::process::id().wrapping_add(1),
            state: Mutex::new(State {
                idle: VecDeque::new(),
                active: 0,
                connecting: 0,
                line: VecDeque::new(),
                next: 0,
                closed: false,
            }),
            changed: Condvar::new(),
        };
        assert!(matches!(other.acquire(NONE, LONG), Err(Refused::Forked)));
    }
}
