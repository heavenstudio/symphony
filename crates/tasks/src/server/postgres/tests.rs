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
    // most, as the settings say them, fit in 512 MiB; each backend's own memory is not in the sum.
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
            ready: true,
            stopping: false
        })
    );
    for (status, stopping) in [("starting", false), ("stopping", true), ("standby ", false)] {
        let record = pid_file(&recorded(status)).unwrap();
        assert_eq!(
            (record.ready, record.stopping),
            (false, stopping),
            "{status}"
        );
    }
    // A record cut short, as a server that is still writing it leaves it, is not ready.
    let short = pid_file("86894\n/tmp/board/postgresql\n1794856071\n5432\n/tmp/s\n").unwrap();
    assert!(!short.ready && !short.stopping);
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
fn the_starters_record_names_a_server_and_its_starter_or_nothing() {
    assert_eq!(starter_of("86894 512\n"), Some((86894, 512)));
    assert_eq!(starter_of("86894 512"), Some((86894, 512)));
    for cut in [
        "",
        "\n",
        "86894",
        "86894 ",
        "86894 512 7\n",
        "86894  512\n",
        "86894\n512\n",
        "0 512\n",
        "86894 -1\n",
        "86894 x\n",
        " 86894 512\n",
    ] {
        assert_eq!(starter_of(cut), None, "{cut:?}");
    }
}

/// No process has this pid: a starter long ended.
const ENDED: i64 = i32::MAX as i64;

#[test]
fn a_server_is_outgoing_when_its_record_names_it_and_its_starter_has_ended() {
    let board = StandIn::installation("starter", "18.1");
    let record = |pid| PidFile {
        pid,
        socket: board.pg.socket(),
        ready: true,
        stopping: false,
    };
    let path = board.pg.home.dir.join(STARTER);
    assert_eq!(
        board.pg.starter_ended(&record(86894)),
        None,
        "no record: started apart or by hand"
    );
    fs::create_dir_all(&board.pg.home.dir).unwrap();
    fs::write(&path, format!("86894 {ENDED}\n")).unwrap();
    assert_eq!(board.pg.starter_ended(&record(86894)), Some(ENDED));
    assert_eq!(
        board.pg.starter_ended(&record(86895)),
        None,
        "another server on the data: the record is of an earlier one"
    );
    fs::write(&path, format!("86894 {}\n", std::process::id())).unwrap();
    assert_eq!(
        board.pg.starter_ended(&record(86894)),
        None,
        "its starter runs: this process"
    );
    fs::write(&path, "86894\n").unwrap();
    assert_eq!(board.pg.starter_ended(&record(86894)), None, "cut short");
}

