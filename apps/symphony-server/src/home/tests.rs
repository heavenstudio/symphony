use super::*;

/// A test's machine folder, beside the project's install folder ([`FOLDER`]).
const MACHINE: &str = "machine";

#[test]
fn server_records_prefer_the_machine_folder_and_read_the_predecessor_once() {
    let tree = crate::testing::Tree::new();
    let home = Home::new(
        &tree.path(MACHINE),
        Path::new("/bin/symphony-server"),
        Path::new("/r"),
        0,
        vec![],
    )
    .legacy(&tree.path(FOLDER));
    assert_eq!(home.recorded(), None);
    let old = Running {
        port: 41001,
        pid: 41,
        root: "/r".into(),
    };
    let new = Running {
        port: 41002,
        pid: 42,
        root: "/r".into(),
    };
    let legacy = format!("{FOLDER}/{ADDRESS}");
    tree.write(&legacy, &serde_json::to_string(&old).unwrap());
    assert_eq!(
        home.recorded(),
        None,
        "an absent predecessor is read only once"
    );
    let reader = Home::new(
        &tree.path(MACHINE),
        Path::new("/bin/symphony-server"),
        Path::new("/r"),
        0,
        vec![],
    )
    .legacy(&tree.path(FOLDER));
    assert_eq!(reader.recorded(), Some(old.clone()));
    tree.write(&legacy, &serde_json::to_string(&new).unwrap());
    assert_eq!(
        reader.recorded(),
        Some(old),
        "the predecessor record is cached"
    );
    tree.write(
        &format!("{MACHINE}/{ADDRESS}"),
        &serde_json::to_string(&new).unwrap(),
    );
    assert_eq!(
        reader.recorded(),
        Some(new.clone()),
        "the primary record wins"
    );
    tree.write(&format!("{MACHINE}/{ADDRESS}"), "broken");
    assert_eq!(
        reader.recorded(),
        None,
        "a corrupt primary must not select an old record"
    );
    fs::remove_file(tree.path(&format!("{MACHINE}/{ADDRESS}"))).unwrap();
    assert_eq!(reader.recorded().unwrap().pid, 41);
    let arbitrary = Home::new(
        &tree.path("private-home"),
        Path::new("/bin/symphony-server"),
        Path::new("/r"),
        0,
        vec![],
    );
    assert_eq!(
        arbitrary.recorded(),
        None,
        "private homes do not adopt unrelated records"
    );
}

#[test]
fn a_predecessor_record_is_up_only_when_its_private_server_answers_as_recorded() {
    let tree = crate::testing::Tree::new();
    let (port, looking) = statuses(vec![status(41), status(99)]);
    tree.write(
        &format!("{FOLDER}/{ADDRESS}"),
        &serde_json::to_string(&Running {
            port,
            pid: 41,
            root: "/r".into(),
        })
        .unwrap(),
    );
    let home = Home::new(
        &tree.path(MACHINE),
        Path::new("/bin/symphony-server"),
        Path::new("/r"),
        port,
        vec![],
    )
    .legacy(&tree.path(FOLDER));
    assert_eq!(home.up().unwrap().pid, 41);
    assert_eq!(
        home.up(),
        None,
        "a different process cannot satisfy the record"
    );
    drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
    assert_eq!(looking.join().unwrap(), 2);
    assert_eq!(home.up(), None, "a dead listener is not a live predecessor");
    let command = format!(
        "/old/bin/review-server serve --home {} --root /r --port {port}",
        tree.path(FOLDER).display()
    );
    assert_eq!(home.parse(41, &command).unwrap().port, port);
    tree.write(
        &format!("{FOLDER}/{LOG}"),
        &format!("→ Symphony at http://127.0.0.1:{port}/  (pid 41)\n"),
    );
    assert_eq!(
        home.parse(41, &command.replace(&format!("--port {port}"), "--port 0"))
            .unwrap()
            .port,
        port
    );
    assert_eq!(
        home.parse(
            41,
            &command.replace("/old/bin/review-server", "/old/bin/other")
        ),
        None
    );
    assert_eq!(
        home.parse(41, &command.replace("/sf-symphony ", "/unrelated ")),
        None
    );
}

