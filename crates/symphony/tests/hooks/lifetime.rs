//! Interrupted real hook calls retain their beat until it has been stopped and reaped.

use super::{
    common::{BIN, retain_coverage_env},
    support::Tree,
};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use symphony_board::sys::{pid_alive, terminate};

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

#[test]
fn interrupting_the_hook_stops_and_reaps_its_beat_even_with_pipe_backpressure() {
    for padding in [0, 1 << 20] {
        let tree = Tree::new();
        let beat = tree.path("held-beat");
        crate::fixture_script::write(
            &beat,
            "#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$0.pid\"\nexec sleep 30\n",
            false,
        );
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
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let pid = std::fs::read_to_string(tree.path("held-beat.pid"))
                .ok()
                .and_then(|text| text.trim().parse().ok());
            if let Some(pid) = pid {
                hook.beat = Some(pid);
                break;
            }
            assert!(
                Instant::now() < until,
                "the {padding}-byte fixture beat never became ready; hook status: {:?}",
                hook.child.try_wait().unwrap()
            );
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
        let until = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = hook.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < until,
                "interrupted hook still waits on beat {pid}"
            );
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