/// A stop whose `pg_ctl` found no record, where the data holds none, judges by the process the
/// stop read: one that has ended was stopped, and one still running when the board's patience is
/// met is left, the refusal saying pg_ctl's words, that the wait stalled and the load.
#[test]
fn a_stop_whose_server_took_its_record_with_it_is_stopped_once_its_process_has_ended() {
    let pg = Home::new(Path::new("/nowhere/board"), Path::new("/s"))
        .patience(std::time::Duration::ZERO)
        .postgresql(Path::new("/nowhere/bin"));
    let record = |pid| PidFile {
        pid,
        socket: pg.socket(),
        ready: true,
        stopping: false,
    };
    let said = "pg_ctl: PID file \"/nowhere/board/postgresql/postmaster.pid\" does not exist";
    assert!(pg.stop_failed(&record(ENDED), said).is_ok());
    let mut running = command("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = i64::from(running.id());
    let refused = pg.stop_failed(&record(pid), said).unwrap_err().to_string();
    drop(running.stdin.take());
    running.wait().unwrap();
    assert!(
        refused.starts_with(&format!(
            "the board's PostgreSQL (pid {pid}) took its record with it before pg_ctl read it \
             ({said}), and has not ended in the 0 s this stop waited for it (load average "
        )),
        "{refused}"
    );
    assert!(
        refused.ends_with(
            "it is left to finish, never signalled again; its log is /nowhere/board/postgresql.log"
        ),
        "{refused}"
    );
}

/// A stop whose `pg_ctl` failed judges the data's record as it reads it again: the same pid
/// still runs and is refused as pg_ctl's wait met its bound; a record of another pid is never
/// taken for the server the stop read, and is refused naming both; and a process `ps` could not
/// be run to tell of is refused at once, never waited for as one that took its record with it.
#[test]
fn a_failed_stop_takes_only_its_own_pid_for_running_and_a_ps_that_cannot_run_for_neither() {
    let pg = Home::new(Path::new("/nowhere/board"), Path::new("/s"))
        .patience(std::time::Duration::from_secs(5))
        .postgresql(Path::new("/nowhere/bin"));
    let record = |pid| PidFile {
        pid,
        socket: pg.socket(),
        ready: true,
        stopping: false,
    };
    let said = "pg_ctl: server does not shut down";
    let log = "its log is /nowhere/board/postgresql.log";

    let refused = pg
        .stop_judged(&record(86894), said, Ok(Some(record(86894))))
        .unwrap_err()
        .to_string();
    assert!(
        refused.starts_with(&format!(
            "the board's PostgreSQL (pid 86894) was asked to stop, and pg_ctl, which waits 5 s at \
             most for its end, says it has not ended ({said}; load average "
        )),
        "{refused}"
    );
    assert!(
        refused.ends_with(&format!(
            "it is left to finish, never signalled again; {log}"
        )),
        "{refused}"
    );

    let refused = pg
        .stop_judged(&record(86894), said, Ok(Some(record(86895))))
        .unwrap_err()
        .to_string();
    assert_eq!(
        refused,
        format!(
            "the board's PostgreSQL (pid 86894) was asked to stop, pg_ctl failed ({said}), and \
             its data's record now names another server of it, pid 86895, which this stop did \
             not read: neither is signalled again; {log}"
        )
    );

    // A process that still runs: waited for as gone, the stop would meet its patience.
    let mut running = command("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = i64::from(running.id());
    let unknown = pg.stop_judged(
        &record(pid),
        said,
        Err("ps could not be run: Resource temporarily unavailable".into()),
    );
    drop(running.stdin.take());
    running.wait().unwrap();
    assert_eq!(
        unknown.unwrap_err().to_string(),
        format!(
            "the board's PostgreSQL (pid {pid}) was asked to stop, pg_ctl failed ({said}), and \
             whether it still runs could not be told (ps could not be run: Resource temporarily \
             unavailable): it is left to finish, never signalled again; {log}"
        )
    );

    // No record, and the process the stop read has ended: stopped.
    assert!(pg.stop_judged(&record(ENDED), said, Ok(None)).is_ok());
}

/// Whether a process is this board's server is what `ps` says its command line is: the board's
/// `postgres -D` on its data, nothing else, and nothing for a pid `ps` says nothing of. A `ps`
/// that could not be run, or that failed saying why on its error output, tells neither, and says
/// why.
#[test]
fn a_server_is_its_programs_postgres_on_its_data_and_a_ps_that_cannot_run_or_fails_is_unknown() {
    use std::os::unix::process::ExitStatusExt as _;
    let pg = Home::new(Path::new("/nowhere/board"), Path::new("/s"))
        .postgresql(Path::new("/nowhere/bin"));
    let said = |code: i32, line: &str, error: &str| {
        Ok(std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: line.as_bytes().to_vec(),
            stderr: error.as_bytes().to_vec(),
        })
    };
    let ps = |code: i32, line: &str| said(code, line, "");
    let serves = |said| pg.serves_as_said(said);
    assert_eq!(
        serves(ps(
            0,
            "/nowhere/bin/postgres -D /nowhere/board/postgresql\n"
        )),
        Ok(true)
    );
    for (line, what) in [
        ("", "no process of that pid"),
        (
            "/bin/sh /nowhere/bin/postgres -D /nowhere/board/postgresql",
            "a script by its name",
        ),
        (
            "/elsewhere/bin/postgres -D /nowhere/board/postgresql",
            "another PostgreSQL's",
        ),
        (
            "/nowhere/bin/postgres -D /nowhere/other/postgresql",
            "another board's",
        ),
        (
            "/nowhere/bin/postgres -D /nowhere/board/postgresql.making",
            "a data beside it",
        ),
        ("/bin/cat", "another program"),
    ] {
        assert_eq!(serves(ps(0, line)), Ok(false), "{what}");
    }
    assert_eq!(
        serves(ps(1, "")),
        Ok(false),
        "ps exits 1 for a pid with no process"
    );
    assert_eq!(
        serves(said(1, "", "\n")),
        Ok(false),
        "nothing but a blank line on its error output says no failure"
    );
    assert_eq!(
        serves(said(1, "", "ps: process id too large: 999999\n")),
        Err("ps failed (exit status: 1): ps: process id too large: 999999".into()),
        "ps that ran and failed, saying why, tells neither"
    );
    assert_eq!(
        serves(said(
            0,
            "/nowhere/bin/postgres -D /nowhere/board/postgresql\n",
            "ps: a warning\n"
        )),
        Ok(true),
        "what a ps that succeeded warns of changes nothing"
    );
    assert_eq!(
        serves(Err(std::io::Error::other("no process could be made"))),
        Err("ps could not be run: no process could be made".into())
    );
}

/// The data's record is a live server of the data, or none: no record, one cut short, one
/// naming a process that has ended, or one naming a live process that is not this data's
/// server (this test's own, which `ps` is run on).
#[test]
fn a_record_counts_only_for_a_live_server_of_the_data() {
    let board = StandIn::installation("record", "18.7");
    let data = board.pg.data();
    assert_eq!(board.pg.recorded_now(), Ok(None), "no record");
    fs::create_dir_all(&data).unwrap();
    let at = |pid: i64| {
        let text = recorded("ready   ").replacen("86894", &pid.to_string(), 1);
        fs::write(data.join("postmaster.pid"), text).unwrap();
    };
    fs::write(data.join("postmaster.pid"), "86894\n").unwrap();
    assert_eq!(board.pg.recorded_now(), Ok(None), "cut short");
    at(ENDED);
    assert_eq!(board.pg.recorded_now(), Ok(None), "its process has ended");
    at(i64::from(std::process::id()));
    assert_eq!(board.pg.recorded_now(), Ok(None), "not this data's server");
    assert_eq!(board.pg.recorded(), None);
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

/// A scratch board of this test's own under the temporary folder, served by stand-ins for
/// PostgreSQL's programs: each of `programs` a shell script by its name. Removed when it goes.
struct StandIn {
    dir: PathBuf,
    pg: Postgres,
}

impl StandIn {
    fn installation(name: &str, release: &str) -> StandIn {
        Self::new(
            name,
            &[
                ("postgres", &postgres(release, "exit 1")),
                ("initdb", INITDB),
                ("pg_ctl", "exit 0"),
                ("psql", "exit 0"),
            ],
        )
    }

    fn configure(&self, folder: &Path) {
        fs::write(
            self.dir.join(crate::project::FILE),
            format!("[board]\npostgresql = {:?}\n", folder.to_str().unwrap()),
        )
        .unwrap();
    }

    fn discover(&self, folders: &[&Path]) -> Result<PathBuf, String> {
        let path = std::env::join_paths(folders).unwrap();
        discover_with_path(&self.dir, Some(&path))
    }

    fn new(name: &str, programs: &[(&str, &str)]) -> StandIn {
        let dir = std::env::temp_dir().join(format!("pg-stand-in-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        for (program, script) in programs {
            let path = bin.join(program);
            fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let pg = Home::new(&dir.join("board"), Path::new("/s"))
            .telling(|_| {})
            .postgresql(&bin);
        StandIn { dir, pg }
    }

    fn refused(&self) -> String {
        self.pg.ensure().unwrap_err().to_string()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = fs::remove_dir(self.pg.socket());
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A `postgres` that says `release`, and does `serving` when it is started on its data.
fn postgres(release: &str, serving: &str) -> String {
    format!(
        "if [ \"$1\" = --version ]; then echo 'postgres (PostgreSQL) {release} (Homebrew)'; exit 0; fi\n{serving}"
    )
}

/// An `initdb` that makes a data folder with no more in it than its release.
const INITDB: &str = "mkdir -p \"$2\" && echo 18 > \"$2/PG_VERSION\"";

#[test]
fn an_alternate_prefix_with_spaces_is_discovered_without_a_project_file() {
    let fixture = StandIn::installation("alternate prefix", "18.7");
    assert!(!fixture.dir.join(crate::project::FILE).exists());
    let found = fixture
        .discover(&[&fixture.dir, &fixture.pg.programs])
        .unwrap();
    assert_eq!(found, fixture.pg.programs);
    for name in ["postgres", "initdb", "pg_ctl", "psql"] {
        assert!(executable(&found.join(name)), "{name}");
    }
    assert!(!fixture.pg.data().exists(), "discovery writes no database");
    // An unrelated project setting leaves discovery enabled too.
    fs::write(
        fixture.dir.join(crate::project::FILE),
        "[board]\nguide = 'x.md'\n",
    )
    .unwrap();
    assert_eq!(fixture.discover(&[&fixture.pg.programs]).unwrap(), found);
}

#[test]
fn an_explicit_override_wins_over_path_and_works_without_path() {
    let chosen = StandIn::installation("override", "18.7");
    let other = StandIn::installation("other-prefix", "17.2");
    chosen.configure(&chosen.pg.programs);
    assert_eq!(
        chosen.discover(&[&other.pg.programs]).unwrap(),
        chosen.pg.programs
    );
    assert_eq!(
        discover_with_path(&chosen.dir, None).unwrap(),
        chosen.pg.programs
    );
    // The public entrypoint also reads the explicit folder, irrespective of the real PATH.
    assert_eq!(discover_programs(&chosen.dir).unwrap(), chosen.pg.programs);
}

#[test]
fn an_invalid_override_is_refused_without_falling_back_to_path() {
    let fixture = StandIn::installation("invalid-override", "18.7");
    for folder in [fixture.dir.join("missing"), fixture.pg.program("postgres")] {
        fixture.configure(&folder);
        let refused = fixture.discover(&[&fixture.pg.programs]).unwrap_err();
        assert!(refused.contains(&folder.display().to_string()), "{refused}");
        assert!(refused.contains("PostgreSQL is not at"), "{refused}");
        assert!(refused.contains("[board] postgresql"), "{refused}");
    }
    for text in ["[board]\npostgresql = 'relative/bin'\n", "[board\n"] {
        fs::write(fixture.dir.join(crate::project::FILE), text).unwrap();
        assert!(fixture.discover(&[&fixture.pg.programs]).is_err());
    }
}

#[test]
fn missing_or_unset_path_names_the_required_release_and_setup_choices() {
    let fixture = StandIn::new("absent-tools", &[]);
    for refused in [
        fixture.discover(&[&fixture.pg.programs]).unwrap_err(),
        discover_with_path(&fixture.dir, None).unwrap_err(),
    ] {
        for needed in [
            "no executable postgres on PATH",
            "PostgreSQL 18 or later",
            "brew install postgresql@18",
            "[board] postgresql",
            "symphony.toml",
        ] {
            assert!(refused.contains(needed), "{needed}: {refused}");
        }
    }
}

#[test]
fn path_skips_non_executable_files_and_directories_named_postgres() {
    let skipped = StandIn::installation("non-executable", "17.2");
    fs::set_permissions(
        skipped.pg.program("postgres"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let folder = StandIn::new("directory-program", &[]);
    fs::create_dir(folder.pg.program("postgres")).unwrap();
    let chosen = StandIn::installation("executable", "18.7");
    assert_eq!(
        chosen
            .discover(&[
                &skipped.pg.programs,
                &folder.pg.programs,
                &chosen.pg.programs
            ])
            .unwrap(),
        chosen.pg.programs
    );
    // An explicit non-executable installation is an error, not a discovery request.
    chosen.configure(&skipped.pg.programs);
    assert!(
        chosen
            .discover(&[&chosen.pg.programs])
            .unwrap_err()
            .contains("not executable")
    );
}

#[test]
fn an_old_first_installation_or_override_is_refused_not_skipped() {
    let old = StandIn::installation("old-first", "17.2");
    let valid = StandIn::installation("valid-second", "18.7");
    for configured in [false, true] {
        if configured {
            valid.configure(&old.pg.programs);
        }
        let refused = valid
            .discover(&[&old.pg.programs, &valid.pg.programs])
            .unwrap_err();
        assert!(
            refused.contains("is PostgreSQL 17.2; the board runs on 18 or later"),
            "{refused}"
        );
        assert!(
            refused.contains(&old.pg.program("postgres").display().to_string()),
            "{refused}"
        );
        assert!(refused.contains("[board] postgresql"), "{refused}");
    }
}

#[test]
fn missing_or_non_executable_companions_are_not_taken_from_another_installation() {
    let complete = StandIn::installation("complete", "18.7");
    for tool in ["initdb", "pg_ctl", "psql"] {
        let partial = StandIn::installation(&format!("missing-{tool}"), "18.7");
        fs::remove_file(partial.pg.program(tool)).unwrap();
        let refused = partial
            .discover(&[&partial.pg.programs, &complete.pg.programs])
            .unwrap_err();
        assert!(
            refused.contains(&partial.pg.program(tool).display().to_string()),
            "{refused}"
        );
        assert!(
            refused.contains("all PostgreSQL tools must be in"),
            "{refused}"
        );
        fs::write(partial.pg.program(tool), "#!/bin/sh\nexit 0\n").unwrap();
        assert!(
            partial
                .discover(&[&partial.pg.programs])
                .unwrap_err()
                .contains("not executable")
        );
    }
}

#[test]
fn a_failed_or_unrunnable_version_probe_is_not_a_compatible_installation() {
    let fixture = StandIn::new(
        "failed-probe",
        &[(
            "postgres",
            "echo 'postgres (PostgreSQL) 18.7'; echo broken >&2; exit 1",
        )],
    );
    let refused = fixture.discover(&[&fixture.pg.programs]).unwrap_err();
    assert!(refused.contains("--version failed"), "{refused}");
    assert!(refused.contains("broken"), "{refused}");
    fs::write(fixture.pg.program("postgres"), "#!/no/such/interpreter\n").unwrap();
    let refused = fixture.discover(&[&fixture.pg.programs]).unwrap_err();
    assert!(refused.contains("could not run"), "{refused}");
    assert!(refused.contains("[board] postgresql"), "{refused}");
}

#[test]
fn a_newer_major_keeps_the_existing_minimum_version_contract() {
    let fixture = StandIn::installation("newer", "19.1");
    assert_eq!(
        fixture.discover(&[&fixture.pg.programs]).unwrap(),
        fixture.pg.programs
    );
}

#[test]
fn discovered_tools_initialize_a_clean_scratch_board_with_owned_fixtures_only() {
    let fixture = StandIn::installation("scratch-setup", "18.7");
    assert!(!fixture.pg.data().exists());
    let found = fixture.discover(&[&fixture.pg.programs]).unwrap();
    let pg = fixture.pg.home.postgresql(&found);
    // The fixture deliberately has no database server: setup reaches its start after initdb,
    // then reports its exit, leaving no running process or database outside this fixture.
    assert!(
        pg.ensure()
            .unwrap_err()
            .to_string()
            .contains("did not start")
    );
    assert_eq!(
        fs::read_to_string(pg.data().join("PG_VERSION")).unwrap(),
        "18\n"
    );
    assert_eq!(
        fs::read_to_string(pg.data().join("postgresql.conf")).unwrap(),
        settings(&pg.socket())
    );
    assert!(!fixture.dir.join("board/postgresql.making").exists());
    assert_eq!(pg.stop().unwrap(), None);
    assert!(!pg.socket().exists());
}

/// A start that finds a complete cluster in a home that names no engine (placed by hand, or by a
/// build from before the identity) names the home PostgreSQL's before it serves the cluster, and
/// makes nothing: no `initdb` runs and what the data holds stays.
#[test]
fn a_cluster_found_with_no_identity_is_given_it_by_its_start_and_never_made_again() {
    let found = StandIn::new(
        "found",
        &[
            ("postgres", &postgres("18.7", "exit 1")),
            ("initdb", "echo 'initdb ran' >&2; exit 1"),
        ],
    );
    fs::create_dir_all(found.pg.data()).unwrap();
    fs::write(found.pg.data().join("PG_VERSION"), "18\n").unwrap();
    fs::write(found.pg.data().join("kept"), "").unwrap();
    assert_eq!(found.pg.home.identity().unwrap(), None);
    // The stand-in serves nothing: the start gets as far as the server, past any initdb.
    let refused = found.refused();
    assert!(refused.contains("did not start"), "{refused}");
    assert_eq!(
        found.pg.home.identity().unwrap(),
        Some(crate::server::Engine::PostgreSql)
    );
    assert!(found.pg.data().join("kept").is_file());
    assert!(!found.dir.join("board/postgresql.making").exists());
    let log = fs::read_to_string(found.pg.log()).unwrap_or_default();
    assert!(!log.contains("initdb ran"), "{log}");
}

#[test]
fn a_release_older_than_the_oldest_or_none_is_refused_naming_it() {
    let old = StandIn::new("old", &[("postgres", &postgres("17.2", "exit 1"))]);
    let refused = old.refused();
    assert!(
        refused.contains("is PostgreSQL 17.2; the board runs on 18 or later"),
        "{refused}"
    );
    assert!(
        !old.pg.data().exists(),
        "nothing is made for a release refused"
    );
    assert!(old.pg.home.unmade().unwrap(), "and no engine is named");
    let none = StandIn::new("none", &[("postgres", "echo 'not a database'")]);
    let refused = none.refused();
    assert!(
        refused.contains("said \"not a database\", no release of PostgreSQL"),
        "{refused}"
    );
}

#[test]
fn an_initdb_that_fails_makes_nothing_where_a_start_looks_and_says_why_in_its_log() {
    let failing = StandIn::new(
        "initdb",
        &[
            ("postgres", &postgres("18.7", "exit 1")),
            (
                "initdb",
                "mkdir -p \"$2\"; echo 'initdb: error: could not write: No space left' >&2; exit 1",
            ),
        ],
    );
    let refused = failing.refused();
    assert!(
        refused.contains("initdb did not make the board's PostgreSQL data"),
        "{refused}"
    );
    assert!(refused.contains("No space left"), "{refused}");
    assert!(!failing.pg.data().exists());
    let log = fs::read_to_string(failing.pg.log()).unwrap();
    assert!(log.contains("No space left"), "{log}");
    // What it left beside the data is made anew by the next start, which then gets as far as the
    // server: this stand-in has none, so it ends before it is ready.
    let making = failing.dir.join("board/postgresql.making");
    assert!(making.is_dir());
    fs::write(
        failing.pg.program("initdb"),
        format!("#!/bin/sh\n{INITDB}\n"),
    )
    .unwrap();
    fs::write(making.join("half-made"), "").unwrap();
    let refused = failing.refused();
    assert!(refused.contains("did not start"), "{refused}");
    assert!(failing.pg.data().join("PG_VERSION").is_file());
    assert!(!failing.pg.data().join("half-made").exists());
    assert!(!making.exists());
}

#[test]
fn a_server_that_ends_before_it_is_ready_is_said_with_what_it_logged() {
    let ending = StandIn::new(
        "ends",
        &[
            (
                "postgres",
                &postgres(
                    "18.7",
                    "echo 'FATAL:  could not create shared memory segment' >&2; exit 1",
                ),
            ),
            ("initdb", INITDB),
        ],
    );
    let refused = ending.refused();
    assert!(
        refused.contains("the board's PostgreSQL (pid "),
        "{refused}"
    );
    assert!(
        refused.contains("did not start: FATAL:  could not create shared memory segment"),
        "{refused}"
    );
    // Its settings were written over the data's, and its socket's folder made private.
    let conf = fs::read_to_string(ending.pg.data().join("postgresql.conf")).unwrap();
    assert_eq!(conf, settings(&ending.pg.socket()));
    let folder = fs::metadata(ending.pg.socket()).unwrap();
    assert_eq!(folder.permissions().mode() & 0o777, 0o700);
    // A second start takes the folder it made; nothing ran to stop.
    assert!(ending.refused().contains("did not start"));
    assert_eq!(ending.pg.stop().unwrap(), None);
    assert!(!ending.pg.socket().exists());
}

#[test]
fn a_socket_folder_another_could_reach_is_refused_before_a_server_starts() {
    let open = StandIn::new(
        "open-socket",
        &[
            ("postgres", &postgres("18.7", "echo started >&2; exit 1")),
            ("initdb", INITDB),
        ],
    );
    fs::DirBuilder::new()
        .mode(0o755)
        .create(open.pg.socket())
        .unwrap();
    fs::set_permissions(open.pg.socket(), fs::Permissions::from_mode(0o755)).unwrap();
    let refused = open.refused();
    assert!(
        refused.contains("is there and is not a folder this user alone reads"),
        "{refused}"
    );
    assert!(
        !open.pg.log().exists()
            || !fs::read_to_string(open.pg.log())
                .unwrap()
                .contains("started")
    );
}

#[test]
fn socket_hash_names_keep_the_same_bytes_for_ascii_unicode_and_non_utf8_paths() {
    use std::os::unix::ffi::OsStrExt as _;
    for (path, hash) in [
        ("", "e3b0c44298fc1c14"),
        ("/tmp/board/postgresql", "370cac515b3de492"),
        ("/tmp/board-ç/postgresql", "c4e39f5fe8ac7408"),
    ] {
        assert_eq!(
            socket_folder(Path::new(path)),
            Path::new("/tmp").join(format!("symphony-pg-{hash}"))
        );
    }
    assert_eq!(
        socket_folder(Path::new(std::ffi::OsStr::from_bytes(
            b"/tmp/board-\xff/postgresql"
        ))),
        Path::new("/tmp/symphony-pg-721bd79b650bfba4")
    );
}

#[test]
fn a_postgresql_snapshot_and_recovery_copy_plainly_when_the_volume_cannot_clone() {
    use crate::server::snapshot::CLONE_FAILS;
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CLONE_FAILS.set(None);
        }
    }
    let source = StandIn::new("copy-fallback", &[("pg_ctl", "exit 3")]);
    fs::create_dir_all(source.pg.data().join("pg_tblspc")).unwrap();
    fs::write(source.pg.data().join("PG_VERSION"), "18").unwrap();
    fs::write(source.pg.data().join("kept"), "whole cluster").unwrap();
    fs::write(source.pg.data().join("postmaster.pid"), "an ended server").unwrap();
    CLONE_FAILS.set(Some("this volume cannot clone"));
    let _reset = Reset;
    let snapshot = source.pg.snapshot(1.0).unwrap();
    assert_eq!(
        fs::read_to_string(snapshot.join("kept")).unwrap(),
        "whole cluster"
    );
    let target = StandIn::new(
        "restore-fallback",
        &[
            ("pg_ctl", "exit 3"),
            ("postgres", "echo 'postgres (PostgreSQL) 18.7'"),
        ],
    );
    target.pg.restore(&snapshot).unwrap();
    assert_eq!(
        fs::read_to_string(target.pg.data().join("kept")).unwrap(),
        "whole cluster"
    );
    assert!(!target.pg.data().join("postmaster.pid").exists());
    assert!(
        snapshot.join("postmaster.pid").exists(),
        "the snapshot is unchanged"
    );
}

#[test]
fn recovery_never_removes_a_source_that_is_also_its_copy_work_folder() {
    let target = StandIn::new(
        "restore-overlap",
        &[
            ("pg_ctl", "exit 3"),
            ("postgres", "echo 'postgres (PostgreSQL) 18.7'"),
        ],
    );
    let source = target.pg.home.dir.join("postgresql.restoring");
    fs::create_dir_all(source.join("pg_tblspc")).unwrap();
    fs::write(source.join("PG_VERSION"), "18").unwrap();
    fs::write(source.join("kept"), "source evidence").unwrap();
    assert!(
        target
            .pg
            .restore(&source)
            .unwrap_err()
            .to_string()
            .contains("overlap")
    );
    assert_eq!(
        fs::read_to_string(source.join("kept")).unwrap(),
        "source evidence"
    );
    assert!(!target.pg.data().exists());
}

#[test]
fn recovery_refuses_a_home_selected_for_surrealdb_before_copying() {
    let target = StandIn::new("restore-wrong-engine", &[]);
    target
        .pg
        .home
        .initialize_backend(crate::server::Engine::SurrealDb)
        .unwrap();
    let source = target.dir.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("kept"), "source evidence").unwrap();
    assert!(
        target
            .pg
            .restore(&source)
            .unwrap_err()
            .to_string()
            .contains("owns SurrealDB")
    );
    assert!(!target.pg.data().exists());
    assert!(!target.pg.home.dir.join("postgresql.restoring").exists());
    assert_eq!(
        fs::read_to_string(source.join("kept")).unwrap(),
        "source evidence"
    );
}

#[test]
fn a_selected_home_uses_the_validated_project_installation() {
    let fixture = StandIn::installation("selected-install", "18.7");
    fixture.configure(&fixture.pg.programs);
    let selected = fixture.pg.home.project_postgresql(&fixture.dir).unwrap();
    assert_eq!(selected.programs, fixture.pg.programs);
    fs::remove_file(fixture.pg.program("pg_ctl")).unwrap();
    assert!(
        fixture
            .pg
            .home
            .project_postgresql(&fixture.dir)
            .err()
            .unwrap()
            .to_string()
            .contains("pg_ctl")
    );
    assert!(!fixture.pg.data().exists(), "discovery starts no database");
}

#[test]
fn the_hosted_checks_postgresql_16_is_the_folder_named_or_homebrews_keg() {
    let named = super::hosted_16_from(Some("/opt/pg16/bin".into()));
    assert_eq!(named, std::path::Path::new("/opt/pg16/bin"));
    for none in [None, Some(std::ffi::OsString::new())] {
        assert_eq!(
            super::hosted_16_from(none),
            std::path::Path::new("/opt/homebrew/opt/postgresql@16/bin")
        );
    }
    assert_eq!(super::HOSTED_16, "SYMPHONY_POSTGRESQL_16");
}
