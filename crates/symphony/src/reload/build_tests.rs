//! The two builds a reload starts, and how one runs, ends, is stopped and is refused, through a shell
//! standing in for the build.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use symphony_board::sys::Command;

use crate::test_tree::Tree;

use super::{client_build, run_build, server_build, server_build_by};

fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}

fn fifo(path: &Path) {
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: the path is NUL-terminated and outlives the call.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0, "{path:?}");
}

#[test]
fn each_build_is_mains_own_command_with_its_arguments() {
    let tree = Tree::new();
    let home = Path::new("/common dir/symphony/server");
    let stage = home.join(".reload-client.7");
    let client = client_build(&stage);
    assert_eq!(client.get_program(), "just");
    assert_eq!(
        client.get_args().collect::<Vec<_>>(),
        [OsStr::new("symphony-ui-dist"), stage.as_os_str()]
    );
    // The server is installed from the host package where main has one, else from the
    // Symphony's own folder, by plain `cargo` where main names no command for it, and by the
    // Symphony's own wrapper in its own workspace.
    for (host, package) in [
        (false, "symphony/apps/symphony-server"),
        (true, "apps/symphony-host"),
        (true, "apps/symphony-server"),
    ] {
        if host {
            tree.write("apps/symphony-host/Cargo.toml", "[package]\n");
        }
        if package == "apps/symphony-server" {
            tree.write("apps/symphony-server/Cargo.toml", "[package]\n");
        }
        let server = server_build(&tree.root, home);
        let standalone = package == "apps/symphony-server";
        assert_eq!(
            server.get_program(),
            if standalone {
                tree.path("tools/cargo.sh").into_os_string()
            } else {
                "cargo".into()
            },
            "host package {host}"
        );
        assert_eq!(
            words(&server),
            installed(&[], package, home),
            "host package {host}"
        );
    }
}

/// `install … --root HOME` of the server's package `package`, after `before`.
fn installed(before: &[&str], package: &str, home: &Path) -> Vec<String> {
    before
        .iter()
        .chain(&[
            "install",
            "--quiet",
            "--locked",
            "--force",
            "--path",
            package,
            "--bin",
            "symphony-server",
            "--root",
        ])
        .map(|word| (*word).to_string())
        .chain([home.display().to_string()])
        .collect()
}

/// The words `command` runs with.
fn words(command: &Command) -> Vec<String> {
    command
        .get_args()
        .map(|word| word.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_server_builds_by_the_command_mains_project_file_names_for_cargo() {
    let tree = Tree::new();
    let home = Path::new("/common dir/symphony");
    tree.write("apps/symphony-server/Cargo.toml", "[package]\n");
    let package = "apps/symphony-server";
    let wrapper = ["tools/wrap.sh", "slots", "cargo"].map(String::from);
    // A program named with a folder is the checkout's own, even in the Symphony's own workspace.
    let server = server_build_by(&tree.root, home, &wrapper);
    assert_eq!(server.get_program(), tree.path("tools/wrap.sh"));
    assert_eq!(
        words(&server),
        installed(&["slots", "cargo"], package, home)
    );
    // A bare name is found on the search path, and an absolute one is itself.
    for (named, program) in [
        ("cargo-nightly", "cargo-nightly"),
        ("/opt/cargo", "/opt/cargo"),
    ] {
        let server = server_build_by(&tree.root, home, &[named.to_string()]);
        assert_eq!(server.get_program(), program);
        assert_eq!(words(&server), installed(&[], package, home));
    }
    // The setting is read from main's project file.
    tree.write(
        "symphony.toml",
        "[project]\ncargo = [\"tools/wrap.sh\", \"slots\", \"cargo\"]\n",
    );
    let server = server_build(&tree.root, home);
    assert_eq!(server.get_program(), tree.path("tools/wrap.sh"));
    assert_eq!(
        words(&server),
        installed(&["slots", "cargo"], package, home)
    );
}

/// What the build saw, `|` after each: its directory, the five variables a build must not see (the
/// board's session and hc's four) and one it keeps, its stdin and stdout, and whether it leads a
/// process group of its own.
const SAW: &str = r#"
    input=other; output=other; leader=no
    test /dev/fd/0 -ef /dev/null && input=null
    test /dev/fd/1 -ef /dev/null && output=null
    test "$(ps -o pgid= -p $$ | tr -d ' ')" = "$$" && leader=yes
    printf '%s|' "$(pwd -P)" "${TASKS_SESSION-unset}" "${HC_TERMINAL_ID-unset}" \
        "${HC_TERMINAL_SOCKET-unset}" "${HC_SESSION-unset}" "${HC_SCOPE-unset}" \
        "${SYMPHONY_KEPT-unset}" "$input" "$output" "$leader" > saw
"#;

#[test]
fn a_build_runs_from_the_root_in_a_group_of_its_own_without_the_session() {
    // The variables are this test's own process's, set when it is started: set here, they would
    // reach every other test's children too.
    const CASE: &str = "SYMPHONY_BUILD_CASE";
    if let Some(root) = std::env::var_os(CASE) {
        run_build(shell(SAW), Path::new(&root), "fixture", || false).unwrap();
        return;
    }
    let tree = Tree::new();
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "reload::build_tests::a_build_runs_from_the_root_in_a_group_of_its_own_without_the_session",
            ])
            .env(CASE, &tree.root)
            .env("TASKS_SESSION", "a-session")
            .env("HC_TERMINAL_ID", "t7")
            .env("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock")
            .env("HC_SESSION", "fixture-core")
            .env("HC_SCOPE", "fixture-scope")
            .env("SYMPHONY_KEPT", "kept"),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        tree.read("saw"),
        format!(
            "{}|unset|unset|unset|unset|unset|kept|null|null|yes|",
            tree.root.canonicalize().unwrap().display()
        )
    );
}

