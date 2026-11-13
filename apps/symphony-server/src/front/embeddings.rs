//! The shared installation maintains derived search vectors without blocking board writes.
use super::{Shared, Stamp, sweep_due, worker};
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
        let mut last = None;
        while !shared.stopping() {
            if !sweep_due(last, clock(), INTERVAL) {
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
                    "· task embedding catch-up skipped: no build of tasks linked at {}",
                    shared
                        .root
                        .join(symphony::builds::LINKS)
                        .join("tasks")
                        .display()
                );
                continue;
            };
            let started = Instant::now();
            eprintln!("· task embedding catch-up started");
            let result = crate::live::measured("task embedding catch-up", || {
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
                    for output in [&done.stdout, &done.stderr] {
                        if !output.is_empty() {
                            eprintln!("{}", String::from_utf8_lossy(output).trim());
                        }
                    }
                }
                Ok(done) => eprintln!(
                    "· task embedding catch-up failed: {}",
                    String::from_utf8_lossy(&done.stderr).trim()
                ),
                Err(_) if shared.stopping() => eprintln!("· task embedding catch-up cancelled"),
                Err(error) => eprintln!("· task embedding catch-up failed: {error}"),
            }
            eprintln!(
                "· task embedding catch-up ended after {:.3}s",
                started.elapsed().as_secs_f64()
            );
        }
    }))
}
