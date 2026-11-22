//! A board of its own for a test, and what serves it: the pinned `surreal` (the store tests'
//! `surreal()`, the one way every test binary finds it), behind a guard whose pipe watchdog ends
//! every server it started if the test process is killed; and a repository of its own, its board in
//! its git dir in the live board's shape ([`Repository`]), whose server is stopped when it goes, by
//! the pid its address file records, and only if that process serves it. The server tests' own, and
//! the project's board-only `tasks` tests' too, which run this crate's dispatch case with that
//! build's executable.

use crate::children::Child as _;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::OnceLock;
use std::time::Duration;
use symphony_board::cli::Env;
use symphony_board::server::{FOLDER, Running};
use symphony_board::sys::{Child, Command, Spawning as _, Stdio};

pub fn surreal() -> PathBuf {
    static FOUND: OnceLock<PathBuf> = OnceLock::new();
    FOUND.get_or_init(crate::store::surreal).clone()
}

/// A test-only executable passed through `SYMPHONY_SURREAL` (or directly to `Home`). It execs the
/// real server, preserving the pid and arguments that production records. Its watchdog reads a pipe
/// whose only writer belongs to this fixture: EOF also happens on abort or SIGKILL, without Drop.
/// Like the store tests' server guard, no test-owned write descriptor reaches a child after exec.
pub struct GuardedSurreal {
    binary: PathBuf,
    /// Its own test (`common.rs`) holds its pipe and pid.
    pub(crate) watchdog: Child,
}

impl GuardedSurreal {
    pub fn new(dir: &Path) -> Self {
        let records = dir.join("surreal-processes");
        std::fs::create_dir(&records).unwrap();
        let helper = crate::store::project_root().join("tools/fixture_process.sh");
        // One reader on one real pipe (symphony_board::sys::pipe): a reader blocked on a FIFO can remain
        // blocked on macOS after its last writer closes. Keep the watchdog owned and reap it on
        // normal teardown; its own group survives an interrupted suite's group.
        let watchdog = crate::store::spawn_with_input(
            Command::of("bash")
                .args([
                    "-c",
                    r#"
. "$1"
fixture_close_fds
fixture_wait_eof
: > "$2/closed"
for record in "$2"/server-*; do
    [ -f "$record" ] || continue
    pid=${record##*/server-}
    IFS= read -r data < "$record" || continue
    command=$(ps -ww -o command= -p "$pid")
    # The server's last argument is its data; a supervisor shell holds it before its log.
    case "$command" in
        *" $data") kill -KILL "$pid" 2>/dev/null ;;
    esac
done
exit 0
"#,
                    "watch-surreal",
                ])
                .arg(&helper)
                .arg(&records)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0),
        )
        .unwrap();
        let binary = dir.join("guarded-surreal");
        let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
        let body = format!(
            r#". {}
fixture_close_fds
if [ "$1" = start ]; then
    for data do :; done
    printf '%s\n' "$data" > {}/server-$$ || exit 1
    # Register before checking closure: shutdown either sees our record, or we refuse to start.
    [ ! -e {}/closed ] || exit 1
fi
exec {} "$@"
"#,
            quote(&helper.display().to_string()),
            quote(&records.display().to_string()),
            quote(&records.display().to_string()),
            quote(&surreal().display().to_string())
        );
        // Re-exec before closing descriptors: the interpreter's input script is also above 2.
        crate::fixture_script::write(
            &binary,
            &format!(
                "#!/bin/sh\nexec /bin/bash -c {} guarded-surreal \"$@\"\n",
                quote(&body)
            ),
            false,
        );
        Self { binary, watchdog }
    }

    pub fn path(&self) -> &Path {
        &self.binary
    }

    pub fn shutdown(&mut self) -> ExitStatus {
        drop(self.watchdog.stdin.take());
        // Given the backstop of the 5 s it once had to end at its EOF, then killed.
        let watchdog = &mut self.watchdog;
        let ended = test_time::until(
            "the watchdog ended at its EOF",
            test_time::backstop(Duration::from_secs(5)),
            || watchdog.try_wait().unwrap(),
        );
        ended.unwrap_or_else(|_| {
            let _ = self.watchdog.kill();
            self.watchdog.wait().unwrap()
        })
    }
}

