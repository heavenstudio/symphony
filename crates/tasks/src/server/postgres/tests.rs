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
        let value = toml::Value::String(folder.display().to_string());
        fs::write(
            self.dir.join(crate::project::FILE),
            format!("[board]\npostgresql = {value}\n"),
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
