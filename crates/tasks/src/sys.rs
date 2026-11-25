//! What health asks of this machine: its host name and whether a process is alive, as
//! `gethostname` and `kill(pid, 0)` answer them; and the end of a
//! process, for the board's server ([`crate::server`]); and the local time a line is stamped with;
//! and the processes this one runs under ([`callers`]) and the folder one works in ([`folder_of`]).
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
//! --skill`), and an hc call would answer for that core and scope alone. Nor does a child keep the
//! marks of how its starter was asked for ([`MARKS`]): it is the project's command or the board's
//! own work, never the one command an agent handed a path to, or the land the `land` recipe asked
//! for. A git is [`git`], which keeps no repository its starter's environment names ([`GIT_VARS`]):
//! it acts on the folder it is run in. Clippy refuses a child made anywhere else in the crate,
//! however its path is spelled, imported or gated (the crate's clippy.toml, `disallowed-methods`;
//! tried by the release run's `tests/child_guard`).

use std::collections::BTreeMap;
use std::ffi::{OsStr, c_char, c_int, c_long};
use std::fs::File;

pub use process_cwd::exit::Exit;
pub use process_cwd::spawn::{
    Child, Command, Interrupts, Spawning, Stdio, connect, output, pipe, set_nonblocking,
    spawn_output, spawn_piped, spawning, wait_output,
};

mod callers;
pub use callers::{Caller, callers, folder_of};
mod gone;
pub use gone::{gone, load_average, load_said};
mod start;
#[cfg(target_os = "macos")]
pub(crate) use start::bsd_start;
pub use start::{Boottime, Start, boot_clock, process_starts};

unsafe extern "C" {
    fn gethostname(name: *mut c_char, len: usize) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn tzset();
    fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
}

/// What ties a process to its starter's session ([`SESSION_VARS`], [`sessionless`]) and what marks
/// how it was asked for ([`MARKS`], [`unmarked`]): the marker the recipes an agent runs unasked set
/// ([`crate::confine::MARKER`]) and the `land` recipe's ([`crate::land::RECIPE`]), which a test
/// here holds equal to the leaf's. The lists live in the leaf the cwd scans share
/// ([`process_cwd::session`]), so a plugin's process starts its children without them and without
/// linking the board. A child the board starts runs the project's own command (`[board] on_land`
/// and `on_review`, a check, a generator, git and its hooks) or the board's own work.
pub use process_cwd::session::{MARKS, SESSION_VARS, sessionless, unmarked};

/// A command for `program` with this process's environment but [`SESSION_VARS`] and [`MARKS`].
/// What the caller sets on it after it keeps: a call that must keep one of them sets it there, and
/// says why.
#[expect(
    clippy::disallowed_methods,
    reason = "one of the two places a board child is made, without the session"
)]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    unmarked(sessionless(&mut command));
    command
}

/// What names a repository, its index or its tree to git in place of the folder it runs in. A git
/// hook inherits some from the git that runs it (every commit hook `GIT_INDEX_FILE`, one in a
/// linked worktree `GIT_DIR` as well), and so does whatever the hook runs: a project's commit gate
/// runs the recorder and board commands, and the land's merge commit and a release's refresh run
/// those gates. A git that kept them would read and write the hook's repository and index.
pub const GIT_VARS: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// `git` by [`command`], on the repository of the folder it runs in (`-C`, `--git-dir` or its
/// current directory) alone: without [`GIT_VARS`]. A call that names an index of its own sets
/// `GIT_INDEX_FILE` on it after.
pub fn git() -> Command {
    let mut git = command("git");
    for name in GIT_VARS {
        git.env_remove(name);
    }
    git
}

/// A command for `program` whose whole environment is `vars` but [`SESSION_VARS`] and [`MARKS`]: a
/// child started in a caller's environment ([`crate::cli::Env`]) rather than this process's. What
/// the caller sets on it after it keeps, as with [`command`].
#[expect(
    clippy::disallowed_methods,
    reason = "one of the two places a board child is made, without the session"
)]
pub fn command_with(program: impl AsRef<OsStr>, vars: &BTreeMap<String, String>) -> Command {
    let mut command = Command::new(program);
    command.env_clear().envs(vars);
    unmarked(sessionless(&mut command));
    command
}