/// A server on loopback whose status answers are `answers`, one per look, in turn; a look past
/// the last is closed unanswered. A connection that sends nothing ends it, with the looks it had.
fn statuses(answers: Vec<symphony_api::Front>) -> (u16, std::thread::JoinHandle<usize>) {
    use std::io::Write;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let looking = std::thread::spawn(move || {
        let mut answers = answers.into_iter();
        let mut looks = 0;
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                match stream.read(&mut byte) {
                    Ok(1) => request.push(byte[0]),
                    _ => break,
                }
            }
            if request.is_empty() {
                return looks;
            }
            looks += 1;
            if let Some(front) = answers.next() {
                let body = serde_json::to_string(&front).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        }
        looks
    });
    (port, looking)
}

/// The status of the server `pid`, up.
fn status(pid: i64) -> symphony_api::Front {
    symphony_api::Front {
        pid,
        port: 1,
        root: "/r".into(),
        ready: true,
        app: None,
        tailnet: None,
        orphans: 0,
        plugins: Vec::new(),
        pin: 0,
        schema: 0,
        projects: Vec::new(),
    }
}

#[test]
fn the_announced_port_is_the_last_line_of_that_pid() {
    let said = "→ Symphony at http://127.0.0.1:41000/  (pid 7)\n\
                    → Symphony at http://127.0.0.1:41001/  (pid 8)\n\
                    → Symphony at http://127.0.0.1:41002/  (pid 7)\n\
                    → Symphony at http://127.0.0.1:x/  (pid 9)\n\
                    mentions http://127.0.0.1:42000/ (pid 10) and goes on\n";
    assert_eq!(announced_port(said, 7), Some(41002));
    assert_eq!(announced_port(said, 8), Some(41001));
    assert_eq!(announced_port(said, 9), None);
    assert_eq!(announced_port(said, 10), None);
    assert_eq!(announced_port(said, 11), None);
    assert_eq!(announced_port("", 7), None);
}

#[test]
fn an_end_says_each_signal_its_pid_and_why() {
    assert_eq!(
        ending(42, libc::SIGTERM, "asked to stop (`--stop`)"),
        "· Symphony: SIGTERM to the server, pid 42: asked to stop (`--stop`)"
    );
    assert_eq!(
        ending(42, libc::SIGKILL, "asked to stop (`--stop`)"),
        "· Symphony: SIGKILL to the server, pid 42: it did not end within 20 s of SIGTERM \
         (asked to stop (`--stop`))"
    );
}

#[test]
fn a_port_is_held_unless_a_connection_to_it_is_refused() {
    let listening = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listening.local_addr().unwrap().port();
    assert!(held(port));
    // The local end of a connection this test holds: bound, and listened on by nothing.
    let connected = TcpStream::connect(("127.0.0.1", port)).unwrap();
    assert!(!held(connected.local_addr().unwrap().port()));
}

#[test]
fn recovery_of_a_server_on_a_picked_port_reads_the_port_from_its_log() {
    let dir = std::env::temp_dir().join(format!("home-picked-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(LOG),
        "→ Symphony at http://127.0.0.1:43210/  (pid 42)\n",
    )
    .unwrap();
    let home = Home::new(
        &dir,
        Path::new("/bin/symphony-server"),
        Path::new("/r"),
        0,
        vec![],
    );
    let command = format!(
        "{0}/bin/symphony-server serve --home {0} --root /r --port 0 --host 127.0.0.1",
        dir.display()
    );
    let found = home.parse(42, &command).map(|running| running.port);
    // Another pid, with nothing in the log, has no port to name.
    let other = home.parse(43, &command);
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(found, Some(43210));
    assert_eq!(other, None);
}

#[test]
fn recovery_reads_whole_paths_and_only_this_homes_server() {
    let home = Home::new(
        Path::new("/repo with spaces/.git/sf-symphony"),
        Path::new("/bin/symphony-server"),
        Path::new("/repo with spaces"),
        8765,
        Vec::new(),
    );
    let command = "/repo with spaces/.git/sf-symphony/bin/symphony-server serve --home /repo with spaces/.git/sf-symphony --root /repo with spaces --port 8765 --host 127.0.0.1 --app stub";
    assert_eq!(
        home.parse(42, command),
        Some(Running {
            port: 8765,
            pid: 42,
            root: "/repo with spaces".into(),
        })
    );
    for bad in [
        command.replace("/bin/symphony-server serve", "/bin/other serve"),
        command.replace("--home /repo with spaces/.git/sf-symphony", "--home /other"),
        command.replace("--port 8765", "--port invalid"),
        command.replace(" --root /repo with spaces", ""),
    ] {
        assert_eq!(home.parse(42, &bad), None, "{bad}");
    }
}
