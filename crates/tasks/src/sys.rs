//! What health asks of this machine: its host name and whether a process is alive, as
//! `gethostname` and `kill(pid, 0)` answer them; and the end of a
//! process, for the board's server ([`crate::server`]); and the local time a line is stamped with.
//! Also how a child is started and its input and output read ([`Command`], [`output`],
//! [`spawn_output`], [`spawn_piped`], [`pipe`]): without a fork and through pipes no child another
//! thread spawns can hold open, from the leaf the cwd scans share ([`process_cwd::spawn`]), and
//! the lock a std spawn beside them takes ([`spawning`], [`Spawning`]); and a TCP connection no
//! spawn copies ([`connect`]). An async caller awaits a child's end through [`Exit`].
//!
//! Every child the board starts is built here ([`command`], [`command_with`]), without the session
//! of the process that starts it ([`SESSION_VARS`]): an agent that runs a board command from its hc
//! terminal hands it the board handle it acts as and the terminal and core it runs in. A child that
//! kept hc's and ran `hc run` would start a terminal under that one, which ends with it (`hc
//! --skill`), and an hc call would answer for that core and scope alone. No other source of the
//! crate makes a `Command` (this module's tests say so).

use std::collections::BTreeMap;
use std::ffi::{OsStr, c_char, c_int, c_long};
use std::fs::File;

pub use process_cwd::exit::Exit;
pub use process_cwd::spawn::{
    Child, Command, Spawning, Stdio, connect, output, pipe, set_nonblocking, spawn_output,
    spawn_piped, spawning, wait_output,
};

unsafe extern "C" {
    fn gethostname(name: *mut c_char, len: usize) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn tzset();
    fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
}

/// What ties a process to its starter's session: the board handle (`TASKS_SESSION`), and hc's
/// terminal, its socket, the core and the scope.
pub const SESSION_VARS: [&str; 5] = [
    "TASKS_SESSION",
    "HC_TERMINAL_ID",
    "HC_TERMINAL_SOCKET",
    "HC_SESSION",
    "HC_SCOPE",
];

/// `command` keeps none of [`SESSION_VARS`], from this process's environment, from one it was
/// given, or set on it before. A launcher handed a command calls it on every one.
pub fn sessionless(command: &mut Command) -> &mut Command {
    for name in SESSION_VARS {
        command.env_remove(name);
    }
    command
}

/// A command for `program` with this process's environment but [`SESSION_VARS`]. What the caller
/// sets on it after it keeps: a call that must keep one of them sets it there, and says why.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    sessionless(&mut command);
    command
}

/// A command for `program` whose whole environment is `vars` but [`SESSION_VARS`]: a child started
/// in a caller's environment ([`crate::cli::Env`]) rather than this process's. What the caller sets
/// on it after it keeps, as with [`command`].
pub fn command_with(program: impl AsRef<OsStr>, vars: &BTreeMap<String, String>) -> Command {
    let mut command = Command::new(program);
    command.env_clear().envs(vars);
    sessionless(&mut command);
    command
}

impl crate::port::Lsof for Command {
    fn lsof(args: &[&str]) -> Self {
        let mut lsof = command("lsof");
        lsof.args(args);
        lsof
    }
}

/// C's `struct tm`, as macOS and glibc lay it out.
#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

const EPERM: i32 = 1;
const SIGKILL: c_int = 9;
const SIGTERM: c_int = 15;

