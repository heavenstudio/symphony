//! A lock whose holder panicked is taken all the same, everywhere in the server: what it guards
//! is kept whole by each holder's own writes, and one panic in a request or a worker must not
//! fail every later one that takes the same lock.

use std::sync::{LockResult, PoisonError};

/// What a lock's call gives, held whether or not a holder panicked: `.lock().held()`,
/// `.read().held()`, `.write().held()`, `.get_mut().held()`, and a condition variable's wait.
pub(crate) trait Held<G> {
    /// The guard, or the value, the call gave, from a poisoned lock as from any other.
    fn held(self) -> G;
}

impl<G> Held<G> for LockResult<G> {
    fn held(self) -> G {
        self.unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::Held as _;
    use std::sync::{Condvar, Mutex, RwLock};
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

    #[test]
    fn an_unpoisoned_lock_is_held_as_it_is() {
        let mutex = Mutex::new(1);
        *mutex.lock().held() += 1;
        assert_eq!(*mutex.lock().held(), 2);
        let rwlock = RwLock::new("a");
        *rwlock.write().held() = "b";
        assert_eq!(*rwlock.read().held(), "b");
    }

    /// A mutex a holder poisoned gives the value the holder left, by its guard, its `get_mut` and
    /// its `into_inner`, and stays poisoned: each later take is held the same way.
    #[test]
    fn a_poisoned_mutex_gives_what_its_holder_left() {
        let mut mutex = Mutex::new(Vec::new());
        panics_holding(|| {
            let mut guard = mutex.lock().held();
            guard.push("written");
            guard
        });
        assert!(mutex.is_poisoned());
        assert_eq!(*mutex.lock().held(), ["written"]);
        mutex.lock().held().push("after");
        mutex.get_mut().held().push("by get_mut");
        assert!(mutex.is_poisoned(), "held, not cleared");
        assert_eq!(
            mutex.into_inner().held(),
            ["written", "after", "by get_mut"]
        );
    }

    #[test]
    fn a_poisoned_rwlock_is_read_and_written() {
        let rwlock = RwLock::new(0);
        panics_holding(|| {
            let mut guard = rwlock.write().held();
            *guard = 7;
            guard
        });
        assert!(rwlock.is_poisoned());
        assert_eq!(*rwlock.read().held(), 7);
        *rwlock.write().held() += 1;
        assert_eq!(*rwlock.read().held(), 8);
    }

    /// A wait on a condition variable over a poisoned mutex gives its guard back.
    #[test]
    fn a_wait_over_a_poisoned_mutex_gives_its_guard() {
        let mutex = Mutex::new(false);
        panics_holding(|| {
            let mut guard = mutex.lock().held();
            *guard = true;
            guard
        });
        let condvar = Condvar::new();
        let (guard, _) = condvar
            .wait_timeout(mutex.lock().held(), Duration::ZERO)
            .held();
        assert!(*guard);
    }
}
