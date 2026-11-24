use super::*;

#[test]
fn an_address_is_served_on_its_port_or_its_schemes() {
    assert_eq!(port_of("http://127.0.0.1:8765/symphony"), Some(8765));
    assert_eq!(port_of("http://127.0.0.1:59266"), Some(59266));
    assert_eq!(port_of("http://localhost/p"), Some(80));
    assert_eq!(port_of("https://h.example/p"), Some(443));
    assert_eq!(port_of("http://[::1]:4100/p"), Some(4100));
    assert_eq!(port_of("http://[::1]/p"), Some(80));
    assert_eq!(port_of("http://h:port/p"), None);
    assert_eq!(port_of("127.0.0.1:8765"), None);
}

#[test]
fn a_boards_postgresql_is_its_server_and_the_processes_it_started() {
    let tree = crate::test_tree::Tree::new();
    let board = |name: &str| {
        let common = tree.path(&format!("{name}/.git"));
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(common.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        common.join("symphony/tasks/postgresql")
    };
    let (lamp, desk) = (board("lamp shop"), board("desk"));
    let said = format!(
        "    1     0 /sbin/launchd\n\
         43782     1 /opt/homebrew/opt/postgresql@18/bin/postgres -D {lamp}\n\
         43790 43782 postgres: checkpointer \n\
         43791 43782 postgres: postgres tasks [local] idle\n\
         50000     1 /opt/homebrew/opt/postgresql@18/bin/postgres -D /elsewhere/postgresql\n\
         50001 50000 postgres: checkpointer \n\
         51000     1 /opt/homebrew/bin/postgres -D {desk}\n\
         60000 43782\n\
         not a line\n",
        lamp = lamp.display(),
        desk = desk.display()
    );
    assert_eq!(
        board_servers(&said),
        [vec![43782, 43790, 43791], vec![51000]],
        "a server on no board's data is none, and one that started nothing is itself alone"
    );
    assert!(board_servers("").is_empty());
}

/// lsof's lines for a process's Unix sockets as macOS prints them: each connected one's own
/// address (`d`) and its other end's (`n->`); a listening one names its path.
#[test]
fn a_sockets_other_end_and_each_processs_own_are_read_from_lsof() {
    let server =
        "p34918\nf1\nd0x6b55\nn->0x7f7a\nf9\nd0x7f7a\nn->0x6b55\nf14\nd0x7a79\nn->0x59d1\n";
    assert_eq!(
        peers(server),
        BTreeSet::from(["0x7f7a".into(), "0x6b55".into(), "0x59d1".into()])
    );
    assert!(peers("p1\nf5\nd0x43\nn/tmp/symphony-pg-1/.s.PGSQL.5432\n").is_empty());
    let started = "p46454\nf8\nd0x59d1\nn/tmp/symphony-pg-1/.s.PGSQL.5432\np55959\np94886\nf8\nd0x1f27\nn/tmp/s/.s.PGSQL.5432\n";
    assert_eq!(
        sockets(started),
        BTreeMap::from([
            (46454, BTreeSet::from(["0x59d1".to_string()])),
            (94886, BTreeSet::from(["0x1f27".to_string()])),
        ])
    );
    assert!(
        sockets("d0x1\n").is_empty(),
        "a socket under no process is none"
    );
}

#[test]
fn the_databases_watched_are_those_one_of_whose_processes_holds_the_servers_connection() {
    let servers = vec![vec![43782, 43790, 43791], vec![51000, 51001], vec![52000]];
    let peers = BTreeSet::from(["0x59d1".to_string(), "0x7f7a".to_string()]);
    let sockets = BTreeMap::from([
        (43791, BTreeSet::from(["0x59d1".to_string()])),
        (51001, BTreeSet::from(["0x1f27".to_string()])),
        // The server's own process listens, and is no connection's end.
        (52000, BTreeSet::from(["0x7f7a".to_string()])),
    ]);
    assert_eq!(
        connected(servers.clone(), &peers, &sockets),
        [vec![43782, 43790, 43791]]
    );
    assert!(connected(servers, &BTreeSet::new(), &sockets).is_empty());
}

#[test]
fn resident_memory_is_read_in_bytes_from_ps_kilobytes() {
    let resident = resident_bytes("20483  96528\n85785 422720\nnot a line\n");
    assert_eq!(resident.get(&20483), Some(&(96528.0 * 1024.0)));
    assert_eq!(resident.get(&85785), Some(&(422_720.0 * 1024.0)));
    assert_eq!(resident.len(), 2);
}

#[test]
fn memory_is_said_in_megabytes_with_the_databases_counted() {
    let memory = Memory {
        heap: Some(12.4e6),
        server: Some(95.0e6),
        databases: Some(423.0e6),
        database_count: 2,
    };
    assert_eq!(
        memory.said(),
        "heap 12 MB  server 95 MB  board db 423 MB (2)"
    );
    assert_eq!(
        Memory::default().said(),
        "heap - MB  server - MB  board db - MB"
    );
}

#[test]
fn nothing_watched_reads_no_memory_but_the_heap_given() {
    let memory = Watched::default().memory(Some(1e6));
    assert_eq!(
        memory,
        Memory {
            heap: Some(1e6),
            ..Memory::default()
        }
    );
}

#[test]
fn this_process_is_watched_as_a_server_by_its_own_resident_memory() {
    let pid = std::process::id();
    let watched = Watched {
        server: Some(pid),
        // One database of two processes, this one twice: its memory is theirs summed.
        databases: vec![vec![pid, pid]],
    };
    let memory = watched.memory(None);
    let server = memory.server.unwrap_or_else(|| panic!("{memory:?}"));
    assert!(server > 0.0, "{memory:?}");
    assert_eq!(
        memory.database_count, 1,
        "a database, however many processes"
    );
    assert_eq!(memory.databases, Some(2.0 * server), "{memory:?}");
}

#[test]
fn the_server_found_is_the_process_listening_on_the_port_and_none_where_none_listens() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let found = Watched::find(&format!("http://127.0.0.1:{port}/p"));
    assert_eq!(found.server, Some(std::process::id()));
    assert!(found.databases.is_empty(), "{found:?}");
    drop(listener);
    assert_eq!(
        Watched::find(&format!("http://127.0.0.1:{port}/p")),
        Watched::default()
    );
    assert_eq!(Watched::find("no address"), Watched::default());
}

