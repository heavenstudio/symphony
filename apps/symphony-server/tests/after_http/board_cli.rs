//! The board command line the fixture's server runs (`SF_TASKS_CLI`): this workspace's build of
//! `tasks`. The fixture's repository is in a temporary folder, so with none named the project's own
//! lookup (`.target/bin/tasks` there, the live build its common dir records, else `tasks` on `PATH`)
//! finds none, or whichever `tasks` the developer's `PATH` happens to hold.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use symphony_board::sys::{Command, Stdio};

/// `SF_TASKS_CLI`'s words for [`tasks`]: its path as one word, as a shell splits it.
pub(super) fn named() -> String {
    format!("'{}'", tasks().display())
}

/// The build the workspace's test run names (`SF_TEST_TASKS_BINARY`), else this checkout's, built
/// once here. Either is run once before it is handed out, so macOS's first-run assessment of a new
/// executable is paid in setup rather than inside a case.
fn tasks() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let binary = std::env::var_os("SF_TEST_TASKS_BINARY")
            .filter(|named| !named.is_empty())
            .map_or_else(built, PathBuf::from);
        // No board is named, so none is reached.
        let done = symphony_board::sys::output(
            Command::new(&binary)
                .arg("--help")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .stdin(Stdio::Null),
        )
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", binary.display()));
        assert!(
            done.status.success(),
            "{} --help: {done:?}",
            binary.display()
        );
        println!(
            "after_http: the fixture's board CLI is {}",
            binary.display()
        );
        binary
    })
}

/// `tasks` built through `tools/rs-bin.sh --build`, which leaves the heartbeat's `.target/bin/`
/// links alone.
fn built() -> PathBuf {
    let wrapper = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/rs-bin.sh");
    let done =
        symphony_board::sys::output(Command::new(wrapper).args(["--build", "tasks"])).unwrap();
    assert!(
        done.status.success(),
        "tools/rs-bin.sh --build tasks: {done:?}"
    );
    PathBuf::from(String::from_utf8(done.stdout).unwrap().trim())
}
