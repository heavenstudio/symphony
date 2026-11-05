//! The leftovers the one server looks for: Symphony processes whose parent is init, which nothing
//! ends on its own (`docs_server_test.py` runs from before 371b52e6 left 1,144 of them, 29.8 GB, on
//! 2026-10-07). They are counted and shown, never killed: that is the owner's call.

use std::ffi::OsStr;
use std::path::Path;
use symphony_board::sys::{Command, Exit, Stdio};

/// The pid `home`'s record names, if it names one.
pub fn recorded(home: &Path) -> Option<i64> {
    let text = std::fs::read_to_string(home.join(crate::home::ADDRESS)).ok()?;
    let running: crate::home::Running = serde_json::from_str(&text).ok()?;
    Some(running.pid)
}

/// How many Symphony processes in `ps -axww -o pid=,ppid=,command=` have init as their parent, whose
/// starter is gone: a `docs_server` app (or a test's stand-in for it), and a `symphony-server serve`
/// other than `own`, the server asking, and other than the one server its `--home`'s record names
/// (`recorded`), both detached on purpose. A leftover server's own app has that server as its parent,
/// so it is not seen: the server is the one counted.
pub fn count(ps: &str, own: i64, recorded: impl Fn(&Path) -> Option<i64>) -> usize {
    ps.lines()
        .filter(|line| {
            let Some((pid, rest)) = line.trim().split_once(char::is_whitespace) else {
                return false;
            };
            let Some((parent, command)) = rest.trim().split_once(char::is_whitespace) else {
                return false;
            };
            let Ok(pid) = pid.parse::<i64>() else {
                return false;
            };
            if parent != "1" {
                return false;
            }
            let Some(args) = crate::sys::server_command(command.trim()) else {
                return line.contains("docs_server");
            };
            let home = args.strip_prefix("--home ").and_then(|rest| {
                rest.split_once(" --root ")
                    .or_else(|| rest.split_once(" --app "))
                    .map(|(home, _)| home)
            });
            pid != own && home.is_none_or(|home| recorded(Path::new(home)) != Some(pid))
        })
        .count()
}

/// The count, from this machine's process table now; `None` when `ps` cannot be read.
/// `ps` starts without a fork ([`Command`]), so it holds no descriptor of the server's but its
/// streams, not even a connection accepted a moment before and not yet close-on-exec. Cancelling
/// the scan kills its owned child, which its [`Exit`]'s thread reaps without holding server
/// shutdown. The table is written to a file ([`crate::sys::capture`]), never a pipe: a child
/// another thread spawns meanwhile could keep a pipe's writer and hold the round open for its
/// whole life.
pub async fn scan() -> Option<usize> {
    scan_with(OsStr::new("ps")).await
}

/// [`scan`] with `ps` as `program` names it (a stand-in, in tests).
async fn scan_with(program: &OsStr) -> Option<usize> {
    let table = crate::sys::capture().ok()?;
    let ps = Command::new(program)
        .args(["-axww", "-o", "pid=,ppid=,command="])
        .stdin(Stdio::null())
        .stdout(table.try_clone().ok()?)
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let status = Exit::of(ps).ok()?.await.ok()?;
    let table = crate::sys::captured(table).ok()?;
    status.success().then(|| {
        count(
            &String::from_utf8_lossy(&table),
            i64::from(std::process::id()),
            recorded,
        )
    })
}

