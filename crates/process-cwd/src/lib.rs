//! Working-directory scans shared by the board's worktree sweep and the cargo cache sweep.
//! Path-only scans keep trees when unreadable or empty. PID-aware scans distinguish readable
//! presence from completeness, so partial listings remain useful without proving absence.
//! Their `lsof` starts without a fork and is read through close-on-exec pipes ([`spawn`]), the
//! helper the board's children share, since this leaf is the one crate both sides link. A caller
//! that chooses lsof's environment hands the scan its own command ([`process_cwds_with`],
//! [`occupied_with`]). An async caller awaits such a child's end, killed if the wait is dropped
//! first, through [`exit`]. A child starts without its starter's session and marks through
//! [`session`], the lists the board's children and a plugin's process share.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod close_on_fork;
pub mod exit;
mod pid;
pub mod session;
pub mod spawn;

pub use close_on_fork::close_on_fork;
pub use pid::PidCwds;
pub use spawn::Command;

/// How long lsof may take to list every process's working directory.
const LSOF_PATIENCE: Duration = Duration::from_secs(60);

/// The working directory of every process `lsof` can see; none when it cannot be asked or lists
/// nothing (this process alone has one: none at all is a listing that could not be read).
pub fn process_cwds() -> Option<Vec<PathBuf>> {
    process_cwds_with(Command::new("lsof"))
}

/// [`process_cwds`] through `lsof`, a command for lsof the caller built, in the environment it
/// chose (the board's starts without its starter's session); this adds the arguments.
pub fn process_cwds_with(lsof: Command) -> Option<Vec<PathBuf>> {
    process_cwds_within(lsof, LSOF_PATIENCE)
}

/// [`process_cwds_with`] given `patience` (a test's backstop, which only a stall reaches).
fn process_cwds_within(mut lsof: Command, patience: Duration) -> Option<Vec<PathBuf>> {
    lsof.args(["-Fn", "-d", "cwd"]);
    cwds_of(&mut lsof, patience)
}

/// [`process_cwds_with`], each working directory with the pid of the process working there, so a
/// caller can ask when that process started. Readable rows survive a failed or partial listing;
/// only a complete listing can prove a process absent. None means the command could not finish, or
/// reading its completed output did not finish before the scan deadline.
pub fn pid_cwds_with(lsof: Command) -> Option<PidCwds> {
    pid_cwds_within(lsof, LSOF_PATIENCE)
}

/// [`pid_cwds_with`] given `patience` (a test's backstop, which only a stall reaches).
fn pid_cwds_within(mut lsof: Command, patience: Duration) -> Option<PidCwds> {
    let deadline = Instant::now() + patience;
    // Request f explicitly: it is not implicit in every lsof version's field output.
    // Device/inode disambiguate literal caret names from lsof's control-byte display.
    lsof.args(["-FpfnDi", "-d", "cwd"]);
    let done = within(
        &mut lsof,
        deadline.saturating_duration_since(Instant::now()),
    )?;
    pid::read(done, deadline)
}

/// Whether any process works in `tree` or a directory below it. The final removal check selects
/// directories by inode (`+D`), never by lsof's escaped display names. An unreadable or timed-out
/// search is unknown; a clean exit 1 with no matches is lsof's normal empty search result.
pub fn occupied(tree: &Path) -> Option<bool> {
    occupied_with(Command::new("lsof"), tree)
}

/// [`occupied`] through `lsof`, a command for lsof the caller built, as [`process_cwds_with`] takes
/// it; this adds the arguments and `tree`.
pub fn occupied_with(lsof: Command, tree: &Path) -> Option<bool> {
    occupied_within(lsof, tree, LSOF_PATIENCE)
}

/// [`occupied_with`] given `patience` (a test's backstop, which only a stall reaches).
fn occupied_within(mut lsof: Command, tree: &Path, patience: Duration) -> Option<bool> {
    lsof.args(["-a", "-d", "cwd", "-Fp", "-x", "f", "+D"])
        .arg(tree);
    occupancy_of(&mut lsof, patience)
}

