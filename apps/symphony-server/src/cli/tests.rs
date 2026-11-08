use super::*;

fn args(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// Set in this test binary's own child, run by the case below: the server a scratch run starts.
const SCRATCH_SERVER: &str = "SF_SCRATCH_RUN_SERVER";

/// Only in that child: a scratch run on the streams and folder the case gave it, ending with its code.
#[test]
fn scratch_run_in_a_child() {
    if let Some(server) = std::env::var_os(SCRATCH_SERVER) {
        let args = ["--port".to_owned(), "an argument".to_owned()];
        let code = scratch(
            Path::new(&server),
            &args,
            "scratchdb",
            Path::new("/asks.db"),
        );
        std::process::exit(code);
    }
}

/// A scratch run starts the server with its arguments, its own board and this process's
/// environment, folder and streams, and ends with its code; one that cannot start or ends on a
/// signal is 2.
#[test]
fn a_scratch_run_keeps_its_arguments_board_folder_streams_and_exit() {
    let dir = std::env::temp_dir().join(format!("scratch-run-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    // The machine's copy of the fixture script, already run: the target's budget covers no
    // first exec of a new file.
    let script = |name: &str, body: &str| {
        let path = dir.join(name);
        crate::fixture_script::write(&path, &format!("#!/bin/sh\n{body}"), false);
        crate::testing::assert_primed(&path);
        path
    };
    let server = script(
        "server",
        "IFS= read -r line\n\
             printf '%s\\n' \"$line\" \"$PWD\" \"$TASKS_DATABASE\" \"$ASKS_DB\" \"$TASKS_SWEEP\" \"$KEPT\" \"$@\"\n\
             printf 'diagnostic\\n' >&2\nexit 7\n",
    );
    let killed = script("killed", "kill -TERM $$\n");
    std::fs::write(dir.join("input"), "request\n").unwrap();
    let run = |server: &Path| {
        let file = |name: &str| std::fs::File::create(dir.join(name)).unwrap();
        let status = symphony_board::sys::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cli::tests::scratch_run_in_a_child"])
            .args(["--nocapture", "--test-threads=1"])
            .env(SCRATCH_SERVER, server)
            .env("KEPT", "inherited")
            .current_dir(&dir)
            .stdin(std::fs::File::open(dir.join("input")).unwrap())
            .stdout(file("out"))
            .stderr(file("err"))
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        (status.code(), read("out"), read("err"))
    };
    let (code, out, err) = run(&server);
    let said = format!(
        "request\n{}\nscratchdb\n/asks.db\noff\ninherited\n--port\nan argument\n",
        dir.display()
    );
    assert_eq!(code, Some(7), "{out}{err}");
    assert!(out.ends_with(&said), "{out}");
    assert!(err.ends_with("diagnostic\n"), "{err}");
    assert_eq!(run(&dir.join("missing")).0, Some(2));
    assert_eq!(run(&killed).0, Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_opener_is_the_systems_own_given_the_url_alone() {
    let url = "http://127.0.0.1:8765/@tools~branch/";
    let opener = opener(url);
    let system = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    assert_eq!(opener.get_program(), system);
    assert_eq!(opener.get_args().collect::<Vec<_>>(), [url]);
    assert_eq!(opener.get_current_dir(), None);
}

#[test]
fn no_arguments_start_the_one_server() {
    assert_eq!(parse(&[]), Ok(Action::Start(Options::default())));
}

#[test]
fn a_port_alone_or_named_and_hosts_are_read() {
    let want = Options {
        open: true,
        port: Some(9000),
        ..Options::default()
    };
    assert_eq!(parse(&args("--open 9000")), Ok(Action::Start(want.clone())));
    assert_eq!(parse(&args("--port 9000 --open")), Ok(Action::Start(want)));
    let Ok(Action::Start(hosts)) = parse(&args("--host 127.0.0.1 --host ::1 --root /x")) else {
        panic!("not a start");
    };
    assert_eq!(hosts.hosts, ["127.0.0.1", "::1"]);
    assert_eq!(hosts.root, Some(PathBuf::from("/x")));
}

#[test]
fn if_up_asks_for_the_running_server_alone() {
    let Ok(Action::Start(options)) = parse(&args("--if-up --open")) else {
        panic!("not a start");
    };
    assert!(options.if_up && options.open);
    assert!(USAGE.contains("--if-up"));
}

#[test]
fn stop_and_help_are_their_own_actions() {
    assert!(matches!(
        parse(&args("--host 127.0.0.1 --stop")),
        Ok(Action::Stop(_))
    ));
    assert_eq!(parse(&args("--help")), Ok(Action::Help));
    assert_eq!(parse(&args("-h")), Ok(Action::Help));
    assert!(USAGE.contains("--port"));
}

#[test]
fn a_server_of_its_own_is_refused_the_one_servers_port_and_the_apps() {
    // The one server on a test's port: that port and the app's are both refused.
    for taken in [41000, crate::home::DEFAULT_PORT] {
        let why = reserved(taken, 41000).expect("a reserved port");
        assert!(
            why.starts_with(&format!("port {taken} is the one server's")),
            "{why}"
        );
    }
    assert_eq!(reserved(41001, 41000), None);
    // The server picks its own port; a one server on a picked port reserves none.
    assert_eq!(reserved(0, 41000), None);
    assert_eq!(reserved(41000, 0), None);
    assert!(reserved(crate::home::DEFAULT_PORT, 0).is_some());
}

#[test]
fn a_bad_line_is_refused() {
    for bad in [
        "--port x",
        "--port",
        "--bogus",
        "1 2",
        "--port 70000",
        "--root",
    ] {
        assert!(parse(&args(bad)).is_err(), "{bad}");
    }
}
