//! Interrupted real hook calls retain their beat until it has been stopped and reaped.
//!
//! The case waits on what it watches, never on how fast a loaded machine starts a process: its
//! beat never ends of itself and its clock never passes, so the hook can end only by answering
//! the interrupt, and each wait lasts until the hook has done what it waits for or has ended
//! without it. [`CEILING`] only turns a hook that never does into a failure instead of a hang.

use super::{
    common::{BIN, retain_coverage_env},
    support::Tree,
};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use symphony_board::sys::{pid_alive, terminate};

/// The longest each wait lasts: for the hook to start its beat, and for the interrupted hook to
/// end. A working hook ends either wait as soon as the machine runs it, however loaded (a freshly
/// linked hook's first run waits on macOS's assessment of it, behind every other new executable
/// on the machine); only one that never starts its beat, or never answers the interrupt, reaches
/// this.
const CEILING: Duration = Duration::from_secs(120);

/// The beat: it writes its pid beside its name, then reads its hold (`NAME.hold`), a fifo the
/// case keeps open and never writes. So it ends only when it is killed, or when the case's process
/// is gone, however it went, and its read meets the fifo's end: it never outlives the case.
const HELD_BEAT: &str =
    "#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$0.pid\"\nread -r line < \"$0.hold\"\n";

struct OwnedHook {
    child: Child,
    beat: Option<i64>,
}

impl Drop for OwnedHook {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(pid) = self.beat.filter(|&pid| pid_alive(pid)) {
            // This is the exact child recorded by this test's private beat executable.
            terminate(pid, true);
        }
    }
}

/// A fifo at `path`, held open for reading and writing by the returned file, which closes on exec,
/// so no process the case starts holds it.
fn hold(path: &Path) -> File {
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a valid NUL-terminated path in this case's private folder.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0, "{path:?}");
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

#[test]
fn interrupting_the_hook_stops_and_reaps_its_beat_even_with_pipe_backpressure() {
    for padding in [0, 1 << 20] {
        let tree = Tree::new();
        let beat = tree.path("held-beat");
        crate::fixture_script::write(&beat, HELD_BEAT, false);
        let _hold = hold(&tree.path("held-beat.hold"));
        let mut command = Command::new(BIN);
        command
            .args(["hook", "tasks-beat"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TASKS_BIN", &beat)
            .env("SYMPHONY_HOOK_BEAT_CLOCK", tree.path("clock-never-passes"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        retain_coverage_env(&mut command);
        let child = command.spawn().unwrap();
        let mut hook = OwnedHook { child, beat: None };
        let event = serde_json::json!({"agent_id":"child", "padding":"x".repeat(padding)});
        hook.child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        let until = Instant::now() + CEILING;
        loop {
            let pid = std::fs::read_to_string(tree.path("held-beat.pid"))
                .ok()
                .and_then(|text| text.trim().parse().ok());
            if let Some(pid) = pid {
                hook.beat = Some(pid);
                break;
            }
            let ended = hook.child.try_wait().unwrap();
            assert!(
                ended.is_none(),
                "the hook ended before its {padding}-byte fixture beat started: {ended:?}"
            );
            assert!(
                Instant::now() < until,
                "the {padding}-byte fixture beat never started in {CEILING:?}"
            );
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: a test looks again for its condition by hand, where test_time::wait::until is the helper"
            )]
            std::thread::sleep(Duration::from_millis(2));
        }
        let pid = hook.beat.unwrap();
        assert!(
            pid_alive(pid),
            "fixture beat {pid} ended before interruption"
        );
        assert!(
            terminate(i64::from(hook.child.id()), false),
            "hook did not receive SIGTERM"
        );
        let until = Instant::now() + CEILING;
        let status = loop {
            if let Some(status) = hook.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < until,
                "interrupted hook still waits on beat {pid} after {CEILING:?}"
            );
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: a test looks again for its condition by hand, where test_time::wait::until is the helper"
            )]
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(status.success(), "silent hook exit: {status:?}");
        assert!(
            !pid_alive(pid),
            "beat {pid} survived or was left unreaped after hook exit"
        );
        hook.beat = None;
    }
}
