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
fn the_listeners_a_server_reached_are_its_connections_far_ends_but_its_own_ports() {
    let said = "p20483\nf10\nn127.0.0.1:59266->127.0.0.1:59472\nf11\n\
                n127.0.0.1:60163->127.0.0.1:50699\nf13\nn127.0.0.1:60504->127.0.0.1:50699\n\
                f14\nn[::1]:61000->[::1]:50606\nf15\nn127.0.0.1:59266\n";
    assert_eq!(ports_reached(said, 59266), [50606, 50699]);
    assert!(ports_reached("", 59266).is_empty());
}

#[test]
fn the_databases_are_the_listeners_named_surreal() {
    let said =
        "p43782\ncsurreal\nf28\np85785\ncsurreal\nf14\np999\ncpostgres\nf3\np85785\ncsurreal\n";
    assert_eq!(listeners_named(said, "surreal"), [43782, 85785]);
    assert!(
        listeners_named("csurreal\n", "surreal").is_empty(),
        "a command with no process"
    );
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
    let watched = Watched {
        server: Some(std::process::id()),
        databases: vec![std::process::id()],
    };
    let memory = watched.memory(None);
    assert!(memory.server.is_some_and(|bytes| bytes > 0.0), "{memory:?}");
    assert_eq!(memory.database_count, 1);
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
