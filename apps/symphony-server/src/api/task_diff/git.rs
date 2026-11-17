//! Git output through a pipe: at most `limit` bytes kept, no disk capture, and the child
//! killed and reaped when its deadline, an output bound or its caller ends the read.

use std::io::{self, Read};
use std::path::Path;
use symphony_board::sys::{Command, Exit, Stdio, set_nonblocking};
use tokio::io::unix::AsyncFd;
use tokio::time::Instant;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Output {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

/// A Git read with no external diff, text conversion, pager or lazy object fetch. Every call
/// shares the request's deadline, including the calls that resolve its comparison.
pub(super) async fn read(
    root: &Path,
    args: &[&str],
    limit: usize,
    deadline: Instant,
) -> Result<Output, String> {
    let mut command = symphony::children::git();
    command
        .current_dir(root)
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["--no-pager", "--literal-pathspecs"])
        .args(args);
    run(command, limit, deadline).await
}

async fn run(mut command: Command, limit: usize, deadline: Instant) -> Result<Output, String> {
    if Instant::now() >= deadline {
        return Err("The diff read exceeded its time limit.".into());
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start the diff read: {e}"))?;
    let stdout = child.stdout.take().expect("the command's piped output");
    // Own the child before any fallible pipe setup. Exit's drop kills the exact unreaped
    // child and its waiting thread reaps it, including when the request is cancelled.
    let exit = Exit::of(child).map_err(|e| e.to_string())?;
    let read = async move {
        set_nonblocking(&stdout, true)?;
        // SAFETY: stdout owns its descriptor, which remains the same until this
        // AsyncFd is dropped. No caller can close or replace it while it is registered.
        let mut pipe = unsafe { AsyncFd::register(stdout) }?;
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            let mut ready = pipe.readable_mut().await?;
            let count = match ready.try_io(|pipe| pipe.get_mut().read(&mut buffer)) {
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(count) => count?,
                Err(_) => continue,
            };
            if count == 0 {
                break;
            }
            let keep = count.min(limit.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&buffer[..keep]);
            if keep < count {
                return Ok(Output {
                    bytes,
                    truncated: true,
                });
            }
        }
        if !exit.await?.success() {
            return Err(io::Error::other("Git could not read this comparison."));
        }
        Ok(Output {
            bytes,
            truncated: false,
        })
    };
    tokio::time::timeout_at(deadline, read)
        .await
        .map_err(|_| "The diff read exceeded its time limit.".to_string())?
        .map_err(|e: io::Error| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn deadline() -> Instant {
        Instant::now() + test_time::backstop(Duration::from_secs(30))
    }

    #[tokio::test]
    async fn git_reads_the_named_repository_and_an_expired_read_starts_nothing() {
        let tree = crate::testing::Tree::new();
        let root = crate::testing::repository(&tree);
        let output = read(&root, &["rev-parse", "--show-toplevel"], 4096, deadline())
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(output.bytes).unwrap().trim(),
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(!output.truncated);
        let error = run(
            symphony::children::command("/no/such/git"),
            1,
            Instant::now(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("time limit"));
    }

    #[tokio::test]
    async fn output_is_bounded_even_when_the_command_never_finishes_writing() {
        let mut command = symphony::children::command("yes");
        command.arg("diff");
        let output = run(command, 37, deadline()).await.unwrap();
        assert_eq!(
            output.bytes,
            b"diff\ndiff\ndiff\ndiff\ndiff\ndiff\ndiff\ndi"
        );
        assert!(output.truncated);
    }

    #[tokio::test]
    async fn exactly_the_limit_is_complete_and_a_failed_command_is_an_error() {
        for limit in [0, 3, 4] {
            let mut command = symphony::children::command("printf");
            command.arg("abc");
            let output = run(command, limit, deadline()).await.unwrap();
            assert_eq!(output.bytes, b"abc"[..limit.min(3)]);
            assert_eq!(output.truncated, limit < 3);
        }
        let command = symphony::children::command("false");
        assert!(
            run(command, 100, deadline())
                .await
                .unwrap_err()
                .contains("Git")
        );
        let command = symphony::children::command("/no/such/git");
        assert!(run(command, 100, deadline()).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn a_stalled_read_is_ended_at_its_deadline() {
        // This child waits on a pipe the test holds; virtual time reaches the deadline
        // without any real sleep or a bound on the machine's speed.
        // The command waits on an explicitly inherited descriptor, since run gives
        // Git null input.
        use std::os::fd::AsRawFd;
        let (reader, _writer) = symphony_board::sys::pipe().unwrap();
        let fd = reader.as_raw_fd();
        let mut command = symphony::children::command("sh");
        command
            .args(["-c", "exec cat <&\"$1\"", "fixture", &fd.to_string()])
            .pass_fd(fd);
        let error = run(command, 100, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(error.contains("time limit"), "{error}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_the_read_kills_and_reaps_its_child() {
        use crate::testing::{Tree, entered, fifo};
        let tree = Tree::new();
        let path = tree.path("entered");
        let marker = fifo(&path);
        let mut command = symphony::children::command("sh");
        command.args([
            "-c",
            "printf '%s\\n' \"$$\" >\"$1\"; exec cat <\"$1\"",
            "fixture",
            path.to_str().unwrap(),
        ]);
        let task = tokio::spawn(run(command, 1024, deadline()));
        let pid = entered(&marker);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        test_time::until_true(
            "the cancelled diff child to be killed and reaped",
            test_time::backstop(Duration::from_secs(30)),
            || !crate::sys::pid_alive(pid),
        )
        .unwrap_or_else(|stall| panic!("{stall}"));
    }
}
