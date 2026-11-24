//! A snapshot of a board's database as it runs ([`Home::export`]), its schema ([`Home::schema`])
//! and its writers ([`Home::writers`]): what it holds, written whole, from its running server
//! without stopping it; a board whose database is not running, one not made yet and one still on
//! SurrealDB are refused, and nothing is started.

use std::fs;
use std::time::Duration;
use symphony_board::server::{GONE, Home, ServerError};
use symphony_board::{NewTask, Store};

/// A board not made yet has no server, and one still on SurrealDB is refused, naming how it
/// moves: nothing is read or exported, nothing started and no folder made.
#[test]
fn a_board_not_made_or_on_surrealdb_is_refused_and_nothing_starts() {
    let s = crate::common::Scratch::new("export-none");
    let home = s.home();
    let file = s.dir.join("snapshots/before.sql");
    for refused in [
        home.export("tasks", &file).unwrap_err(),
        home.schema("tasks").unwrap_err(),
        home.writers("tasks").unwrap_err(),
    ] {
        assert!(
            matches!(&refused, ServerError::NotUp(why) if why.contains("is not running (no address is recorded)")),
            "{refused:?}"
        );
    }
    assert!(home.unmade().unwrap(), "nothing was made");
    assert!(!file.exists() && !s.dir.join("snapshots").exists());

    fs::create_dir(s.dir.join("data")).unwrap();
    for refused in [
        home.export("tasks", &file).unwrap_err(),
        home.schema("tasks").unwrap_err(),
        home.writers("tasks").unwrap_err(),
    ] {
        let said = refused.to_string();
        assert!(said.contains(GONE), "{said}");
        assert!(said.ends_with("Nothing is read or exported"), "{said}");
    }
    assert!(!file.exists() && !s.dir.join("snapshots").exists());
    assert!(!s.dir.join("postgresql").exists() && !s.dir.join("backend.json").exists());
}

/// A board is read and exported by its project's programs, as it runs: its schema, its writers,
/// and `pg_dump`'s text; stopped, it is refused, nothing is started and no folder is made; an
/// export past its time is stopped and writes nothing, its database serving on.
#[test]
fn a_board_on_postgresql_is_read_and_exported_through_its_running_server() {
    use std::path::Path;
    use symphony_board::server::{Engine, postgres};
    let root = std::env::temp_dir().join(format!("export-pg-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let common = root.join(".git");
    fs::create_dir_all(&common).unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let machine = postgres::discover_programs(&workspace).unwrap();
    // The project's programs are the machine's, but for a `pg_dump` that, while `hang` is there,
    // writes part of its file, then hangs until it is killed: one waited out would end well, its
    // file written, so the refusal naming the limit says it was not.
    let programs = root.join("programs");
    fs::create_dir_all(&programs).unwrap();
    for entry in fs::read_dir(&machine).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != "pg_dump" {
            std::os::unix::fs::symlink(entry.path(), programs.join(entry.file_name())).unwrap();
        }
    }
    let hang = root.join("hang");
    crate::common::script(
        &programs.join("pg_dump"),
        &format!(
            "if [ -e '{}' ]; then\nfor last; do :; done\necho partial > \"$last\"\nexec sleep 600\nfi\nexec '{}' \"$@\"",
            hang.display(),
            machine.join("pg_dump").display()
        ),
    );
    fs::write(
        root.join("symphony.toml"),
        format!(
            "[project]\nid = \"pg\"\n[board]\npostgresql = {:?}\n",
            programs.display().to_string()
        ),
    )
    .unwrap();
    let home = Home::in_common(&common).telling(|_| {});
    home.initialize_backend(Engine::PostgreSql).unwrap();
    let pg = home.postgresql(&programs);
    let file = home.dir().join("snapshots/before.sql");

    let stopped = home.schema("tasks").unwrap_err();
    assert!(
        matches!(&stopped, ServerError::NotUp(why) if why.contains("PostgreSQL") && why.contains("is not running")),
        "{stopped}"
    );
    assert!(matches!(home.writers("tasks"), Err(ServerError::NotUp(_))));
    assert!(matches!(
        home.export("tasks", &file),
        Err(ServerError::NotUp(_))
    ));
    assert!(pg.current().unwrap().is_none(), "nothing was started");
    assert!(!home.dir().join("snapshots").exists());

    let serving = pg.ensure().unwrap();
    let store = Store::opening_on(
        symphony_board::pg::Client::new(&serving.socket, "tasks").into(),
        Box::new(|| 1_800_000_000.0),
        Box::new(|_| true),
        "this-host",
        &symphony_board::Opening::write(),
    )
    .unwrap();
    store
        .create(&NewTask::new(
            "sim/exported",
            "A task the snapshot holds",
            "owner",
        ))
        .unwrap();
    assert_eq!(home.schema("tasks").unwrap(), Some(symphony_board::SCHEMA));
    let writers = home.writers("tasks").unwrap();
    assert_eq!(writers.len(), 1, "{writers:?}");
    assert_eq!(writers[0].schema, symphony_board::SCHEMA);
    let read = home.export("tasks", &file).unwrap();
    assert_eq!(read.pid, serving.pid);
    assert_eq!(
        read.addr,
        format!("postgresql:{}", serving.socket.display())
    );
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("CREATE TABLE") && text.contains("sim/exported"),
        "{text}"
    );
    let left: Vec<String> = fs::read_dir(home.dir().join("snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["before.sql"], "no partial export is left");
    // A database that is not there fails, and leaves no part of a file.
    let missing = home.export("never_made", &home.dir().join("snapshots/none.sql"));
    assert!(
        matches!(&missing, Err(ServerError::Io(why)) if why.contains("failed")),
        "{missing:?}"
    );
    assert_eq!(
        fs::read_dir(home.dir().join("snapshots")).unwrap().count(),
        1
    );
    assert_eq!(
        pg.current().unwrap().map(|s| s.pid),
        Some(serving.pid),
        "still serving"
    );

    // An export past its time is stopped and writes nothing; the database itself is never
    // signalled.
    fs::write(&hang, "").unwrap();
    let late = home.dir().join("snapshots/late.sql");
    let refused = home
        .export_within("tasks", &late, Duration::from_secs(1))
        .unwrap_err();
    assert!(
        matches!(&refused, ServerError::Io(why) if why.contains("did not start or end within 1 s")
            && why.contains("nothing was written")),
        "{refused:?}"
    );
    assert!(!late.exists());
    assert_eq!(
        fs::read_dir(home.dir().join("snapshots")).unwrap().count(),
        1,
        "no partial export is left"
    );
    assert_eq!(pg.current().unwrap().map(|s| s.pid), Some(serving.pid));
    assert_eq!(home.schema("tasks").unwrap(), Some(symphony_board::SCHEMA));
    assert!(symphony_board::server::EXPORT_LIMIT >= Duration::from_secs(60));
    pg.stop().unwrap();
    fs::remove_dir_all(&root).unwrap();
}