fn occupancy_of(command: &mut Command, patience: Duration) -> Option<bool> {
    let done = within(command, patience)?;
    if done.stdout.split(|b| *b == b'\n').any(|line| {
        line.strip_prefix(b"p")
            .is_some_and(|pid| !pid.is_empty() && pid.iter().all(u8::is_ascii_digit))
    }) {
        return Some(true);
    }
    (done.stdout.is_empty() && done.stderr.is_empty() && matches!(done.status.code(), Some(0 | 1)))
        .then_some(false)
}

/// What `command`, run as `lsof -Fn -d cwd` would be, lists; whatever its exit code (`lsof` exits 1
/// when it could not read some process), killed if it takes longer than `patience`
/// (`LSOF_PATIENCE` outside tests).
fn cwds_of(command: &mut Command, patience: Duration) -> Option<Vec<PathBuf>> {
    let done = within(command, patience)?;
    parse_cwds(&done.stdout)
}

/// Pids from a caller-configured `lsof -t -- FILE`, bounded so diagnosing a leaked lock cannot
/// stall its waiter. Partial output is useful even when lsof cannot inspect every process.
pub fn open_file_pids(command: &mut Command) -> Option<Vec<i64>> {
    let done = within(command, Duration::from_secs(2))?;
    parse_pids(&done.stdout)
}

fn parse_pids(stdout: &[u8]) -> Option<Vec<i64>> {
    let mut pids: Vec<i64> = String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .filter(|pid| *pid > 0)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    (!pids.is_empty()).then_some(pids)
}

/// `lsof -Fn` read: each `n` line is a path. A path that is not UTF-8 is read with U+FFFD for what
/// cannot be decoded, so the ones that can still match.
fn parse_cwds(stdout: &[u8]) -> Option<Vec<PathBuf>> {
    let text = String::from_utf8_lossy(stdout);
    let found: Vec<PathBuf> = text
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .map(PathBuf::from)
        .collect();
    (!found.is_empty()).then_some(found)
}

