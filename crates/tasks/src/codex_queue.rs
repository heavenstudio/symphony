//! The native Codex queue, shared by callback delivery and resumed-terminal recovery.

use crate::sys::{Command, Stdio};
use std::time::{Duration, Instant};

/// Queue once, with the same five-second deadline as callback delivery. Acceptance is not a
/// receipt that a turn started. A failure may be ambiguous and must never trigger a blind resend.
pub(crate) fn queue(command: Command, thread: &str, text: &str) -> Result<(), String> {
    queue_until(command, thread, text, |waited| {
        waited >= Duration::from_secs(5)
    })
}

/// The queue deadline may be an event in an owned stand-in rather than elapsed host time.
pub(crate) fn queue_until(
    mut command: Command,
    thread: &str,
    text: &str,
    due: impl Fn(Duration) -> bool,
) -> Result<(), String> {
    let mut child = command
        .args(["queue", "--thread", thread, "--message", text])
        .env_remove("CLAUDE_CODE_MESSAGING_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("codex queue exited {status}"))
                };
            }
            None if !due(started.elapsed()) => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("codex queue timed out".into());
            }
        }
    }
}
