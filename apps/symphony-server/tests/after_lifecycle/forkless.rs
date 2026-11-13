//! Shared strict fork/stream/terminal/cleanup assertions for native and consuming-recipe drivers.
use super::{Command, Once, Repo, Stdio};
use std::io::Write;
pub(super) fn check(recipe: bool) {
    use std::cell::Cell;
    thread_local! {
        /// The forks this thread made, counted by the handler `fork` runs before it forks.
        static FORKS: Cell<u32> = const { Cell::new(0) };
    }
    extern "C" fn prepare() {
        FORKS.set(FORKS.get() + 1);
    }
    static HANDLER: Once = Once::new();
    HANDLER.call_once(|| {
        // SAFETY: the handler only bumps the forking thread's count; it is registered once.
        let registered = unsafe { libc::pthread_atfork(Some(prepare), None, None) };
        assert_eq!(registered, 0);
    });
    {
        // The repository's setup, a recipe's start (a bare `just` in a cleared environment),
        // a piped child, a terminal's, and the drop's stop and descendant waits.
        let repo = Repo::new("forkless");
        repo.start(&repo.main, recipe);
        let mut command = Command::new("/bin/cat");
        command.stdin(Stdio::piped());
        let mut piped = repo.run(command);
        piped
            .child
            .stdin
            .take()
            .unwrap()
            .write_all(b"echo")
            .unwrap();
        piped.success();
        assert_eq!(piped.output(), "echo");
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "test -t 1"]);
        let (mut terminal, _master) = repo.run_terminal(command);
        terminal.success();
    }
    assert_eq!(FORKS.get(), 0, "a fixture child forked");
}
