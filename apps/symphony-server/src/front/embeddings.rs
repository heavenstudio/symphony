//! The shared installation maintains derived search vectors without blocking board writes, for
//! each project it serves that has a board ([`super::maintenance`]).
use super::{Shared, Stamp, has_board, of_project, sweep_due, worker};
use crate::sys;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const INTERVAL: Duration = Duration::from_secs(60);
// A first backfill can take hours under contention. Completed sources survive cancellation;
// the server still owns and cancels the child immediately on shutdown or handover.
pub(super) const TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

pub(super) fn start(
    shared: &Arc<Shared>,
    home: Option<&Path>,
    clock: impl Fn() -> Stamp + Send + 'static,
) -> Option<std::thread::JoinHandle<()>> {
    home?;
    let shared = Arc::clone(shared);
    Some(worker("review task embeddings", move || {
        let of = of_project(&shared);
        let mut last = None;
        while !shared.stopping() {
            // A pass due waits while the project has no board, so none is made for it.
            if !sweep_due(last, clock(), INTERVAL) || !has_board(&shared) {
                if shared.wait(Duration::from_millis(250)) {
                    break;
                }
                continue;
            }
            last = Some(clock());
            // The project's build of the board, linked by its build setup: the main checkout's
            // keeps branch code off the live board. A private preview never starts this worker.
            let Some(tasks) = symphony::builds::linked(&shared.root, "tasks") else {
                eprintln!(
                    "· task embedding catch-up{of} skipped: no build of tasks linked at {}",
                    shared
                        .root
                        .join(symphony::builds::LINKS)
                        .join("tasks")
                        .display()
                );
                continue;
            };
            let started = Instant::now();
            eprintln!("· task embedding catch-up{of} started");
            let result = crate::live::measured(&format!("task embedding catch-up{of}"), || {
                let mut command = symphony_board::sys::Command::new("nice");
                command
                    .args(["-n", "10"])
                    .arg(&tasks)
                    .args(["embed", "--maintenance", "--threads", "1"])
                    .current_dir(&shared.root)
                    .env_remove("TASKS_DATABASE")
                    .env_remove("TASKS_SERVER");
                sys::output_while(
                    command,
                    TIMEOUT,
                    || !shared.stopping(),
                    || {
                        let at = clock();
                        (at.mono, at.wall)
                    },
                )
            });
            match result {
                Ok(done) if done.status.success() => {
                    let said: Vec<String> = [&done.stdout, &done.stderr]
                        .into_iter()
                        .filter(|output| !output.is_empty())
                        .map(|output| String::from_utf8_lossy(output).trim().to_owned())
                        .collect();
                    if !said.is_empty() {
                        eprintln!("· task embedding catch-up{of} says:\n{}", said.join("\n"));
                    }
                }
                Ok(done) => eprintln!(
                    "· task embedding catch-up{of} failed: {}",
                    String::from_utf8_lossy(&done.stderr).trim()
                ),
                Err(_) if shared.stopping() => {
                    eprintln!("· task embedding catch-up{of} cancelled")
                }
                Err(error) => eprintln!("· task embedding catch-up{of} failed: {error}"),
            }
            eprintln!(
                "· task embedding catch-up{of} ended after {:.3}s",
                started.elapsed().as_secs_f64()
            );
        }
    }))
}
