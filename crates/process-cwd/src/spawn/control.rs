//! Nonblocking pipes and scoped interruption for a command-line child's polling owner.
//! Signal handlers only set a flag; the caller stops and reaps its child before dropping the scope.

use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

static LISTENING: Mutex<()> = Mutex::new(());
/// The first signal the scope's handler took, else 0.
static INTERRUPTED: AtomicI32 = AtomicI32::new(0);
const SIGNALS: [libc::c_int; 3] = [libc::SIGHUP, libc::SIGINT, libc::SIGTERM];

extern "C" fn interrupted(signal: libc::c_int) {
    let _ = INTERRUPTED.compare_exchange(0, signal, Ordering::Relaxed, Ordering::Relaxed);
}

/// A single command-line owner's interruption scope. Default HUP, INT and TERM dispositions set
/// a cancellation flag; ignored signals and existing handlers stay as they are. Only one scope
/// may be active in a process. The owner must poll [`Self::received`] through child startup, pipe
/// backpressure and waiting, and stop and reap its child before this restores the dispositions.
pub struct Interrupts {
    taken: Vec<(libc::c_int, libc::sigaction)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Interrupts {
    /// Install the polling scope before starting the owned child. Failure starts no scope.
    pub fn listen() -> io::Result<Self> {
        let lock = LISTENING
            .try_lock()
            .map_err(|_| io::Error::other("an interruption scope is already active or poisoned"))?;
        let mut scope = Self {
            taken: Vec::new(),
            _lock: lock,
        };
        INTERRUPTED.store(0, Ordering::Relaxed);
        for signal in SIGNALS {
            // SAFETY: a query writes only the initialized stack action; signal is a valid constant.
            let mut before: libc::sigaction = unsafe { std::mem::zeroed() };
            // SAFETY: before is writable and no replacement is requested.
            if unsafe { libc::sigaction(signal, std::ptr::null(), &mut before) } == -1 {
                return Err(io::Error::last_os_error());
            }
            if before.sa_sigaction != libc::SIG_DFL {
                continue;
            }
            // SAFETY: the initialized action is populated before it is passed to sigaction.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = interrupted as extern "C" fn(libc::c_int) as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            // SAFETY: action's mask is writable; the handler touches only a lock-free atomic flag.
            unsafe { libc::sigemptyset(&mut action.sa_mask) };
            // SAFETY: the complete action and valid signal outlive this syscall.
            if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == -1 {
                return Err(io::Error::last_os_error());
            }
            scope.taken.push((signal, before));
        }
        Ok(scope)
    }

    /// Whether a handled interruption arrived, including while the child was being started.
    pub fn received(&self) -> bool {
        self.signal().is_some()
    }

    /// The first handled signal that arrived, so the owner can end by it once its child is reaped.
    pub fn signal(&self) -> Option<libc::c_int> {
        Some(INTERRUPTED.load(Ordering::Relaxed)).filter(|signal| *signal != 0)
    }
}

impl Drop for Interrupts {
    fn drop(&mut self) {
        for (signal, action) in &self.taken {
            // SAFETY: these are the exact dispositions this exclusive scope saved for each signal.
            unsafe { libc::sigaction(*signal, action, std::ptr::null_mut()) };
        }
    }
}

/// `fd` made to read and write without blocking, or to block again.
pub fn set_nonblocking(fd: &impl AsFd, nonblocking: bool) -> io::Result<()> {
    let fd = fd.as_fd().as_raw_fd();
    // SAFETY: F_GETFL reads the status flags of a descriptor the borrow keeps open.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    let flags = if nonblocking {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };
    // SAFETY: F_SETFL sets the status flags of that same open descriptor, from an int.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn existing_handler(_: libc::c_int) {}

    #[test]
    fn interruptions_are_recorded_exclusively_and_restore_prior_dispositions() {
        const CASE: &str = "SYMPHONY_INTERRUPT_TEST";
        if let Ok(case) = std::env::var(CASE) {
            let signal: i32 = case.parse().unwrap();
            // SAFETY: the valid signal is set to a known disposition in this isolated test process.
            let before = unsafe { libc::signal(signal, libc::SIG_DFL) };
            let scope = Interrupts::listen().unwrap();
            assert!(!scope.received(), "an earlier scope leaked cancellation");
            assert!(
                Interrupts::listen().is_err(),
                "overlapping scopes took the same handlers"
            );
            // SAFETY: this isolated process has installed its handler for this valid signal.
            let raised = unsafe { libc::raise(signal) };
            assert_eq!(raised, 0);
            assert!(scope.received(), "signal {signal} was not recorded");
            assert_eq!(scope.signal(), Some(signal), "the signal is named");
            drop(scope);
            // SAFETY: queries the saved disposition and restores the test process's original one.
            let restored = unsafe { libc::signal(signal, before) };
            assert_eq!(restored, libc::SIG_DFL, "signal {signal} was not restored");
            let next = Interrupts::listen().unwrap();
            assert!(!next.received(), "a completed scope leaked cancellation");
            assert_eq!(next.signal(), None);
            drop(next);
            let custom = existing_handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
            for disposition in [libc::SIG_IGN, custom] {
                // SAFETY: installs a valid disposition in this isolated process for a valid signal.
                unsafe { libc::signal(signal, disposition) };
                let scope = Interrupts::listen().unwrap();
                // SAFETY: the ignored or no-op handler installed above handles this valid signal.
                let raised = unsafe { libc::raise(signal) };
                assert_eq!(raised, 0);
                assert!(
                    !scope.received(),
                    "the existing disposition was replaced for {signal}"
                );
                drop(scope);
                // SAFETY: restores the test's original disposition and queries the prior one.
                let preserved = unsafe { libc::signal(signal, before) };
                assert_eq!(
                    preserved, disposition,
                    "signal {signal} disposition changed"
                );
            }
            return;
        }
        for signal in SIGNALS {
            let mut command = super::super::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "spawn::control::tests::interruptions_are_recorded_exclusively_and_restore_prior_dispositions"])
                .env(CASE, signal.to_string());
            let done = super::super::output(&mut command).unwrap();
            assert!(done.status.success(), "signal {signal}: {done:?}");
        }
    }
}
