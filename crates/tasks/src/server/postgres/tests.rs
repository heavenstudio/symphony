//! What a board's PostgreSQL is read and set up by, without a server: its socket's place, its
//! settings, the project file's key, the release its programs say, PostgreSQL's record and what
//! the supervisor says. `tests/server/postgres.rs` starts real ones.

use super::*;

#[test]
fn a_socket_is_short_and_its_own_whatever_the_boards_folder() {
    let long = PathBuf::from(format!(
        "/private/tmp/{}/checkout/artifacts/tools/board-pg-server/run-1/board/postgresql",
        "a-checkout-under-a-long-path".repeat(8)
    ));
    assert!(long.as_os_str().len() > 250);
    let folder = socket_folder(&long);
    let socket = folder.join(SOCKET_FILE);
    assert_eq!(socket.as_os_str().len(), 47, "{}", socket.display());
    assert!(socket.as_os_str().len() <= SOCKET_PATH_MAX);
    assert!(folder.starts_with("/tmp"), "{}", folder.display());
    assert_eq!(
        folder,
        socket_folder(&long),
        "the same board, the same place"
    );
    assert_ne!(
        folder,
        socket_folder(Path::new("/tmp/other/board/postgresql"))
    );
    let name = folder.file_name().unwrap().to_str().unwrap();
    let hex = name.strip_prefix("symphony-pg-").unwrap();
    assert_eq!(hex.len(), 16);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
}

#[test]
fn the_settings_listen_on_the_socket_alone_sized_for_512_mib() {
    let said = settings(Path::new("/tmp/symphony-pg-0123456789abcdef"));
    for line in [
        "listen_addresses = ''",
        "unix_socket_directories = '/tmp/symphony-pg-0123456789abcdef'",
        "unix_socket_permissions = 0700",
        "max_connections = 64",
        "shared_buffers = 128MB",
        "work_mem = 4MB",
        "maintenance_work_mem = 32MB",
        "autovacuum_max_workers = 2",
        "max_parallel_workers_per_gather = 0",
        "lc_messages = 'C'",
        "timezone = 'UTC'",
    ] {
        assert!(
            said.lines().any(|written| written == line),
            "{line}:\n{said}"
        );
    }
    // The shared buffers, every connection sorting at once and every autovacuum worker at its
    // most, as the settings say them, fit in 512 MiB.
    let value = |key: &str| -> u64 {
        let line = said
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key} = ")));
        let line = line.unwrap_or_else(|| panic!("{key}:\n{said}"));
        line.trim_end_matches("MB").parse().unwrap()
    };
    let most = value("shared_buffers")
        + value("max_connections") * value("work_mem")
        + value("autovacuum_max_workers") * value("maintenance_work_mem");
    assert_eq!(most, 128 + 64 * 4 + 2 * 32);
    assert!(most <= 512, "{most} MiB");
}