impl Drop for GuardedSurreal {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The command line of process `pid`, if it runs.
pub fn command_of(pid: i64) -> Option<String> {
    let out = symphony_board::sys::output(Command::of("ps").args([
        "-ww",
        "-o",
        "command=",
        "-p",
        &pid.to_string(),
    ]))
    .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// Whether process `pid` runs a server on `data`: its command line's last argument, as a server's
/// is, where a scratch board's supervisor shell holds it before its log.
pub fn serves_data(pid: i64, data: &str) -> bool {
    command_of(pid).is_some_and(|line| line.ends_with(&format!(" {data}")))
}

/// The parent of process `pid`, if it runs.
pub fn parent_of(pid: i64) -> Option<i64> {
    let out = symphony_board::sys::output(Command::of("ps").args([
        "-o",
        "ppid=",
        "-p",
        &pid.to_string(),
    ]))
    .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

pub fn signal(pid: i64, name: &str) {
    let _ = std::process::Command::of("kill")
        .arg(format!("-{name}"))
        .arg(pid.to_string())
        .status_locked();
}

/// Waits until `pid` is gone, for up to the backstop of the 20 s it once had: whether it went.
pub fn gone(pid: i64) -> bool {
    test_time::until_true(
        format!("{pid} is gone"),
        test_time::backstop(Duration::from_secs(20)),
        || !symphony_board::sys::pid_alive(pid),
    )
    .is_ok()
}

/// A repository of its own, its board the one in its git dir in the live board's shape: reached
/// from the repository with no `TASKS_*` variable naming it, its server started on disk there by
/// the first command that needs one. That server is stopped when the repository goes, by the pid
/// its address file records, if that process serves it; its pipe watchdog also stops it when the
/// test process ends without running Drop.
pub struct Repository {
    pub dir: PathBuf,
    surreal: GuardedSurreal,
}

impl Repository {
    /// A new repository, in a folder named for `name` in the test target's folder.
    pub fn new(name: &str) -> Repository {
        Repository::under(Path::new(env!("CARGO_TARGET_TMPDIR")), name)
    }

    /// A new repository in the temporary folder, a scratch place
    /// ([`symphony_board::server::scratch`]): its board's server is supervised, ending with the
    /// process that started it.
    pub fn in_scratch(name: &str) -> Repository {
        Repository::under(&std::env::temp_dir(), name)
    }

    fn under(base: &Path, name: &str) -> Repository {
        let dir = base.join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let done =
            symphony_board::sys::output(Command::of("git").args(["init", "-q"]).arg(&dir)).unwrap();
        assert!(done.status.success(), "git init: {done:?}");
        let dir = dir.canonicalize().unwrap();
        let surreal = GuardedSurreal::new(&dir);
        Repository { dir, surreal }
    }

    pub fn common(&self) -> PathBuf {
        self.dir.join(".git")
    }

    /// The `surreal` its board's server is started with.
    pub fn surreal(&self) -> &Path {
        self.surreal.path()
    }

    /// What a command in the repository reads: that `surreal`, and no variable naming a board.
    pub fn env(&self) -> Env {
        let surreal = self.surreal().display().to_string();
        Env {
            vars: [("SYMPHONY_SURREAL".to_string(), surreal)]
                .into_iter()
                .collect(),
            cwd: self.dir.clone(),
        }
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        // A board its first command made on PostgreSQL, whose server outlives that command
        // outside a scratch place.
        let _ = symphony_board::server::Home::in_common(&self.common(), Some(self.surreal()))
            .stop_board();
        self.surreal.shutdown();
        let home = self.common().join(FOLDER);
        let recorded = std::fs::read_to_string(home.join("server.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Running>(&text).ok());
        if let Some(running) = recorded {
            let data = format!("surrealkv://{}", home.join("data").display());
            if serves_data(running.pid, &data) {
                signal(running.pid, "KILL");
                gone(running.pid);
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
