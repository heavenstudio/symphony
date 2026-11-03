//! Working-directory scans shared by the board's worktree sweep and the cargo cache sweep.
//! Unreadable scans keep trees; an empty global listing is unreadable. Partial listings remain useful.
//! Their `lsof` starts without a fork and is read through close-on-exec pipes ([`spawn`]), the
//! helper the board's children share, since this leaf is the one crate both sides link.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::mpsc;
use std::time::Duration;

pub mod spawn;

pub use spawn::Command;

/// How long lsof may take to list every process's working directory.
const LSOF_PATIENCE: Duration = Duration::from_secs(60);

/// The working directory of every process `lsof` can see; none when it cannot be asked or lists
/// nothing (this process alone has one: none at all is a listing that could not be read).
pub fn process_cwds() -> Option<Vec<PathBuf>> {
    let mut lsof = Command::new("lsof");
    lsof.args(["-Fn", "-d", "cwd"]);
    cwds_of(&mut lsof)
}

/// Whether any process works in `tree` or a directory below it. The final removal check selects
/// directories by inode (`+D`), never by lsof's escaped display names. An unreadable or timed-out
/// search is unknown; a clean exit 1 with no matches is lsof's normal empty search result.
pub fn occupied(tree: &Path) -> Option<bool> {
    let mut lsof = Command::new("lsof");
    lsof.args(["-a", "-d", "cwd", "-Fp", "-x", "f", "+D"])
        .arg(tree);
    occupancy_of(&mut lsof)
}

fn occupancy_of(command: &mut Command) -> Option<bool> {
    let done = within(command, LSOF_PATIENCE)?;
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
/// when it could not read some process), killed if it takes longer than `LSOF_PATIENCE`.
fn cwds_of(command: &mut Command) -> Option<Vec<PathBuf>> {
    let done = within(command, LSOF_PATIENCE)?;
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
        assert_eq!(cwds_of(&mut Command::new("/no/such/lsof")), None);
        for code in ["0", "1", "2"] {
            let mut empty = Command::new("sh");
            empty.args(["-c", &format!("exit {code}")]);
            assert_eq!(cwds_of(&mut empty), None, "{code}");
        }
        let mut failing = Command::new("sh");
        failing.args(["-c", "printf 'p1\\nn/work/a\\n'; exit 1"]);
        assert_eq!(cwds_of(&mut failing), Some(vec![PathBuf::from("/work/a")]));
    }

    #[test]
    fn a_cwd_that_is_not_utf8_is_read_and_the_others_still_match() {
        let mut stub = Command::new("printf");
        stub.arg("p1\\nn/work/a\\np2\\nn/mnt/\\377x\\n");
        assert_eq!(
            cwds_of(&mut stub),
            Some(vec![
                PathBuf::from("/work/a"),
                PathBuf::from("/mnt/\u{fffd}x")
            ])
        );
    }

    #[test]
    fn the_real_cwd_scan_sees_this_process() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        let seen: Vec<PathBuf> = process_cwds()
            .unwrap()
            .into_iter()
            .map(|p| p.canonicalize().unwrap_or(p))
            .collect();
        assert!(seen.contains(&here), "{}", here.display());
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
                occupancy_of(Command::new("sh").args(["-c", script])),
                expected,
                "{script}"
            );
        }
        assert_eq!(occupancy_of(&mut Command::new("/no/such/lsof")), None);
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
        let root = std::env::temp_dir().join(format!("sf-cwd-scoped-{}", std::process::id()));
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        for name in ["tab\tname", "café", "literal\\t^A"] {
            let dir = root.join(name).join("inner");
            std::fs::create_dir_all(&dir).unwrap();
            let child = Child(
                std::process::Command::new("sleep")
                    .arg("60")
                    .current_dir(&dir)
                    .spawn()
                    .unwrap(),
            );
            assert_eq!(occupied(dir.parent().unwrap()), Some(true), "{name}");
            assert_eq!(occupied(&empty), Some(false));
            drop(child);
        }
        assert_eq!(occupied(&root.join("missing")), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
