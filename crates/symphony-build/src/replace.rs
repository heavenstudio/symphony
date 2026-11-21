//! This process made another program, for the two places that end as one: the test-binary adapter
//! (`commands/exec_test.rs`) and a due count's hand-off to the build of its tip (`release.rs`).

use std::ffi::{CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt as _;

/// Replaces this process with the program at the path `program`, run with `rest` and with this
/// process's environment, each variable of `add` set over it, and returns only where that start
/// failed. The program is a path and is never looked for on `PATH`.
///
/// It is `execve` itself and never std's `CommandExt::exec`, which first makes the calling
/// process the child it would become (SIGPIPE back to its default, the thread's signal mask
/// emptied) and, where the start fails, returns with both left so: every later write of the
/// caller to a pipe nobody reads then kills it. Here a failed start changes nothing of its caller,
/// its environment included, as the one started is built here and handed over. The image started
/// inherits this process's dispositions instead, SIGPIPE ignored as Rust's runtime sets it: the
/// same a Rust program, the only image either caller starts, sets for itself as it starts.
pub(crate) fn replace(program: &OsStr, rest: &[String], add: &[(&str, &OsStr)]) -> io::Error {
    let words = std::iter::once(program)
        .chain(rest.iter().map(OsStr::new))
        .map(|word| CString::new(word.as_bytes()))
        .collect::<Result<Vec<CString>, _>>();
    let variables = environment(std::env::vars_os(), add)
        .into_iter()
        .map(CString::new)
        .collect::<Result<Vec<CString>, _>>();
    let (words, variables) = match (words, variables) {
        (Ok(words), Ok(variables)) => (words, variables),
        (Err(nul), _) | (_, Err(nul)) => return io::Error::new(io::ErrorKind::InvalidInput, nul),
    };
    let mut argv: Vec<*const libc::c_char> = words.iter().map(|word| word.as_ptr()).collect();
    argv.push(std::ptr::null());
    let mut envp: Vec<*const libc::c_char> = variables.iter().map(|one| one.as_ptr()).collect();
    envp.push(std::ptr::null());
    // SAFETY: `argv` and `envp` are null-terminated arrays of pointers to NUL-terminated strings
    // that `words` and `variables` keep alive across the call, and the first of `argv` is the
    // program. execve returns only where it replaced nothing.
    unsafe {
        libc::execve(argv[0], argv.as_ptr(), envp.as_ptr());
    }
    io::Error::last_os_error()
}

/// The environment `inherited` is with each variable of `add` set: one `NAME=VALUE` each, the
/// inherited ones `add` does not name in their order, then `add`'s.
fn environment(
    inherited: impl Iterator<Item = (OsString, OsString)>,
    add: &[(&str, &OsStr)],
) -> Vec<Vec<u8>> {
    let pair = |name: &OsStr, value: &OsStr| {
        let mut one = name.as_bytes().to_vec();
        one.push(b'=');
        one.extend_from_slice(value.as_bytes());
        one
    };
    inherited
        .filter(|(name, _)| !add.iter().any(|(added, _)| OsStr::new(added) == name))
        .map(|(name, value)| pair(&name, &value))
        .chain(
            add.iter()
                .map(|(name, value)| pair(OsStr::new(name), value)),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    /// What this process does on SIGPIPE: `SIG_IGN` in a Rust program, by its runtime.
    fn on_broken_pipe() -> libc::sighandler_t {
        // SAFETY: sigaction is plain data, for which all zeroes is a value.
        let mut now: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: a null new action changes nothing; the old one is written to `now`.
        let read = unsafe { libc::sigaction(libc::SIGPIPE, std::ptr::null(), &raw mut now) };
        assert_eq!(read, 0);
        now.sa_sigaction
    }

    /// Changes this thread's signal mask as `how` says with SIGUSR2, and answers whether the
    /// mask held it before.
    fn mask_usr2(how: libc::c_int) -> bool {
        // SAFETY: sigset_t is plain data, for which all zeroes is a value.
        let mut one: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        let mut before: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: `one` is a writable set.
        unsafe { libc::sigemptyset(&raw mut one) };
        // SAFETY: `one` is an initialised set and SIGUSR2 a signal.
        unsafe { libc::sigaddset(&raw mut one, libc::SIGUSR2) };
        // SAFETY: both sets are live, and only this thread's mask changes.
        let set = unsafe { libc::pthread_sigmask(how, &raw const one, &raw mut before) };
        assert_eq!(set, 0);
        // SAFETY: `before` was written by the call above.
        unsafe { libc::sigismember(&raw const before, libc::SIGUSR2) == 1 }
    }

    /// A variable no test and no machine sets, which a failed start is given to add.
    const ADDED: &str = "SYMPHONY_BUILD_REPLACE_TEST_ADDED";

    /// The tests of this library share one process under Cargo, and a release run goes on in its
    /// own after a hand-off that failed, so a start that failed must leave its caller as it was:
    /// std's `exec` returned with SIGPIPE at its default, and a later write to a child that had
    /// ended killed the whole process. Every program here is one that cannot start, so a check
    /// that stops refusing fails the start as well and never becomes another program.
    #[test]
    fn a_start_that_fails_leaves_its_callers_signals_and_environment_as_they_were() {
        let tree = Tree::new();
        let add = [(ADDED, OsStr::new("driver"))];
        assert_eq!(on_broken_pipe(), libc::SIG_IGN, "Rust's runtime ignores it");
        assert!(!mask_usr2(libc::SIG_BLOCK), "nothing blocked it before");

        let missing = replace(OsStr::new("/no/such/program"), &["argument".into()], &add);
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        // A name with no folder is a path as well, here one of this folder: PATH is not read.
        let unnamed = replace(OsStr::new("no-such-program-on-any-path"), &[], &[]);
        assert_eq!(unnamed.kind(), io::ErrorKind::NotFound);
        let plain = tree.write("built", "a file nobody may execute");
        let barred = replace(plain.as_os_str(), &[], &add);
        assert_eq!(barred.kind(), io::ErrorKind::PermissionDenied);
        let folder = replace(tree.path("").as_os_str(), &[], &add);
        assert_eq!(folder.kind(), io::ErrorKind::PermissionDenied);

        for nul in [
            replace(OsStr::new("/no/such/program"), &["a\0b".into()], &[]),
            replace(OsStr::new("/no/such\0program"), &[], &[]),
            replace(
                OsStr::new("/no/such/program"),
                &[],
                &[(ADDED, OsStr::new("a\0b"))],
            ),
            replace(
                OsStr::new("/no/such/program"),
                &[],
                &[("A\0B", OsStr::new("value"))],
            ),
        ] {
            assert_eq!(nul.kind(), io::ErrorKind::InvalidInput);
        }

        assert_eq!(on_broken_pipe(), libc::SIG_IGN);
        assert!(mask_usr2(libc::SIG_UNBLOCK), "the mask it blocked is kept");
        assert_eq!(
            std::env::var_os(ADDED),
            None,
            "set for the one started only"
        );
    }

    #[test]
    fn the_one_started_is_given_the_inherited_environment_with_each_added_variable_set_over_it() {
        let inherited = || {
            [("HOME", "/home"), ("DRIVER", "old"), ("EMPTY", "")]
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
        };
        let said = |add: &[(&str, &OsStr)]| -> Vec<String> {
            environment(inherited(), add)
                .into_iter()
                .map(|one| String::from_utf8(one).unwrap())
                .collect()
        };
        assert_eq!(said(&[]), ["HOME=/home", "DRIVER=old", "EMPTY="]);
        assert_eq!(
            said(&[("DRIVER", OsStr::new("/built")), ("NEW", OsStr::new("a=b"))]),
            ["HOME=/home", "EMPTY=", "DRIVER=/built", "NEW=a=b"]
        );
    }
}
