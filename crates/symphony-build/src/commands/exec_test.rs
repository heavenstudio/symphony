//! Cargo's target-runner adapter for the release step's shared library fixtures. Cargo supplies
//! the artifact, cwd, environment (including dynamic-library paths) and libtest arguments. The
//! map made from the one workspace build selects the shared libraries; nextest runs the others.
//! No shell sits between Cargo and the library image, so macOS keeps its dynamic-library paths.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::io::{self, Write};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    binary: String,
    shared: bool,
}

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        let _ = writeln!(
            out,
            "usage: symphony-build exec-test-binary MAP BINARY [ARGS]..."
        );
        return 0;
    }
    let [map, binary, rest @ ..] = args else {
        let _ = writeln!(
            err,
            "usage: symphony-build exec-test-binary MAP BINARY [ARGS]..."
        );
        return 2;
    };
    let selected = std::fs::read(map)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<Vec<Artifact>>(&bytes).map_err(|error| error.to_string())
        });
    match selected {
        Ok(selected) => {
            let mut known = BTreeSet::new();
            for artifact in &selected {
                if !known.insert(&artifact.binary) {
                    let _ = writeln!(
                        err,
                        "exec-test-binary: ambiguous library artifact {} in {map}",
                        artifact.binary
                    );
                    return 2;
                }
            }
            match selected.iter().find(|artifact| artifact.binary == *binary) {
                Some(Artifact { shared: false, .. }) => 0,
                Some(Artifact { shared: true, .. }) => {
                    let error = replace(binary, rest);
                    let _ = writeln!(err, "exec-test-binary: cannot execute {binary}: {error}");
                    2
                }
                None => {
                    let _ = writeln!(
                        err,
                        "exec-test-binary: {binary} is not a library artifact in {map}"
                    );
                    2
                }
            }
        }
        Err(error) => {
            let _ = writeln!(err, "exec-test-binary: cannot read {map}: {error}");
            2
        }
    }
}

/// Replaces this process with `binary` run with `rest`, found on `PATH` as a shell would and
/// given this process's environment, and returns only where that start failed.
///
/// It is `execvp` itself and never std's `CommandExt::exec`, which first makes the calling
/// process the child it would become (SIGPIPE back to its default, the thread's signal mask
/// emptied) and, where the start fails, returns with both left so: every later write of the
/// caller to a pipe nobody reads then kills it. Here a failed start changes nothing of its caller.
/// The image started inherits this process's dispositions instead, SIGPIPE ignored as Rust's
/// runtime sets it: the same a Rust test binary, the only image this adapter starts, sets for
/// itself as it starts.
fn replace(binary: &str, rest: &[String]) -> io::Error {
    let words: Result<Vec<CString>, _> = std::iter::once(binary)
        .chain(rest.iter().map(String::as_str))
        .map(CString::new)
        .collect();
    let words = match words {
        Ok(words) => words,
        Err(nul) => return io::Error::new(io::ErrorKind::InvalidInput, nul),
    };
    let mut argv: Vec<*const libc::c_char> = words.iter().map(|word| word.as_ptr()).collect();
    argv.push(std::ptr::null());
    // SAFETY: `argv` is a null-terminated array of pointers to NUL-terminated strings that
    // `words` keeps alive across the call, and its first is the program. execvp returns only
    // where it replaced nothing.
    unsafe {
        libc::execvp(argv[0], argv.as_ptr());
    }
    io::Error::last_os_error()
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

    /// The tests of this library share one process under Cargo, so a start that failed must leave
    /// its caller as it was: std's `exec` returned with SIGPIPE at its default, and a later test's
    /// write to a child that had ended killed the whole run.
    #[test]
    fn a_start_that_fails_leaves_its_callers_signals_as_they_were() {
        assert_eq!(on_broken_pipe(), libc::SIG_IGN, "Rust's runtime ignores it");
        assert!(!mask_usr2(libc::SIG_BLOCK), "nothing blocked it before");
        let missing = replace("/no/such/program", &["argument".into()]);
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        let unnamed = replace("no-such-program-on-any-path", &[]);
        assert_eq!(unnamed.kind(), io::ErrorKind::NotFound);
        let nul = replace("/bin/echo", &["a\0b".into()]);
        assert_eq!(nul.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(on_broken_pipe(), libc::SIG_IGN);
        assert!(mask_usr2(libc::SIG_UNBLOCK), "the mask it blocked is kept");
    }

    fn run(args: &[String], err: &mut dyn Write) -> i32 {
        main(args, &mut Vec::new(), err)
    }

    #[test]
    fn help_needs_no_selection_file_and_writes_only_stdout() {
        for flag in ["-h", "--help"] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(main(&[flag.into()], &mut out, &mut err), 0);
            assert_eq!(
                out,
                b"usage: symphony-build exec-test-binary MAP BINARY [ARGS]...\n"
            );
            assert!(err.is_empty());
        }
    }

    #[test]
    fn only_an_explicit_known_stateless_library_is_left_to_nextest() {
        let tree = Tree::new();
        let map = tree.write("map.json", r#"[{"binary":"/known stateless","shared":false},{"binary":"/shared missing","shared":true}]"#);
        let map = map.to_str().unwrap().to_string();
        let mut err = Vec::new();
        assert_eq!(run(&[map.clone(), "/known stateless".into()], &mut err), 0);
        assert!(err.is_empty());
        assert_eq!(run(&[map.clone(), "/unknown".into()], &mut err), 2);
        assert!(String::from_utf8_lossy(&err).contains("not a library artifact"));
        err.clear();
        assert_eq!(run(&[map, "/shared missing".into()], &mut err), 2);
        assert!(String::from_utf8_lossy(&err).contains("cannot execute /shared missing"));
    }

    #[test]
    fn missing_malformed_and_unreadable_maps_refuse() {
        let tree = Tree::new();
        for args in [vec![], vec!["one".into()]] {
            let mut err = Vec::new();
            assert_eq!(run(&args, &mut err), 2);
            assert!(String::from_utf8_lossy(&err).starts_with("usage:"));
        }
        for content in ["{", r#"[{"binary":"/known","shared":"yes"}]"#] {
            let map = tree.write("bad.json", content);
            let mut err = Vec::new();
            assert_eq!(
                run(&[map.display().to_string(), "/known".into()], &mut err),
                2
            );
            assert!(String::from_utf8_lossy(&err).contains("cannot read"));
        }
        let mut err = Vec::new();
        assert_eq!(
            run(
                &[tree.path("absent").display().to_string(), "/known".into()],
                &mut err
            ),
            2
        );
    }

    #[test]
    fn duplicate_artifacts_are_ambiguous_even_if_their_selection_agrees() {
        let tree = Tree::new();
        for shared in [true, false] {
            let map = tree.write(
                "duplicates.json",
                &serde_json::json!([
                    {"binary":"/known", "shared":false}, {"binary":"/known", "shared":shared}
                ])
                .to_string(),
            );
            let mut err = Vec::new();
            assert_eq!(
                run(&[map.display().to_string(), "/known".into()], &mut err),
                2
            );
            assert!(String::from_utf8_lossy(&err).contains("ambiguous library artifact"));
        }
    }
}
