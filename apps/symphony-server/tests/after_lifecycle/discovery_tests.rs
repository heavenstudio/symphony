//! Starts with no live server to find read no process table and announce before their first board
//! read; a live one behind a lost record or a lying hint is still found and adopted.

use super::*;

/// The line the first board read logs before it releases what waited on it.
const WARMED: &str = "→ native read data warmed:";

#[test]
fn a_cold_start_announces_its_url_before_its_first_board_read() {
    let repo = Repo::new("announce before board");
    repo.start(&repo.main, false);
    let log = repo.home.join("server.log");
    let said = until("the first board read's line", || {
        fs::read_to_string(&log)
            .ok()
            .filter(|said| said.contains(WARMED))
    });
    let announced = said.find("→ Symphony at ");
    let warmed = said.find(WARMED);
    assert!(
        announced.is_some_and(|announced| Some(announced) < warmed),
        "announced at {announced:?}, board read at {warmed:?}: {said}"
    );
}

#[test]
fn cold_auto_port_starts_skip_census_but_live_announcements_survive_bad_hints() {
    let repo = Repo::new("discovery port hints");
    let started = repo.start(&repo.main, false);
    assert!(
        !repo
            .base
            .join(format!("env.{}", started.child.id()))
            .exists(),
        "a cold auto-port start read the process table: {}",
        started.output()
    );
    let running = repo.record().unwrap();
    let mut dead = Command::new("/usr/bin/true").spawn().unwrap();
    let stale_pid = dead.id();
    dead.wait().unwrap();
    let record = |pid, port, root: &str| {
        serde_json::json!({"pid": pid, "port": port, "root": root}).to_string()
    };
    for contents in [
        None,
        Some(String::new()),
        Some("{malformed".into()),
        Some(record(i64::from(stale_pid), running.port, &running.root)),
        Some(record(running.pid, 0, &running.root)),
        Some(record(running.pid, running.port, "/wrong root")),
        Some(record(
            i64::from(std::process::id()),
            running.port,
            &running.root,
        )),
    ] {
        let address = repo.home.join("server.json");
        fs::remove_file(&address).unwrap();
        if let Some(contents) = contents {
            fs::write(&address, contents).unwrap();
        }
        let recovered = repo.start(&repo.main, false);
        assert!(
            repo.base
                .join(format!("env.{}", recovered.child.id()))
                .exists(),
            "a live server's record was recovered without checking its process: {}",
            recovered.output()
        );
        assert_eq!(repo.record(), Some(running.clone()));
        assert_eq!(repo.check_server().pid, running.pid);
    }
    // A misleading port hint cannot hide the live server named in its owned log.
    let listening = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let connected = TcpStream::connect(listening.local_addr().unwrap()).unwrap();
    let free = connected.local_addr().unwrap().port();
    // Nothing listens there: the connection is refused, however long a loaded machine takes.
    let refused = TcpStream::connect(("127.0.0.1", free)).unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::ConnectionRefused);
    fs::remove_file(repo.home.join("server.json")).unwrap();
    write(
        &repo.main.join("artifacts/docs-app.port"),
        &free.to_string(),
    );
    repo.start(&repo.main, false);
    assert_eq!(repo.record(), Some(running.clone()));
    assert_eq!(repo.check_server().pid, running.pid);
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert_eq!(
        log.matches("→ Symphony at ").count(),
        1,
        "recovery restarted the server: {log}"
    );
}

#[test]
fn malformed_and_dead_records_without_a_server_are_replaced() {
    for malformed in [true, false] {
        let repo = Repo::new("dead record");
        let mut dead = Command::new("/usr/bin/true").spawn().unwrap();
        let pid = dead.id();
        dead.wait().unwrap();
        let text = if malformed {
            "{malformed".into()
        } else {
            serde_json::json!({"pid": pid, "port": 1, "root": "/gone"}).to_string()
        };
        write(&repo.home.join("server.json"), &text);
        let started = repo.start(&repo.main, false);
        assert!(
            !repo
                .base
                .join(format!("env.{}", started.child.id()))
                .exists(),
            "a dead record without startup history read the process table: {}",
            started.output()
        );
        assert_ne!(repo.check_server().pid, i64::from(pid));
    }
}
