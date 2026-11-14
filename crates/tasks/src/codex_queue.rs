//! The native Codex queue, shared by callback delivery and resumed-terminal recovery.

use crate::cli::Env;
use crate::sys::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a delivery to Codex is given: its queue, or a steer's requests all together.
pub const PATIENCE: Duration = Duration::from_secs(5);
/// What sets, in seconds, the time a delivery to Codex is given in place of [`PATIENCE`]: a test's,
/// whose stand-in for `codex` takes a loaded machine's time to answer.
pub const PATIENCE_VAR: &str = "TASKS_CODEX_WAIT";

/// How long a delivery to Codex in `env` is given: [`PATIENCE`], unless [`PATIENCE_VAR`] says. A
/// callback's delivery reads this process's own environment, where it finds the `codex` it starts.
pub fn patience(env: &Env) -> Duration {
    env.seconds(PATIENCE_VAR, PATIENCE)
}

/// Queue once, within `patience`, the same deadline as callback delivery's. Acceptance is not a
/// receipt that a turn started. A failure may be ambiguous and must never trigger a blind resend.
pub(crate) fn queue(
    command: Command,
    thread: &str,
    text: &str,
    patience: Duration,
) -> Result<(), String> {
    queue_until(command, thread, text, |waited| waited >= patience)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delivery_is_given_five_seconds_unless_its_environment_says() {
        let env = |pairs: &[(&str, &str)]| Env {
            vars: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            cwd: std::path::PathBuf::new(),
        };
        assert_eq!(patience(&env(&[])), Duration::from_secs(5));
        assert_eq!(
            patience(&env(&[(PATIENCE_VAR, "120")])),
            Duration::from_secs(120)
        );
        assert_eq!(
            patience(&env(&[(PATIENCE_VAR, "0.25")])),
            Duration::from_millis(250)
        );
        for odd in ["soon", "-1", ""] {
            assert_eq!(patience(&env(&[(PATIENCE_VAR, odd)])), PATIENCE, "{odd}");
        }
    }
}
