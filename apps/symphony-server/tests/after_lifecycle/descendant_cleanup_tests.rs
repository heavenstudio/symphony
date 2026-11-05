//! An orphaned fixture descendant holds cleanup until its last write, whichever owner reaps it.

use super::*;

#[test]
fn cleanup_waits_for_orphaned_fixture_descendants_before_removing_scratch() {
    for repository in [false, true] {
        let repo = Repo::new("descendant-cleanup");
        let base = repo.base.clone();
        let mut command = Command::new("/bin/sh");
        // The launcher exits while its child holds the lifetime endpoint and waits for the
        // test's input. The child's final write must finish before scratch can disappear.
        // The launcher reassigns every single-digit descriptor first, as a script may: the
        // endpoint must reach the child under a number none of them replaces.
        command
            .args([
                "-c",
                "exec 3<&0 4<&0 5<&0 6<&0 7<&0 8<&0 9<&0; sh -c 'printf ready; read -r line; printf finished > \"$1\"' fixture \"$1\" <&3 & exit",
                "fixture",
            ])
            .arg(base.join("late-write"))
            .stdin(Stdio::piped());
        let mut process = if repository {
            repo.run(command)
        } else {
            Process::spawn(command, &repo.log("orphan"))
        };
        let mut input = process.child.stdin.take().unwrap();
        process.success();
        until("orphaned fixture readiness", || {
            (process.output() == "ready").then_some(())
        });
        let (finished, completion) = std::sync::mpsc::channel();
        let cleanup = std::thread::spawn(move || {
            let began = Instant::now();
            drop(process);
            drop(repo);
            finished.send((began.elapsed(), Instant::now())).unwrap();
        });
        // A bounded negative observation, not cleanup's synchronization. Input releases the
        // descendant; EOF on its inherited endpoint is what releases cleanup.
        let early = completion.recv_timeout(Duration::from_millis(50));
        if let Ok((waited, _)) = early {
            panic!(
                "cleanup (repository {repository}) ended after {waited:?}, before the orphaned descendant's final write"
            );
        }
        let scratch_present = base.is_dir();
        let released = Instant::now();
        writeln!(input, "finish").unwrap();
        let ended = completion.recv_timeout(PATIENCE);
        // A cleanup still running at the deadline fails here: joining it would wait unbounded.
        if matches!(ended, Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
            panic!("cleanup (repository {repository}) did not end within {PATIENCE:?} of release");
        }
        // It sent or ended without sending: the join returns at once and carries its panic.
        if let Err(failure) = cleanup.join() {
            std::panic::resume_unwind(failure);
        }
        let (waited, end) = ended.unwrap_or_else(|error| {
            panic!("cleanup (repository {repository}) ended without its report: {error:?}")
        });
        // The descendant's write follows the release, so a cleanup that waited for it ends later.
        assert!(
            end > released,
            "cleanup (repository {repository}) ended {:?} before the orphaned descendant was released",
            released.duration_since(end)
        );
        println!(
            "cleanup (repository {repository}) waited {waited:?}, {:?} of it after release",
            end.duration_since(released)
        );
        assert!(
            scratch_present,
            "scratch removed while a descendant could write"
        );
        assert!(!base.exists());
    }
}
