//! A private in-memory SurrealDB on a leash, the store harness's (`crates/tasks/tests/store`):
//! `bash` starts it, says its pid first (`server pid N`), and stays its parent, and a watchdog
//! ends it once the test's end of the shell's input closes, which happens however the test
//! process ends. A test killed by a signal runs no destructor; without the leash its database
//! would run on under init.

use std::path::{Path, PathBuf};
use symphony_board::sys::Command;

/// The shell's script: `$0` the database, `$1` its address, `$2` `tools/fixture_process.sh`.
const LEASHED: &str = concat!(
    r#". "$2"; fixture_close_fds; exec 3<&0 </dev/null; "#,
    r#""$0" start --unauthenticated --bind "$1" --log info memory 3<&- & server=$!; "#,
    r#"echo "server pid $server"; "#,
    r#"(fixture_wait_eof <&3; kill $server 2>/dev/null) >/dev/null 2>&1 & leash=$!; "#,
    r#"exec 3<&-; wait $server; status=$?; kill $leash 2>/dev/null; wait $leash 2>/dev/null; "#,
    r#"exit $status"#,
);

/// The pinned SurrealDB: `SYMPHONY_SURREAL`, else the one `tools/surreal.sh` names.
pub fn binary() -> PathBuf {
    std::env::var_os("SYMPHONY_SURREAL").map_or_else(
        || {
            let found = symphony_board::sys::output(
                Command::new("bash")
                    .arg(workspace().join("tools/surreal.sh"))
                    .env_remove("BASH_ENV"),
            )
            .unwrap();
            assert!(found.status.success(), "SurrealDB lookup: {found:?}");
            PathBuf::from(String::from_utf8(found.stdout).unwrap().trim())
        },
        PathBuf::from,
    )
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The command that starts `surreal` in memory on `address`, leashed. Spawn it with its input
/// piped (`symphony_board::sys::spawn_piped`) and keep that input until the database should end:
/// closing it ends the database, and then the shell.
pub fn memory(surreal: &Path, address: &str) -> Command {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(LEASHED)
        .arg(surreal)
        .arg(address)
        .arg(workspace().join("tools/fixture_process.sh"))
        .env("NO_COLOR", "1")
        .env_remove("BASH_ENV");
    command
}

/// The database's pid, from the line its leash says first.
pub fn pid_of(line: &str) -> Option<u32> {
    line.strip_prefix("server pid ")?.trim().parse().ok()
}

/// Close `child`'s input, if it was given one, and let it end, a few seconds at most: for a
/// leashed database, its watchdog ends it while the shell, its parent, still waits for it, so
/// the watchdog never signals a pid already reaped. The caller kills and reaps what is left.
pub fn release(child: &mut symphony_board::sys::Child) {
    if child.stdin.take().is_none() {
        return;
    }
    let began = std::time::Instant::now();
    while began.elapsed() < std::time::Duration::from_secs(5)
        && matches!(child.try_wait(), Ok(None))
    {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for its child's end, where a wait on the child is the event"
        )]
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