/// The line every page shows while there are any.
pub fn banner(count: usize) -> String {
    let (noun, verb, them) = if count == 1 {
        ("process", "is", "it")
    } else {
        ("processes", "are", "them")
    };
    format!(
        "<div class=\"code-banner\" role=\"status\"><p><strong>{count} Symphony {noun} from test runs \
         {verb} running with no parent</strong>: nothing ends {them} on its own, and the Symphony kills \
         none. <code>ps -axo pid,ppid,command | grep -e docs_server -e 'symphony-server serve'</code> lists \
         {them}, with parent 1; stop {them} by pid.</p></div>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // An app is known by `docs_server` on its command line, whatever interpreter runs it (`interp` here).
    const PS: &str = "  101     1 /usr/bin/interp -B tools/docs_server --root /tmp/x --port 5 --host 127.0.0.1\n\
                      \x20 102     1 sh -c t=$1; shift; ... sh 100 interp -B tools/docs_server --root /tmp/y\n\
                      \x20 103   555 /usr/bin/interp -B tools/docs_server --root /repo --port 6\n\
                      \x20 104     1 /Users/o/.git/sf-symphony/bin/symphony-server serve --home /x --app interp tools/docs_server\n\
                      \x20 105     1 /private/tmp/t/deps/symphony-server serve --home /tmp/y/sf-symphony --app stub\n\
                      \x20 106   555 /private/tmp/t/deps/symphony-server serve --root /tmp/z --port 7 --app stub\n\
                      \x20 107     1 /Users/o/.git/sf-symphony/bin/symphony-server --stop\n\
                      \x20 108     1 /usr/sbin/cron\n\
                      garbage line\n";

    /// No home's record names any server.
    fn unrecorded(_: &Path) -> Option<i64> {
        None
    }

    #[test]
    fn a_docs_server_or_a_symphony_server_whose_parent_is_init_is_counted_but_the_one_server_itself()
     {
        // 101 and 102 are apps under init; 105 is a test's server left under init, whose own app has it as
        // its parent and so is not seen; 104 is the one server, which asks.
        assert_eq!(count(PS, 104, unrecorded), 3);
    }

    #[test]
    fn a_server_its_home_does_not_record_counts_whoever_asks() {
        assert_eq!(count(PS, 999, unrecorded), 4);
        let another = |home: &Path| (home == Path::new("/x")).then_some(4242);
        assert_eq!(count(PS, 999, another), 4);
    }

    #[test]
    fn the_one_server_its_home_records_is_no_leftover_to_a_server_of_its_own_port() {
        // 106 asks, a server of its own port; 104 is the machine's one server, named by /x/server.json.
        let one = |home: &Path| (home == Path::new("/x")).then_some(104);
        assert_eq!(count(PS, 106, one), 3);
    }

    #[test]
    fn nothing_is_counted_in_an_empty_table() {
        assert_eq!(count("", 1, unrecorded), 0);
    }

    #[test]
    fn spaces_in_the_executable_and_home_paths_do_not_hide_a_recorded_server() {
        let ps = "101 1 /repo with spaces/.git/sf-symphony/bin/symphony-server serve --home /repo with spaces/.git/sf-symphony --root /repo with spaces --port 8765 --app stub\n\
                  102 1 /repo with spaces/.git/sf-symphony/bin/symphony-server serve --root /own root --port 9000 --app stub\n";
        let recorded = |home: &Path| {
            assert_eq!(home, Path::new("/repo with spaces/.git/sf-symphony"));
            Some(101)
        };
        assert_eq!(count(ps, 999, recorded), 1);
        assert_eq!(count(ps, 999, unrecorded), 2);
    }

    #[test]
    fn a_homes_record_is_read_for_its_pid() {
        let home = std::env::temp_dir().join(format!("orphans-record-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(recorded(&home), None);
        std::fs::write(home.join("server.json"), "{not a record").unwrap();
        assert_eq!(recorded(&home), None);
        std::fs::write(
            home.join("server.json"),
            r#"{"port":5,"pid":104,"root":"/r"}"#,
        )
        .unwrap();
        assert_eq!(recorded(&home), Some(104));
        std::fs::remove_dir_all(&home).unwrap();
    }

    /// A `ps` stand-in, `sh` running `body`, in a folder of its own removed however the test ends;
    /// `$dir` in the body is that folder.
    struct StandIn(std::path::PathBuf);

    impl StandIn {
        fn new(name: &str, body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let dir = std::env::temp_dir().join(format!("orphans-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let dir = dir.canonicalize().unwrap();
            let program = dir.join("ps");
            std::fs::write(&program, format!("#!/bin/sh\ndir=\"${{0%/*}}\"\n{body}\n")).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self(dir)
        }

        fn program(&self) -> std::path::PathBuf {
            self.0.join("ps")
        }

        fn said(&self, name: &str) -> String {
            std::fs::read_to_string(self.0.join(name)).unwrap_or_default()
        }
    }

    impl Drop for StandIn {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What a stand-in writes of how it was started: its arguments, directory and environment
    /// to `said`, and where its streams go to `streams`.
    const RECORD: &str = "{ printf '%s\\n' \"$@\"; printf 'cwd=%s\\n' \"$(pwd -P)\"; /usr/bin/env | /usr/bin/sort; } > \"$dir/said\"\n\
                          { test /dev/fd/0 -ef /dev/null && printf 'stdin null\\n'; \
                          test /dev/fd/1 -ef /dev/null || printf 'stdout kept\\n'; \
                          test /dev/fd/2 -ef /dev/null && printf 'stderr null\\n'; } > \"$dir/streams\"\n";

    #[tokio::test]
    async fn the_census_runs_ps_with_its_arguments_environment_and_streams_and_counts_its_table() {
        let stand_in = StandIn::new(
            "contract",
            &format!(
                "{RECORD}printf 'unread\\n' >&2\n\
                 printf '101 1 /usr/bin/interp -B tools/docs_server --root /gone\\n'\n\
                 printf '102 555 /usr/bin/interp -B tools/docs_server --root /kept\\n'"
            ),
        );
        // What std's spawn, as tokio's was, hands the same stand-in: this process's environment
        // and directory, unchanged.
        let theirs = {
            use symphony_board::sys::Spawning as _;
            let ran = std::process::Command::new(stand_in.program())
                .args(["-axww", "-o", "pid=,ppid=,command="])
                .output_locked()
                .unwrap();
            assert!(ran.status.success());
            stand_in.said("said")
        };
        assert_eq!(scan_with(stand_in.program().as_os_str()).await, Some(1));
        let said = stand_in.said("said");
        assert!(
            said.starts_with("-axww\n-o\npid=,ppid=,command=\ncwd="),
            "{said}"
        );
        assert_eq!(
            said, theirs,
            "the census changed the environment or directory"
        );
        assert_eq!(
            stand_in.said("streams"),
            "stdin null\nstdout kept\nstderr null\n"
        );
    }

    #[tokio::test]
    async fn a_census_whose_ps_cannot_start_fails_or_is_killed_counts_nothing() {
        let missing = StandIn::new("missing", "exit 0");
        assert_eq!(scan_with(missing.0.join("absent").as_os_str()).await, None);
        let rows = "printf '101 1 /usr/bin/interp -B tools/docs_server --root /gone\\n'";
        let failed = StandIn::new("failed", &format!("{rows}\nexit 1"));
        assert_eq!(scan_with(failed.program().as_os_str()).await, None);
        let killed = StandIn::new("killed", &format!("{rows}\nkill -KILL $$"));
        assert_eq!(scan_with(killed.program().as_os_str()).await, None);
        // An empty table read whole is a count, of none.
        let empty = StandIn::new("empty", "exit 0");
        assert_eq!(scan_with(empty.program().as_os_str()).await, Some(0));
    }

    /// The census has no deadline of its own: a `ps` that never answers holds its scan until the
    /// scan is dropped, which kills it, and it is still reaped.
    #[tokio::test]
    async fn a_hung_census_ps_is_killed_and_reaped_when_its_scan_is_dropped() {
        let stand_in = StandIn::new(
            "hung",
            "printf '%s' $$ > \"$dir/pid.partial\"; /bin/mv \"$dir/pid.partial\" \"$dir/pid\"\nexec /bin/sleep 60",
        );
        let program = stand_in.program();
        let scan = tokio::spawn(async move { scan_with(program.as_os_str()).await });
        until("the stand-in never started", || {
            !stand_in.said("pid").is_empty()
        })
        .await;
        let pid: i64 = stand_in.said("pid").parse().unwrap();
        assert!(crate::sys::pid_alive(pid));
        assert!(!scan.is_finished(), "a hung ps answered");
        scan.abort();
        assert!(scan.await.unwrap_err().is_cancelled());
        // A zombie is still alive to kill(0): only a reaped child is gone.
        until("the dropped census's ps was never reaped", || {
            !crate::sys::pid_alive(pid)
        })
        .await;
    }

    /// Waits for `done`, failing with `what` after a bound only a condition that never holds
    /// reaches, letting the runtime's other tasks run meanwhile.
    async fn until(what: &str, done: impl Fn() -> bool) {
        let began = std::time::Instant::now();
        while !done() {
            assert!(
                began.elapsed() < std::time::Duration::from_secs(30),
                "{what}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// The census's `ps` keeps none of the server's descriptors that are not close-on-exec, as a
    /// connection is for a moment after its accept. This test reruns itself with such a
    /// descriptor handed to it; a std child, as tokio's spawn made, keeps it.
    #[tokio::test]
    async fn the_census_ps_holds_no_descriptor_the_server_left_inheritable() {
        use std::os::fd::{AsRawFd, RawFd};
        const INHERITED: &str = "SF_CENSUS_INHERITED_FD";
        let Some(fd) = std::env::var_os(INHERITED) else {
            let null = std::fs::File::open("/dev/null").unwrap();
            let done = symphony_board::sys::output(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "orphans::tests::the_census_ps_holds_no_descriptor_the_server_left_inheritable",
                        "--nocapture",
                    ])
                    .env(INHERITED, null.as_raw_fd().to_string())
                    // Handed under the same number, and not close-on-exec in the rerun.
                    .pass_fd(null.as_raw_fd()),
            )
            .unwrap();
            let said = String::from_utf8_lossy(&done.stdout);
            assert!(
                done.status.success() && said.contains("1 passed"),
                "{said}\n{}",
                String::from_utf8_lossy(&done.stderr)
            );
            return;
        };
        let fd: RawFd = fd.to_str().unwrap().parse().unwrap();
        let stand_in = StandIn::new(
            "inherited",
            &format!("if test -e /dev/fd/{fd}; then printf held > \"$dir/held\"; fi"),
        );
        {
            use symphony_board::sys::Spawning as _;
            let ran = std::process::Command::new(stand_in.program())
                .status_locked()
                .unwrap();
            assert!(ran.success());
        }
        assert_eq!(
            stand_in.said("held"),
            "held",
            "the descriptor is not inheritable"
        );
        std::fs::remove_file(stand_in.0.join("held")).unwrap();
        assert_eq!(scan_with(stand_in.program().as_os_str()).await, Some(0));
        assert_eq!(
            stand_in.said("held"),
            "",
            "the census's ps held the descriptor"
        );
    }

    #[test]
    fn the_banner_names_the_count_and_says_nothing_is_killed() {
        let said = banner(3);
        assert!(said.contains("3 "), "{said}");
        assert!(said.contains("class=\"code-banner\""), "{said}");
        assert!(banner(1).contains("1 Symphony process "), "{}", banner(1));
        assert!(said.contains("symphony-server serve"), "{said}");
    }
}
