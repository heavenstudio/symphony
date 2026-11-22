//! A lock whose holder panicked is taken all the same: what it guards is kept whole by each
//! holder's own writes, and one panic in a request, a worker or a test's thread must not fail every
//! later one that takes the same lock. The board, the library and the server recover a poisoned
//! lock through [`Recovered`] alone, never by hand; a test that means a poisoned lock to fail it
//! keeps its `unwrap()`.

use std::sync::{LockResult, PoisonError};

/// What a lock's call gives, recovered whether or not a holder panicked: `.lock().recover()`,
/// `.read().recover()`, `.write().recover()`, `.get_mut().recover()`, `.into_inner().recover()`,
/// and a condition variable's wait (`.wait(guard).recover()`, `.wait_timeout(..).recover()`).
pub trait Recovered<G> {
    /// The guard, or the value, the call gave, from a poisoned lock as from any other. The lock
    /// stays poisoned, so each later take is recovered the same way.
    fn recover(self) -> G;
}

impl<G> Recovered<G> for LockResult<G> {
    fn recover(self) -> G {
        self.unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::Recovered as _;
    use std::sync::{Condvar, LockResult, Mutex, RwLock};
    use std::time::Duration;

    /// `holding` run on a thread of its own, which panics with the guard it returns still held,
    /// so the lock it took is poisoned.
    fn panics_holding<G>(holding: impl FnOnce() -> G + Send) {
        std::thread::scope(|scope| {
            let panicked = scope
                .spawn(|| {
                    let _guard = holding();
                    panic!("a holder panics");
                })
                .join();
            assert!(panicked.is_err());
        });
    }

    /// What `result` recovers, once it is seen to be poisoned exactly when `poisoned` says.
    fn recovered<G>(result: LockResult<G>, poisoned: bool) -> G {
        assert_eq!(result.is_err(), poisoned, "poisoned: {poisoned}");
        result.recover()
    }

    /// A mutex holding `["written"]`, written by a holder that then panicked when `poisoned`.
    fn mutex(poisoned: bool) -> Mutex<Vec<&'static str>> {
        let mutex = Mutex::new(Vec::new());
        if poisoned {
            panics_holding(|| {
                let mut guard = mutex.lock().recover();
                guard.push("written");
                guard
            });
        } else {
            mutex.lock().recover().push("written");
        }
        assert_eq!(mutex.is_poisoned(), poisoned);
        mutex
    }

    /// A rwlock holding 7, written by a holder that then panicked when `poisoned`.
    fn rwlock(poisoned: bool) -> RwLock<u32> {
        let rwlock = RwLock::new(0);
        if poisoned {
            panics_holding(|| {
                let mut guard = rwlock.write().recover();
                *guard = 7;
                guard
            });
        } else {
            *rwlock.write().recover() = 7;
        }
        assert_eq!(rwlock.is_poisoned(), poisoned);
        rwlock
    }

    /// A mutex gives the value its last holder left, poisoned or not, and recovering it leaves
    /// the poison in place.
    #[test]
    fn a_mutex_is_locked_poisoned_or_not() {
        for poisoned in [false, true] {
            let mutex = mutex(poisoned);
            recovered(mutex.lock(), poisoned).push("after");
            assert_eq!(*recovered(mutex.lock(), poisoned), ["written", "after"]);
            assert_eq!(mutex.is_poisoned(), poisoned, "recovered, not cleared");
        }
    }

    #[test]
    fn a_rwlock_is_read_and_written_poisoned_or_not() {
        for poisoned in [false, true] {
            let rwlock = rwlock(poisoned);
            assert_eq!(*recovered(rwlock.read(), poisoned), 7);
            *recovered(rwlock.write(), poisoned) += 1;
            assert_eq!(*recovered(rwlock.read(), poisoned), 8);
            assert_eq!(rwlock.is_poisoned(), poisoned, "recovered, not cleared");
        }
    }

    #[test]
    fn get_mut_gives_the_value_poisoned_or_not() {
        for poisoned in [false, true] {
            let mut mutex = mutex(poisoned);
            recovered(mutex.get_mut(), poisoned).push("by get_mut");
            assert_eq!(
                *recovered(mutex.lock(), poisoned),
                ["written", "by get_mut"]
            );
            let mut rwlock = rwlock(poisoned);
            *recovered(rwlock.get_mut(), poisoned) += 1;
            assert_eq!(*recovered(rwlock.read(), poisoned), 8);
        }
    }

    #[test]
    fn into_inner_gives_the_value_poisoned_or_not() {
        for poisoned in [false, true] {
            assert_eq!(
                recovered(mutex(poisoned).into_inner(), poisoned),
                ["written"]
            );
            assert_eq!(recovered(rwlock(poisoned).into_inner(), poisoned), 7);
        }
    }

    /// A condition variable's wait over a mutex gives its guard back, poisoned or not: a wait
    /// that times out, and one a notifier wakes.
    #[test]
    fn a_condvar_wait_gives_its_guard_poisoned_or_not() {
        for poisoned in [false, true] {
            let mutex = mutex(poisoned);
            let condvar = Condvar::new();
            let taken = recovered(mutex.lock(), poisoned);
            let (guard, _) = recovered(condvar.wait_timeout(taken, Duration::ZERO), poisoned);
            assert_eq!(*guard, ["written"]);
            drop(guard);
            std::thread::scope(|scope| {
                let mut guard = recovered(mutex.lock(), poisoned);
                scope.spawn(|| {
                    mutex.lock().recover().push("notified");
                    condvar.notify_all();
                });
                while guard.len() < 2 {
                    guard = recovered(condvar.wait(guard), poisoned);
                }
                assert_eq!(*guard, ["written", "notified"]);
            });
        }
    }
}