#[test]
fn the_cpu_busy_is_its_share_of_the_ticks_between_two_readings() {
    assert_eq!(busy_share(Some([100, 1000]), Some([400, 1400])), Some(75.0));
    assert_eq!(
        busy_share(Some([100, 1000]), Some([100, 1000])),
        None,
        "no tick passed"
    );
    assert_eq!(
        busy_share(Some([500, 1000]), Some([100, 2000])),
        None,
        "a counter wrapped"
    );
    assert_eq!(busy_share(None, Some([1, 2])), None);
    let [busy, all] = cpu_ticks().expect("this machine counts its CPU's ticks");
    assert!(busy <= all, "{busy} of {all}");
}

#[test]
fn proc_stat_says_busy_as_all_but_idle_and_disk_waits() {
    assert_eq!(
        proc_stat_ticks("cpu  100 5 50 800 40 3 2 0 0 0"),
        Some([160, 1000])
    );
    assert_eq!(proc_stat_ticks("cpu  10 0 10 80"), Some([20, 100]));
    assert_eq!(proc_stat_ticks("cpu0 1 2 3 4"), None, "one core's line");
    assert_eq!(proc_stat_ticks("cpu 1 2"), None);
}

#[test]
fn the_load_is_said_whole_or_unknown() {
    assert_eq!(load_said(Some([80.4, 100.0, 170.6])), "load 80 100 171");
    assert_eq!(load_said(None), "load unknown");
    assert!(load_now().is_some(), "this machine says its load");
}

/// A board's PostgreSQL this process holds a connection to is one of its databases, with the
/// processes it started; before the connection, it is none.
#[test]
fn a_board_this_process_is_connected_to_is_one_of_its_databases() {
    use symphony_board::server::{Home, postgres};
    let tree = crate::test_tree::Tree::new();
    let common = tree.path("lamp/.git");
    std::fs::create_dir_all(&common).unwrap();
    std::fs::write(common.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let programs = postgres::discover_programs(&workspace).unwrap();
    let pg = Home::in_common(&common)
        .supervised(true)
        .postgresql(&programs);
    let serving = pg.ensure().unwrap();
    let server = u32::try_from(serving.pid).unwrap();
    let watched = |found: &[Vec<u32>]| found.iter().any(|database| database[0] == server);
    let pid = std::process::id();
    let before = databases_of(pid);
    let client = symphony_board::pg::Client::new(&serving.socket, "postgres");
    client.simple("SELECT 1").unwrap();
    let after = databases_of(pid);
    drop(client);
    pg.stop().unwrap();
    assert!(!watched(&before), "no connection yet: {before:?}");
    assert!(
        after
            .iter()
            .any(|database| database[0] == server && database.len() > 1),
        "{after:?}"
    );
}