#[test]
fn the_supervisor_runs_the_server_on_its_data_and_says_when_it_is_ready() {
    assert!(SUPERVISED.contains(r#""$0" -D "$1" 2>&1 3<&- 4>&- &"#));
    assert!(SUPERVISED.contains("database system is ready to accept connections"));
    assert!(SUPERVISED.contains("kill -INT \"$server\"; kill -TERM \"$server\""));
    assert!(SUPERVISED.contains(r#"printf '%s\n' "$line" >>"$2""#));
}

/// A project file at a folder of this test's own, removed after.
fn project_file(text: Option<&str>, name: &str) -> Result<Option<PathBuf>, String> {
    let root =
        std::env::temp_dir().join(format!("postgres-programs-{}-{name}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    if let Some(text) = text {
        fs::write(root.join(crate::project::FILE), text).unwrap();
    }
    let read = programs(&root);
    fs::remove_dir_all(&root).unwrap();
    read
}

#[test]
fn the_project_file_names_the_programs_folder_or_none() {
    assert_eq!(project_file(None, "none"), Ok(None));
    assert_eq!(
        project_file(Some("[board]\nguide = \"x.md\"\n"), "unnamed"),
        Ok(None)
    );
    assert_eq!(
        project_file(
            Some("[board]\npostgresql = \"/opt/homebrew/opt/postgresql@18/bin\"\n"),
            "named"
        ),
        Ok(Some(PathBuf::from("/opt/homebrew/opt/postgresql@18/bin")))
    );
    for (text, said) in [
        (
            "[board]\npostgresql = \"pg/bin\"\n",
            "is \"pg/bin\", not the absolute folder",
        ),
        (
            "[board]\npostgresql = 18\n",
            "is 18, not the absolute folder",
        ),
        ("[board\n", "symphony.toml"),
    ] {
        let refused = project_file(Some(text), "refused").unwrap_err();
        assert!(refused.contains(said), "{text}: {refused}");
    }
}

#[test]
fn the_release_is_what_postgres_says_and_its_major_comes_first() {
    assert_eq!(
        version_of("postgres (PostgreSQL) 18.7 (Homebrew)\n").as_deref(),
        Some("18.7")
    );
    assert_eq!(
        version_of("postgres (PostgreSQL) 19beta2").as_deref(),
        Some("19beta2")
    );
    assert_eq!(version_of("postgres (PostgreSQL) devel"), None);
    assert_eq!(version_of("surreal 3.3.0"), None);
    assert_eq!(major("18.7"), Some(18));
    assert_eq!(major("19beta2"), Some(19));
    assert_eq!(major("x"), None);
}

/// `postmaster.pid` as PostgreSQL 18 writes it for a server on a socket alone: pid, data, start,
/// port, socket folder, no listen address, its shared memory's key, and its status.
fn recorded(status: &str) -> String {
    format!(
        "86894\n/tmp/board/postgresql\n1794856071\n5432\n/tmp/symphony-pg-try1\n\n  5432001    65536\n{status}\n"
    )
}

#[test]
fn postgresqls_record_says_its_server_its_socket_and_whether_it_is_ready() {
    assert_eq!(
        pid_file(&recorded("ready   ")),
        Some(PidFile {
            pid: 86894,
            socket: PathBuf::from("/tmp/symphony-pg-try1"),
            ready: true
        })
    );
    for status in ["starting", "stopping", "standby "] {
        assert!(!pid_file(&recorded(status)).unwrap().ready, "{status}");
    }
    // A record cut short, as a server that is still writing it leaves it, is not ready.
    let short = pid_file("86894\n/tmp/board/postgresql\n1794856071\n5432\n/tmp/s\n").unwrap();
    assert!(!short.ready);
    assert_eq!(pid_file("86894\n/tmp/board/postgresql\n"), None);
    assert_eq!(pid_file("not a pid\n"), None);
    assert_eq!(pid_file(""), None);
}

#[test]
fn the_supervisor_says_the_pid_then_whether_the_server_was_ready_or_ended() {
    let said = |pid, ready| Said { pid, ready };
    assert_eq!(Said::of("4242\nready\n"), said(Some(4242), Some(true)));
    assert_eq!(Said::of("ready\n4242\n"), said(Some(4242), Some(true)));
    assert_eq!(Said::of("4242\nended\n"), said(Some(4242), Some(false)));
    assert_eq!(Said::of("4242\n"), said(Some(4242), None));
    assert_eq!(Said::of(""), said(None, None));
    assert_eq!(
        Said::of("4242\n4343\nready\n"),
        said(Some(4242), Some(true))
    );
}

#[test]
fn a_board_outside_a_scratch_place_is_refused_before_anything_is_made() {
    let board = PathBuf::from(format!("/Users/nobody-{}/board", std::process::id()));
    let refused = Home::new(&board, Path::new("/s"))
        .postgresql(Path::new("/opt/pg/bin"))
        .ensure()
        .unwrap_err()
        .to_string();
    assert!(refused.contains("is not in a scratch place"), "{refused}");
    assert!(!board.exists());
}

#[test]
fn programs_that_are_not_there_are_refused_naming_the_setting_and_the_install() {
    let refused = Home::new(Path::new("/tmp/board"), Path::new("/s"))
        .postgresql(Path::new("/nowhere/pg/bin"))
        .version()
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("PostgreSQL is not at /nowhere/pg/bin"),
        "{refused}"
    );
    assert!(refused.contains("[board] postgresql"), "{refused}");
    assert!(refused.contains("brew install postgresql@18"), "{refused}");
}
