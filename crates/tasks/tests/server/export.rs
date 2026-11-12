//! A snapshot of a board's database as it runs ([`Home::export`]) and its schema
//! ([`Home::schema`]): what it holds, written whole, from the recorded server without stopping it;
//! a board whose database is not running is refused and nothing is started.

use crate::common::Scratch;
use std::fs;
use symphony_board::rpc::Client;
use symphony_board::server::ServerError;
use symphony_board::{NewTask, Store};

#[test]
fn a_running_board_is_exported_whole_and_its_server_keeps_serving() {
    let s = Scratch::new("export");
    let home = s.home();
    let running = home.ensure().unwrap();
    let store = Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap();
    store
        .create(&NewTask::new(
            "sim/exported",
            "A task the snapshot holds",
            "owner",
        ))
        .unwrap();
    assert_eq!(
        home.schema("tasks").unwrap(),
        Some(symphony_board::SCHEMA),
        "the board's schema, read as it runs"
    );
    assert_eq!(
        home.schema("never_made").unwrap(),
        None,
        "a database never made has no schema"
    );
    let file = s.dir.join("snapshots/before.surql");
    let read = home.export("tasks", &file).unwrap();
    assert_eq!(read, running, "the recorded server, as it runs");
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("sim/exported"), "{text}");
    assert!(text.contains("A task the snapshot holds"));
    let left: Vec<String> = fs::read_dir(s.dir.join("snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["before.surql"], "no partial export is left");
    // Not stopped, not replaced: the same server answers and takes writes.
    assert_eq!(home.recorded(), Some(running.clone()));
    store
        .create(&NewTask::new("sim/after", "Written after", "owner"))
        .unwrap();
    assert!(!fs::read_to_string(&file).unwrap().contains("sim/after"));
}

#[test]
fn a_board_whose_database_is_not_running_is_refused_and_nothing_starts() {
    let s = Scratch::new("export-none");
    let home = s.home();
    let file = s.dir.join("snapshots/before.surql");
    let refused = home.export("tasks", &file).unwrap_err();
    assert!(
        matches!(&refused, ServerError::NotUp(why) if why.contains("is not running (no address is recorded)")),
        "{refused:?}"
    );
    assert!(matches!(
        home.schema("tasks"),
        Err(ServerError::NotUp(why)) if why.contains("is not running (no address is recorded)")
    ));
    assert!(!s.dir.join("server.json").exists(), "nothing was started");
    assert!(!file.exists() && !s.dir.join("snapshots").exists());

    // A record naming a process that does not serve this board's data is not trusted.
    s.record(&symphony_board::server::Running {
        addr: "127.0.0.1:9".into(),
        pid: i64::from(std::process::id()),
        version: "3.0.0".into(),
    });
    let refused = home.export("tasks", &file).unwrap_err();
    assert!(
        matches!(&refused, ServerError::NotUp(why) if why.contains("is not running as recorded")),
        "{refused:?}"
    );
    assert!(matches!(
        home.schema("tasks"),
        Err(ServerError::NotUp(why)) if why.contains("is not running as recorded")
    ));
    assert!(!file.exists());

    let missing = symphony_board::server::Home::new(&s.dir, &s.dir.join("no-surreal"));
    assert!(matches!(
        missing.export("tasks", &file),
        Err(ServerError::NoSurreal(_))
    ));
}

#[test]
fn an_export_past_its_time_is_stopped_and_writes_nothing_while_the_database_serves_on() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{Duration, Instant};
    let s = Scratch::new("export-slow");
    let running = s.home().ensure().unwrap();
    // An export client that writes part of its file, then hangs.
    let slow = s.dir.join("slow-surreal");
    fs::write(
        &slow,
        "#!/bin/sh\nfor last; do :; done\necho partial > \"$last\"\nexec sleep 30\n",
    )
    .unwrap();
    fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
    let home = symphony_board::server::Home::new(&s.dir, &slow);
    let file = s.dir.join("snapshots/before.surql");
    let began = Instant::now();
    let refused = home
        .export_within("tasks", &file, Duration::from_secs(1))
        .unwrap_err();
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "it did not wait the hang out"
    );
    assert!(
        matches!(&refused, ServerError::Io(why) if why.contains("did not start or end within 1 s")
            && why.contains("nothing was written")),
        "{refused:?}"
    );
    assert!(!file.exists());
    let left: Vec<_> = fs::read_dir(s.dir.join("snapshots")).unwrap().collect();
    assert!(left.is_empty(), "no partial export is left: {left:?}");
    // The database itself was never signalled: the same server answers.
    assert_eq!(s.home().recorded(), Some(running.clone()));
    assert!(s.home().schema("tasks").is_ok(), "it answers as it did");
    assert!(symphony_board::server::EXPORT_LIMIT >= Duration::from_secs(60));
}