#[test]
fn a_failed_build_answers_how_it_ended_and_all_it_said_on_one_line() {
    let tree = Tree::new();
    let build = |script: &str| run_build(shell(script), &tree.root, "fixture", || false);
    assert_eq!(
        build("printf 'first line\\n\\tsecond  line\\n' >&2; exit 3").unwrap_err(),
        "fixture build exit status: 3: first line second line"
    );
    assert_eq!(
        build("kill -KILL $$").unwrap_err(),
        "fixture build signal: 9 (SIGKILL): "
    );
    // More than a pipe holds: read while the build runs, so the build can end.
    assert_eq!(
        build("head -c 300000 /dev/zero | tr '\\0' x >&2; exit 1").unwrap_err(),
        format!("fixture build exit status: 1: {}", "x".repeat(300_000))
    );
    // What a build that succeeds says is no answer.
    build("printf 'warning: unused\\n' >&2").unwrap();
}

#[test]
fn a_build_that_cannot_start_names_its_product_and_the_cause() {
    let tree = Tree::new();
    let missing = std::io::Error::from_raw_os_error(libc::ENOENT);
    for (command, root) in [
        (Command::new(tree.path("no-such-build")), tree.root.clone()),
        (shell("exit 0"), tree.path("no-such-root")),
    ] {
        assert_eq!(
            run_build(command, &root, "fixture", || false).unwrap_err(),
            format!("could not build fixture: {missing}")
        );
    }
}

#[test]
fn a_stop_ends_a_running_build_and_answers_the_cancel() {
    let tree = Tree::new();
    let started = tree.path("started");
    // Not stopped, it would end on its own a minute on and fail.
    let stopped = run_build(
        shell("printf started > started; sleep 60; exit 5"),
        &tree.root,
        "fixture",
        || started.exists(),
    );
    assert_eq!(stopped.unwrap_err(), "reload cancelled by shutdown");
}

