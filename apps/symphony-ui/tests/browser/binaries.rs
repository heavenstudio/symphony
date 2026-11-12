//! The workspace's binaries the browser cases run: the board's CLI and the Symphony server, each had
//! once per run of this test binary, before a case starts a readiness clock of its own.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The variable naming the workspace's build of the board's CLI: the workspace's test run
/// (`sf-rust-tests`, `TASKS_BINARY` in tools/gate/src/tool_tests/rust_workspace.rs) builds it before
/// any test executable starts, so no case builds one under the run's clock.
const TASKS_BINARY: &str = "SF_TEST_TASKS_BINARY";

/// The board's CLI, once for this run: the build `SF_TEST_TASKS_BINARY` names, which must be a file,
/// since a build here would hide the runner's missing one under the case's clock. Run alone (`cargo
/// test -p symphony-ui --test browser`), with the variable unset, the case builds it.
pub(super) fn tasks() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let Some(named) = std::env::var_os(TASKS_BINARY).filter(|named| !named.is_empty()) else {
            eprintln!("{TASKS_BINARY} is unset: building tasks through tools/rs-bin.sh --build");
            return built("tasks");
        };
        let named = PathBuf::from(named);
        assert!(
            named.is_file(),
            "{TASKS_BINARY} names {}, which is not a file: the workspace's build of tasks is missing",
            named.display()
        );
        named
    })
}

/// The Symphony server holding the shelf fixture plugin (`symphony-shelf`), once for this run, as
/// `tools/rs-bin.sh` builds it: a release build, as the server is. The runner hands none. The
/// case's child is this binary itself, never a compiler wrapper whose PID can change or leave
/// builders behind.
pub(super) fn shelf_server() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| built("symphony-shelf"))
}

/// `name` built through `tools/rs-bin.sh --build`, which leaves the hooks' `.target/bin/` links alone.
pub(super) fn built(name: &str) -> PathBuf {
    let wrapper = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/rs-bin.sh");
    let done = Command::new(wrapper)
        .args(["--build", name])
        .output()
        .unwrap_or_else(|error| panic!("tools/rs-bin.sh --build {name}: {error}"));
    assert!(
        done.status.success(),
        "tools/rs-bin.sh --build {name}: {}\n{}",
        done.status,
        String::from_utf8_lossy(&done.stderr)
    );
    PathBuf::from(String::from_utf8(done.stdout).unwrap().trim())
}

/// Set in the run of a case below that is that case's child.
const CHILD: &str = "SYMPHONY_BROWSER_BINARIES_CHILD";

/// This test binary run again on the case `test` alone, as the child, with `binary` as
/// `SF_TEST_TASKS_BINARY` and a PATH holding none of the tools a build runs, so any build in it
/// fails it. Whether it passed, and its output, both streams.
fn child(test: &str, binary: &Path) -> (bool, String) {
    let done = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env(CHILD, "1")
        .env(TASKS_BINARY, binary)
        .env("PATH", "/no-builder-tools-expected")
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    (done.status.success(), said)
}

/// A folder of this case's own, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(case: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "symphony-browser-binaries-{case}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_case_started_with_the_workspaces_board_cli_named_runs_it_and_builds_nothing() {
    const TEST: &str =
        "binaries::a_case_started_with_the_workspaces_board_cli_named_runs_it_and_builds_nothing";
    if std::env::var_os(CHILD).is_some() {
        // What owner_controls' scratch board asks for before it starts the board's clock.
        assert_eq!(tasks(), Path::new(&std::env::var_os(TASKS_BINARY).unwrap()));
        return;
    }
    let scratch = Scratch::new("named");
    let tasks = scratch.0.join("tasks");
    std::fs::write(&tasks, "a build the runner made").unwrap();
    let (passed, said) = child(TEST, &tasks);
    assert!(passed, "{said}");
    assert!(!said.contains("is unset: building"), "{said}");
}

#[test]
fn a_named_board_cli_that_is_missing_fails_naming_it_and_builds_nothing() {
    const TEST: &str =
        "binaries::a_named_board_cli_that_is_missing_fails_naming_it_and_builds_nothing";
    if std::env::var_os(CHILD).is_some() {
        tasks();
        return;
    }
    let scratch = Scratch::new("missing");
    let missing = scratch.0.join("tasks");
    let (passed, said) = child(TEST, &missing);
    assert!(!passed, "{said}");
    assert!(
        said.contains(&format!(
            "SF_TEST_TASKS_BINARY names {}, which is not a file: the workspace's build of tasks is missing",
            missing.display()
        )),
        "{said}"
    );
    assert!(!said.contains("is unset: building"), "{said}");
}