/// `command`'s output, if it starts and ends within `limit`; else it is killed, by its own pid. Read
/// through [`spawn::pipe`]s, so a child another thread spawns meanwhile cannot hold its end.
fn within(command: &mut Command, limit: Duration) -> Option<Output> {
    let child = spawn::spawn_output(command).ok()?;
    let pid = i64::from(child.id());
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(spawn::wait_output(child));
    });
    match receive.recv_timeout(limit) {
        Ok(done) => done.ok(),
        Err(_) => {
            if let Ok(pid) = libc::pid_t::try_from(pid)
                && pid > 0
            {
                // SAFETY: this is the exact child started above, still owned by the waiting thread.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The patience a scan here is given: lsof's own, as a backstop only a stall reaches (RULES.md
    /// `tests/no-test-fails-on-load`), so a loaded machine's slow lsof is never a scan that failed.
    const PATIENT: Duration = test_time::backstop(LSOF_PATIENCE);

    #[test]
    fn holder_pids_are_sorted_unique_positive_and_ignore_unreadable_lines() {
        for (bytes, expected) in [
            (b"42\n7\n42\n0\n-1\nbad\n".as_slice(), Some(vec![7, 42])),
            (b" 17\r\n3\n17\n\xff\n".as_slice(), Some(vec![3, 17])),
            (b"".as_slice(), None),
            (b"0\n-1\nbad\n9223372036854775808\n".as_slice(), None),
        ] {
            assert_eq!(parse_pids(bytes), expected, "{bytes:?}");
        }
    }

    #[test]
    fn lsof_output_nobody_could_have_given_is_unreadable() {
        assert_eq!(parse_cwds(b""), None);
        assert_eq!(parse_cwds(b"p1\nfcwd\n"), None);
        assert_eq!(
            parse_cwds(b"p1\nn/work/a\n"),
            Some(vec![PathBuf::from("/work/a")])
        );
        assert_eq!(cwds_of(&mut Command::new("/no/such/lsof"), PATIENT), None);
        for code in ["0", "1", "2"] {
            let mut empty = Command::new("sh");
            empty.args(["-c", &format!("exit {code}")]);
            assert_eq!(cwds_of(&mut empty, PATIENT), None, "{code}");
        }
        let mut failing = Command::new("sh");
        failing.args(["-c", "printf 'p1\\nn/work/a\\n'; exit 1"]);
        assert_eq!(
            cwds_of(&mut failing, PATIENT),
            Some(vec![PathBuf::from("/work/a")])
        );
    }

    #[test]
    fn each_cwd_is_read_with_the_pid_listed_before_it() {
        assert_eq!(
            pid_cwds_within(
                stand_in("printf 'p5\\nfcwd\\nn/args/%s\\n' \"$*\""),
                PATIENT
            ),
            Some(PidCwds {
                listed: vec![(5, PathBuf::from("/args/-FpfnDi -d cwd"))],
                complete: true,
            })
        );
        assert_eq!(pid_cwds_with(Command::new("/no/such/lsof")), None);
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        let mine = i64::from(std::process::id());
        let seen = pid_cwds_within(Command::new("lsof"), PATIENT).unwrap();
        assert!(
            seen.listed
                .iter()
                .any(|(pid, cwd)| *pid == mine && cwd.canonicalize().ok().as_ref() == Some(&here)),
            "here={here:?}, mine={mine}, seen={seen:?}"
        );
    }

    #[test]
    fn pid_listing_retains_positive_rows_and_separates_authoritative_absence() {
        struct Listing {
            script: &'static str,
            has_rows: bool,
            complete: bool,
        }
        for Listing {
            script,
            has_rows,
            complete,
        } in [
            Listing {
                script: "printf 'p5\\nfcwd\\nn/work/a\\n'",
                has_rows: true,
                complete: true,
            },
            Listing {
                script: "printf 'p5\\nfcwd\\nn/work/a\\n'; exit 1",
                has_rows: true,
                complete: false,
            },
            Listing {
                script: "printf 'p5\\nfcwd\\nn/work/a\\n'; printf 'Permission denied' >&2",
                has_rows: true,
                complete: false,
            },
            Listing {
                script: "exit 0",
                has_rows: false,
                complete: true,
            },
            Listing {
                script: "exit 1",
                has_rows: false,
                complete: false,
            },
            Listing {
                script: "exit 2",
                has_rows: false,
                complete: false,
            },
            Listing {
                script: "printf 'Permission denied' >&2",
                has_rows: false,
                complete: false,
            },
        ] {
            let scan = pid_cwds_within(stand_in(script), PATIENT).unwrap();
            let listed = if has_rows {
                vec![(5, PathBuf::from("/work/a"))]
            } else {
                vec![]
            };
            assert_eq!(scan, PidCwds { listed, complete }, "{script}");
        }
    }

    #[test]
    fn a_cwd_that_is_not_utf8_is_read_and_the_others_still_match() {
        let mut stub = Command::new("printf");
        stub.arg("p1\\nn/work/a\\np2\\nn/mnt/\\377x\\n");
        assert_eq!(
            cwds_of(&mut stub, PATIENT),
            Some(vec![
                PathBuf::from("/work/a"),
                PathBuf::from("/mnt/\u{fffd}x")
            ])
        );
    }

    #[test]
    fn the_real_cwd_scan_sees_this_process() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        let seen: Vec<PathBuf> = process_cwds_within(Command::new("lsof"), PATIENT)
            .unwrap()
            .into_iter()
            .map(|p| p.canonicalize().unwrap_or(p))
            .collect();
        assert!(seen.contains(&here), "{}", here.display());
    }

    /// A stand-in lsof run by `sh`: `script` sees lsof's arguments as `$@`.
    fn stand_in(script: &str) -> Command {
        let mut lsof = Command::new("/bin/sh");
        lsof.args(["-c", script, "lsof"]);
        lsof
    }

    /// The scans run the caller's command, in the environment the caller gave it, with lsof's
    /// arguments added: what it lists is the answer, and nothing of this process's own lsof.
    #[test]
    fn a_scan_runs_the_lsof_its_caller_built() {
        let mut listing =
            stand_in("printf 'p1\\nn%s|%s\\n' \"$*\" \"${SYMPHONY_CWD_GIVEN-unset}\"");
        listing.env("SYMPHONY_CWD_GIVEN", "given");
        assert_eq!(
            process_cwds_within(listing, PATIENT),
            Some(vec![PathBuf::from("-Fn -d cwd|given")])
        );
        assert_eq!(process_cwds_with(Command::new("/no/such/lsof")), None);
        let search = "[ \"$*|${SYMPHONY_CWD_GIVEN-unset}\" = '-a -d cwd -Fp -x f +D /work/a b|given' ] \
                      && printf 'p7\\n'; exit 1";
        let mut given = stand_in(search);
        given.env("SYMPHONY_CWD_GIVEN", "given");
        assert_eq!(
            occupied_within(given, Path::new("/work/a b"), PATIENT),
            Some(true)
        );
        let mut elsewhere = stand_in(search);
        elsewhere.env("SYMPHONY_CWD_GIVEN", "given");
        assert_eq!(
            occupied_within(elsewhere, Path::new("/work/c"), PATIENT),
            Some(false)
        );
        assert_eq!(
            occupied_within(stand_in(search), Path::new("/work/a b"), PATIENT),
            Some(false),
            "a command without what its caller set"
        );
        assert_eq!(
            occupied_with(Command::new("/no/such/lsof"), Path::new("/work")),
            None
        );
    }

    #[test]
    fn a_command_that_exceeds_its_deadline_has_no_scan_result() {
        let mut sleeping = Command::new("sleep");
        sleeping.arg("60");
        assert!(within(&mut sleeping, Duration::ZERO).is_none());
    }

    #[test]
    fn a_scoped_search_distinguishes_empty_matches_from_failure() {
        for (script, expected) in [
            ("exit 1", Some(false)),
            ("exit 0", Some(false)),
            ("printf 'p123\\n'; exit 1", Some(true)),
            ("printf 'warning' >&2; exit 1", None),
            ("printf 'garbled'; exit 0", None),
            ("exit 2", None),
        ] {
            assert_eq!(
                occupancy_of(Command::new("sh").args(["-c", script]), PATIENT),
                expected,
                "{script}"
            );
        }
        assert_eq!(
            occupancy_of(&mut Command::new("/no/such/lsof"), PATIENT),
            None
        );
    }

    #[test]
    fn scoped_search_sees_nested_cwds_with_names_lsof_escapes() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let root = std::env::temp_dir().join(format!("symphony-cwd-scoped-{}", std::process::id()));
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        for name in ["tab\tname", "café", "literal\\t^A"] {
            let dir = root.join(name).join("inner");
            std::fs::create_dir_all(&dir).unwrap();
            // cat reads its input, which `child` holds open: it works there until it is killed.
            let child = Child({
                let _spawning = spawn::spawning();
                std::process::Command::new("cat")
                    .stdin(std::process::Stdio::piped())
                    .current_dir(&dir)
                    .spawn()
                    .unwrap()
            });
            let occupied = |tree: &Path| occupied_within(Command::new("lsof"), tree, PATIENT);
            assert_eq!(occupied(dir.parent().unwrap()), Some(true), "{name}");
            assert_eq!(occupied(&empty), Some(false));
            drop(child);
        }
        assert_eq!(
            occupied_within(Command::new("lsof"), &root.join("missing"), PATIENT),
            None
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