/// A file lock released explicitly before its descriptor closes. A sibling thread can fork while
/// it is held: closing our copy alone leaves the lock held by that child until its exec.
pub struct FileLock(pub(crate) File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// This machine's host name, or empty if it has none it will say.
pub fn hostname() -> String {
    let mut name = [0 as c_char; 256];
    // SAFETY: the buffer is as long as the length given, and gethostname writes no further.
    if unsafe { gethostname(name.as_mut_ptr(), name.len()) } != 0 {
        return String::new();
    }
    let bytes: Vec<u8> = name
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether process `pid` exists: one this user may not signal still exists.
pub fn pid_alive(pid: i64) -> bool {
    let Ok(pid) = c_int::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false; // 0 and below name process groups, not a process
    }
    // SAFETY: signal 0 sends nothing; kill only checks the process exists and may be signalled.
    let signalled = unsafe { kill(pid, 0) } == 0;
    signalled || std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
}

/// `seconds` as `time.strftime("%Y-%m-%d %H:%M", time.localtime(seconds))` writes it: this machine's
/// time zone, the seconds floored.
pub fn local_minute(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation)] // a time in seconds is far inside i64
    let time = seconds.floor() as i64;
    let mut tm = Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
    };
    // SAFETY: tzset reads TZ; localtime_r writes only the struct it is given, which outlives the call.
    let done = unsafe {
        tzset();
        localtime_r(&raw const time, &raw mut tm)
    };
    if done.is_null() {
        return String::new();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// Ask process `pid` to end (SIGTERM), or make it (SIGKILL, `hard`): whether the signal went. Only
/// ever one process: 0 and below, which name process groups, are refused.
pub fn terminate(pid: i64, hard: bool) -> bool {
    let Ok(pid) = c_int::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    let signal = if hard { SIGKILL } else { SIGTERM };
    // SAFETY: kill reads nothing of ours; a pid above 0 names one process.
    unsafe { kill(pid, signal) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The session this test's re-run is started in, as an agent's hc terminal starts a command:
    /// its board handle, and hc's terminal, socket, core and scope; beside one variable of no
    /// session.
    const STARTED: [(&str, &str); 6] = [
        ("TASKS_SESSION", "fixture-session"),
        ("HC_TERMINAL_ID", "t7"),
        ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
        ("HC_SESSION", "fixture-core"),
        ("HC_SCOPE", "fixture-scope"),
        ("SF_BOARD_KEPT", "kept"),
    ];

    /// What a child holds of [`STARTED`], `|` between, run by `sh`.
    fn saw(command: &mut Command) -> String {
        let said = "printf '%s|%s|%s|%s|%s|%s' \"${TASKS_SESSION-unset}\" \
                    \"${HC_TERMINAL_ID-unset}\" \"${HC_TERMINAL_SOCKET-unset}\" \
                    \"${HC_SESSION-unset}\" \"${HC_SCOPE-unset}\" \"${SF_BOARD_KEPT-unset}\"";
        let done = output(command.args(["-c", said])).unwrap();
        assert!(done.status.success(), "{done:?}");
        String::from_utf8(done.stdout).unwrap()
    }

    #[test]
    fn a_child_runs_without_its_starters_session_but_with_what_its_caller_sets() {
        // Set in this process, the variables would reach every other test's children: a re-run of
        // this test alone is started in them.
        const CASE: &str = "SF_BOARD_CHILDREN_CASE";
        if std::env::var_os(CASE).is_some() {
            let none = "unset|unset|unset|unset|unset";
            let all = "fixture-session|t7|/fixture/hc/t7.sock|fixture-core|fixture-scope|kept";
            assert_eq!(saw(&mut Command::new("/bin/sh")), all, "the starter's own");
            assert_eq!(saw(&mut command("/bin/sh")), format!("{none}|kept"));
            assert_eq!(
                saw(command("/bin/sh").env("TASKS_SESSION", "an-actor")),
                "an-actor|unset|unset|unset|unset|kept",
                "an actor's command keeps the handle it is given"
            );
            // A caller's environment, its session in it, is the child's whole environment but that.
            let mut given: BTreeMap<String, String> = std::env::vars().collect();
            given.insert("SF_BOARD_KEPT".into(), "given".into());
            assert_eq!(
                saw(&mut command_with("/bin/sh", &given)),
                format!("{none}|given")
            );
            given.remove("SF_BOARD_KEPT");
            assert_eq!(
                saw(command_with("/bin/sh", &given).env("HC_TERMINAL_ID", "t9")),
                "unset|t9|unset|unset|unset|unset"
            );
            // A launcher handed a command drops even what its caller set before.
            let mut handed = Command::new("/bin/sh");
            handed.env("HC_SESSION", "other-core");
            assert_eq!(saw(sessionless(&mut handed)), format!("{none}|kept"));
            return;
        }
        let done = output(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sys::tests::a_child_runs_without_its_starters_session_but_with_what_its_caller_sets",
                ])
                .env(CASE, "1")
                .envs(STARTED),
        )
        .unwrap();
        let said = String::from_utf8_lossy(&done.stdout);
        assert!(
            done.status.success() && said.contains("1 passed"),
            "{said}{}",
            String::from_utf8_lossy(&done.stderr)
        );
    }

    /// Every child the board starts is built by [`command`] or [`command_with`]: no other source of
    /// the crate makes a `Command` or clears one's environment, so a new launcher cannot hand its
    /// child the starter's session. Test code starts what it likes: a file of tests, a `tests`
    /// folder, and a `#[cfg(test)]` module, which ends its file. `port.rs` uses std's alone, for a
    /// crate that compiles it without the board; the board's own `lsof` is this module's.
    #[test]
    fn no_source_but_this_module_builds_a_child() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let exempt = [src.join("sys.rs"), src.join("port.rs")];
        let mut found = Vec::new();
        let mut folders = vec![src.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    if name != "tests" {
                        folders.push(path);
                    }
                    continue;
                }
                let tests = name == "tests.rs" || name.ends_with("_tests.rs");
                if !name.ends_with(".rs") || tests || exempt.contains(&path) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (at, line) in production(&text) {
                    if line.contains("Command::new") || line.contains("env_clear") {
                        let file = path.strip_prefix(&src).unwrap().display();
                        found.push(format!("src/{file}:{at}: {}", line.trim()));
                    }
                }
            }
        }
        found.sort();
        assert!(
            found.is_empty(),
            "a child built past sys::command and sys::command_with:\n{}",
            found.join("\n")
        );
    }

    /// The lines of `text` that are no test code nor a comment, numbered from 1: none from a
    /// `#[cfg(test)]` module with a body on, and none of a `#[cfg(test)]` declaration (`mod x;`).
    fn production(text: &str) -> Vec<(usize, &str)> {
        let mut lines = Vec::new();
        let mut gated = false;
        for (at, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed == "#[cfg(test)]" {
                gated = true;
            } else if gated {
                if trimmed.ends_with('{') {
                    break;
                }
                gated = !trimmed.ends_with(';');
            } else if !trimmed.starts_with("//") {
                lines.push((at + 1, line));
            }
        }
        lines
    }

    #[test]
    fn production_code_ends_at_a_test_module_and_skips_a_test_declaration() {
        let text = "use a;\n// Command::new\n#[cfg(test)]\n#[path = \"x.rs\"]\nmod x;\nfn b() {}\n\
                    #[cfg(test)]\nmod tests {\n    fn c() {}\n}\n";
        assert_eq!(production(text), [(1, "use a;"), (6, "fn b() {}")]);
    }

    #[test]
    fn the_host_name_is_the_one_the_system_gives() {
        let said = output(&mut Command::new("hostname")).unwrap();
        assert_eq!(hostname(), String::from_utf8(said.stdout).unwrap().trim());
        assert!(!hostname().is_empty());
    }

    #[test]
    fn this_process_is_alive_and_a_reaped_child_is_not() {
        assert!(pid_alive(i64::from(std::process::id())));
        let mut child = Command::new("true").spawn().unwrap();
        let pid = i64::from(child.id());
        child.wait().unwrap();
        assert!(!pid_alive(pid));
    }

    /// Children killed and reaped however the test ends.
    struct Racers(Vec<std::process::Child>);

    impl Drop for Racers {
        fn drop(&mut self) {
            for racer in &mut self.0 {
                let _ = racer.kill();
                let _ = racer.wait();
            }
        }
    }

    /// The board's long std children, each spawned under the helpers' lock ([`Spawning`]), keep
    /// none of the pipes made beside them: an answer ends with its child, and a piped child's input
    /// refuses a write once it ends, while every racer still lives.
    #[test]
    fn std_spawns_racing_the_helpers_hold_none_of_their_pipes() {
        use std::io::Write as _;
        use std::sync::atomic::{AtomicBool, Ordering};
        let spawned = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let racers = scope.spawn(|| {
                let racers = Racers(
                    (0..32)
                        .map(|_| {
                            std::process::Command::new("sleep")
                                .arg("60")
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn_locked()
                                .unwrap()
                        })
                        .collect(),
                );
                spawned.store(true, Ordering::Relaxed);
                racers
            });
            let mut answers = 0;
            while !spawned.load(Ordering::Relaxed) {
                let mut child = spawn_piped(&mut Command::new("true")).unwrap();
                let mut input = child.stdin.take().unwrap();
                assert!(child.wait().unwrap().success());
                let refused = input.write_all(b"unread").unwrap_err();
                assert_eq!(
                    refused.kind(),
                    std::io::ErrorKind::BrokenPipe,
                    "answer {answers}"
                );
                let done = output(Command::new("/bin/echo").arg("answer")).unwrap();
                assert_eq!(done.stdout, b"answer\n");
                answers += 1;
            }
            let mut racers = racers.join().unwrap();
            // A wait some racer held would have ended only with that racer, 60 s on.
            for racer in &mut racers.0 {
                assert!(racer.try_wait().unwrap().is_none(), "a racer ended first");
            }
            assert!(answers > 0, "no answer was waited for beside the spawns");
        });
    }

    #[test]
    fn a_process_this_user_may_not_signal_is_alive() {
        assert!(pid_alive(1), "launchd or init is pid 1 and belongs to root");
    }

    #[test]
    fn a_local_minute_is_the_one_date_gives() {
        for seconds in [0.0_f64, 1_800_000_000.9, 1_791_362_047.125_689] {
            let whole = format!("{}", seconds.floor());
            let bsd = output(Command::new("date").args(["-r", &whole, "+%Y-%m-%d %H:%M"])).unwrap();
            let date = if bsd.status.success() {
                bsd
            } else {
                output(Command::new("date").args(["-d", &format!("@{whole}"), "+%Y-%m-%d %H:%M"]))
                    .unwrap()
            };
            assert_eq!(
                local_minute(seconds),
                String::from_utf8(date.stdout).unwrap().trim(),
                "{seconds}"
            );
        }
    }

    #[test]
    fn terminate_ends_one_process_softly_or_hard() {
        use std::os::unix::process::ExitStatusExt;
        for (hard, signal) in [(false, 15), (true, 9)] {
            let mut child = Command::new("sleep").arg("30").spawn().unwrap();
            assert!(terminate(i64::from(child.id()), hard));
            assert_eq!(child.wait().unwrap().signal(), Some(signal));
        }
    }

    #[test]
    fn terminate_never_signals_a_group() {
        for pid in [0, -1, i64::from(i32::MAX) + 1] {
            assert!(!terminate(pid, false), "{pid}");
        }
    }
    #[test]
    fn no_pid_outside_the_range_is_alive() {
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(i64::from(i32::MAX) + 1));
    }
}
