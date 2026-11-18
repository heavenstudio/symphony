//! The library adapter execs the exact Cargo artifact: the same PID, cwd, environment and argv.
//! A private libtest case is the probe, so no interpreter sanitizes dynamic-library variables.

use std::fs;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[path = "../../symphony/tests/common/tree.rs"]
mod tree;
use tree::Tree;

const BUILD: &str = env!("CARGO_BIN_EXE_symphony-build");
const PROBE: &str = "SYMPHONY_EXEC_TEST_PROBE";
const MODE: &str = "SYMPHONY_EXEC_TEST_MODE";

#[test]
#[ignore = "private subprocess used to observe the adapter's exec boundary"]
fn execution_probe() {
    let destination = std::env::var(PROBE).expect("the parent named its observation file");
    let observation = serde_json::json!({
        "pid": std::process::id(),
        "cwd": std::env::current_dir().unwrap(),
        "args": std::env::args().skip(1).collect::<Vec<_>>(),
        "env": ["HC_BIN", "CARGO_MANIFEST_DIR", "RUST_TEST_THREADS",
            "DYLD_FALLBACK_LIBRARY_PATH", "LD_LIBRARY_PATH"].into_iter()
            .map(|name| (name, std::env::var(name).unwrap())).collect::<std::collections::BTreeMap<_, _>>(),
    });
    let destination = PathBuf::from(destination);
    let pending = destination.with_extension("pending");
    fs::write(&pending, observation.to_string()).unwrap();
    fs::rename(pending, destination).unwrap();
    match std::env::var(MODE).unwrap_or_default().as_str() {
        "exit" => std::process::exit(17),
        "wait" => {
            use std::io::Read as _;
            std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        }
        _ => {}
    }
}

const ARGS: &[&str] = &[
    "--ignored",
    "--exact",
    "execution_probe",
    "--nocapture",
    "--test-threads=1",
    "--skip",
    "a filter with spaces",
];

fn probe_command(tree: &Tree, selected: bool, mode: &str) -> (Command, PathBuf) {
    let binary = std::env::current_exe().unwrap();
    let cwd = tree.path("package with spaces");
    fs::create_dir_all(&cwd).unwrap();
    let map = tree.write(
        "map.json",
        &serde_json::json!([{"binary": binary.display().to_string(), "shared": selected}])
            .to_string(),
    );
    let destination = tree.path("observed.json");
    let mut command = Command::new(BUILD);
    command
        .arg("exec-test-binary")
        .arg(map)
        .arg(binary)
        .args(ARGS)
        .current_dir(cwd)
        .env(PROBE, &destination)
        .env(MODE, mode)
        .env("HC_BIN", "/callers/hc")
        .env("CARGO_MANIFEST_DIR", "/callers/package")
        .env("RUST_TEST_THREADS", "3")
        .env("DYLD_FALLBACK_LIBRARY_PATH", "/callers/dyld")
        .env("LD_LIBRARY_PATH", "/callers/ld")
        .stdin(Stdio::null());
    (command, destination)
}

#[test]
fn cargo_environment_working_directory_and_argument_boundaries_survive_exec() {
    let tree = Tree::new();
    let (mut command, destination) = probe_command(&tree, true, "");
    let done = command.output().unwrap();
    assert!(done.status.success(), "{done:?}");
    let observed: serde_json::Value =
        serde_json::from_slice(&fs::read(destination).unwrap()).unwrap();
    assert_eq!(
        observed["cwd"],
        tree.path("package with spaces")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    assert_eq!(observed["args"], serde_json::json!(ARGS));
    for (name, expected) in [
        ("HC_BIN", "/callers/hc"),
        ("CARGO_MANIFEST_DIR", "/callers/package"),
        ("RUST_TEST_THREADS", "3"),
        ("DYLD_FALLBACK_LIBRARY_PATH", "/callers/dyld"),
        ("LD_LIBRARY_PATH", "/callers/ld"),
    ] {
        assert_eq!(observed["env"][name], expected, "{name}: {observed}");
    }
    let (mut command, destination) = probe_command(&tree, false, "");
    fs::remove_file(&destination).unwrap();
    let skipped = command.output().unwrap();
    assert!(skipped.status.success(), "{skipped:?}");
    assert!(
        !destination.exists(),
        "the stateless library belongs to nextest"
    );
}

struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_test_image_keeps_the_adapter_pid_exit_status_and_signal_behavior() {
    let tree = Tree::new();
    let (mut command, _) = probe_command(&tree, true, "exit");
    assert_eq!(command.output().unwrap().status.code(), Some(17));
    let (mut command, destination) = probe_command(&tree, true, "wait");
    fs::remove_file(&destination).unwrap();
    let mut child = Owned(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    test_time::until_true(
        "the exec probe published its identity",
        test_time::backstop(Duration::from_secs(30)),
        || destination.exists(),
    )
    .unwrap();
    let observed: serde_json::Value =
        serde_json::from_slice(&fs::read(destination).unwrap()).unwrap();
    assert_eq!(
        observed["pid"],
        child.0.id(),
        "exec keeps Cargo's process identity"
    );
    child.0.kill().unwrap();
    assert_eq!(child.0.wait().unwrap().signal(), Some(libc::SIGKILL));
}

#[test]
fn a_binary_not_named_by_the_build_metadata_is_refused_without_execution() {
    let tree = Tree::new();
    let map = tree.write("map.json", "[]");
    let done = Command::new(BUILD)
        .arg("exec-test-binary")
        .arg(map)
        .arg(Path::new("not-an-artifact"))
        .output()
        .unwrap();
    assert_eq!(done.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&done.stderr).contains("not a library artifact"));
}