impl crate::port::Lsof for Command {
    fn lsof(program: &str, args: &[&str]) -> Self {
        let mut lsof = command(program);
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

/// The bytes a process of this user may still write on the filesystem holding `path`, as `statvfs`
/// says; none when it cannot be read.
pub fn free_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    /// Every field of `statvfs` is unsigned and no wider than 64 bits, on each Unix.
    fn widen(count: impl Into<u64>) -> u64 {
        count.into()
    }
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: the path is NUL-terminated and outlives the call; statvfs writes only the struct it is
    // given, which is read only once the call says it filled it.
    let stat = unsafe {
        if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) != 0 {
            return None;
        }
        stat.assume_init()
    };
    widen(stat.f_bavail).checked_mul(widen(stat.f_frsize))
}

/// Ask every process of the group `group` to end (SIGTERM), or make them (SIGKILL, `hard`):
/// whether the signal went. Only a group a child was started in of its own
/// (`Command::process_group(0)`, so its pid): 0 and 1, this process's own group and init's, are
/// refused.
pub fn end_group(group: u32, hard: bool) -> bool {
    let Ok(group) = c_int::try_from(group) else {
        return false;
    };
    if group <= 1 {
        return false;
    }
    let signal = if hard { SIGKILL } else { SIGTERM };
    // SAFETY: kill reads nothing of ours; a pid below 0 names the group of its absolute value.
    unsafe { kill(-group, signal) == 0 }
}

/// Whether the process group `group` still holds a process, as [`pid_alive`] judges one: a group a
/// child was started in of its own, never this process's or init's (0 and 1 are none).
pub fn group_alive(group: u32) -> bool {
    let Ok(group) = c_int::try_from(group) else {
        return false;
    };
    if group <= 1 {
        return false;
    }
    // SAFETY: signal 0 sends nothing; a pid below 0 names the group of its absolute value.
    let signalled = unsafe { kill(-group, 0) } == 0;
    signalled || std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
}