/// The server's case: a connection accepted on macOS is not close-on-exec until a second call
/// marks it, and a build started between the two must neither hold it nor keep its client waiting
/// for the end of its answer.
#[test]
fn a_connection_not_yet_close_on_exec_reaches_no_build_and_its_answer_ends_while_one_runs() {
    let tree = Tree::new();
    let release = tree.path("release");
    fifo(&release);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (accepted, _) = listener.accept().unwrap();
    // SAFETY: this test owns the descriptor; only its close-on-exec flag changes.
    assert_eq!(
        unsafe { libc::fcntl(accepted.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    // Its own stderr, which it does hold, shows the shell can see a descriptor it has.
    let script = format!(
        "if test ! -e /dev/fd/2; then held=blind; elif test -e /dev/fd/{}; then held=held; \
         else held=free; fi; printf %s \"$held\" > checked; read -r line < release",
        accepted.as_raw_fd()
    );
    std::thread::scope(|scope| {
        let build = scope.spawn(|| {
            let built = run_build(shell(&script), &tree.root, "fixture", || false);
            // A build that never opened its release still lets the wait for it below end.
            let reader = File::options()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&release);
            (built, reader)
        });
        // This opens once the build waits on its release: it started while the connection was
        // open here. Dropped however the test ends, it lets the build end.
        let mut go = File::options().write(true).open(&release).unwrap();
        assert_eq!(
            tree.read("checked"),
            "free",
            "the build holds the connection"
        );
        // The server answers and closes its end: the client reads to the answer's end while the
        // build runs.
        (&accepted).write_all(b"answer").unwrap();
        drop(accepted);
        let mut said = Vec::new();
        client.read_to_end(&mut said).unwrap();
        assert_eq!(said, b"answer");
        writeln!(go, "built").unwrap();
        drop(go);
        let (built, _reader) = build.join().unwrap();
        built.unwrap();
    });
}

#[test]
fn standalone_sources_are_owned_and_missing_sources_never_search_a_hostile_parent() {
    let tree = Tree::new();
    tree.write("apps/symphony-host/Cargo.toml", "[package]\n");
    tree.write("symphony/apps/symphony-server/Cargo.toml", "[package]\n");
    let own = tree.path("own");
    tree.write(
        "own/apps/symphony-server/Cargo.toml",
        "[package]\n[dependencies]\nleaf = { path = '../../crates/leaf' }\n",
    );
    tree.write("own/apps/symphony-server/src/lib.rs", "// owned\n");
    tree.write("own/crates/leaf/Cargo.toml", "[package]\n");
    tree.write("own/apps/symphony-ui/Cargo.toml", "[package]\n");
    tree.write(
        "own/symphony/apps/symphony-server/Cargo.toml",
        "[package]\n",
    );
    let directories = super::source_directories(&own).unwrap();
    let own = own.canonicalize().unwrap();
    assert_eq!(
        directories,
        ["apps/symphony-server", "apps/symphony-ui", "crates/leaf"]
            .map(|p| own.join(p))
            .into()
    );
    let mut files = std::collections::BTreeSet::new();
    super::crate_inputs(directories, &mut files).unwrap();
    assert!(files.contains(&own.join("apps/symphony-server/src/lib.rs")));
    tree.write(
        "own/apps/symphony-server/client/public/index.html",
        "release",
    );
    let mut changed = std::collections::BTreeSet::new();
    super::crate_inputs(super::source_directories(&own).unwrap(), &mut changed).unwrap();
    assert!(changed.contains(&own.join("apps/symphony-server/client/public/index.html")));
    std::fs::remove_dir_all(own.join("apps")).unwrap();
    std::fs::remove_dir_all(own.join("symphony")).unwrap();
    let error = super::source_directories(&own).unwrap_err();
    assert!(
        error.contains("own/symphony/apps/symphony-server"),
        "{error}"
    );
}

#[test]
fn consuming_host_tracks_local_plugin_sources_and_its_lockfile_pin() {
    let tree = Tree::new();
    tree.write("apps/symphony-host/Cargo.toml", "[package]\n[dependencies]\nplugin = { path = '../../plugin' }\nserver = { git = 'https://example.test/kit' }\n");
    tree.write("plugin/Cargo.toml", "[package]\n");
    let root = tree.root.canonicalize().unwrap();
    assert_eq!(
        super::source_directories(&root).unwrap(),
        [root.join("apps/symphony-host"), root.join("plugin")].into()
    );
    tree.write("Cargo.lock", "first immutable dependency pin");
    let before = super::hash_files(&root, super::common_inputs(&root).unwrap()).unwrap();
    tree.write("Cargo.lock", "second immutable dependency pin");
    let after = super::hash_files(&root, super::common_inputs(&root).unwrap()).unwrap();
    assert_ne!(before, after);
}

#[test]
fn a_client_build_starts_from_the_installed_builds_and_the_list_of_them() {
    let tree = Tree::new();
    tree.write("home/client/public/index.html", "installed page");
    tree.write(
        "home/client/public/assets/board-0123456789abcdef.js",
        "installed module",
    );
    tree.write(
        "home/client/public/assets/chunks/old-dxh789.js",
        "installed chunk",
    );
    tree.write("home/client/public/builds.json", "{\"builds\": []}");
    let installed = tree.path("home/client/public");
    super::copy_installed(&installed, &tree.path("stage/public")).unwrap();
    assert_eq!(tree.names("stage/public"), ["assets", "builds.json"]);
    assert_eq!(tree.read("stage/public/builds.json"), "{\"builds\": []}");
    assert_eq!(
        tree.read("stage/public/assets/board-0123456789abcdef.js"),
        "installed module"
    );
    assert_eq!(
        tree.read("stage/public/assets/chunks/old-dxh789.js"),
        "installed chunk"
    );
    // An installed client listing no builds copies no list, and none installed copies nothing.
    std::fs::remove_file(installed.join("builds.json")).unwrap();
    super::copy_installed(&installed, &tree.path("second/public")).unwrap();
    assert_eq!(tree.names("second/public"), ["assets"]);
    super::copy_installed(&tree.path("nothing/public"), &tree.path("third/public")).unwrap();
    assert!(!tree.path("third").exists());
}
