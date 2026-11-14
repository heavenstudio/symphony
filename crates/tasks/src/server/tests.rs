//! Which process-table lines are the board's server: the server's own, however its `surreal` is
//! run, and never the supervisor shell of a scratch board's server or its watchdog, which keep the
//! server's words among their own arguments.

use super::*;

const SURREAL: &str = "/repo/.git/symphony/surreal/3.3.0/surreal";
const ADDR: &str = "127.0.0.1:4100";
const DATA: &str = "surrealkv:///repo/.git/symphony/tasks/data";
const LOG: &str = "/repo/.git/symphony/tasks/server.log";
const ASKED: &str = "/repo/.git/symphony/tasks/ending";

/// The server's command line as `script` starts it, `"$0"` being `program`: the words before its
/// redirections, with its arguments in place.
fn server_line(script: &str, program: &str, data: &str) -> String {
    let at = script.find("\"$0\"").unwrap();
    let words = &script[at..at + script[at..].find(" >>").unwrap()];
    words
        .replace("\"$0\"", program)
        .replace("\"$1\"", ADDR)
        .replace("\"$2\"", data)
}

/// The supervisor of [`OWNED_START`] (and its watchdog, a fork of it) as `ps -ww` prints it: the
/// script's lines joined by `\012`, then its arguments, the folder of servers asked to end last.
fn supervisor_line(surreal: &str, data: &str) -> String {
    format!(
        "/bin/sh -c {} {surreal} {ADDR} {data} {LOG} {ASKED}",
        OWNED_START.replace('\n', "\\012")
    )
}

fn found(line: &str) -> Option<String> {
    server_on(line, Path::new(SURREAL), DATA)
}

#[test]
fn the_server_either_start_runs_is_found_at_the_address_it_binds() {
    for script in [START, OWNED_START] {
        let line = server_line(script, SURREAL, DATA);
        assert!(line.starts_with(&format!("{SURREAL} start ")), "{line}");
        assert_eq!(found(&line).as_deref(), Some(ADDR), "{line}");
    }
}

#[test]
fn a_scratch_boards_supervisor_and_its_watchdog_are_never_its_server() {
    let line = supervisor_line(SURREAL, DATA);
    for word in [" start ", " --bind ", &format!(" {DATA} ")] {
        assert!(line.contains(word), "the supervisor holds {word:?}: {line}");
    }
    assert_eq!(found(&line), None, "{line}");
    assert!(!last_argument_is(&line, DATA), "{line}");
}

#[test]
fn a_surreal_run_by_a_scripts_interpreter_or_execd_by_a_wrapper_is_the_server() {
    let stand_in = "/repo/fixtures/surreal-stand-in";
    let interpreted =
        format!("/bin/sh {stand_in} start --unauthenticated --log error --bind {ADDR} {DATA}");
    assert_eq!(
        server_on(&interpreted, Path::new(stand_in), DATA).as_deref(),
        Some(ADDR)
    );
    // The tests' guard execs the pinned binary: the process is that `surreal`, not the guard.
    let execd = format!("{SURREAL} start --unauthenticated --log error --bind {ADDR} {DATA}");
    assert_eq!(
        server_on(&execd, Path::new("/repo/fixtures/guarded-surreal"), DATA).as_deref(),
        Some(ADDR)
    );
    let bare = format!("/opt/bin/surreal start --bind {ADDR} {DATA}");
    assert_eq!(found(&bare).as_deref(), Some(ADDR));
}

#[test]
fn paths_with_spaces_are_read_whole() {
    let surreal = "/Users/o/My Work/.git/symphony/surreal/3.3.0/surreal";
    let data = "surrealkv:///Users/o/My Work/.git/symphony/tasks/data";
    let line = server_line(OWNED_START, surreal, data);
    assert_eq!(
        server_on(&line, Path::new(surreal), data).as_deref(),
        Some(ADDR)
    );
    assert_eq!(
        server_on(&supervisor_line(surreal, data), Path::new(surreal), data),
        None
    );
    let path_with_start = "/Users/o/a start here/surreal";
    let line = server_line(START, path_with_start, data);
    assert_eq!(
        server_on(&line, Path::new(path_with_start), data).as_deref(),
        Some(ADDR)
    );
}

#[test]
fn another_program_other_data_or_no_address_is_not_the_server() {
    for line in [
        format!("/usr/bin/other start --bind {ADDR} {DATA}"),
        format!("{SURREAL} export --endpoint http://{ADDR} {DATA}"),
        format!("{SURREAL} start --bind {ADDR} {DATA}-old"),
        format!("{SURREAL} start --bind {ADDR} surrealkv:///repo/.git/symphony/tasks/reclaim/data"),
        format!("{SURREAL} start --bind \"$1\" {DATA}"),
        format!("{SURREAL} start --unauthenticated {ADDR} {DATA}"),
        format!("{SURREAL} started --bind {ADDR} {DATA}"),
        format!("{SURREAL}x start --bind {ADDR} {DATA}"),
        DATA.to_string(),
        String::new(),
    ] {
        assert_eq!(found(&line), None, "{line}");
    }
}

#[test]
fn a_board_server_names_the_common_dir_of_the_data_it_serves() {
    let root = std::env::temp_dir().join(format!("board-served-{}", std::process::id()));
    let common = root.join("My Work/.git");
    fs::create_dir_all(common.join(FOLDER)).unwrap();
    fs::write(common.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    // The board's data folder (`DATA` here is the tests' own command-line spelling of one).
    let data = store(&common.join(FOLDER).join("data"));
    for script in [START, OWNED_START] {
        let line = server_line(script, SURREAL, &data);
        assert_eq!(board_served(&line), Some(common.clone()), "{line}");
    }
    let bare = format!("/opt/bin/surreal start --bind {ADDR} {data}");
    assert_eq!(board_served(&bare), Some(common.clone()));
    // A scratch board's supervisor, other data, data in no git common dir, a folder of the
    // board's last name elsewhere in the common dir, another command.
    let loose = root.join("loose").join(FOLDER);
    let elsewhere = store(&loose.join("data"));
    fs::create_dir_all(&loose).unwrap();
    fs::create_dir_all(common.join("elsewhere/tasks")).unwrap();
    for line in [
        format!(
            "{SURREAL} start --bind {ADDR} {}",
            store(&common.join("elsewhere/tasks/data"))
        ),
        supervisor_line(SURREAL, &data),
        format!("{SURREAL} start --bind {ADDR} memory"),
        format!("{SURREAL} start --bind {ADDR} {data}-old"),
        format!(
            "{SURREAL} start --bind {ADDR} {}",
            store(&common.join(FOLDER).join("reclaim/data"))
        ),
        format!("{SURREAL} start --bind {ADDR} {elsewhere}"),
        format!("{SURREAL} export --endpoint http://{ADDR} {data}"),
        format!("{SURREAL} start --bind \"$1\" {data}"),
        format!("{SURREAL} started --bind {ADDR} {data}"),
        data.clone(),
        String::new(),
    ] {
        assert_eq!(board_served(&line), None, "{line}");
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_last_argument_is_a_whole_word() {
    assert!(last_argument_is(&format!("{SURREAL} start {DATA}"), DATA));
    assert!(!last_argument_is(&format!("{SURREAL} start x{DATA}"), DATA));
    assert!(!last_argument_is(
        &format!("{SURREAL} start {DATA} {LOG}"),
        DATA
    ));
    assert!(
        !last_argument_is(DATA, DATA),
        "a program is not its own argument"
    );
}