/// End this process by `signal`'s default action, as a command a signal ended ends, once it has
/// said what it had to; returns only when that action does not end it.
pub fn end_by(signal: c_int) {
    // SAFETY: the default action of a signal number, then that signal to this process itself.
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
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

/// Whether a child std forks beside this process's connections holds `fd` before its exec, as
/// the child's own table says: the probe of a connection no spawn copies (the board's PostgreSQL
/// socket, the `rpc` client's kept connection), against std's socket as the control.
#[cfg(all(test, target_vendor = "apple"))]
pub(crate) fn a_forked_child_holds(fd: std::os::fd::RawFd) -> bool {
    use crate::children::Child as _;
    use std::os::unix::process::CommandExt as _;
    let mut command = std::process::Command::of("/usr/bin/true");
    // SAFETY: fcntl is async-signal-safe and reads only the forked child's own table.
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(fd, libc::F_GETFD) == -1 {
                Ok(())
            } else {
                Err(std::io::Error::from_raw_os_error(libc::EEXIST))
            }
        });
    }
    let _spawning = spawning();
    match command.status() {
        Ok(status) => {
            assert!(status.success(), "{status}");
            false
        }
        Err(e) if e.raw_os_error() == Some(libc::EEXIST) => true,
        Err(e) => panic!("{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::children::Child as _;
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
        ("SYMPHONY_BOARD_KEPT", "kept"),
    ];

    /// What a child holds of [`STARTED`], `|` between, run by `sh`.
    fn saw(command: &mut Command) -> String {
        let said = "printf '%s|%s|%s|%s|%s|%s' \"${TASKS_SESSION-unset}\" \
                    \"${HC_TERMINAL_ID-unset}\" \"${HC_TERMINAL_SOCKET-unset}\" \
                    \"${HC_SESSION-unset}\" \"${HC_SCOPE-unset}\" \"${SYMPHONY_BOARD_KEPT-unset}\"";
        let done = output(command.args(["-c", said])).unwrap();
        assert!(done.status.success(), "{done:?}");
        String::from_utf8(done.stdout).unwrap()
    }

    #[test]
    fn a_child_runs_without_its_starters_session_but_with_what_its_caller_sets() {
        // Set in this process, the variables would reach every other test's children: a re-run of
        // this test alone is started in them.
        const CASE: &str = "SYMPHONY_BOARD_CHILDREN_CASE";
        if std::env::var_os(CASE).is_some() {
            let none = "unset|unset|unset|unset|unset";
            let all = "fixture-session|t7|/fixture/hc/t7.sock|fixture-core|fixture-scope|kept";
            assert_eq!(saw(&mut Command::of("/bin/sh")), all, "the starter's own");
            assert_eq!(saw(&mut command("/bin/sh")), format!("{none}|kept"));
            assert_eq!(
                saw(command("/bin/sh").env("TASKS_SESSION", "an-actor")),
                "an-actor|unset|unset|unset|unset|kept",
                "an actor's command keeps the handle it is given"
            );
            // A caller's environment, its session in it, is the child's whole environment but that.
            let mut given: BTreeMap<String, String> = std::env::vars().collect();
            given.insert("SYMPHONY_BOARD_KEPT".into(), "given".into());
            assert_eq!(
                saw(&mut command_with("/bin/sh", &given)),
                format!("{none}|given")
            );
            given.remove("SYMPHONY_BOARD_KEPT");
            assert_eq!(
                saw(command_with("/bin/sh", &given).env("HC_TERMINAL_ID", "t9")),
                "unset|t9|unset|unset|unset|unset"
            );
            // A launcher handed a command drops even what its caller set before.
            let mut handed = Command::of("/bin/sh");
            handed.env("HC_SESSION", "other-core");
            assert_eq!(saw(sessionless(&mut handed)), format!("{none}|kept"));
            return;
        }
        let done = output(
            Command::of(std::env::current_exe().unwrap())
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

    #[test]
    fn the_marks_no_child_keeps_are_the_unasked_recipes_and_the_lands() {
        // The leaf names them so a plugin's process drops them too; the board's are these.
        assert_eq!(MARKS, [crate::confine::MARKER, crate::land::RECIPE]);
    }

    #[test]
    fn a_child_runs_without_the_marks_of_how_its_starter_was_asked_for() {
        // Set in this process, the marks would reach every other test: a re-run of this test
        // alone is started under them, as a land's `tasks` is.
        const CASE: &str = "SYMPHONY_BOARD_MARKS_CASE";
        let saw = |command: &mut Command| {
            let said = "printf '%s|%s|%s' \"${SYMPHONY_UNASKED-unset}\" \
                        \"${SYMPHONY_LAND-unset}\" \"${SYMPHONY_BOARD_KEPT-unset}\"";
            let done = output(command.args(["-c", said])).unwrap();
            assert!(done.status.success(), "{done:?}");
            String::from_utf8(done.stdout).unwrap()
        };
        if std::env::var_os(CASE).is_some() {
            assert_eq!(
                saw(&mut Command::of("/bin/sh")),
                "1|1|kept",
                "the starter's own"
            );
            assert_eq!(saw(&mut command("/bin/sh")), "unset|unset|kept");
            let given: BTreeMap<String, String> = std::env::vars().collect();
            assert_eq!(
                saw(&mut command_with("/bin/sh", &given)),
                "unset|unset|kept"
            );
            // A starter that must keep one sets it back, as the land does for its regenerator.
            assert_eq!(
                saw(command_with("/bin/sh", &given).env(crate::land::RECIPE, "1")),
                "unset|1|kept"
            );
            let mut handed = Command::of("/bin/sh");
            handed.env(crate::confine::MARKER, "again");
            assert_eq!(saw(unmarked(&mut handed)), "unset|unset|kept");
            return;
        }
        let done = output(
            Command::of(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sys::tests::a_child_runs_without_the_marks_of_how_its_starter_was_asked_for",
                ])
                .env(CASE, "1")
                .envs(MARKS.map(|name| (name, "1")))
                .env("SYMPHONY_BOARD_KEPT", "kept"),
        )
        .unwrap();
        let said = String::from_utf8_lossy(&done.stdout);
        assert!(
            done.status.success() && said.contains("1 passed"),
            "{said}{}",
            String::from_utf8_lossy(&done.stderr)
        );
    }

    /// A git hook hands what it runs the repository and index of the commit it is in (every commit
    /// hook `GIT_INDEX_FILE`, one in a linked worktree `GIT_DIR` too, its reference-transaction
    /// `GIT_WORK_TREE`): the board's git acts on the folder it is run in all the same, and leaves
    /// that repository's index and tree as they were.
    #[test]
    fn a_git_acts_on_the_repository_of_its_folder_whatever_repository_its_starter_names() {
        // Set in this process, the variables would move every other test's git: a re-run of this
        // test alone is started in them, with the scratch folder and what it holds in these.
        const CASE: &str = "SYMPHONY_BOARD_GIT_CASE";
        const TREE: &str = "SYMPHONY_BOARD_GIT_TREE";
        let said = |command: &mut Command| {
            let done = output(command).unwrap();
            assert!(done.status.success(), "{done:?}");
            String::from_utf8(done.stdout).unwrap().trim().to_string()
        };
        let real = |path: &str| Path::new(path).canonicalize().unwrap();
        if let Some(root) = std::env::var_os(CASE) {
            let root = Path::new(&root).canonicalize().unwrap();
            let (ours, theirs) = (root.join("ours"), root.join("theirs"));
            let index = std::fs::read(theirs.join(".git/index")).unwrap();
            let common = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
            let kept = said(command("git").arg("-C").arg(&ours).args(common));
            assert_eq!(real(&kept), theirs.join(".git"), "a git that keeps them");
            let own = said(git().arg("-C").arg(&ours).args(common));
            assert_eq!(real(&own), ours.join(".git"));
            // The recorder's tree and dirtiness, asked from a subfolder as a gate that went there,
            // within a backstop only a stall reaches, as the recorder's own tests give it.
            let patient = test_time::backstop(std::time::Duration::from_secs(30));
            let read = crate::runs::tree_and_dirty(&ours.join("sub"), "HEAD", false, patient);
            assert_eq!(read, (std::env::var(TREE).ok(), Some(false)));
            // A session's branch, and the worktrees a launch looks for its branch among.
            let branch = crate::cli::probes::branch_of(&ours.to_string_lossy());
            assert_eq!(branch.as_deref(), Ok("ours-branch"));
            let listed = crate::sweep::git::worktrees(&ours).unwrap();
            assert_eq!(listed.len(), 1, "{listed:?}");
            assert_eq!(listed[0].path.canonicalize().unwrap(), ours);
            // A folder in no repository is in none, not in theirs.
            assert!(crate::git::common_dir(&root.join("nowhere")).is_err());
            // A worktree made as a launch makes one: registered, then checked out by itself.
            let added = root.join("added");
            let add = ["worktree", "add", "--no-checkout", "-b", "added"];
            said(git().arg("-C").arg(&ours).args(add).arg(&added).arg("HEAD"));
            said(git().arg("-C").arg(&added).args(["checkout", "--force"]));
            assert_eq!(
                std::fs::read_to_string(added.join("sub/f")).unwrap(),
                "ours\n"
            );
            let status = said(git().arg("-C").arg(&added).args(["status", "--porcelain"]));
            assert_eq!(status, "", "its own index holds its files");
            assert_eq!(std::fs::read(theirs.join(".git/index")).unwrap(), index);
            assert_eq!(
                std::fs::read_to_string(theirs.join("f")).unwrap(),
                "changed\n"
            );
            return;
        }
        let root = std::env::temp_dir().join(format!("symphony-sys-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (ours, theirs) = (root.join("ours"), root.join("theirs"));
        std::fs::create_dir_all(root.join("nowhere")).unwrap();
        for (repo, file, branch) in [(&ours, "sub/f", "ours-branch"), (&theirs, "f", "main")] {
            std::fs::create_dir_all(repo.join("sub")).unwrap();
            std::fs::write(
                repo.join(file),
                format!("{}\n", repo.file_name().unwrap().display()),
            )
            .unwrap();
            let words = ["-c", "user.email=t@t", "-c", "user.name=t", "-C"];
            let run = |args: &[&str]| said(git().args(words).arg(repo).args(args));
            run(&["init", "-q", "-b", branch]);
            run(&["add", "."]);
            run(&["commit", "-q", "-m", "first"]);
        }
        // Theirs has a change of its own that no git of ours may check out over.
        std::fs::write(theirs.join("f"), "changed\n").unwrap();
        let tree = said(
            git()
                .arg("-C")
                .arg(&ours)
                .args(["rev-parse", "HEAD^{tree}"]),
        );
        let done = output(
            Command::of(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sys::tests::a_git_acts_on_the_repository_of_its_folder_whatever_repository_its_starter_names",
                ])
                .env(CASE, &root)
                .env(TREE, &tree)
                .env("GIT_DIR", theirs.join(".git"))
                .env("GIT_WORK_TREE", &theirs)
                .env("GIT_INDEX_FILE", theirs.join(".git/index")),
        )
        .unwrap();
        let out = String::from_utf8_lossy(&done.stdout);
        assert!(
            done.status.success() && out.contains("1 passed"),
            "{out}{}",
            String::from_utf8_lossy(&done.stderr)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Clippy reads the nearest clippy.toml alone: the crate's, which refuses a child made past
    /// [`command`] and [`command_with`], must hold the workspace's settings whole before its own.
    /// symphony-app's lib tests hold every crate's to that (crates/symphony/src/channel_tests.rs);
    /// this holds the board's in the board's own run, which an edit to it alone is tested by.
    #[test]
    fn the_crates_clippy_settings_start_with_the_workspaces() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let ours = std::fs::read_to_string(manifest.join("clippy.toml")).unwrap();
        let workspace = std::fs::read_to_string(manifest.join("../../clippy.toml")).unwrap();
        assert!(
            ours.starts_with(&workspace),
            "crates/tasks/clippy.toml must start with the workspace's clippy.toml whole"
        );
    }

    #[test]
    fn the_host_name_is_the_one_the_system_gives() {
        let said = output(&mut Command::of("hostname")).unwrap();
        assert_eq!(hostname(), String::from_utf8(said.stdout).unwrap().trim());
        assert!(!hostname().is_empty());
    }

    #[test]
    fn this_process_is_alive_and_a_reaped_child_is_not() {
        assert!(pid_alive(i64::from(std::process::id())));
        let mut child = Command::of("true").spawn().unwrap();
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
                            std::process::Command::of("sleep")
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
                let mut child = spawn_piped(&mut Command::of("true")).unwrap();
                let mut input = child.stdin.take().unwrap();
                assert!(child.wait().unwrap().success());
                let refused = input.write_all(b"unread").unwrap_err();
                assert_eq!(
                    refused.kind(),
                    std::io::ErrorKind::BrokenPipe,
                    "answer {answers}"
                );
                let done = output(Command::of("/bin/echo").arg("answer")).unwrap();
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
            let bsd = output(Command::of("date").args(["-r", &whole, "+%Y-%m-%d %H:%M"])).unwrap();
            let date = if bsd.status.success() {
                bsd
            } else {
                output(Command::of("date").args(["-d", &format!("@{whole}"), "+%Y-%m-%d %H:%M"]))
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
            let mut child = Command::of("sleep").arg("30").spawn().unwrap();
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
    fn end_group_ends_a_childs_own_group_whole_and_never_ours_or_inits() {
        use std::os::unix::process::ExitStatusExt;
        for (hard, signal) in [(false, 15), (true, 9)] {
            // The shell and the sleep it started, one group: it says the sleep's pid once it has
            // made it, so the signal finds both.
            let mut child = spawn_output(
                Command::of("sh")
                    .args(["-c", "sleep 30 & echo $!; wait $!; exit 3"])
                    .process_group(0),
            )
            .unwrap();
            let mut made = String::new();
            std::io::BufRead::read_line(
                &mut std::io::BufReader::new(child.stdout.as_mut().unwrap()),
                &mut made,
            )
            .unwrap();
            assert!(made.trim().parse::<u32>().is_ok(), "{made:?}");
            assert!(end_group(child.id(), hard));
            assert_eq!(child.wait().unwrap().signal(), Some(signal));
        }
        for group in [0, 1, u32::MAX] {
            assert!(!end_group(group, false), "{group}");
        }
    }

    #[test]
    fn a_group_is_alive_while_any_process_of_it_lives_even_once_its_first_ended() {
        // The shell ends at once, leaving the sleep it started in the background in its group, one
        // no slow look at the group outlives: only the kill below ends it.
        let mut child = Command::of("sh")
            .args(["-c", "sleep 600 &"])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(child.wait().unwrap().success());
        let group = child.id();
        assert!(group_alive(group), "the sleep still holds the group");
        assert!(end_group(group, true));
        test_time::until_true(
            "the killed group is gone",
            test_time::backstop(std::time::Duration::from_secs(10)),
            || !group_alive(group),
        )
        .unwrap();
        for group in [0, 1, u32::MAX] {
            assert!(!group_alive(group), "{group}");
        }
    }

    #[test]
    fn no_pid_outside_the_range_is_alive() {
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(i64::from(i32::MAX) + 1));
    }
}
